//! The install plan (roadmap M2.4): what `edel install` erases and what it
//! writes, built once and shown three ways (ADR-008): `--plan` prints
//! it, a terminal run shows it before the person types the disk's name,
//! and a run with neither a terminal nor `--yes` prints it and exits 3.
//! Settings' installer page shows the same plan (M6.6a), so it lives in the
//! library.

use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};

use crate::battery_care;
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
    /// Whether the data partition is encrypted (M8.2)
    pub encrypted: bool,
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
        let data = if self.encrypted {
            n_("  4: data, {mib} MiB, encrypted, starting from the settings file {file}")
        } else {
            n_("  4: data, {mib} MiB, starting from the settings file {file}")
        };
        writeln!(
            f,
            "{}",
            trf(
                data,
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
/// `system.profiles` before its step) is left out by [`asked`], so the
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
        title: n_("Your username"),
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
    /// One profile (M7.6), or [`NO_PROFILE`]
    pub profile: Option<String>,
}

/// The profile answer for none: the machine names no profile.
pub const NO_PROFILE: &str = "none";

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
                        "{name:?} is not a username; use lowercase letters, digits, - and _, starting with a letter",
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
        if let Some(profile) = self.profile.as_ref().filter(|p| *p != NO_PROFILE) {
            text.push_str(&format!("\n[system]\nprofiles = [{}]\n", quoted(profile)));
        }
        let problems = settings::check(&text)?;
        if !problems.is_empty() {
            bail!("{}", problems.join("\n"));
        }
        Ok(text)
    }
}

/// The question keys a settings file answers already (M6.6b), so the
/// installer asks only the rest: `user` when it names anyone (the password
/// is still asked, as a file never holds one), and each region, network
/// and layout key the questions ask that it sets.
pub fn answered(file: &settings::SettingsFile) -> Vec<&'static str> {
    QUESTIONS
        .iter()
        .map(|q| q.key)
        .filter(|key| match *key {
            "user" => !file.users.is_empty(),
            "region.language" => file.region.language.is_some(),
            "region.keyboard" => file.region.keyboard.is_some(),
            "region.timezone" => file.region.timezone.is_some(),
            "network.hostname" => file.network.hostname.is_some(),
            "layout.preset" => file.layout.preset.is_some(),
            "system.profiles" => file.system.profiles.is_some(),
            _ => false,
        })
        .collect()
}

/// `text`, a settings file, with the answers to the questions it leaves
/// open added through `settings::set`, so the file's own lines and comments
/// stay as they are (M6.6b). An answer the file answers already is left
/// out, and so is the Classic preset, which applies anyway; the one person
/// asked for, when the file names nobody, is an administrator.
pub fn merged(text: &str, answers: &Answers) -> Result<String> {
    let file = settings::read(text)?.file;
    let done = answered(&file);
    let mut wanted: Vec<(String, String)> = Vec::new();
    if let Some(user) = answers.user.as_ref().filter(|_| !done.contains(&"user")) {
        wanted.push((format!("users.{user}.admin"), "true".into()));
    }
    let preset = answers.preset.clone().filter(|p| p != DEFAULT_PRESET);
    for (key, value) in [
        ("region.language", &answers.language),
        ("region.keyboard", &answers.keyboard),
        ("region.timezone", &answers.timezone),
        ("network.hostname", &answers.hostname),
        ("layout.preset", &preset),
    ] {
        if let Some(value) = value.as_ref().filter(|_| !done.contains(&key)) {
            wanted.push((key.to_string(), value.clone()));
        }
    }
    if let Some(profile) = answers
        .profile
        .as_ref()
        .filter(|p| *p != NO_PROFILE && !done.contains(&"system.profiles"))
    {
        wanted.push(("system.profiles".into(), format!("[{}]", quoted(profile))));
    }
    let mut text = text.to_string();
    for (key, value) in wanted {
        text = settings::set(&text, &key, &value)?;
    }
    // A language whose script the image has no font for brings its fonts'
    // add-on (M7.2a); a file that lists it already stays as it is.
    if let Some(addon) = answers
        .language
        .as_deref()
        .and_then(crate::languages::font_addon)
    {
        text = settings::edit_list(&text, "addons.installed", addon, true)?;
    }
    let problems = settings::check(&text)?;
    if !problems.is_empty() {
        bail!("{}", problems.join("\n"));
    }
    Ok(text)
}

/// The settings file for installing in `dir`, the top of a disk mounted
/// read-only: the settings file by its name or a former one, first at the
/// top, then in `EFI/edel` (where the stick's own seed is kept). Its path
/// and text, when one is there.
pub fn file_in(dir: &Path) -> Option<(std::path::PathBuf, String)> {
    for inner in [dir.to_path_buf(), dir.join("EFI/edel")] {
        let path = crate::places::found(&crate::places::settings_in(&inner));
        if let Ok(text) = fs::read_to_string(&path) {
            return Some((path, text));
        }
    }
    None
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
        // xkb names Arabic and Latin American Spanish by language, not country.
        ("ar", _) => "ara".to_string(),
        ("es", "mx") => "latam".to_string(),
        // Uzbek is written in Latin letters here; xkb's `uz` is Cyrillic.
        ("uz", _) => "uz(latin)".to_string(),
        (_, "") | ("en", "us") => "us".to_string(),
        ("en", "gb") => "gb".to_string(),
        (_, "us") => "us".to_string(),
        (_, c) => c.to_string(),
    };
    keyboard_for(&layout)
}

/// The keyboard a layout gives the installer: a layout that types no Latin
/// letters gets `us` as a second layout, so a login name and a password can
/// be typed.
pub fn keyboard_for(layout: &str) -> String {
    let name = layout.split('(').next().unwrap_or(layout);
    if NON_LATIN.contains(&name) {
        format!("{layout},us")
    } else {
        layout.to_string()
    }
}

/// xkb layouts that type no Latin letters.
pub const NON_LATIN: &[&str] = &[
    "ru", "ua", "by", "bg", "rs", "mk", "kz", "kg", "mn", "gr", "il", "ir", "ara", "th", "kr",
    "jp", "cn", "tw", "in", "am", "ge",
];

/// The language answer as the installer takes it, such as `de_DE.UTF-8`, or
/// the words that say what it should be; the terminal and Settings ask the
/// same.
pub fn language_answer(value: &str) -> Result<String, String> {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.@-".contains(c))
    {
        Ok(value.to_string())
    } else {
        Err(tr("a language such as en_US.UTF-8 or de_DE.UTF-8").to_string())
    }
}

/// The login name answer: a login name, never root or the stick's own
/// account (`places::LIVE_USER`).
pub fn user_answer(value: &str) -> Result<String, String> {
    if settings::is_user_name(value) && value != "root" && value != crate::places::LIVE_USER {
        Ok(value.to_string())
    } else {
        Err(trf(
            "lowercase letters, digits, - and _, starting with a letter, and not root or {live}",
            &[("live", crate::places::LIVE_USER)],
        ))
    }
}

/// The profile answer (M7.6): one of the release's profiles, or
/// [`NO_PROFILE`].
pub fn profile_answer(value: &str) -> Result<String, String> {
    if value == NO_PROFILE || crate::profiles::is_built_in(value) {
        Ok(value.to_string())
    } else {
        Err(trf(
            "one of {names}",
            &[(
                "names",
                &format!("{NO_PROFILE}, {}", crate::profiles::NAMES.join(", ")),
            )],
        ))
    }
}

/// The computer name answer: a host name.
pub fn hostname_answer(value: &str) -> Result<String, String> {
    if settings::is_hostname(value) {
        Ok(value.to_string())
    } else {
        Err(tr("letters, digits and hyphens, up to 63").to_string())
    }
}

/// What kind of computer this is, for the suggested computer name, from
/// `sys` (`/sys`, or a test's tree): a laptop has a battery, a virtual
/// machine says so in its firmware, anything else is a pc.
pub fn form_factor(sys: &Path) -> &'static str {
    // The supplies folder is the one `battery_care` names, under `sys`.
    let supplies = sys.join(battery_care::POWER_SUPPLY.trim_start_matches("/sys/"));
    let battery = !battery_care::batteries(&supplies).is_empty();
    let vendor = fs::read_to_string(sys.join("class/dmi/id/sys_vendor")).unwrap_or_default();
    if battery {
        "laptop"
    } else if ["QEMU", "VMware", "innotek", "Xen", "Microsoft"]
        .iter()
        .any(|v| vendor.contains(v))
    {
        "vm"
    } else {
        "pc"
    }
}

/// `major:minor` of the device mounted at `/` in `mountinfo`'s text. The
/// last matching line wins, since a later mount hides an earlier one.
pub fn root_device(mountinfo: &str) -> Option<&str> {
    mountinfo.lines().rev().find_map(|line| {
        let mut fields = line.split_whitespace();
        let dev = fields.nth(2)?;
        (fields.nth(1)? == "/").then_some(dev)
    })
}

/// The whole disk, as `/sys/block` names it, whose partition is mounted at
/// `/`; none when `/` is not on a disk partition. `sys` is `/sys`, or a
/// test's tree.
pub fn running_disk(mountinfo: &str, sys: &Path) -> Option<String> {
    let part = fs::canonicalize(sys.join("dev/block").join(root_device(mountinfo)?)).ok()?;
    if !part.join("partition").exists() {
        return None;
    }
    Some(part.parent()?.file_name()?.to_string_lossy().into_owned())
}

/// Whether the disk `name` is removable, or on USB, as a stick is; `sys` is
/// `/sys`, or a test's tree.
pub fn is_removable(sys: &Path, name: &str) -> bool {
    let block = sys.join("block").join(name);
    let removable = fs::read_to_string(block.join("removable")).is_ok_and(|r| r.trim() == "1");
    let on_usb = fs::canonicalize(&block).is_ok_and(|path| {
        path.components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with("usb"))
    });
    removable || on_usb
}

/// Whether a disk is a stick: removable, and not one `edel install` wrote,
/// which leaves [`places::INSTALLED`] on it (M6.17). A machine installed
/// onto a USB disk is a computer like any other.
pub fn is_stick(removable: bool, installed: bool) -> bool {
    removable && !installed
}

/// Whether the disk `name`, which the running system is on, is a stick.
pub fn running_from_stick(sys: &Path, name: &str) -> bool {
    is_stick(
        is_removable(sys, name),
        Path::new(crate::places::INSTALLED).exists(),
    )
}

/// Whether this system runs from a stick: Settings then shows its
/// installer page, and the stick's live account may install (M6.6a).
pub fn from_a_stick() -> bool {
    let sys = Path::new("/sys");
    fs::read_to_string("/proc/self/mountinfo")
        .ok()
        .and_then(|m| running_disk(&m, sys))
        .is_some_and(|disk| running_from_stick(sys, &disk))
}

/// A disk the installer page offers: its name, such as `sda`, and how
/// people know it, its model and size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskChoice {
    pub name: String,
    pub title: String,
}

/// The disks Settings' installer page offers, read from `sys` (`/sys`, or
/// a test's tree) with no root: every candidate whole disk but `running`,
/// the one the system runs from, the computer's own first, then by name. Their partitions are what
/// `edel install --list` shows, as only root may read their labels.
pub fn disk_choices(sys: &Path, running: Option<&str>) -> Vec<DiskChoice> {
    let Ok(entries) = fs::read_dir(sys.join("block")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_candidate(n) && Some(n.as_str()) != running)
        .collect();
    // A computer's own disks first, so the one offered first is never a
    // stick that happens to be plugged in.
    names.sort_by_key(|n| (is_removable(sys, n), n.clone()));
    names
        .into_iter()
        .filter_map(|name| {
            let block = sys.join("block").join(&name);
            let sectors: u64 = fs::read_to_string(block.join("size"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            let model = fs::read_to_string(block.join("device/model"))
                .ok()
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty());
            let capacity = size(sectors * 512);
            // The name (`sda`) stays in the choice, not the title people read.
            let title = match model {
                Some(model) => trf(
                    "{model} · {size}",
                    &[("model", &model), ("size", &capacity)],
                ),
                None => trf("{size} disk", &[("size", &capacity)]),
            };
            (sectors > 0).then_some(DiskChoice { name, title })
        })
        .collect()
}

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

    #[test]
    fn a_removable_disk_edel_install_wrote_is_no_stick() {
        assert!(is_stick(true, false));
        assert!(!is_stick(true, true));
        assert!(!is_stick(false, false));
        assert!(!is_stick(false, true));
    }

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
            encrypted: false,
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
        assert!(shown.contains(", starting from the settings file"));
        let mut encrypted = plan(Vec::new());
        encrypted.encrypted = true;
        assert!(
            encrypted
                .to_string()
                .contains("MiB, encrypted, starting from")
        );
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
    fn a_profile_answer_names_only_the_profile_and_none_writes_nothing() {
        let gaming = Answers {
            profile: Some("gaming".into()),
            ..Answers::default()
        };
        let text = gaming.settings_text().unwrap();
        assert!(text.contains("[system]\nprofiles = [\"gaming\"]"), "{text}");
        assert!(!text.contains("Steam"), "{text}");
        let none = Answers {
            profile: Some(NO_PROFILE.into()),
            ..Answers::default()
        };
        assert!(!none.settings_text().unwrap().contains("profiles"));
        assert!(profile_answer("chess").unwrap_err().contains("office"));
        let added = merged("format = 1\n", &gaming).unwrap();
        assert!(added.contains("profiles = [\"gaming\"]"), "{added}");
        let kept = merged("format = 1\n[system]\nprofiles = [\"office\"]\n", &gaming).unwrap();
        assert!(!kept.contains("gaming"), "{kept}");
    }

    #[test]
    fn a_zone_answer_is_written_unless_the_file_has_one() {
        let answers = Answers {
            timezone: Some("Europe/Berlin".into()),
            ..Answers::default()
        };
        assert!(
            answers
                .settings_text()
                .unwrap()
                .contains("timezone = \"Europe/Berlin\"")
        );
        let added = merged("format = 1\n", &answers).unwrap();
        assert!(added.contains("timezone = \"Europe/Berlin\""), "{added}");
        let kept = merged(
            "format = 1\n[region]\ntimezone = \"Asia/Tashkent\"\n",
            &answers,
        )
        .unwrap();
        assert!(
            kept.contains("Asia/Tashkent") && !kept.contains("Berlin"),
            "{kept}"
        );
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
        assert_eq!(suggested_keyboard("ar_SA.UTF-8"), "ara,us");
        assert_eq!(suggested_keyboard("es_MX.UTF-8"), "latam");
        assert_eq!(suggested_keyboard("uz_UZ.UTF-8"), "uz(latin)");
        assert_eq!(keyboard_for("ru"), "ru,us");
        assert_eq!(keyboard_for("de"), "de");
        assert_eq!(keyboard_for("ru(phonetic)"), "ru(phonetic),us");
        assert_eq!(keyboard_for("uz(latin)"), "uz(latin)");
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

    /// A sysfs tree with a USB stick `sda` (root on its second partition),
    /// a NVMe disk with a model and a CD drive, under `/sys/block` links as
    /// the kernel makes them.
    fn sysfs(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let disks = [
            (
                "sda",
                "devices/pci0/usb1/1-1/host0/block/sda",
                "1",
                "60000000",
            ),
            (
                "nvme0n1",
                "devices/pci0/nvme/nvme0/block/nvme0n1",
                "0",
                "1000215216",
            ),
            ("sr0", "devices/pci0/ata1/block/sr0", "1", "2097151"),
            (
                "sdb",
                "devices/pci0/usb1/1-2/host1/block/sdb",
                "1",
                "8000000",
            ),
        ];
        fs::create_dir_all(dir.join("block")).unwrap();
        fs::create_dir_all(dir.join("dev/block")).unwrap();
        for (name, path, removable, sectors) in disks {
            let disk = dir.join(path);
            fs::create_dir_all(disk.join("device")).unwrap();
            fs::write(disk.join("removable"), format!("{removable}\n")).unwrap();
            fs::write(disk.join("size"), format!("{sectors}\n")).unwrap();
            std::os::unix::fs::symlink(format!("../{path}"), dir.join("block").join(name)).unwrap();
        }
        fs::write(
            dir.join("devices/pci0/nvme/nvme0/block/nvme0n1/device/model"),
            "Samsung SSD 980   \n",
        )
        .unwrap();
        let part = dir.join("devices/pci0/usb1/1-1/host0/block/sda/sda2");
        fs::create_dir_all(&part).unwrap();
        fs::write(part.join("partition"), "2\n").unwrap();
        std::os::unix::fs::symlink(
            "../../devices/pci0/usb1/1-1/host0/block/sda/sda2",
            dir.join("dev/block/8:2"),
        )
        .unwrap();
        dir
    }

    const MOUNTINFO: &str = "1 0 0:2 / / rw - rootfs rootfs rw\n\
        22 1 8:2 / / ro,relatime - ext4 /dev/root rw\n\
        23 22 0:5 / /dev rw - devtmpfs devtmpfs rw\n";

    #[test]
    fn finds_the_stick_it_runs_from_and_offers_the_other_disks() {
        let sys = sysfs("install-disks");
        assert_eq!(root_device(MOUNTINFO), Some("8:2"));
        assert_eq!(
            root_device("23 22 0:5 / /dev rw - devtmpfs devtmpfs rw\n"),
            None
        );
        let running = running_disk(MOUNTINFO, &sys);
        assert_eq!(running.as_deref(), Some("sda"));
        assert!(is_removable(&sys, "sda"));
        assert!(!is_removable(&sys, "nvme0n1"));
        assert_eq!(
            disk_choices(&sys, running.as_deref()),
            [
                DiskChoice {
                    name: "nvme0n1".into(),
                    title: "Samsung SSD 980 · 512 GB".into(),
                },
                DiskChoice {
                    name: "sdb".into(),
                    title: "4.1 GB disk".into(),
                },
            ],
            "the computer's own disk first, then another stick; the running stick and the CD drive are not offered"
        );
        let tmpfs = "1 0 0:2 / / rw - tmpfs tmpfs rw\n";
        assert_eq!(running_disk(tmpfs, &sys), None, "/ on no disk");
        let _ = fs::remove_dir_all(&sys);
    }

    #[test]
    fn tells_a_laptop_from_a_vm_and_a_pc() {
        let dir = std::env::temp_dir().join(format!("edel-form-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("class/dmi/id")).unwrap();
        assert_eq!(form_factor(&dir), "pc");
        fs::write(dir.join("class/dmi/id/sys_vendor"), "QEMU\n").unwrap();
        assert_eq!(form_factor(&dir), "vm");
        fs::create_dir_all(dir.join("class/power_supply/BAT0")).unwrap();
        fs::write(dir.join("class/power_supply/BAT0/type"), "Battery\n").unwrap();
        assert_eq!(form_factor(&dir), "laptop");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn checks_each_answer_the_way_the_terminal_and_settings_do() {
        assert_eq!(language_answer("de_DE.UTF-8").as_deref(), Ok("de_DE.UTF-8"));
        assert!(language_answer("").is_err() && language_answer("de DE").is_err());
        assert_eq!(user_answer("ali").as_deref(), Ok("ali"));
        for name in ["root", "live", "Ali", "1ali", ""] {
            assert!(user_answer(name).is_err(), "{name}");
        }
        assert_eq!(
            user_answer("root").unwrap_err(),
            "lowercase letters, digits, - and _, starting with a letter, and not root or live"
        );
        assert_eq!(hostname_answer("ali-laptop").as_deref(), Ok("ali-laptop"));
        assert!(hostname_answer("ali laptop").is_err());
    }

    #[test]
    fn a_file_answers_its_own_questions_and_the_rest_are_added_to_it() {
        let text = "format = 1\n\n# The office's laptops\n[network]\nhostname = \"office-1\"\n\n[users.bea]\nadmin = true\n";
        let file = settings::read(text).unwrap().file;
        assert_eq!(answered(&file), ["user", "network.hostname"]);
        let answers = Answers {
            language: Some("de_DE.UTF-8".into()),
            keyboard: Some("de".into()),
            user: Some("ali".into()),
            hostname: Some("ali-laptop".into()),
            preset: Some("classic".into()),
            ..Answers::default()
        };
        let out = merged(text, &answers).unwrap();
        assert!(
            out.contains("# The office's laptops"),
            "the file's own lines stay: {out}"
        );
        assert!(out.contains("hostname = \"office-1\""), "{out}");
        assert!(!out.contains("ali"), "the file names its people: {out}");
        assert!(out.contains("language = \"de_DE.UTF-8\""), "{out}");
        assert!(out.contains("keyboard = \"de\""), "{out}");
        assert!(!out.contains("classic"), "Classic applies anyway: {out}");
        let empty = merged("format = 1\n", &answers).unwrap();
        assert!(
            empty.contains("[users.ali]") && empty.contains("admin = true"),
            "{empty}"
        );
        assert!(empty.contains("hostname = \"ali-laptop\""), "{empty}");
        assert!(merged("format = 9\n", &answers).is_err());
        assert!(!empty.contains("addons"), "{empty}");
        let japanese = Answers {
            language: Some("ja_JP.UTF-8".into()),
            ..answers
        };
        let out = merged("format = 1\n", &japanese).unwrap();
        assert!(out.contains("installed = [\"fonts-cjk\"]"), "{out}");
        let twice = merged(&out, &japanese).unwrap();
        assert_eq!(twice.matches("fonts-cjk").count(), 1, "{twice}");
    }

    #[test]
    fn finds_the_settings_file_at_the_top_before_efi() {
        let dir = std::env::temp_dir().join(format!("edel-file-in-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("EFI/edel")).unwrap();
        assert_eq!(file_in(&dir), None);
        let efi = crate::places::settings_in(&dir.join("EFI/edel"));
        fs::write(&efi, "format = 1\n# efi\n").unwrap();
        assert_eq!(file_in(&dir), Some((efi, "format = 1\n# efi\n".into())));
        let top = crate::places::settings_in(&dir);
        fs::write(&top, "format = 1\n# top\n").unwrap();
        assert_eq!(file_in(&dir).unwrap().0, top);
        let _ = fs::remove_dir_all(&dir);
    }
}
