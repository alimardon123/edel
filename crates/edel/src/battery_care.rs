//! Battery care (roadmap M7.16e): where the firmware offers a charge limit,
//! the battery stops charging at 80 percent, so it keeps more of its health
//! over the years. The kernel shows it as `charge_control_end_threshold` in
//! a battery's folder under `/sys/class/power_supply`, and
//! `charge_control_start_threshold` beside it, where the firmware has one
//! (ThinkPads through thinkpad_acpi, and the drivers that copy its names).
//! Some firmware (LG, Toshiba) accepts only 80 as an end, which is why the
//! limit is 80 and not some lower number.
//!
//! The order of writes: the start is kept below the end at every
//! step, as the kernel requires. Lowering the limit writes the start first,
//! then the end; raising it writes the end first, then the start at 0.
//! A start the firmware refuses is not an error, since some firmware takes
//! no start at all; an end it refuses is, and the caller says so.
//!
//! The limit's state is in the kernel's files, which the firmware can forget
//! when the computer sleeps, so `edel boot battery-care` sets it again after
//! every resume (`features/power/etc/zzz.d/resume`). Std only, so the desktop
//! parts can read the same battery list without `os`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::i18n::trf;

/// The kernel's power supplies, one folder each: batteries, the charger and
/// the devices with a battery of their own (a mouse's).
pub const POWER_SUPPLY: &str = "/sys/class/power_supply";

/// The file that holds a battery's charge limit: charging stops at this
/// percent.
pub const END: &str = "charge_control_end_threshold";

/// The file that holds the charge at which charging starts again, below
/// [`END`].
pub const START: &str = "charge_control_start_threshold";

/// The limit battery care sets (M7.16e): 80 percent, which keeps a battery
/// healthiest and is the only end some firmware accepts.
pub const LIMIT_PERCENT: u32 = 80;

/// The charge at which charging starts again under the limit: a few points
/// below it, so the battery is not topped up for every small loss.
pub const START_PERCENT: u32 = 75;

/// The battery folders under `dir` (`POWER_SUPPLY`, or a test's tree), in
/// name order: a supply whose `type` is `Battery` and whose `scope` is not
/// `Device`, so a mouse's battery is not the computer's.
pub fn batteries(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| read(path, "type") == "Battery" && read(path, "scope") != "Device")
        .collect();
    found.sort();
    found
}

/// Whether some battery under `dir` offers a charge limit, so battery care
/// has something to set.
pub fn offered(dir: &Path) -> bool {
    batteries(dir)
        .iter()
        .any(|battery| battery.join(END).exists())
}

/// Whether the charger is connected: a mains or USB supply reports itself
/// online.
pub fn plugged_in(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        matches!(read(&path, "type").as_str(), "Mains" | "USB") && read(&path, "online") == "1"
    })
}

/// The limit the settings want: [`LIMIT_PERCENT`] while battery care is on
/// and no full charge is pending for a trip (`full_once`), else 100.
pub fn wanted(care: bool, full_once: bool) -> u32 {
    if care && !full_once {
        LIMIT_PERCENT
    } else {
        100
    }
}

/// The limit the first battery under `dir` has now, or None when none
/// offers one.
pub fn limit_now(dir: &Path) -> Option<u32> {
    batteries(dir).iter().find_map(|battery| {
        fs::read_to_string(battery.join(END))
            .ok()
            .and_then(|text| text.trim().parse().ok())
    })
}

/// The writes that set a limit of `percent`, in order, as (file, value)
/// pairs: lowering writes the start first, raising writes the end first, so
/// the start is always below the end.
fn writes(percent: u32) -> [(&'static str, u32); 2] {
    if percent < 100 {
        [(START, START_PERCENT), (END, percent)]
    } else {
        [(END, percent), (START, 0)]
    }
}

/// Sets the charge limit to `percent` on every battery under `dir` that
/// offers one, and returns how many. A start the firmware refuses is
/// ignored; an end it refuses is an error that says to run as root.
pub fn set(dir: &Path, percent: u32) -> Result<usize, String> {
    let mut set = 0;
    for battery in batteries(dir) {
        if !battery.join(END).exists() {
            continue;
        }
        for (name, value) in writes(percent) {
            let path = battery.join(name);
            if name == START {
                if path.exists() {
                    let _ = fs::write(&path, format!("{value}\n"));
                }
                continue;
            }
            fs::write(&path, format!("{value}\n")).map_err(|err| {
                trf(
                    "could not write {path}: {err}; run as root",
                    &[
                        ("path", &path.display().to_string()),
                        ("err", &err.to_string()),
                    ],
                )
            })?;
        }
        set += 1;
    }
    Ok(set)
}

/// A file's text without its line end, or empty when it cannot be read.
fn read(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name))
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for `test` under the temp directory, named after the
    /// test and this process, which the test removes when it is done.
    fn tree(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("edel-battery-care-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A supply `name` under `dir` with the files given.
    fn supply(dir: &Path, name: &str, files: &[(&str, &str)]) {
        let folder = dir.join(name);
        fs::create_dir_all(&folder).unwrap();
        for (file, text) in files {
            fs::write(folder.join(file), text).unwrap();
        }
    }

    #[test]
    fn offered_only_where_a_battery_has_an_end_threshold() {
        let dir = tree("offered");
        assert!(!offered(&dir));
        supply(&dir, "BAT0", &[("type", "Battery\n")]);
        assert!(!offered(&dir), "a battery with no limit offers none");
        supply(&dir, "BAT1", &[("type", "Battery\n"), (END, "100\n")]);
        assert!(offered(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_device_battery_is_not_the_computers() {
        let dir = tree("device-battery");
        supply(
            &dir,
            "hidpp_battery_0",
            &[("type", "Battery\n"), ("scope", "Device\n"), (END, "100\n")],
        );
        assert!(batteries(&dir).is_empty());
        assert!(!offered(&dir), "a mouse's battery offers no limit here");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lowering_writes_the_start_first_and_raising_the_end_first() {
        assert_eq!(writes(80), [(START, 75), (END, 80)]);
        assert_eq!(writes(100), [(END, 100), (START, 0)]);
    }

    #[test]
    fn set_writes_both_thresholds_and_counts_the_batteries() {
        let dir = tree("set");
        supply(
            &dir,
            "BAT0",
            &[("type", "Battery\n"), (END, "100\n"), (START, "0\n")],
        );
        supply(&dir, "BAT1", &[("type", "Battery\n"), (END, "100\n")]);
        assert_eq!(set(&dir, LIMIT_PERCENT), Ok(2));
        for battery in ["BAT0", "BAT1"] {
            assert_eq!(read(&dir.join(battery), END), "80");
        }
        assert_eq!(read(&dir.join("BAT0"), START), "75");
        assert!(!dir.join("BAT1").join(START).exists(), "no start made");
        assert_eq!(limit_now(&dir), Some(80));
        assert_eq!(set(&dir, 100), Ok(2));
        assert_eq!(limit_now(&dir), Some(100));
        assert_eq!(read(&dir.join("BAT0"), START), "0");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refused_end_says_to_run_as_root() {
        let dir = tree("refused");
        supply(&dir, "BAT0", &[("type", "Battery\n")]);
        fs::create_dir_all(dir.join("BAT0").join(END)).unwrap();
        let err = set(&dir, LIMIT_PERCENT).unwrap_err();
        assert!(err.starts_with("could not write "), "{err}");
        assert!(err.ends_with("; run as root"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wanted_is_the_limit_only_with_care_and_no_trip_charge() {
        assert_eq!(wanted(true, false), LIMIT_PERCENT);
        assert_eq!(wanted(true, true), 100);
        assert_eq!(wanted(false, false), 100);
        assert_eq!(wanted(false, true), 100);
    }

    #[test]
    fn the_charger_is_a_mains_or_usb_supply_that_is_online() {
        let dir = tree("plugged");
        supply(&dir, "BAT0", &[("type", "Battery\n"), ("online", "1\n")]);
        assert!(!plugged_in(&dir), "a battery is not the charger");
        supply(&dir, "AC", &[("type", "Mains\n"), ("online", "0\n")]);
        assert!(!plugged_in(&dir));
        supply(
            &dir,
            "ucsi-source-psy",
            &[("type", "USB\n"), ("online", "1\n")],
        );
        assert!(plugged_in(&dir));
        let _ = fs::remove_dir_all(&dir);
    }
}
