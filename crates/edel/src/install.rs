//! The install plan (roadmap M2.4): what `edel install` erases and what it
//! writes, built once and shown three ways (ADR-008): `--plan` prints
//! it, a terminal run shows it before the person types the disk's name,
//! and a run with neither a terminal nor `--yes` prints it and exits 3.
//! Settings' installer page shows the same plan (M6.6a), so it lives in the
//! library.

use std::fmt;

use anyhow::{Result, bail};

use crate::i18n::{n_, tr, trf};
use crate::settings;

const MIB: u64 = 1024 * 1024;
/// The EFI system partition, as `boot.rs` lays it out.
pub const ESP_MIB: u64 = 64;
/// The smallest data partition worth installing with.
pub const MIN_DATA_MIB: u64 = 256;
/// Each slot of an installed machine, whatever the medium it was installed
/// from: a machine installed from a preview must take every later update,
/// so slots are sized for the base's growth, not today's (roadmap M3.3b,
/// `docs/FORMATS.md`).
pub const SLOT_MIB: u64 = 4096;

/// A partition already on the target disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partition {
    /// Kernel name, such as `sda1`
    pub name: String,
    /// The file system's label, if it has one
    pub label: Option<String>,
    /// The file system's type as blkid names it, such as `ntfs`
    pub fs: Option<String>,
    pub bytes: u64,
}

/// The target disk as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disk {
    /// Kernel name, such as `sda`
    pub name: String,
    pub model: Option<String>,
    pub bytes: u64,
    pub partitions: Vec<Partition>,
}

/// What `edel install` will do to one disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub disk: Disk,
    /// Each slot's size, `SLOT_MIB`
    pub slot_mib: u64,
    /// The settings file the new machine starts with
    pub system_file: String,
}

impl Plan {
    /// The data partition fills the rest, less 1 MiB in front and 1 MiB
    /// for the backup partition table at the end.
    pub fn data_mib(&self) -> u64 {
        (self.disk.bytes / MIB).saturating_sub(1 + ESP_MIB + 2 * self.slot_mib + 1)
    }

    /// Refuses a disk too small for two slots and some data.
    pub fn check(&self) -> Result<()> {
        if self.data_mib() < MIN_DATA_MIB {
            let need = 2 + ESP_MIB + 2 * self.slot_mib + MIN_DATA_MIB;
            bail!(
                "{}",
                trf(
                    "/dev/{disk} has {has}, and Edel OS needs at least {needs}; use a larger disk",
                    &[
                        ("disk", &self.disk.name),
                        ("has", &size(self.disk.bytes)),
                        ("needs", &size(need * MIB))
                    ]
                )
            );
        }
        Ok(())
    }
}

/// Decimal sizes, as disks are sold: `420 GB`, `3.2 GB`, `512 MB`.
pub fn size(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 10.0 {
        format!("{gb:.0} GB")
    } else if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1e6)
    }
}

/// `ntfs` as people know it: `NTFS`.
fn fs_name(fs: &str) -> String {
    match fs {
        "vfat" => "FAT".into(),
        "ntfs" | "exfat" | "btrfs" | "xfs" | "zfs_member" => {
            fs.trim_end_matches("_member").to_uppercase()
        }
        "crypto_LUKS" => "encrypted".into(),
        "swap" => "swap".into(),
        other => other.to_string(),
    }
}

impl fmt::Display for Partition {
    /// `sda3: Windows (NTFS, 420 GB)`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = self.label.as_deref().unwrap_or(tr("no name"));
        match &self.fs {
            Some(fs) => write!(
                f,
                "{}",
                trf(
                    "{name}: {label} ({fs}, {size})",
                    &[
                        ("name", &self.name),
                        ("label", label),
                        ("fs", &fs_name(fs)),
                        ("size", &size(self.bytes))
                    ]
                )
            ),
            None => write!(
                f,
                "{}",
                trf(
                    "{name}: {label} (no file system, {size})",
                    &[
                        ("name", &self.name),
                        ("label", label),
                        ("size", &size(self.bytes))
                    ]
                )
            ),
        }
    }
}

impl fmt::Display for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let disk = &self.disk;
        let model = disk.model.as_deref().unwrap_or(tr("no model name"));
        writeln!(
            f,
            "{}",
            trf(
                "Install Edel OS on /dev/{disk} ({model}, {size})",
                &[
                    ("disk", &disk.name),
                    ("model", model),
                    ("size", &size(disk.bytes))
                ]
            )
        )?;
        if disk.partitions.is_empty() {
            writeln!(f, "{}", tr("The disk has no partitions."))?;
        } else {
            writeln!(f, "{}", tr("This erases everything on it:"))?;
            for partition in &disk.partitions {
                writeln!(f, "  {partition}")?;
            }
        }
        writeln!(f, "{}", tr("and writes:"))?;
        writeln!(
            f,
            "{}",
            trf(
                "  1: EFI system partition, {mib} MiB",
                &[("mib", &ESP_MIB.to_string())]
            )
        )?;
        writeln!(
            f,
            "{}",
            trf(
                "  2: slot A, {mib} MiB, a copy of the running system",
                &[("mib", &self.slot_mib.to_string())]
            )
        )?;
        writeln!(
            f,
            "{}",
            trf(
                "  3: slot B, {mib} MiB, empty until the first update",
                &[("mib", &self.slot_mib.to_string())]
            )
        )?;
        writeln!(
            f,
            "{}",
            trf(
                "  4: data, {mib} MiB, starting from the settings file {file}",
                &[
                    ("mib", &self.data_mib().to_string()),
                    ("file", &self.system_file)
                ]
            )
        )
    }
}

/// Reads busybox or util-linux `blkid DEVICE` output: `LABEL` and `TYPE`.
pub fn parse_blkid(line: &str) -> (Option<String>, Option<String>) {
    let field = |name: &str| {
        let start = line.find(&format!(" {name}=\""))? + name.len() + 3;
        let len = line[start..].find('"')?;
        Some(line[start..start + len].to_string()).filter(|v| !v.is_empty())
    };
    (field("LABEL"), field("TYPE"))
}

/// One question the installer asks (roadmap M6.6a, M6.6b): the same list,
/// in the same order, on a terminal (`edel install` without `--settings`)
/// and on Settings' installer page, so the two never ask differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Question {
    /// What the answer is: a settings file key, or `user`, `password` and
    /// `disk`, which are no key of the file
    pub key: &'static str,
    /// The question as people read it
    pub title: &'static str,
    /// Under More options: asked only when the person asks for them, with
    /// a suggestion already filled in
    pub more: bool,
}

/// The installer's questions, in the order they are asked. A key the
/// release does not act on yet (`settings::KEYS`' `supported`, such as
/// `region.timezone` before its step) is left out by [`asked`], so the
/// file never holds a key `edel settings check` refuses.
pub const QUESTIONS: &[Question] = &[
    Question {
        key: "region.language",
        title: n_("Language"),
        more: false,
    },
    Question {
        key: "region.keyboard",
        title: n_("Keyboard"),
        more: false,
    },
    Question {
        key: "user",
        title: n_("Your login name"),
        more: false,
    },
    Question {
        key: "password",
        title: n_("Your password"),
        more: false,
    },
    Question {
        key: "disk",
        title: n_("Disk to install on"),
        more: false,
    },
    Question {
        key: "region.timezone",
        title: n_("Time zone"),
        more: false,
    },
    Question {
        key: "network.hostname",
        title: n_("Computer name"),
        more: true,
    },
    Question {
        key: "layout.preset",
        title: n_("Layout"),
        more: true,
    },
    Question {
        key: "system.profiles",
        title: n_("Profile"),
        more: true,
    },
];

/// The questions this release asks: those that are no key, and the keys
/// it acts on.
pub fn asked() -> Vec<&'static Question> {
    QUESTIONS
        .iter()
        .filter(|q| {
            !q.key.contains('.')
                || settings::KEYS
                    .iter()
                    .any(|k| k.path == q.key && k.supported)
        })
        .collect()
}

/// The answers, each `None` while it is not given. The password is never
/// here: it goes from the question to its hash, and the hash to the new
/// machine's first boot, never into the settings file (ADR-006).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answers {
    pub language: Option<String>,
    pub keyboard: Option<String>,
    pub user: Option<String>,
    /// An ssh key instead of a password, for a vm or a server
    pub ssh_key: Option<String>,
    pub timezone: Option<String>,
    pub hostname: Option<String>,
    pub preset: Option<String>,
}

/// The preset a new machine has when the file names none.
const DEFAULT_PRESET: &str = "classic";

impl Answers {
    /// The new machine's settings file: only what was answered, and never
    /// a value that applies anyway (ADR-008: writers never write a
    /// default), so the Classic preset is left out. The one person is an
    /// administrator. Checked by `settings::check` before it is returned.
    pub fn settings_text(&self) -> Result<String> {
        let mut text = String::from("format = 1\n");
        if let Some(user) = &self.user {
            if !settings::is_user_name(user) {
                bail!(
                    "{}",
                    trf(
                        "{name:?} is not a login name; use lowercase letters, digits, - and _, starting with a letter",
                        &[("name", user)]
                    )
                );
            }
            text.push_str(&format!("\n[users.{user}]\nadmin = true\n"));
            if let Some(key) = &self.ssh_key {
                text.push_str(&format!("ssh_keys = [{}]\n", quoted(key)));
            }
        }
        if let Some(hostname) = &self.hostname {
            text.push_str(&format!("\n[network]\nhostname = {}\n", quoted(hostname)));
        }
        let region: Vec<(&str, &String)> = [
            ("language", &self.language),
            ("keyboard", &self.keyboard),
            ("timezone", &self.timezone),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.as_ref().map(|v| (key, v)))
        .collect();
        if !region.is_empty() {
            text.push_str("\n[region]\n");
            for (key, value) in region {
                text.push_str(&format!("{key} = {}\n", quoted(value)));
            }
        }
        if let Some(preset) = self.preset.as_ref().filter(|p| *p != DEFAULT_PRESET) {
            text.push_str(&format!("\n[layout]\npreset = {}\n", quoted(preset)));
        }
        let problems = settings::check(&text)?;
        if !problems.is_empty() {
            bail!("{}", problems.join("\n"));
        }
        Ok(text)
    }
}

/// A TOML basic string.
fn quoted(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

/// The computer's name suggested under More options: the login name and
/// the form factor, `ali-laptop`, cut to a hostname's 63 letters.
pub fn suggested_hostname(user: &str, form: &str) -> String {
    let name: String = format!("{user}-{form}")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-');
    name[..name.len().min(63)].trim_end_matches('-').to_string()
}

/// The keyboard a language suggests: its country's layout as xkb names it
/// (`de_DE` gives `de`, `en_GB` gives `gb`), `us` for a language with no
/// country, and a layout that writes no Latin letters gets `us` second, so
/// a login name and a password can be typed (M6.6a).
pub fn suggested_keyboard(language: &str) -> String {
    let base = language.split(['.', '@']).next().unwrap_or(language);
    let (lang, country) = match base.split_once('_') {
        Some((l, c)) => (l, c.to_ascii_lowercase()),
        None => (base, String::new()),
    };
    let layout = match (lang, country.as_str()) {
        (_, "") | ("en", "us") => "us".to_string(),
        ("en", "gb") => "gb".to_string(),
        (_, "us") => "us".to_string(),
        (_, c) => c.to_string(),
    };
    if NON_LATIN.contains(&layout.as_str()) {
        format!("{layout},us")
    } else {
        layout
    }
}

/// xkb layouts that type no Latin letters.
const NON_LATIN: &[&str] = &[
    "ru", "ua", "by", "bg", "rs", "mk", "kz", "kg", "mn", "gr", "il", "ir", "sa", "eg", "ae", "th",
    "kr", "jp", "cn", "tw", "in", "am", "ge",
];

/// A disk the installer may offer: a whole disk, not a loop, RAM, zram,
/// optical or device-mapper one.
pub fn is_candidate(name: &str) -> bool {
    !["loop", "ram", "zram", "sr", "dm-", "md", "fd", "nbd"]
        .iter()
        .any(|p| name.starts_with(p))
        && !name.is_empty()
}

/// One line of `edel install --list`: a disk, its model and size, whether
/// it is removable and whether Edel OS is running from it, then its
/// partitions as the plan names them.
pub fn list_entry(disk: &Disk, removable: bool, running: bool) -> String {
    let mut marks = Vec::new();
    if removable {
        marks.push(tr("removable"));
    }
    if running {
        marks.push(tr("running from it, so not offered"));
    }
    let mut line = format!(
        "/dev/{}: {}, {}",
        disk.name,
        disk.model.as_deref().unwrap_or(tr("no model name")),
        size(disk.bytes)
    );
    if !marks.is_empty() {
        line.push_str(&format!(" ({})", marks.join(", ")));
    }
    if disk.partitions.is_empty() {
        line.push_str(&format!("\n  {}", tr("no partitions")));
    }
    for partition in &disk.partitions {
        line.push_str(&format!("\n  {partition}"));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(partitions: Vec<Partition>) -> Plan {
        Plan {
            disk: Disk {
                name: "sda".into(),
                model: Some("Samsung SSD 870".into()),
                bytes: 500_107_862_016,
                partitions,
            },
            slot_mib: SLOT_MIB,
            system_file: "/run/media/seed.toml".into(),
        }
    }

    #[test]
    fn shows_what_it_erases_as_people_know_it() {
        let shown = plan(vec![
            Partition {
                name: "sda1".into(),
                label: None,
                fs: Some("vfat".into()),
                bytes: 104_857_600,
            },
            Partition {
                name: "sda3".into(),
                label: Some("Windows".into()),
                fs: Some("ntfs".into()),
                bytes: 420_000_000_000,
            },
        ])
        .to_string();
        assert!(shown.starts_with(
            "Install Edel OS on /dev/sda (Samsung SSD 870, 500 GB)\nThis erases everything on it:\n"
        ));
        assert!(shown.contains("  sda1: no name (FAT, 105 MB)\n"));
        assert!(shown.contains("  sda3: Windows (NTFS, 420 GB)\n"));
        assert!(shown.contains("  2: slot A, 4096 MiB, a copy of the running system\n"));
    }

    #[test]
    fn the_data_partition_takes_the_rest() {
        let mut plan = plan(Vec::new());
        assert_eq!(plan.data_mib(), 476_940 - 1 - 64 - 8192 - 1);
        assert!(plan.check().is_ok());
        plan.disk.bytes = 8 * 1024 * MIB;
        let error = plan.check().unwrap_err().to_string();
        assert!(error.contains("needs at least 8.9 GB"), "{error}");
    }

    #[test]
    fn reads_blkid_lines() {
        assert_eq!(
            parse_blkid("/dev/vdb3: LABEL=\"Windows\" UUID=\"1234\" TYPE=\"ntfs\""),
            (Some("Windows".into()), Some("ntfs".into()))
        );
        assert_eq!(
            parse_blkid("/dev/vdb1: UUID=\"ab\" TYPE=\"vfat\""),
            (None, Some("vfat".into()))
        );
        assert_eq!(parse_blkid(""), (None, None));
    }

    #[test]
    fn asks_only_the_keys_this_release_acts_on() {
        let keys: Vec<&str> = asked().iter().map(|q| q.key).collect();
        assert_eq!(
            &keys[..5],
            [
                "region.language",
                "region.keyboard",
                "user",
                "password",
                "disk"
            ]
        );
        for q in asked() {
            if q.key.contains('.') {
                assert!(
                    settings::KEYS
                        .iter()
                        .any(|k| k.path == q.key && k.supported),
                    "{}",
                    q.key
                );
            }
        }
        for q in QUESTIONS.iter().filter(|q| q.key.contains('.')) {
            assert!(
                settings::KEYS.iter().any(|k| k.path == q.key),
                "{} is no key",
                q.key
            );
        }
    }

    #[test]
    fn the_file_holds_only_the_answers() {
        let answers = Answers {
            language: Some("de_DE.UTF-8".into()),
            keyboard: Some("de".into()),
            user: Some("ali".into()),
            hostname: Some("ali-laptop".into()),
            preset: Some("classic".into()),
            ..Answers::default()
        };
        let text = answers.settings_text().unwrap();
        assert_eq!(
            text,
            "format = 1\n\n[users.ali]\nadmin = true\n\n[network]\nhostname = \"ali-laptop\"\n\n[region]\nlanguage = \"de_DE.UTF-8\"\nkeyboard = \"de\"\n"
        );
        assert_eq!(Answers::default().settings_text().unwrap(), "format = 1\n");
        let mac = Answers {
            preset: Some("mac-like".into()),
            ssh_key: Some("ssh-ed25519 AAAA ali@home".into()),
            user: Some("ali".into()),
            ..Answers::default()
        };
        let text = mac.settings_text().unwrap();
        assert!(
            text.contains("ssh_keys = [\"ssh-ed25519 AAAA ali@home\"]"),
            "{text}"
        );
        assert!(text.contains("[layout]\npreset = \"mac-like\""), "{text}");
    }

    #[test]
    fn refuses_a_bad_answer() {
        let bad = Answers {
            user: Some("Ali Baba".into()),
            ..Answers::default()
        };
        assert!(bad.settings_text().is_err());
        let bad = Answers {
            hostname: Some("not a name".into()),
            ..Answers::default()
        };
        assert!(bad.settings_text().is_err());
    }

    #[test]
    fn suggests_a_computer_name_and_a_keyboard() {
        assert_eq!(suggested_hostname("ali", "laptop"), "ali-laptop");
        assert_eq!(suggested_hostname("_x", "vm"), "x-vm");
        assert_eq!(suggested_keyboard("en_US.UTF-8"), "us");
        assert_eq!(suggested_keyboard("en_GB"), "gb");
        assert_eq!(suggested_keyboard("de_DE.UTF-8"), "de");
        assert_eq!(suggested_keyboard("ru_RU.UTF-8"), "ru,us");
        assert_eq!(suggested_keyboard("C"), "us");
    }

    #[test]
    fn lists_whole_disks_with_their_marks() {
        assert!(is_candidate("sda") && is_candidate("nvme0n1") && is_candidate("vdb"));
        for name in ["loop0", "ram0", "zram0", "sr0", "dm-0", "md0"] {
            assert!(!is_candidate(name), "{name}");
        }
        let disk = Disk {
            name: "sdb".into(),
            model: Some("SanDisk Ultra".into()),
            bytes: 32_000_000_000,
            partitions: vec![],
        };
        assert_eq!(
            list_entry(&disk, true, true),
            "/dev/sdb: SanDisk Ultra, 32 GB (removable, running from it, so not offered)\n  no partitions"
        );
    }
}
