//! `edel boot guard`: a slot that hangs falls back too, not only one that
//! crashes (roadmap M1.5, ADR-006). The `edel-guard` service starts it in
//! the background early in boot. It opens the hardware watchdog and pets it
//! until every health file the image names exists, then confirms the slot,
//! stops the watchdog and exits, so no daemon stays. If the files do not
//! appear in time it stops petting: the watchdog resets the machine and
//! GRUB counts the try.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::update;

/// Health names an image may list in `[image] health`, and the file under
/// `/run` that says each one is reached. OpenRC's own `softlevel` is never
/// used: OpenRC writes it when a runlevel starts, while services may still
/// hang.
const SENTINELS: [(&str, &str); 2] = [
    ("default-runlevel", "edel/default-reached"),
    ("compositor", "edel/ready"),
];
/// How long a slot may take to become healthy when the image says nothing.
pub const DEFAULT_TIMEOUT: u64 = 120;
const CONFIRMED: &str = "/run/edel/confirmed";

/// Whether `name` is a health name edel knows.
pub fn is_health_name(name: &str) -> bool {
    SENTINELS.iter().any(|(n, _)| *n == name)
}

/// The value of `key` in an os-release file, without quotes.
fn os_release_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// The health files to wait for and the timeout, from os-release; `run` is
/// `/run`, or a test directory. Unknown names are skipped and returned, so
/// a newer image's name never stops an older guard.
fn health_plan(os_release: &str, run: &Path) -> (Vec<PathBuf>, u64, Vec<String>) {
    let mut files = Vec::new();
    let mut unknown = Vec::new();
    for name in os_release_value(os_release, "EDEL_HEALTH")
        .unwrap_or_default()
        .split([' ', ','])
        .filter(|n| !n.is_empty())
    {
        match SENTINELS.iter().find(|(n, _)| *n == name) {
            Some((_, file)) => files.push(run.join(file)),
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

/// The watchdog device, loading softdog when the hardware has none.
fn open_watchdog() -> Option<File> {
    let open = || OpenOptions::new().write(true).open("/dev/watchdog").ok();
    open().or_else(|| {
        let _ = Command::new("modprobe").arg("softdog").status();
        sleep(Duration::from_millis(500));
        open()
    })
}

/// Waits for the slot to become healthy, then confirms it.
pub fn guard() -> Result<()> {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let (files, timeout, unknown) = health_plan(&os_release, Path::new("/run"));
    for name in unknown {
        eprintln!("warning: edel guard: unknown health name {name:?} skipped");
    }
    let mut watchdog = open_watchdog();
    if watchdog.is_none() {
        eprintln!("warning: edel guard: no watchdog, so a hang will not fall back");
    }
    let start = Instant::now();
    loop {
        if let Some(dog) = watchdog.as_mut() {
            let _ = dog.write_all(b".");
        }
        if healthy(&files) {
            // A healthy machine is never reset, even when confirming fails.
            let lines = match update::confirm_running() {
                Ok(lines) => lines,
                Err(err) => vec![format!("edel update: could not confirm this slot: {err:#}")],
            };
            fs::write(CONFIRMED, lines.join("\n") + "\n")?;
            if let Some(mut dog) = watchdog.take() {
                // The magic close: stop the watchdog instead of resetting.
                let _ = dog.write_all(b"V");
            }
            return Ok(());
        }
        if start.elapsed() >= Duration::from_secs(timeout) {
            eprintln!(
                "edel guard: this slot is not healthy after {timeout} s; letting the watchdog restart the machine"
            );
            // Closing without the magic byte leaves the watchdog running.
            std::process::exit(1);
        }
        sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OS_RELEASE: &str =
        "NAME=\"Edel OS\"\nEDEL_HEALTH=\"default-runlevel newer-thing\"\nEDEL_HEALTH_TIMEOUT=30\n";

    #[test]
    fn reads_health_names_and_timeout_and_skips_unknown_ones() {
        let (files, timeout, unknown) = health_plan(OS_RELEASE, Path::new("/run"));
        assert_eq!(files, [PathBuf::from("/run/edel/default-reached")]);
        assert_eq!(timeout, 30);
        assert_eq!(unknown, ["newer-thing"]);
        let (files, timeout, _) = health_plan("NAME=x\n", Path::new("/run"));
        assert!(files.is_empty());
        assert_eq!(timeout, DEFAULT_TIMEOUT);
    }

    #[test]
    fn accepts_the_sentinel_files_only_not_softlevel() {
        let run = std::env::temp_dir().join(format!("edel-guard-health-{}", std::process::id()));
        fs::create_dir_all(run.join("openrc")).unwrap();
        fs::create_dir_all(run.join("edel")).unwrap();
        let (files, _, _) = health_plan(OS_RELEASE, &run);
        // OpenRC says the default runlevel started; that is not health.
        fs::write(run.join("openrc/softlevel"), "default").unwrap();
        assert!(!healthy(&files));
        fs::write(run.join("edel/default-reached"), "").unwrap();
        assert!(healthy(&files));
        fs::remove_dir_all(&run).unwrap();
    }
}
