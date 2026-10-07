//! `edel boot guard`: a slot that hangs falls back too, not only one that
//! crashes (roadmap M1.5, ADR-006). The `edel-guard` service starts it in
//! the background early in boot. It opens the watchdog and pets it until
//! every health file the image names exists, then confirms the slot, stops
//! the watchdog and exits, so no daemon stays. If the files do not appear
//! in time it stops petting: the watchdog resets the machine and GRUB
//! counts the try. A machine with no watchdog at all, such as a cloud VM
//! that offers none on a kernel without softdog, is restarted by the
//! guard itself, forced, so a hung service cannot hold the restart up
//! (M1.10).
//!
//! Early in boot the only watchdog is often softdog, a kernel timer that a
//! frozen kernel never fires, or none at all (linux-virt has no softdog).
//! A hardware watchdog (a laptop's iTCO_wdt or sp5100_tco, QEMU's
//! i6300esb) loads later, from hwdrivers, so the guard takes the first one
//! that appears and stops softdog.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::Result;

use edel::places;

use crate::release::os_release_value;
use crate::update;

/// The health names edel itself writes, and the file that says each one
/// is reached. OpenRC's own `softlevel` is never used: OpenRC writes it
/// when a runlevel starts, while services may still hang. Any other name
/// is a feature's own check, its file in [`places::HEALTH_DIR`] (M1.11).
const SENTINELS: [(&str, &str); 2] = [
    ("default-runlevel", places::DEFAULT_REACHED),
    ("compositor", places::READY_FILE),
];
/// How long a slot may take to become healthy when the image says nothing.
pub const DEFAULT_TIMEOUT: u64 = 120;
const CONFIRMED: &str = places::CONFIRMED;
/// Where the kernel lists watchdogs, each with its `identity`.
const WATCHDOG_CLASS: &str = "/sys/class/watchdog";
/// softdog's identity; every other watchdog is hardware.
const SOFTDOG: &str = "Software Watchdog";

/// Whether `name` is one of the health names edel itself writes.
pub fn is_built_in(name: &str) -> bool {
    SENTINELS.iter().any(|(n, _)| *n == name)
}

/// The file that says health `name` is reached: edel's own, else the
/// feature's in [`places::HEALTH_DIR`]; none for a name that is not one
/// (a-z, 0-9 and '-').
pub fn health_file(name: &str) -> Option<PathBuf> {
    match SENTINELS.iter().find(|(n, _)| *n == name) {
        Some((_, file)) => Some(PathBuf::from(file)),
        None if edel::features::is_name(name) => Some(Path::new(places::HEALTH_DIR).join(name)),
        None => None,
    }
}

/// The health files to wait for and the timeout, from os-release. A name
/// that is no name is skipped and returned.
fn health_plan(os_release: &str) -> (Vec<PathBuf>, u64, Vec<String>) {
    let mut files = Vec::new();
    let mut unknown = Vec::new();
    for name in os_release_value(os_release, "EDEL_HEALTH")
        .unwrap_or_default()
        .split([' ', ','])
        .filter(|n| !n.is_empty())
    {
        match health_file(name) {
            Some(file) => files.push(file),
            None => unknown.push(name.to_string()),
        }
    }
    let timeout = os_release_value(os_release, "EDEL_HEALTH_TIMEOUT")
        .and_then(|t| t.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT);
    (files, timeout, unknown)
}

fn healthy(files: &[PathBuf]) -> bool {
    files.iter().all(|f| f.exists())
}

/// The health names of `files` not reached yet, for the guard's message.
fn not_reached(files: &[PathBuf]) -> String {
    let names: Vec<String> = files
        .iter()
        .filter(|f| !f.exists())
        .map(
            |f| match SENTINELS.iter().find(|(_, file)| Path::new(file) == f) {
                Some((name, _)) => name.to_string(),
                None => f
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            },
        )
        .collect();
    names.join(", ")
}

/// The watchdog device, loading softdog when the hardware has none.
fn open_watchdog() -> Option<File> {
    let open = || OpenOptions::new().write(true).open("/dev/watchdog").ok();
    open().or_else(|| {
        let _ = Command::new("modprobe").arg("softdog").status();
        sleep(Duration::from_millis(500));
        open()
    })
}

/// The watchdogs in `class` (`/sys/class/watchdog`) by device name, such
/// as `watchdog1`, with their identity, in order.
fn watchdogs(class: &Path) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(class) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let identity = fs::read_to_string(e.path().join("identity")).unwrap_or_default();
            (name, identity.trim().to_string())
        })
        .collect();
    found.sort();
    found
}

/// The first hardware watchdog in `found`: one whose identity sysfs gives
/// and is not softdog's.
fn first_hardware(found: &[(String, String)]) -> Option<&(String, String)> {
    found
        .iter()
        .find(|(_, identity)| !identity.is_empty() && identity != SOFTDOG)
}

/// Opens the first hardware watchdog once one has loaded, stops softdog
/// with the magic close if the guard held it, and sets `on_hardware`; does
/// nothing once the guard pets hardware.
fn take_hardware(watchdog: &mut Option<File>, on_hardware: &mut bool) {
    if *on_hardware {
        return;
    }
    let found = watchdogs(Path::new(WATCHDOG_CLASS));
    let Some((name, identity)) = first_hardware(&found) else {
        return;
    };
    let Ok(hardware) = OpenOptions::new()
        .write(true)
        .open(Path::new("/dev").join(name))
    else {
        // mdev has not made its node yet; the next look tries again.
        return;
    };
    if let Some(mut softdog) = watchdog.replace(hardware) {
        let _ = softdog.write_all(b"V");
    }
    *on_hardware = true;
    println!("edel guard: using the hardware watchdog {identity} ({name})");
}

/// Waits for the slot to become healthy, then confirms it.
pub fn guard() -> Result<()> {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let (files, timeout, unknown) = health_plan(&os_release);
    for name in unknown {
        eprintln!(
            "warning: edel guard: {name:?} in EDEL_HEALTH is not a health name (a-z, 0-9 and '-'), so it is skipped"
        );
    }
    let mut watchdog = open_watchdog();
    if watchdog.is_none() {
        eprintln!(
            "warning: edel guard: no watchdog yet; if none loads, the guard restarts the machine itself when this slot is not healthy after {timeout} s"
        );
    }
    // /dev/watchdog is watchdog0, the first one registered.
    let mut on_hardware = watchdog.is_some()
        && watchdogs(Path::new(WATCHDOG_CLASS))
            .iter()
            .any(|(name, identity)| name == "watchdog0" && identity != SOFTDOG);
    if on_hardware {
        println!("edel guard: using the hardware watchdog (watchdog0)");
    }
    let start = Instant::now();
    loop {
        take_hardware(&mut watchdog, &mut on_hardware);
        if let Some(dog) = watchdog.as_mut() {
            let _ = dog.write_all(b".");
        }
        if healthy(&files) {
            // A healthy machine is never reset, even when confirming fails.
            let lines = match confirm_when_free(&mut watchdog) {
                Ok(lines) => lines,
                Err(err) => vec![format!("edel update: could not confirm this slot: {err:#}")],
            };
            if let Some(mut dog) = watchdog.take() {
                // The magic close: stop the watchdog instead of resetting.
                let _ = dog.write_all(b"V");
            }
            if let Err(err) = fs::write(CONFIRMED, lines.join("\n") + "\n") {
                eprintln!("warning: edel guard: could not write {CONFIRMED}: {err}");
            }
            return Ok(());
        }
        if start.elapsed() >= Duration::from_secs(timeout) {
            eprintln!(
                "edel guard: health not reached after {timeout} s: {}",
                not_reached(&files)
            );
            match on_timeout(watchdog.is_some()) {
                Timeout::LetTheWatchdog => {
                    eprintln!(
                        "edel guard: this slot is not healthy after {timeout} s; letting the watchdog restart the machine"
                    );
                    // Closing without the magic byte leaves the watchdog
                    // running.
                    std::process::exit(1);
                }
                Timeout::RestartItself => {
                    eprintln!(
                        "edel guard: this slot is not healthy after {timeout} s and the machine has no watchdog; restarting it now, so the boot loader counts the try"
                    );
                    restart_now();
                }
            }
        }
        // Looked at ten times a second, so the slot is confirmed as soon as
        // it is healthy, not up to a second later; petting the watchdog
        // as often costs nothing.
        sleep(Duration::from_millis(100));
    }
}

/// What the guard does when the slot is not healthy in time.
#[derive(Debug, PartialEq)]
enum Timeout {
    /// Stops petting: the watchdog, hardware or softdog, resets the
    /// machine, even when the kernel itself froze (hardware only).
    LetTheWatchdog,
    /// No watchdog at all: the guard restarts the machine itself.
    RestartItself,
}

fn on_timeout(has_watchdog: bool) -> Timeout {
    if has_watchdog {
        Timeout::LetTheWatchdog
    } else {
        Timeout::RestartItself
    }
}

/// Writes what is cached to the disks, then restarts at once, as a
/// watchdog reset would: through the kernel, not init, which could wait
/// on the service that hung (`reboot -f`).
fn restart_now() -> ! {
    // SAFETY: sync and reboot take no pointers; reboot with RB_AUTOBOOT
    // returns only when it fails, as without the right to restart.
    let err = unsafe {
        libc::sync();
        libc::reboot(libc::RB_AUTOBOOT);
        std::io::Error::last_os_error()
    };
    eprintln!("edel guard: could not restart the machine: {err}");
    std::process::exit(1);
}

/// Confirms the slot, waiting while another updater holds the lock (an
/// install started before the health files appeared) and petting the
/// watchdog meanwhile: the machine is healthy, so it is never reset here.
/// Gives up after 30 minutes.
fn confirm_when_free(watchdog: &mut Option<File>) -> Result<Vec<String>> {
    let lock = Path::new(places::UPDATE_LOCK);
    let start = Instant::now();
    loop {
        match update::confirm_running() {
            Ok(lines) => return Ok(lines),
            Err(_) if lock.exists() && start.elapsed() < Duration::from_secs(1800) => {
                if let Some(dog) = watchdog.as_mut() {
                    let _ = dog.write_all(b".");
                }
                sleep(Duration::from_secs(1));
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OS_RELEASE: &str = "NAME=\"Edel OS\"\nEDEL_HEALTH=\"default-runlevel network Bad_Name\"\nEDEL_HEALTH_TIMEOUT=30\n";

    #[test]
    fn reads_health_names_and_timeout_and_skips_what_is_no_name() {
        let (files, timeout, unknown) = health_plan(OS_RELEASE);
        assert_eq!(
            files,
            [
                PathBuf::from(places::DEFAULT_REACHED),
                Path::new(places::HEALTH_DIR).join("network"),
            ]
        );
        assert_eq!(timeout, 30);
        assert_eq!(unknown, ["Bad_Name"]);
        assert_eq!(
            health_file("compositor"),
            Some(PathBuf::from(places::READY_FILE))
        );
        assert!(is_built_in("compositor") && !is_built_in("network"));
        let (files, timeout, _) = health_plan("NAME=x\n");
        assert!(files.is_empty());
        assert_eq!(timeout, DEFAULT_TIMEOUT);
    }

    #[test]
    fn without_any_watchdog_the_guard_restarts_the_machine_itself() {
        // A hardware watchdog, or softdog only: the watchdog does it.
        assert_eq!(on_timeout(true), Timeout::LetTheWatchdog);
        // None, as on linux-virt with no watchdog device.
        assert_eq!(on_timeout(false), Timeout::RestartItself);
    }

    #[test]
    fn takes_the_first_hardware_watchdog() {
        let w = |name: &str, identity: &str| (name.to_string(), identity.to_string());
        // A laptop: softdog first, its hardware watchdog loaded later.
        let laptop = [w("watchdog0", SOFTDOG), w("watchdog1", "iTCO_wdt")];
        assert_eq!(first_hardware(&laptop), Some(&laptop[1]));
        assert_eq!(first_hardware(&laptop[..1]), None);
        // A VM whose kernel has no softdog: i6300esb is the only one.
        let vm = [w("watchdog0", "i6300ESB timer")];
        assert_eq!(first_hardware(&vm), Some(&vm[0]));
        // A watchdog whose identity sysfs cannot give yet is not used.
        assert_eq!(
            first_hardware(&[w("watchdog0", SOFTDOG), w("watchdog1", "")]),
            None
        );
    }

    #[test]
    fn lists_watchdogs_from_sysfs() {
        let class = std::env::temp_dir().join(format!("edel-guard-class-{}", std::process::id()));
        let _ = fs::remove_dir_all(&class);
        for (name, identity) in [
            ("watchdog1", "iTCO_wdt\n"),
            ("watchdog0", "Software Watchdog\n"),
        ] {
            fs::create_dir_all(class.join(name)).unwrap();
            fs::write(class.join(name).join("identity"), identity).unwrap();
        }
        let found = watchdogs(&class);
        fs::remove_dir_all(&class).unwrap();
        assert_eq!(found[0], ("watchdog0".to_string(), SOFTDOG.to_string()));
        assert_eq!(first_hardware(&found).unwrap().1, "iTCO_wdt");
    }

    #[test]
    fn accepts_the_sentinel_files_only_not_softlevel() {
        let run = std::env::temp_dir().join(format!("edel-guard-health-{}", std::process::id()));
        fs::create_dir_all(run.join("openrc")).unwrap();
        fs::create_dir_all(run.join("edel/health")).unwrap();
        // The plan's files, under this test's directory in place of /run.
        let (files, _, _) = health_plan(OS_RELEASE);
        let files: Vec<PathBuf> = files
            .iter()
            .map(|f| run.join(f.strip_prefix("/run").unwrap()))
            .collect();
        // OpenRC says the default runlevel started; that is not health.
        fs::write(run.join("openrc/softlevel"), "default").unwrap();
        assert!(!healthy(&files));
        fs::write(run.join("edel/default-reached"), "").unwrap();
        assert!(!healthy(&files), "network is not reached yet");
        assert_eq!(
            not_reached(&[
                PathBuf::from(places::DEFAULT_REACHED),
                run.join("edel/health/network"),
                run.join("edel/health/job"),
            ]),
            "default-runlevel, network, job"
        );
        fs::write(run.join("edel/health/network"), "").unwrap();
        assert!(healthy(&files));
        fs::remove_dir_all(&run).unwrap();
    }
}
