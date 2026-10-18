//! The three facts a person asks first about a machine (roadmap M8.14,
//! ADR-009): is it up to date, is its data encrypted, and is its firewall
//! on. They are read here once: `edel status` and Settings' System page both
//! show them from this module (ADR-010), so the words and the rules agree.
//!
//! The lines `edel status` prints keep their keys in English (`updates:`,
//! `data:`, `firewall:`), as CI and scripts read them. Their values go
//! through `tr`, as the rest of what a person reads does (`docs/MESSAGES.md`),
//! and Settings shows the same values as sentences. The log words are
//! English, as the boot logs are.

use std::cmp::Ordering;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::base;
use crate::features::Feature;
use crate::i18n::{tr, trf};
use crate::{places, services, version};

/// The feature whose services are the firewall (`features/firewall.toml`).
const FIREWALL: &str = "firewall";
/// The format of the last-check record this release writes and reads; any
/// other format is read as no check, so the status never claims more.
const RECORD_FORMAT: u32 = 1;
/// The uuid of a device-mapper device that is encrypted (LUKS2 writes
/// `CRYPT-LUKS2-...`).
const CRYPT_PREFIX: &str = "CRYPT-";
const OS_RELEASE: &str = "/usr/lib/os-release";
const MOUNTS: &str = "/proc/self/mounts";
const BLOCK_DEVICES: &str = "/sys/class/block";

/// Whether the running version is the newest one the last check saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Updates {
    /// An update is in the other slot and a restart starts it, so it wins
    /// over any check.
    RestartPending,
    /// No check is on record, or the record is not one this release reads.
    NotChecked,
    /// The last check found this newer version.
    Available(String),
    /// The last check found nothing newer, on this date (`YYYY-MM-DD`).
    UpToDate { checked: String },
}

/// Whether the data partition is encrypted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    Encrypted,
    NotEncrypted,
    /// Nothing is mounted on `places::DATA_MOUNT`.
    NoDataPartition,
}

/// Whether the firewall's services are started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Firewall {
    On,
    Off,
    /// The machine has no firewall feature (a container image has none).
    NotInSystem,
}

/// The three facts together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub updates: Updates,
    pub data: Data,
    pub firewall: Firewall,
}

/// The last check's record, as `edel update` writes it. Read leniently:
/// unknown keys are ignored.
#[derive(Deserialize)]
struct Record {
    #[serde(default)]
    format: u32,
    #[serde(default)]
    checked: String,
    #[serde(default)]
    available: String,
}

/// The record `edel update --check` and `edel update` write once they have
/// verified the channel's list (`places::LAST_CHECK`).
pub fn last_check_text(date: &str, version: &str) -> String {
    format!("format = {RECORD_FORMAT}\nchecked = \"{date}\"\navailable = \"{version}\"\n")
}

/// The update state from the running version, the record's text (none when
/// the file is not there) and whether a restart is pending.
pub fn updates(running: &str, record: Option<&str>, restart_pending: bool) -> Updates {
    if restart_pending {
        return Updates::RestartPending;
    }
    let Some(record) = record.and_then(|text| toml::from_str::<Record>(text).ok()) else {
        return Updates::NotChecked;
    };
    if record.format != RECORD_FORMAT || record.checked.is_empty() {
        return Updates::NotChecked;
    }
    if version::compare(&record.available, running) == Ordering::Greater {
        Updates::Available(record.available)
    } else {
        Updates::UpToDate {
            checked: record.checked,
        }
    }
}

/// Whether the data partition is encrypted, from the text of
/// `/proc/self/mounts` and `dm_uuid`, which gives the uuid of a mount's
/// source (none when it is not a device-mapper device).
pub fn data(mounts: &str, dm_uuid: impl Fn(&str) -> Option<String>) -> Data {
    let source = mounts.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let (source, at) = (fields.next()?, fields.next()?);
        (at == places::DATA_MOUNT).then_some(source)
    });
    match source {
        None => Data::NoDataPartition,
        Some(source) if dm_uuid(source).is_some_and(|u| u.starts_with(CRYPT_PREFIX)) => {
            Data::Encrypted
        }
        Some(_) => Data::NotEncrypted,
    }
}

/// The firewall's state: on when the feature is installed and every one of
/// its services is started (`started` says so for a service's name).
pub fn firewall(installed: &[(String, Feature)], started: impl Fn(&str) -> bool) -> Firewall {
    let Some((_, feature)) = installed.iter().find(|(name, _)| name == FIREWALL) else {
        return Firewall::NotInSystem;
    };
    if feature
        .services
        .entries()
        .iter()
        .all(|(_, service)| started(service))
    {
        Firewall::On
    } else {
        Firewall::Off
    }
}

/// The device-mapper uuid of `source`, from sysfs: `/dev/mapper/NAME` is
/// `dm-N` once canonicalized, and its `dm/uuid` names what it maps.
fn dm_uuid(source: &str) -> Option<String> {
    let device = fs::canonicalize(source).ok()?;
    let name = device.file_name()?.to_str()?;
    let uuid = Path::new(BLOCK_DEVICES).join(name).join("dm/uuid");
    fs::read_to_string(uuid).ok().map(|u| u.trim().to_string())
}

/// The three facts on this machine now. Only files are read, so nothing
/// runs and no privilege is needed.
pub fn now() -> Facts {
    let os_release = fs::read_to_string(OS_RELEASE).unwrap_or_default();
    let running = version::os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let record = fs::read_to_string(places::LAST_CHECK).ok();
    let mounts = fs::read_to_string(MOUNTS).unwrap_or_default();
    Facts {
        updates: updates(
            &running,
            record.as_deref(),
            Path::new(places::RESTART_PENDING).exists(),
        ),
        data: data(&mounts, dm_uuid),
        firewall: firewall(&services::installed(), base::is_started),
    }
}

/// What one fact says: its value, and the hint `edel status` adds after it
/// (what to do next), when there is one.
struct Said {
    value: String,
    hint: Option<&'static str>,
}

impl Updates {
    fn said(&self) -> Said {
        match self {
            Updates::RestartPending => Said {
                value: tr("an update waits for a restart").to_string(),
                hint: Some(tr("restart to use it")),
            },
            Updates::NotChecked => Said {
                value: tr("not checked yet").to_string(),
                hint: Some(tr(
                    "check on Settings' Updates page or with edel update --check",
                )),
            },
            Updates::Available(version) => Said {
                value: trf("version {version} is available", &[("version", version)]),
                hint: Some(tr(
                    "install it on Settings' Updates page or with doas edel update",
                )),
            },
            Updates::UpToDate { checked } => Said {
                value: trf("up to date, checked on {date}", &[("date", checked)]),
                hint: None,
            },
        }
    }
}

impl Data {
    fn said(&self) -> Said {
        let value = match self {
            Data::Encrypted => tr("encrypted"),
            Data::NotEncrypted => tr("not encrypted"),
            Data::NoDataPartition => tr("no data partition"),
        };
        Said {
            value: value.to_string(),
            hint: None,
        }
    }
}

impl Firewall {
    fn said(&self) -> Said {
        match self {
            Firewall::On => Said {
                value: tr("on").to_string(),
                hint: None,
            },
            Firewall::Off => Said {
                value: tr("off").to_string(),
                hint: Some(tr(
                    "turn it on with doas edel settings set services.firewall=true, then doas edel settings apply",
                )),
            },
            Firewall::NotInSystem => Said {
                value: tr("not part of this system").to_string(),
                hint: None,
            },
        }
    }
}

impl Facts {
    /// The three facts with their titles and values, in the order
    /// `edel status` and Settings show them.
    pub fn rows(&self) -> [(&'static str, String); 3] {
        [
            (tr("Updates"), self.updates.said().value),
            (tr("Data"), self.data.said().value),
            (tr("Firewall"), self.firewall.said().value),
        ]
    }

    /// The three lines `edel status` prints: the key, the value and the
    /// hint after `; `.
    pub fn lines(&self) -> Vec<String> {
        let said = [
            ("updates", self.updates.said()),
            ("data", self.data.said()),
            ("firewall", self.firewall.said()),
        ];
        said.into_iter()
            .map(|(key, said)| match said.hint {
                Some(hint) => format!("{key}: {}; {hint}", said.value),
                None => format!("{key}: {}", said.value),
            })
            .collect()
    }

    /// The facts in English words for the logs and CI, as the boot logs
    /// are English: `firewall on, data not-encrypted, updates not-checked`.
    pub fn log_words(&self) -> String {
        let updates = match self.updates {
            Updates::RestartPending => "restart-pending",
            Updates::NotChecked => "not-checked",
            Updates::Available(_) => "available",
            Updates::UpToDate { .. } => "up-to-date",
        };
        let data = match self.data {
            Data::Encrypted => "encrypted",
            Data::NotEncrypted => "not-encrypted",
            Data::NoDataPartition => "no-data-partition",
        };
        let firewall = match self.firewall {
            Firewall::On => "on",
            Firewall::Off => "off",
            Firewall::NotInSystem => "not-in-system",
        };
        format!("firewall {firewall}, data {data}, updates {updates}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features;

    /// A feature whose default runlevel lists `services`.
    fn feature(services: &[&str]) -> Feature {
        let list: Vec<String> = services.iter().map(|s| format!("\"{s}\"")).collect();
        let text = format!(
            "format = 1\nsummary = \"test\"\n[services]\ndefault = [{}]\n",
            list.join(", ")
        );
        features::read(&text).unwrap().0
    }

    #[test]
    fn a_pending_restart_wins() {
        let record = last_check_text("2026-10-01", "2026.10.9");
        assert_eq!(
            updates("2026.10.5", Some(&record), true),
            Updates::RestartPending
        );
    }

    #[test]
    fn no_record_is_not_checked() {
        assert_eq!(updates("2026.10.5", None, false), Updates::NotChecked);
    }

    #[test]
    fn a_newer_version_is_available() {
        let record = last_check_text("2026-10-18", "2026.10.9");
        assert_eq!(
            updates("2026.10.5", Some(&record), false),
            Updates::Available("2026.10.9".to_string())
        );
    }

    #[test]
    fn the_same_version_is_up_to_date() {
        let record = last_check_text("2026-10-18", "2026.10.5");
        assert_eq!(
            updates("2026.10.5", Some(&record), false),
            Updates::UpToDate {
                checked: "2026-10-18".to_string()
            }
        );
    }

    #[test]
    fn an_unknown_format_is_not_checked() {
        let record = "format = 2\nchecked = \"2026-10-18\"\navailable = \"2026.10.5\"\n";
        assert_eq!(
            updates("2026.10.5", Some(record), false),
            Updates::NotChecked
        );
        assert_eq!(
            updates("2026.10.5", Some("not toml ="), false),
            Updates::NotChecked
        );
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let record =
            "format = 1\nchecked = \"2026-10-18\"\navailable = \"2026.10.5\"\nextra = true\n";
        assert_eq!(
            updates("2026.10.5", Some(record), false),
            Updates::UpToDate {
                checked: "2026-10-18".to_string()
            }
        );
    }

    #[test]
    fn a_crypt_mapping_is_encrypted() {
        let mounts = "/dev/mapper/data /data ext4 rw 0 0\n";
        let uuid = |_: &str| Some("CRYPT-LUKS2-0123abcd-data".to_string());
        assert_eq!(data(mounts, uuid), Data::Encrypted);
    }

    #[test]
    fn a_plain_partition_is_not_encrypted() {
        let mounts = "/dev/vda3 /data ext4 rw 0 0\n";
        assert_eq!(data(mounts, |_| None), Data::NotEncrypted);
    }

    #[test]
    fn no_data_mount_is_said() {
        let mounts = "/dev/vda2 / ext4 ro 0 0\n";
        assert_eq!(data(mounts, |_| None), Data::NoDataPartition);
    }

    #[test]
    fn firewall_on_needs_every_service_started() {
        let installed = vec![("firewall".to_string(), feature(&["edel-firewall", "extra"]))];
        assert_eq!(
            firewall(&installed, |s| s == "edel-firewall" || s == "extra"),
            Firewall::On
        );
        assert_eq!(
            firewall(&installed, |s| s == "edel-firewall"),
            Firewall::Off
        );
    }

    #[test]
    fn no_firewall_feature_is_not_part_of_this_system() {
        let installed = vec![("ssh".to_string(), feature(&["sshd"]))];
        assert_eq!(firewall(&installed, |_| true), Firewall::NotInSystem);
    }

    #[test]
    fn status_lines_read_exactly() {
        let facts = Facts {
            updates: Updates::UpToDate {
                checked: "2026-10-18".to_string(),
            },
            data: Data::NotEncrypted,
            firewall: Firewall::Off,
        };
        assert_eq!(
            facts.lines(),
            vec![
                "updates: up to date, checked on 2026-10-18".to_string(),
                "data: not encrypted".to_string(),
                "firewall: off; turn it on with doas edel settings set services.firewall=true, then doas edel settings apply".to_string(),
            ]
        );
        let waiting = Facts {
            updates: Updates::Available("2026.10.9".to_string()),
            data: Data::NoDataPartition,
            firewall: Firewall::NotInSystem,
        };
        assert_eq!(
            waiting.lines()[0],
            "updates: version 2026.10.9 is available; install it on Settings' Updates page or with doas edel update"
        );
        assert_eq!(waiting.lines()[2], "firewall: not part of this system");
    }

    #[test]
    fn the_log_words_are_english() {
        let facts = Facts {
            updates: Updates::NotChecked,
            data: Data::NotEncrypted,
            firewall: Firewall::On,
        };
        assert_eq!(
            facts.log_words(),
            "firewall on, data not-encrypted, updates not-checked"
        );
    }
}
