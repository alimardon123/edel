//! Where each add-on of this machine stands (M7.2b): the one reading that
//! `edel addon list` and Settings' Add-ons page show, so both say the same
//! (ADR-010). An add-on is offered when the image was built with it
//! (`places::OFFERED_ADDONS_DIR`, read without the network), and stands
//! as one of [`State`], from the files the tool leaves on the data
//! partition: the version's add-ons folder, its pending folder and its
//! failed list, and what is mounted now.

use std::fs;
use std::path::{Path, PathBuf};

use crate::features::Feature;
use crate::hardware::{self, Device, Pack};
use crate::i18n::n_;
use crate::places;
use crate::services;

/// Where one add-on stands on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Mounted now.
    Installed,
    /// Built into the slot, so always there.
    BuiltIn,
    /// Fetched, and tried at the next restart.
    Pending,
    /// Chosen, and its first boot did not finish.
    Failed,
    /// Chosen, and not fetched yet (no network, or the update brings it).
    Waiting,
    /// Here, and not chosen any more: it goes at the next restart.
    Leaving,
    /// Not chosen.
    Off,
}

impl State {
    /// The word `edel addon list` shows, the same in every language's
    /// columns: it is what a script reads.
    pub fn word(self) -> &'static str {
        match self {
            State::Installed => "installed",
            State::BuiltIn => "built-in",
            State::Pending => "pending",
            State::Failed => "failed",
            State::Waiting => "chosen",
            State::Leaving => "leaving",
            State::Off => "",
        }
    }

    /// What it means for the person, in one line (`n_` marked, to be
    /// shown with `tr`).
    pub fn line(self) -> &'static str {
        match self {
            State::Installed => n_("Installed"),
            State::BuiltIn => n_("Built into this version of Edel OS"),
            State::Pending => n_("Downloaded; it starts at the next restart"),
            State::Failed => n_(
                "It did not start, so Edel OS started without it; turn it off and on to try again",
            ),
            State::Waiting => {
                n_("Chosen; it downloads once this computer is online, or with the next update")
            }
            State::Leaving => n_("Goes at the next restart"),
            State::Off => n_("Not installed"),
        }
    }

    /// Whether the add-on is chosen in this state, which is what a switch
    /// shows.
    pub fn chosen(self) -> bool {
        matches!(
            self,
            State::Installed | State::BuiltIn | State::Pending | State::Failed | State::Waiting
        )
    }
}

/// What the machine's files say, read once ([`Facts::read`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Facts {
    /// `addons.installed` of the machine's settings file.
    pub wanted: Vec<String>,
    /// Mounted now.
    pub mounted: Vec<String>,
    /// In the slot as features of their own (not mounted).
    pub built_in: Vec<String>,
    /// In the version's pending folder.
    pub pending: Vec<String>,
    /// In the version's failed list.
    pub failed: Vec<String>,
}

/// Where `name` stands by `facts`.
pub fn state(name: &str, facts: &Facts) -> State {
    let has = |list: &[String]| list.iter().any(|n| n == name);
    let wanted = has(&facts.wanted);
    if has(&facts.built_in) {
        State::BuiltIn
    } else if has(&facts.mounted) {
        if wanted {
            State::Installed
        } else {
            State::Leaving
        }
    } else if !wanted {
        State::Off
    } else if has(&facts.failed) {
        State::Failed
    } else if has(&facts.pending) {
        State::Pending
    } else {
        State::Waiting
    }
}

/// The add-ons the image was built with, by name and summary, in name
/// order; none when the image has none.
pub fn offered() -> Vec<(String, String)> {
    offered_in(Path::new(places::OFFERED_ADDONS_DIR))
}

/// [`offered`] from the folder `dir`.
pub fn offered_in(dir: &Path) -> Vec<(String, String)> {
    offered_features_in(dir)
        .into_iter()
        .map(|(name, feature)| (name, feature.summary))
        .collect()
}

/// The feature files of the add-ons the image was built with, read from
/// the folder `dir` (`places::OFFERED_ADDONS_DIR`).
pub fn offered_features_in(dir: &Path) -> Vec<(String, Feature)> {
    services::installed_in(dir)
}

/// The hardware packs among `offered` that serve `devices` and that
/// `state` does not show as chosen (M7.11), in the order `offered` lists
/// them: what the packs file holds and `edel status` and Settings name.
pub fn unchosen_packs(
    offered: &[(String, Feature)],
    devices: &[Device],
    state: impl Fn(&str) -> State,
) -> Vec<Pack> {
    offered
        .iter()
        .filter(|(name, feature)| {
            !feature.hardware.is_empty()
                && hardware::serves(&feature.hardware, devices)
                && !state(name).chosen()
        })
        .map(|(name, feature)| Pack {
            name: name.clone(),
            summary: feature.summary.clone(),
        })
        .collect()
}

/// The add-ons folder of the running version (`VERSION_ID`).
pub fn running_dir() -> PathBuf {
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let version = os_release
        .lines()
        .find_map(|line| line.strip_prefix("VERSION_ID="))
        .unwrap_or_default()
        .trim_matches('"');
    Path::new(places::ADDONS_DIR).join(version)
}

/// The add-ons in `dir`, by the names of their `.ext4` files, sorted.
pub fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter_map(|n| n.strip_suffix(".ext4").map(String::from))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The failed list in a version's add-ons folder.
pub fn failed_in(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join(places::ADDONS_FAILED))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// The add-ons mounted now, by name, sorted: a folder in
/// `places::ADDONS_RUN` with a mount on it.
pub fn mounted() -> Vec<String> {
    let mounts = fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    let mut names: Vec<String> = mounts
        .lines()
        .filter_map(|line| line.split(' ').nth(1))
        .filter_map(|at| at.strip_prefix(places::ADDONS_RUN))
        .filter_map(|rest| rest.strip_prefix('/'))
        .filter(|name| !name.is_empty() && !name.contains('/'))
        .map(String::from)
        .collect();
    names.sort();
    names.dedup();
    names
}

impl Facts {
    /// Reads them from this machine; each part is empty where its file is
    /// missing.
    pub fn read() -> Facts {
        let dir = running_dir();
        let path = places::found(&places::machine_settings());
        let wanted = crate::settings::read_on_machine(&path)
            .ok()
            .and_then(|read| crate::profiles::applied(&read.file).addons.installed)
            .unwrap_or_default();
        let mounted = mounted();
        let built_in = services::installed()
            .into_iter()
            .filter(|(name, f)| f.addon && !mounted.contains(name))
            .map(|(name, _)| name)
            .collect();
        Facts {
            wanted,
            mounted,
            built_in,
            pending: names_in(&dir.join(places::ADDONS_PENDING)),
            failed: failed_in(&dir),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn each_state_comes_from_the_files() {
        let facts = Facts {
            wanted: names(&["a", "b", "c", "d"]),
            mounted: names(&["a", "e"]),
            built_in: names(&["f"]),
            pending: names(&["b"]),
            failed: names(&["c"]),
        };
        assert_eq!(state("a", &facts), State::Installed);
        assert_eq!(state("b", &facts), State::Pending);
        assert_eq!(state("c", &facts), State::Failed);
        assert_eq!(state("d", &facts), State::Waiting);
        assert_eq!(state("e", &facts), State::Leaving);
        assert_eq!(state("f", &facts), State::BuiltIn);
        assert_eq!(state("g", &facts), State::Off);
        assert!(State::Pending.chosen() && !State::Leaving.chosen() && !State::Off.chosen());
    }

    #[test]
    fn the_offered_add_ons_are_read_from_their_feature_files() {
        let dir = std::env::temp_dir().join(format!("edel-offered-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("orca.toml"),
            "format = 1\nsummary = \"Screen reader\"\nwhy = \"x\"\naddon = true\n",
        )
        .unwrap();
        fs::write(dir.join("notes.txt"), "not a feature").unwrap();
        assert_eq!(
            offered_in(&dir),
            vec![("orca".to_string(), "Screen reader".to_string())]
        );
        fs::write(dir.join("x.ext4"), "").unwrap();
        fs::write(dir.join("failed"), "c\n\n d \n").unwrap();
        assert_eq!(names_in(&dir), names(&["x"]));
        assert_eq!(failed_in(&dir), names(&["c", "d"]));
        fs::remove_dir_all(&dir).unwrap();
    }

    fn pack(name: &str, hardware: &str) -> (String, Feature) {
        let text = format!(
            "format = 1\nsummary = \"{name} summary\"\nwhy = \"x\"\naddon = true\nhardware = [\"{hardware}\"]\n"
        );
        let (feature, _) = crate::features::read(&text).unwrap();
        (name.to_string(), feature)
    }

    fn device(modalias: &str) -> Device {
        Device {
            bus: "pci".into(),
            name: "0000:00:02.0".into(),
            modalias: modalias.into(),
        }
    }

    #[test]
    fn an_unchosen_pack_that_serves_this_machine_is_listed() {
        let offered = vec![
            pack("graphics-nvidia", "pci:v000010DEd*"),
            pack("ci-hello", "pci:v00001AF4d*"),
            ("orca".to_string(), Feature::default()),
        ];
        let devices = [device("pci:v000010DEd00001EB8sv0bc03")];
        let off = |_: &str| State::Off;
        assert_eq!(
            unchosen_packs(&offered, &devices, off),
            [Pack {
                name: "graphics-nvidia".into(),
                summary: "graphics-nvidia summary".into(),
            }]
        );
    }

    #[test]
    fn a_chosen_pack_is_not_listed_as_one_to_add() {
        let offered = vec![pack("graphics-nvidia", "pci:v000010DEd*")];
        let devices = [device("pci:v000010DEd00001EB8sv0bc03")];
        let waiting = |_: &str| State::Waiting;
        assert!(unchosen_packs(&offered, &devices, waiting).is_empty());
        assert!(unchosen_packs(&offered, &[], |_: &str| State::Off).is_empty());
    }
}
