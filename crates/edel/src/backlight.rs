//! The screen's backlight as the kernel shows it in `/sys/class/backlight`
//! (roadmap M5.9c): its level, read and set in percent. The panel's keys
//! and quick settings' brightness slider set it, and Settings' Displays
//! page and the panel will read it, so the reading is written once.
//!
//! The level is the hardware's own state, so no key of the settings file
//! holds it. A udev rule of the `udev` feature lets the people at the
//! machine, group `seat`, write it, so no daemon is needed. A machine with
//! no backlight (a desktop, a VM, a server) has none, and says so. Std only.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The directory the kernel lists the backlights in.
pub const ROOT: &str = "/sys/class/backlight";

/// One backlight: its directory's name, the directory itself, and its
/// highest and current levels.
#[derive(Debug, Clone, PartialEq)]
pub struct Backlight {
    pub name: String,
    pub dir: PathBuf,
    pub max: u32,
    pub now: u32,
}

/// The kernel's order of preference, first to last: the machine's own
/// interface (`firmware`), then `platform`, then the chip's registers
/// (`raw`), then anything else.
fn rank(kind: &str) -> u8 {
    match kind {
        "firmware" => 0,
        "platform" => 1,
        "raw" => 2,
        _ => 3,
    }
}

/// A number a file of a backlight's directory holds, if it reads and parses.
fn number(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The backlight the kernel prefers among those listed in `root`. A
/// directory whose files do not read, or whose `max_brightness` is 0, is
/// skipped; among equals the first by name is chosen.
pub fn find_in(root: &Path) -> Option<Backlight> {
    let mut found: Vec<(u8, Backlight)> = Vec::new();
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let dir = entry.path();
        let Ok(kind) = std::fs::read_to_string(dir.join("type")) else {
            continue;
        };
        let (Some(max), Some(now)) = (
            number(&dir.join("max_brightness")),
            number(&dir.join("brightness")),
        ) else {
            continue;
        };
        if max == 0 {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        found.push((
            rank(kind.trim()),
            Backlight {
                name,
                dir,
                max,
                now,
            },
        ));
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().next().map(|(_, b)| b)
}

/// The backlight of this machine, if it has one.
pub fn find() -> Option<Backlight> {
    find_in(Path::new(ROOT))
}

impl Backlight {
    /// The current level in percent, 0 to 100, rounded.
    pub fn percent(&self) -> u32 {
        if self.max == 0 {
            return 0;
        }
        let share = (u64::from(self.now) * 100 + u64::from(self.max) / 2) / u64::from(self.max);
        (share as u32).min(100)
    }

    /// Sets the level to `percent` of the highest, through the kernel's
    /// file the udev rule lets the people at the machine write.
    pub fn set_percent(&self, percent: u32) -> Result<()> {
        let path = self.dir.join("brightness");
        let level = level_for(self.max, percent);
        std::fs::write(&path, format!("{level}\n")).with_context(|| {
            format!(
                "could not set the screen's brightness through {path:?}; the people at the machine may write it once the udev feature's rule has run, so log in again"
            )
        })
    }
}

/// The level `percent` of `max` is: rounded, never above `max`, and never
/// below 1 when `max` is at least 1, so a key or a slider cannot make the
/// screen go dark.
pub fn level_for(max: u32, percent: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let level = (u64::from(percent.min(100)) * u64::from(max) + 50) / 100;
    (level as u32).clamp(1, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty directory under the temp dir, named after the test and
    /// this process.
    fn scratch(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("edel-backlight-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A backlight `name` of `kind` in `root`, at `now` of `max`.
    fn backlight(root: &Path, name: &str, kind: &str, max: u32, now: u32) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("type"), format!("{kind}\n")).unwrap();
        std::fs::write(dir.join("max_brightness"), format!("{max}\n")).unwrap();
        std::fs::write(dir.join("brightness"), format!("{now}\n")).unwrap();
    }

    #[test]
    fn finds_the_firmware_backlight_first() {
        let root = scratch("firmware-first");
        backlight(&root, "intel_backlight", "raw", 4000, 2000);
        backlight(&root, "acpi_video0", "firmware", 100, 40);
        assert_eq!(find_in(&root).unwrap().name, "acpi_video0");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_raw_backlight_alone_is_found_and_reads_its_percent() {
        let root = scratch("raw-alone");
        backlight(&root, "intel_backlight", "raw", 1200, 600);
        let found = find_in(&root).unwrap();
        assert_eq!(found.name, "intel_backlight");
        assert_eq!(found.percent(), 50);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_backlight_is_none() {
        let root = scratch("none");
        assert_eq!(find_in(&root), None, "an empty directory has none");
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(find_in(&root), None, "a missing directory has none");
    }

    #[test]
    fn a_level_never_goes_dark() {
        assert_eq!(level_for(1200, 0), 1);
        assert_eq!(level_for(1200, 100), 1200);
        assert_eq!(level_for(1200, 50), 600);
        assert_eq!(level_for(0, 50), 0);
    }

    #[test]
    fn setting_writes_the_level() {
        let root = scratch("setting");
        backlight(&root, "acpi_video0", "firmware", 400, 0);
        let found = find_in(&root).unwrap();
        found.set_percent(25).unwrap();
        let written = std::fs::read_to_string(root.join("acpi_video0/brightness")).unwrap();
        assert_eq!(written, "100\n");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
