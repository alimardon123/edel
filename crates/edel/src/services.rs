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
//! graphics card ([`has_screen`]), its default runlevel's services are
//! taken out at boot (those of the boot runlevel only make folders or
//! return, and OpenRC has listed them already), so one image runs a
//! desktop and a server alike, and they come back at the next boot with a
//! screen. A switchable one still starts where the file says
//! `services.NAME = true`. The boot decides once ([`decide_screen`]) and
//! every later reader follows that answer ([`screen_now`]). The image's
//! `headless` features (ssh) are on by default without a screen, as
//! nobody sits at such a machine (`EDEL_SERVICES_HEADLESS`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::features::{self, Feature};
use crate::i18n::trf;
use crate::places;

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

/// The names os-release's line `KEY"a b"` lists.
fn listed(os_release: &str, key: &str) -> Vec<String> {
    os_release
        .lines()
        .find_map(|l| l.strip_prefix(key))
        .map(|v| {
            v.trim_matches('"')
                .split_whitespace()
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// The graphics drivers of servers' management chips (BMCs): their card
/// only mirrors the console to a remote viewer, so it is no screen anyone
/// sits at (M7.4).
const BMC_DRIVERS: [&str; 2] = ["ast", "mgag200"];

/// Whether this machine has a screen: the kernel shows a graphics card
/// under `sys` (`/sys`), `class/drm/cardN`, whose driver is not a BMC's
/// ([`BMC_DRIVERS`]). A server or a virtual machine with no display device
/// has none; the firmware's framebuffer counts, so a machine whose
/// graphics driver fails still has its screen.
pub fn has_screen(sys: &Path) -> bool {
    fs::read_dir(sys.join("class/drm")).is_ok_and(|entries| {
        entries.filter_map(|e| e.ok()).any(|e| {
            let card = e
                .file_name()
                .to_str()
                .and_then(|n| n.strip_prefix("card"))
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            let bmc = fs::read_link(e.path().join("device/driver")).is_ok_and(|driver| {
                driver
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| BMC_DRIVERS.contains(&n))
            });
            card && !bmc
        })
    })
}

/// This boot's answer to whether the machine has a screen: the one the
/// boot's apply wrote ([`decide_screen`]), else the kernel's now.
pub fn screen_now() -> bool {
    match fs::read_to_string(places::SCREEN).as_deref().map(str::trim) {
        Ok("yes") => true,
        Ok("no") => false,
        _ => has_screen(Path::new("/sys")),
    }
}

/// Decides at boot whether the machine has a screen, after udev has
/// settled, and writes it to [`places::SCREEN`] for the rest of the boot.
pub fn decide_screen() -> bool {
    let screen = has_screen(Path::new("/sys"));
    let written = fs::create_dir_all(places::RUN_DIR)
        .and_then(|()| fs::write(places::SCREEN, if screen { "yes\n" } else { "no\n" }));
    if let Err(err) = written {
        eprintln!("warning: could not write {}: {err}", places::SCREEN);
    }
    screen
}

/// What the image chose for its switchable features (os-release).
#[derive(Clone, Debug, Default)]
pub struct Defaults {
    /// Shipped off (`EDEL_SERVICES_OFF`).
    pub off: Vec<String>,
    /// On by default on a machine with no screen (`EDEL_SERVICES_HEADLESS`).
    pub headless: Vec<String>,
}

impl Defaults {
    /// The image's choices from os-release's text.
    pub fn read(os_release: &str) -> Defaults {
        Defaults {
            off: listed(os_release, "EDEL_SERVICES_OFF="),
            headless: listed(os_release, "EDEL_SERVICES_HEADLESS="),
        }
    }
}

/// Whether feature `name` is on: the file's choice for a switchable one,
/// else off for a `screen` feature on a machine without one, else, for a
/// switchable one, on without a screen when the image lists it headless,
/// and on unless the image left it off.
pub fn is_on(
    name: &str,
    feature: &Feature,
    wanted: &BTreeMap<String, bool>,
    image: &Defaults,
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
    let named = |list: &[String]| list.iter().any(|o| o == name);
    !feature.switchable || (!screen && named(&image.headless)) || !named(&image.off)
}

/// `wanted` with each switchable feature that names a key (`while_set`,
/// M7.5) and is not named there on while `file` sets that key to
/// anything but `off` or `false`.
pub fn with_keys(
    wanted: &BTreeMap<String, bool>,
    installed: &[(String, Feature)],
    file: &crate::settings::SettingsFile,
) -> BTreeMap<String, bool> {
    let doc = toml::Value::try_from(file).ok();
    let set = |key: &str| {
        doc.as_ref()
            .and_then(|doc| key.split('.').try_fold(doc, |v, part| v.get(part)))
            .is_some_and(|v| v.as_str() != Some("off") && v.as_bool() != Some(false))
    };
    let mut wanted = wanted.clone();
    for (name, feature) in installed {
        if let (true, Some(key)) = (feature.switchable, &feature.while_set) {
            wanted.entry(name.clone()).or_insert_with(|| set(key));
        }
    }
    wanted
}

/// The steps that make the runlevels `enabled` match `wanted`: a feature
/// that is on has each of its services in its runlevel; one that is off
/// has taken out those of its services that no feature that is on lists.
/// Only the services of switchable and `screen` features are ever touched;
/// `screen` says whether the machine has one ([`has_screen`]).
pub fn plan(
    installed: &[(String, Feature)],
    wanted: &BTreeMap<String, bool>,
    image: &Defaults,
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
        .filter(|(n, f)| is_on(n, f, wanted, image, screen))
        .flat_map(|(_, f)| entries(f))
        .collect();
    let mut steps = Vec::new();
    for (name, feature) in installed.iter().filter(|(_, f)| f.switchable || f.screen) {
        let on = is_on(name, feature, wanted, image, screen);
        // A screen feature's earlier runlevels run everywhere (see above).
        let touched = entries(feature)
            .into_iter()
            .filter(|(level, _)| feature.switchable || level == "default");
        for entry in touched {
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
            plan(&machine(), &wanted, &Defaults::default(), true, &enabled),
            [Step {
                level: "default".into(),
                service: "bluetooth".into(),
                on: false
            }]
        );
    }

    #[test]
    fn an_absent_key_is_the_images_choice() {
        let off = Defaults {
            off: vec!["ssh".to_string()],
            ..Default::default()
        };
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
            Defaults::read("NAME=Edel\nEDEL_SERVICES_OFF=\"ssh bluetooth\"\n").off,
            ["ssh", "bluetooth"]
        );
        assert!(Defaults::read("NAME=Edel\n").off.is_empty());
    }

    #[test]
    fn a_machine_without_a_screen_starts_no_desktop_service() {
        let mut login = feature(false, &["greetd"]);
        login.screen = true;
        login.services.boot = vec!["edel-live".to_string()];
        let mut bluetooth = feature(true, &["dbus", "bluetooth"]);
        bluetooth.screen = true;
        let mut installed = machine();
        installed[0].1 = bluetooth;
        installed.push(("login".into(), login));
        let enabled: BTreeSet<Entry> = ["dbus", "bluetooth", "networkmanager", "greetd"]
            .into_iter()
            .map(entry)
            .collect();
        let off = Defaults {
            off: vec!["ssh".to_string()],
            headless: vec!["ssh".to_string()],
        };
        let step = |service: &str, on| Step {
            level: "default".into(),
            service: service.into(),
            on,
        };
        // No screen: the login screen and bluetooth go, dbus stays for
        // network, edel-live (boot) is left alone and ssh comes on.
        assert_eq!(
            plan(&installed, &BTreeMap::new(), &off, false, &enabled),
            [
                step("bluetooth", false),
                step("sshd", true),
                step("greetd", false)
            ]
        );
        // A screen again: they come back and ssh goes off.
        let headless: BTreeSet<Entry> = ["dbus", "networkmanager", "sshd"]
            .into_iter()
            .map(entry)
            .collect();
        assert_eq!(
            plan(&installed, &BTreeMap::new(), &off, true, &headless),
            [
                step("bluetooth", true),
                step("sshd", false),
                step("greetd", true)
            ]
        );
        // The file's word wins over the screen for a switchable feature.
        let wanted = BTreeMap::from([("bluetooth".to_string(), true), ("ssh".to_string(), false)]);
        assert_eq!(
            plan(&installed, &wanted, &off, false, &enabled),
            [step("greetd", false)]
        );
    }

    #[test]
    fn a_feature_that_follows_a_key_runs_only_while_the_key_asks() {
        let updates = Feature {
            while_set: Some("updates.automatic".into()),
            ..feature(true, &["crond"])
        };
        let installed = vec![("updates".to_string(), updates)];
        let file = |text: &str| crate::settings::read(text).unwrap().file;
        let none = BTreeMap::new();
        let on = |text: &str| {
            with_keys(&none, &installed, &file(text))
                .get("updates")
                .copied()
        };
        assert_eq!(on("format = 1\n"), Some(false));
        assert_eq!(
            on("format = 1\n[updates]\nautomatic = \"off\"\n"),
            Some(false)
        );
        assert_eq!(
            on("format = 1\n[updates]\nautomatic = \"install\"\n"),
            Some(true)
        );
        // services.NAME still wins.
        let named = BTreeMap::from([("updates".to_string(), false)]);
        let text = "format = 1\n[updates]\nautomatic = \"install\"\n";
        assert_eq!(
            with_keys(&named, &installed, &file(text)).get("updates"),
            Some(&false)
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
        // A server's management chip mirrors its console, no screen.
        let drivers = sys.join("bus/pci/drivers");
        fs::create_dir_all(drivers.join("ast")).unwrap();
        fs::create_dir_all(drivers.join("i915")).unwrap();
        fs::create_dir_all(drm.join("card0/device")).unwrap();
        std::os::unix::fs::symlink(drivers.join("ast"), drm.join("card0/device/driver")).unwrap();
        assert!(!has_screen(&sys));
        fs::create_dir_all(drm.join("card1/device")).unwrap();
        std::os::unix::fs::symlink(drivers.join("i915"), drm.join("card1/device/driver")).unwrap();
        assert!(has_screen(&sys));
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
