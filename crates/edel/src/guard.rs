//! `edel boot guard`: a slot that hangs falls back too, not only one that
//! crashes (roadmap M1.5, ADR-006). The `edel-guard` service starts it in
//! the background early in boot. It opens the watchdog and pets it until
//! every health file the image names exists, then confirms the slot, stops
//! the watchdog and exits, so no daemon stays. If the files do not appear
//! in time it stops petting: the watchdog resets the machine and GRUB
//! counts the try. A machine with no watchdog at all, such as a cloud VM
//! that offers none on a kernel without softdog, is restarted by the
//! guard itself, forced, so a hung service cannot hold the restart up
//! (M1.10). A restart only helps while some slot has a try left in GRUB's
//! environment: once none has, GRUB starts a slot anyway, so the guard
//! stops the watchdog and keeps the machine running, says why on the
//! console and in `edel status`, and still confirms the slot should it
//! become healthy later (M6.17).
//!
//! Early in boot the only watchdog is often softdog, a kernel timer that a
//! frozen kernel never fires, or none at all (a kernel built without it).
//! A hardware watchdog (a laptop's iTCO_wdt or sp5100_tco, QEMU's
//! i6300esb) loads later, from hwdrivers, so the guard takes the first one
//! that appears and stops softdog. Until the slot is confirmed, the
//! kernel's lockup detectors panic and so restart it too (`softlockup_panic=1`
//! and `nmi_watchdog=panic` on the command line, `boot::WATCHDOG_ARGS`): a
//! kernel that locks up before the guard runs, or with no hardware
//! watchdog, costs a try instead of hanging for ever. Once the slot is
//! confirmed, or the guard stands down, [`calm_lockups`] turns that off,
//! so a running machine only logs a lockup, as before (M6.17), and turns
//! the NMI watchdog off: its hard-lockup detector holds a performance
//! counter of every processor, which keeps them from resting as deep, while
//! soft lockups are still logged (M7.16d, as powertop advises).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::Result;

use edel::places;

use crate::boot::TRIES;
use crate::grubenv::{Env, Slot};
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

/// What the guard waits for, from os-release.
#[derive(Clone)]
struct HealthPlan {
    /// The health files every machine waits for.
    files: Vec<PathBuf>,
    /// The desktop's, only a machine with a screen waits for
    /// (`EDEL_HEALTH_SCREEN`, M8.17): a server or a virtual machine with no
    /// graphics card starts no desktop, so nothing would ever write them.
    screen_files: Vec<PathBuf>,
    timeout: u64,
    /// Names that are no health name, skipped.
    unknown: Vec<String>,
}

/// The health files to wait for and the timeout, from os-release. A name
/// that is no name is skipped and returned.
fn health_plan(os_release: &str) -> HealthPlan {
    let names = |key| -> Vec<String> {
        os_release_value(os_release, key)
            .unwrap_or_default()
            .split([' ', ','])
            .filter(|n| !n.is_empty())
            .map(String::from)
            .collect()
    };
    let screen = names("EDEL_HEALTH_SCREEN");
    let mut plan = HealthPlan {
        files: Vec::new(),
        screen_files: Vec::new(),
        timeout: os_release_value(os_release, "EDEL_HEALTH_TIMEOUT")
            .and_then(|t| t.parse().ok())
            .unwrap_or(DEFAULT_TIMEOUT),
        unknown: Vec::new(),
    };
    for name in names("EDEL_HEALTH") {
        match health_file(&name) {
            Some(file) if screen.contains(&name) => plan.screen_files.push(file),
            Some(file) => plan.files.push(file),
            None => plan.unknown.push(name),
        }
    }
    plan
}

impl HealthPlan {
    /// This plan with the health of the add-ons mounted under `run_dir`
    /// (`places::ADDONS_RUN`), each read from the feature files it ships
    /// (M8.17c): the desktop add-on's compositor is waited for as a built-in
    /// one is, so a broken desktop falls back with its slot. The guard
    /// starts before the add-ons mount, so it asks again at every look.
    fn with_addons(&self, run_dir: &Path) -> HealthPlan {
        let mut plan = self.clone();
        let features_dir = edel::features::DIR.trim_start_matches('/');
        let Ok(mounts) = fs::read_dir(run_dir) else {
            return plan;
        };
        let mut features: Vec<edel::features::Feature> = mounts
            .flatten()
            .flat_map(|m| edel::services::installed_in(&m.path().join(features_dir)))
            .map(|(_, f)| f)
            .collect();
        // Waited for everywhere when any feature without screen lists it.
        features.sort_by_key(|f| f.screen);
        for feature in features {
            for name in &feature.health {
                let Some(file) = health_file(name) else {
                    continue;
                };
                if plan.files.contains(&file) || plan.screen_files.contains(&file) {
                    continue;
                }
                match feature.screen {
                    true => plan.screen_files.push(file),
                    false => plan.files.push(file),
                }
            }
        }
        plan
    }

    /// The files this boot waits for: the desktop's too only while the
    /// machine has a screen, asked only once the others are there, so by
    /// then the kernel has found its graphics card (udev settles in
    /// sysinit, before the default runlevel's health).
    fn wanted(&self, has_screen: impl FnOnce() -> bool) -> Vec<PathBuf> {
        let mut wanted = self.files.clone();
        if !self.screen_files.is_empty() && (!healthy(&self.files) || has_screen()) {
            wanted.extend(self.screen_files.iter().cloned());
        }
        wanted
    }
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
    let plan = health_plan(&os_release);
    let timeout = plan.timeout;
    for name in &plan.unknown {
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
    let mut start = Instant::now();
    let mut stood_down = false;
    let mut said_no_screen = false;
    loop {
        let plan = plan.with_addons(Path::new(places::ADDONS_RUN));
        let files = plan.wanted(edel::services::screen_now);
        if !said_no_screen
            && files.len() == plan.files.len()
            && healthy(&files)
            && !plan.screen_files.is_empty()
        {
            said_no_screen = true;
            println!(
                "edel guard: this machine has no screen, so the desktop does not start and the guard does not wait for it"
            );
        }
        // Once it stood down, no watchdog is taken again.
        if !stood_down {
            take_hardware(&mut watchdog, &mut on_hardware);
        }
        if let Some(dog) = watchdog.as_mut() {
            let _ = dog.write_all(b".");
        }
        // While the data's passphrase is asked (M8.2), the time is the
        // person's, not the slot's: the clock starts again once it is typed.
        if Path::new(places::UNLOCK_WAITING).exists() {
            start = Instant::now();
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
            calm_lockups(Path::new(LOCKUP_PANICS));
            if let Err(err) = fs::write(CONFIRMED, lines.join("\n") + "\n") {
                eprintln!("warning: edel guard: could not write {CONFIRMED}: {err}");
            }
            if stood_down {
                let _ = fs::remove_file(places::NO_HEALTHY_SLOT);
                println!("edel guard: this slot became healthy after all and is confirmed");
            }
            return Ok(());
        }
        if !stood_down && start.elapsed() >= Duration::from_secs(timeout) {
            eprintln!(
                "edel guard: health not reached after {timeout} s: {}",
                not_reached(&files)
            );
            let retry = match update::boot_env() {
                // With developer mode's layer on, a restart is no loop: no
                // slot has a try left, so the next start is a safe one,
                // without the layer (M7.1).
                Ok(env) => {
                    may_retry(&env)
                        || layer_is_on(
                            &fs::read_to_string(places::DEVELOPER_NOW).unwrap_or_default(),
                        )
                }
                // GRUB's block unread, its own word still counts: a safe
                // start means no slot had a try left, so a restart would
                // only loop (the review of 2026-10-25).
                Err(err)
                    if crate::data::is_safe_start(
                        &fs::read_to_string("/proc/cmdline").unwrap_or_default(),
                    ) =>
                {
                    eprintln!(
                        "warning: edel guard: could not read how many tries the slots have left, and this is a safe start, so the machine keeps running: {err:#}"
                    );
                    false
                }
                Err(err) => {
                    eprintln!(
                        "warning: edel guard: could not read how many tries the slots have left, so the machine restarts as before: {err:#}"
                    );
                    true
                }
            };
            match on_timeout(watchdog.is_some(), retry) {
                Timeout::LetTheWatchdog => {
                    eprintln!(
                        "edel guard: this version of Edel OS did not finish starting in {timeout} s; the watchdog restarts the computer, so Edel OS can go back to the version before; your files are safe"
                    );
                    // Closing without the magic byte leaves the watchdog
                    // running.
                    std::process::exit(1);
                }
                Timeout::RestartItself => {
                    eprintln!(
                        "edel guard: this version of Edel OS did not finish starting in {timeout} s; restarting the computer now, so Edel OS can go back to the version before; your files are safe"
                    );
                    restart_now();
                }
                Timeout::StandDown => {
                    if let Some(mut dog) = watchdog.take() {
                        let _ = dog.write_all(b"V");
                    }
                    calm_lockups(Path::new(LOCKUP_PANICS));
                    let missing = not_reached(&files);
                    eprintln!("edel guard: {}", stood_down_text(timeout, &missing));
                    // The facts alone, so edel status says them in the
                    // person's language; /run/edel may not exist yet when
                    // the boot hung before the default runlevel.
                    let note = format!("{timeout}\t{missing}\n");
                    let _ = fs::create_dir_all(places::RUN_DIR);
                    if let Err(err) = fs::write(places::NO_HEALTHY_SLOT, note) {
                        eprintln!(
                            "warning: edel guard: could not write {}: {err}",
                            places::NO_HEALTHY_SLOT
                        );
                    }
                    stood_down = true;
                }
            }
        }
        // Looked at ten times a second, so the slot is confirmed as soon as
        // it is healthy, not up to a second later; petting the watchdog
        // as often costs nothing. Once it stood down, every 2 s is enough.
        sleep(Duration::from_millis(if stood_down { 2000 } else { 100 }));
    }
}

/// Where the kernel keeps whether its lockup detectors panic.
const LOCKUP_PANICS: &str = "/proc/sys/kernel";

/// Turns off the lockup detectors' panic the command line turned on, in
/// `kernel` (`/proc/sys/kernel`): a running machine logs a soft lockup and
/// goes on, as before. The NMI watchdog goes off too, so the processors
/// rest deeper (M7.16d). A kernel without a detector has no file, which is
/// fine.
fn calm_lockups(kernel: &Path) {
    for name in ["softlockup_panic", "hardlockup_panic", "nmi_watchdog"] {
        let path = kernel.join(name);
        // A kernel without the detector shows its file read-only (a
        // virtual machine's nmi_watchdog, with no performance counter),
        // or not at all: nothing there to calm.
        let writable = fs::metadata(&path).is_ok_and(|m| m.permissions().mode() & 0o200 != 0);
        if writable && let Err(err) = fs::write(&path, "0\n") {
            eprintln!(
                "warning: edel guard: could not write {}: {err}",
                path.display()
            );
        }
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
    /// No slot has a try left, so a restart would start this one again:
    /// the machine keeps running (M6.17).
    StandDown,
}

fn on_timeout(has_watchdog: bool, may_retry: bool) -> Timeout {
    if !may_retry {
        Timeout::StandDown
    } else if has_watchdog {
        Timeout::LetTheWatchdog
    } else {
        Timeout::RestartItself
    }
}

/// What the guard and `edel status` say when the guard stood down: the
/// seconds it waited and the health names not reached.
pub fn stood_down_text(seconds: u64, missing: &str) -> String {
    edel::i18n::trf(
        "Edel OS did not finish starting in {seconds} s ({missing} did not start), and no version has a start left, so the computer keeps running as it is; run edel report and send the file it writes",
        &[("seconds", &seconds.to_string()), ("missing", missing)],
    )
}

/// The guard's note, `SECONDS\tNAMES`, as `edel status` says it.
pub fn stood_down_note(note: &str) -> Option<String> {
    let (seconds, missing) = note.trim_end().split_once('\t')?;
    Some(stood_down_text(seconds.parse().ok()?, missing))
}

/// Whether a restart could start a slot afresh: one that is OK and has a
/// try left, GRUB having counted this boot's already. Once none has, GRUB
/// starts a slot without counting, so restarting would only loop.
fn may_retry(env: &Env) -> bool {
    [Slot::A, Slot::B]
        .into_iter()
        .any(|slot| env.ok(slot) && env.tries(slot) < TRIES)
}

/// Whether developer mode's layer is mounted in this boot, from what
/// `places::DEVELOPER_NOW` holds (M7.1).
fn layer_is_on(now: &str) -> bool {
    now.trim() == "on"
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

    #[test]
    fn the_guard_waits_for_the_mounted_add_ons_health() {
        let run = std::env::temp_dir().join(format!("edel-guard-addons-{}", std::process::id()));
        let features = run
            .join("desktop")
            .join(edel::features::DIR.trim_start_matches('/'));
        fs::create_dir_all(&features).unwrap();
        fs::create_dir_all(run.join("printing")).unwrap();
        fs::write(
            features.join("compositor.toml"),
            "format = 1\nsummary = \"c\"\nscreen = true\nhealth = [\"compositor\"]\n",
        )
        .unwrap();
        fs::write(
            features.join("sound.toml"),
            "format = 1\nsummary = \"s\"\nhealth = [\"sound\"]\n",
        )
        .unwrap();
        let plan = health_plan("EDEL_HEALTH=\"default-runlevel\"\n").with_addons(&run);
        assert_eq!(plan.screen_files, [health_file("compositor").unwrap()]);
        assert_eq!(
            plan.files,
            [
                health_file("default-runlevel").unwrap(),
                health_file("sound").unwrap()
            ]
        );
        // Nothing mounted, nothing more.
        let none = health_plan("EDEL_HEALTH=\"default-runlevel\"\n").with_addons(&run.join("none"));
        assert_eq!(none.files.len(), 1);
        fs::remove_dir_all(&run).unwrap();
    }

    const OS_RELEASE: &str = "NAME=\"Edel OS\"\nEDEL_HEALTH=\"default-runlevel network Bad_Name\"\nEDEL_HEALTH_TIMEOUT=30\n";

    #[test]
    fn restarts_once_more_only_while_the_layer_is_on() {
        assert!(layer_is_on("on\n"));
        assert!(!layer_is_on("held\n"));
        assert!(!layer_is_on(""));
    }

    #[test]
    fn reads_health_names_and_timeout_and_skips_what_is_no_name() {
        let HealthPlan {
            files,
            timeout,
            unknown,
            ..
        } = health_plan(OS_RELEASE);
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
        let plan = health_plan("NAME=x\n");
        assert!(plan.files.is_empty() && plan.screen_files.is_empty());
        assert_eq!(plan.timeout, DEFAULT_TIMEOUT);
    }

    #[test]
    fn a_machine_without_a_screen_does_not_wait_for_the_desktop() {
        let dir = std::env::temp_dir().join(format!("edel-guard-screen-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let reached = dir.join("default-reached");
        let ready = dir.join("ready");
        let plan = HealthPlan {
            files: vec![reached.clone()],
            screen_files: vec![ready.clone()],
            timeout: 30,
            unknown: Vec::new(),
        };
        // Before the default runlevel the screen is not asked about.
        assert_eq!(
            plan.wanted(|| panic!("asked too early")),
            [reached.clone(), ready.clone()]
        );
        fs::write(&reached, "").unwrap();
        assert_eq!(plan.wanted(|| true), [reached.clone(), ready.clone()]);
        assert_eq!(plan.wanted(|| false), vec![reached.clone()]);
        let split = health_plan(
            "EDEL_HEALTH=\"default-runlevel compositor\"\nEDEL_HEALTH_SCREEN=\"compositor\"\n",
        );
        assert_eq!(split.files, [PathBuf::from(places::DEFAULT_REACHED)]);
        assert_eq!(split.screen_files, [PathBuf::from(places::READY_FILE)]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_any_watchdog_the_guard_restarts_the_machine_itself() {
        // A hardware watchdog, or softdog only: the watchdog does it.
        assert_eq!(on_timeout(true, true), Timeout::LetTheWatchdog);
        // None, as on linux-virt with no watchdog device.
        assert_eq!(on_timeout(false, true), Timeout::RestartItself);
        // No slot has a try left: nothing restarts, watchdog or not.
        assert_eq!(on_timeout(true, false), Timeout::StandDown);
        assert_eq!(on_timeout(false, false), Timeout::StandDown);
    }

    #[test]
    fn the_note_is_said_again_from_its_facts() {
        assert_eq!(
            stood_down_note("120\tcompositor\n"),
            Some(stood_down_text(120, "compositor"))
        );
        assert!(stood_down_text(120, "compositor").contains("in 120 s (compositor did not start)"));
        assert_eq!(stood_down_note("garbled"), None);
    }

    #[test]
    fn restarts_only_while_some_slot_has_a_try_left() {
        let env = |text: &str| Env::parse(text).unwrap();
        // A stick's third failed boot: slot A used its tries, B is empty.
        assert!(!may_retry(&env(
            "ORDER=A B\nA_OK=1\nA_TRY=3\nB_OK=0\nB_TRY=0\n"
        )));
        // Its first: A may try again.
        assert!(may_retry(&env(
            "ORDER=A B\nA_OK=1\nA_TRY=1\nB_OK=0\nB_TRY=0\n"
        )));
        // An update in B failing: A, the slot to fall back to, has tries.
        assert!(may_retry(&env(
            "ORDER=B A\nA_OK=1\nA_TRY=0\nB_OK=1\nB_TRY=3\n"
        )));
        // Both used their tries, the boot GRUB started anyway.
        assert!(!may_retry(&env(
            "ORDER=B A\nA_OK=1\nA_TRY=3\nB_OK=1\nB_TRY=3\n"
        )));
    }

    #[test]
    fn lockups_stop_panicking_once_the_guard_is_done() {
        let dir = std::env::temp_dir().join(format!("edel-lockups-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("softlockup_panic"), "1\n").unwrap();
        fs::write(dir.join("nmi_watchdog"), "1\n").unwrap();
        // No hardlockup_panic: a kernel without that detector.
        calm_lockups(&dir);
        for name in ["softlockup_panic", "nmi_watchdog"] {
            assert_eq!(fs::read_to_string(dir.join(name)).unwrap(), "0\n");
        }
        assert!(!dir.join("hardlockup_panic").exists());
        // Read-only, as a virtual machine shows nmi_watchdog: left alone.
        fs::write(dir.join("nmi_watchdog"), "1\n").unwrap();
        fs::set_permissions(dir.join("nmi_watchdog"), fs::Permissions::from_mode(0o444)).unwrap();
        calm_lockups(&dir);
        assert_eq!(fs::read_to_string(dir.join("nmi_watchdog")).unwrap(), "1\n");
        fs::remove_dir_all(&dir).unwrap();
        assert!(crate::boot::WATCHDOG_ARGS.contains("softlockup_panic=1 nmi_watchdog=panic"));
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
        let files = health_plan(OS_RELEASE).files;
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
