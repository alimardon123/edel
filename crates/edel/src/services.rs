//! `[services]` (roadmap M6.10): the features marked `switchable` (ssh,
//! bluetooth) turned on or off by name, `services.NAME = false`, never by
//! a daemon's name, so swapping a feature's daemon never breaks a file
//! (ADR-008). A feature switched off takes out only the services no other
//! feature that is on still lists: bluetooth off keeps dbus while network
//! lists it. An absent key is the image's choice, on unless the image
//! definition lists the feature in `off` (`EDEL_SERVICES_OFF`).
//!
//! A feature marked `screen` (M8.17: the login screen, the seat, the
//! battery's daemon) is the desktop's: on a machine whose kernel shows no
//! graphics card ([`has_screen`]), its services are taken out at boot, so
//! one image runs a desktop and a server alike, and come back at the next
//! boot with a screen. A switchable one still starts where the file says
//! `services.NAME = true`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::features::{self, Feature};
use crate::i18n::trf;

/// One service in one runlevel, as OpenRC keeps it
/// (`/etc/runlevels/LEVEL/SERVICE`).
pub type Entry = (String, String);

/// One change to a runlevel: `service` added to `level` (`on`) or taken
/// out of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub level: String,
    pub service: String,
    pub on: bool,
}

/// The features installed on this machine (`features::DIR`), each by name,
/// read leniently; none when the folder is not there (a build host).
pub fn installed() -> Vec<(String, Feature)> {
    installed_in(Path::new(features::DIR))
}

/// [`installed`] from the folder `dir`.
pub fn installed_in(dir: &Path) -> Vec<(String, Feature)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, Feature)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_stem()?.to_str()?.to_string();
            if path.extension()? != "toml" {
                return None;
            }
            let text = fs::read_to_string(&path).ok()?;
            Some((name, features::read(&text).ok()?.0))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// The names of the features that may be switched off.
pub fn switchable(installed: &[(String, Feature)]) -> Vec<String> {
    installed
        .iter()
        .filter(|(_, f)| f.switchable)
        .map(|(n, _)| n.clone())
        .collect()
}

/// What is wrong with `wanted` on a machine with `installed`: a name that
/// is no switchable feature, one line each, naming those that are.
pub fn problems(wanted: &BTreeMap<String, bool>, installed: &[(String, Feature)]) -> Vec<String> {
    let names = switchable(installed);
    let listed = if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    };
    wanted
        .keys()
        .filter(|name| !names.contains(name))
        .map(|name| {
            trf(
                "services.{name}: {name} is not a feature that can be switched on or off; the ones this machine has are {listed}",
                &[("name", name), ("listed", &listed)],
            )
        })
        .collect()
}

/// The features the image definition left off, from os-release's
/// `EDEL_SERVICES_OFF="ssh ..."`.
pub fn off_by_default(os_release: &str) -> Vec<String> {
    os_release
        .lines()
        .find_map(|l| l.strip_prefix("EDEL_SERVICES_OFF="))
        .map(|v| {
            v.trim_matches('"')
                .split_whitespace()
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether this machine has a screen: the kernel shows a graphics card
/// under `sys` (`/sys`), `class/drm/cardN`. A server or a virtual machine
/// with no display device has none; the firmware's framebuffer counts, so
/// a machine whose graphics driver fails still has its screen.
pub fn has_screen(sys: &Path) -> bool {
    fs::read_dir(sys.join("class/drm")).is_ok_and(|entries| {
        entries.filter_map(|e| e.ok()).any(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_prefix("card"))
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
    })
}

/// Whether feature `name` is on: the file's choice for a switchable one,
/// else off for a `screen` feature on a machine without one, else on
/// unless the image left it off.
pub fn is_on(
    name: &str,
    feature: &Feature,
    wanted: &BTreeMap<String, bool>,
    off: &[String],
    screen: bool,
) -> bool {
    if feature.switchable {
        if let Some(&on) = wanted.get(name) {
            return on;
        }
    }
    if feature.screen && !screen {
        return false;
    }
    !feature.switchable || !off.iter().any(|o| o == name)
}

/// The steps that make the runlevels `enabled` match `wanted`: a feature
/// that is on has each of its services in its runlevel; one that is off
/// has taken out those of its services that no feature that is on lists.
/// Only the services of switchable and `screen` features are ever touched;
/// `screen` says whether the machine has one ([`has_screen`]).
pub fn plan(
    installed: &[(String, Feature)],
    wanted: &BTreeMap<String, bool>,
    off: &[String],
    screen: bool,
    enabled: &BTreeSet<Entry>,
) -> Vec<Step> {
    let entries = |f: &Feature| -> Vec<Entry> {
        f.services
            .entries()
            .into_iter()
            .map(|(l, s)| (l.to_string(), s.to_string()))
            .collect()
    };
    let kept: BTreeSet<Entry> = installed
        .iter()
        .filter(|(n, f)| is_on(n, f, wanted, off, screen))
        .flat_map(|(_, f)| entries(f))
        .collect();
    let mut steps = Vec::new();
    for (name, feature) in installed.iter().filter(|(_, f)| f.switchable || f.screen) {
        let on = is_on(name, feature, wanted, off, screen);
        for entry in entries(feature) {
            let here = enabled.contains(&entry);
            let step = if on && !here {
                Some(true)
            } else if !on && here && !kept.contains(&entry) {
                Some(false)
            } else {
                None
            };
            if let Some(on) = step {
                let step = Step {
                    level: entry.0,
                    service: entry.1,
                    on,
                };
                if !steps.contains(&step) {
                    steps.push(step);
                }
            }
        }
    }
    steps
}

/// The services in the runlevels under `runlevels` (`/etc/runlevels`), as
/// (level, service) pairs.
pub fn enabled_in(runlevels: &Path) -> BTreeSet<Entry> {
    let mut found = BTreeSet::new();
    for level in ["sysinit", "boot", "default", "shutdown"] {
        if let Ok(entries) = fs::read_dir(runlevels.join(level)) {
            for e in entries.filter_map(|e| e.ok()) {
                found.insert((
                    level.to_string(),
                    e.file_name().to_string_lossy().into_owned(),
                ));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(switchable: bool, default: &[&str]) -> Feature {
        Feature {
            switchable,
            services: features::Services {
                default: default.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn machine() -> Vec<(String, Feature)> {
        vec![
            ("bluetooth".into(), feature(true, &["dbus", "bluetooth"])),
            (
                "network".into(),
                feature(false, &["dbus", "networkmanager"]),
            ),
            ("ssh".into(), feature(true, &["sshd"])),
        ]
    }

    fn entry(s: &str) -> Entry {
        ("default".into(), s.into())
    }

    #[test]
    fn switching_bluetooth_off_keeps_dbus_while_network_lists_it() {
        let enabled: BTreeSet<Entry> = ["dbus", "bluetooth", "networkmanager", "sshd"]
            .into_iter()
            .map(entry)
            .collect();
        let wanted = BTreeMap::from([("bluetooth".to_string(), false)]);
        assert_eq!(
            plan(&machine(), &wanted, &[], true, &enabled),
            [Step {
                level: "default".into(),
                service: "bluetooth".into(),
                on: false
            }]
        );
    }

    #[test]
    fn an_absent_key_is_the_images_choice() {
        let off = vec!["ssh".to_string()];
        let enabled: BTreeSet<Entry> = ["dbus", "bluetooth", "networkmanager"]
            .into_iter()
            .map(entry)
            .collect();
        assert!(plan(&machine(), &BTreeMap::new(), &off, true, &enabled).is_empty());
        let wanted = BTreeMap::from([("ssh".to_string(), true)]);
        assert_eq!(
            plan(&machine(), &wanted, &off, true, &enabled),
            [Step {
                level: "default".into(),
                service: "sshd".into(),
                on: true
            }]
        );
        assert_eq!(
            off_by_default("NAME=Edel\nEDEL_SERVICES_OFF=\"ssh bluetooth\"\n"),
            ["ssh", "bluetooth"]
        );
        assert!(off_by_default("NAME=Edel\n").is_empty());
    }

    #[test]
    fn a_machine_without_a_screen_starts_no_desktop_service() {
        let mut login = feature(false, &["greetd"]);
        login.screen = true;
        let mut bluetooth = feature(true, &["dbus", "bluetooth"]);
        bluetooth.screen = true;
        let mut installed = machine();
        installed[0].1 = bluetooth;
        installed.push(("login".into(), login));
        let enabled: BTreeSet<Entry> = ["dbus", "bluetooth", "networkmanager", "greetd"]
            .into_iter()
            .map(entry)
            .collect();
        let off = vec!["ssh".to_string()];
        let step = |service: &str, on| Step {
            level: "default".into(),
            service: service.into(),
            on,
        };
        // No screen: the login screen and bluetooth go, dbus stays for network.
        assert_eq!(
            plan(&installed, &BTreeMap::new(), &off, false, &enabled),
            [step("bluetooth", false), step("greetd", false)]
        );
        // A screen again: they come back.
        let headless: BTreeSet<Entry> = ["dbus", "networkmanager"].into_iter().map(entry).collect();
        assert_eq!(
            plan(&installed, &BTreeMap::new(), &off, true, &headless),
            [step("bluetooth", true), step("greetd", true)]
        );
        // The file's word wins over the screen for a switchable feature.
        let wanted = BTreeMap::from([("bluetooth".to_string(), true)]);
        assert_eq!(
            plan(&installed, &wanted, &off, false, &enabled),
            [step("greetd", false)]
        );
    }

    #[test]
    fn a_screen_is_a_graphics_card_the_kernel_shows() {
        let sys = std::env::temp_dir().join(format!("edel-screen-{}", std::process::id()));
        let drm = sys.join("class/drm");
        fs::create_dir_all(drm.join("renderD128")).unwrap();
        fs::create_dir_all(drm.join("card0-Virtual-1")).unwrap();
        assert!(!has_screen(&sys));
        fs::create_dir_all(drm.join("card0")).unwrap();
        assert!(has_screen(&sys));
        assert!(!has_screen(&sys.join("nothing")));
        fs::remove_dir_all(&sys).unwrap();
    }

    #[test]
    fn refuses_a_feature_that_cannot_be_switched() {
        let wanted = BTreeMap::from([("udev".to_string(), false), ("ssh".to_string(), false)]);
        let problems = problems(&wanted, &machine());
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].contains("udev") && problems[0].contains("bluetooth, ssh"),
            "{}",
            problems[0]
        );
    }

    #[test]
    fn reads_the_runlevels() {
        let dir = std::env::temp_dir().join(format!("edel-runlevels-{}", std::process::id()));
        fs::create_dir_all(dir.join("default")).unwrap();
        fs::write(dir.join("default/sshd"), "").unwrap();
        assert_eq!(enabled_in(&dir), BTreeSet::from([entry("sshd")]));
        fs::remove_dir_all(&dir).unwrap();
    }
}
