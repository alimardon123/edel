//! The settings file: one TOML file that describes a whole machine
//! (ADR-006), named in `edel::places` (ADR-008's same names decision). This is the one parser for it: `edel` today, and the
//! compositor, shell-ui and Settings later, read it with the same code and
//! the same key table, so this module needs no network or signing (ADR-008).
//!
//! Checkers are strict and readers on a machine are lenient (ADR-008,
//! section 2). [`check`] refuses an unknown key, value or format. [`read`]
//! keeps every other key, drops what it cannot use and reports it, because
//! an old slot reads files a newer release wrote and cannot be patched.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use toml::{Table, Value};
use toml_edit::DocumentMut;

/// The settings file format this release reads and writes. A key is never
/// removed or renamed within a format: `tests/keys.txt` lists every key a
/// format has had, and a cargo test holds the structs to it.
pub const FORMAT: i64 = 1;

/// One page of the Settings app and the section of the settings file it
/// shows: `edel settings` lists them in this order, and `edel settings get
/// SECTION` shows one in the app's words (ADR-008's same names decision).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page {
    pub section: &'static str,
    pub title: &'static str,
    /// What the page holds, in a few words
    pub about: &'static str,
}

/// The Settings app's pages, in its order; every section of the file is
/// one of them. Settings (M5.6) reads the same table.
pub const PAGES: &[Page] = &[
    Page {
        section: "layout",
        title: "Layout",
        about: "the preset, tiling, title bars and panels",
    },
    Page {
        section: "displays",
        title: "Displays",
        about: "each screen's place, scale and resolution",
    },
    Page {
        section: "appearance",
        title: "Appearance",
        about: "light or dark, the accent, fonts and animations",
    },
    Page {
        section: "shortcuts",
        title: "Shortcuts",
        about: "the keys for each action",
    },
    Page {
        section: "region",
        title: "Region",
        about: "language, keyboard and time zone",
    },
    Page {
        section: "users",
        title: "Users",
        about: "the people who log in, and their ssh keys",
    },
    Page {
        section: "network",
        title: "Network",
        about: "the device's name",
    },
    Page {
        section: "default_apps",
        title: "Default apps",
        about: "the browser, files, editor, terminal and mail",
    },
    Page {
        section: "startup",
        title: "Startup",
        about: "apps that start after login",
    },
    Page {
        section: "power",
        title: "Power",
        about: "the lid, the power button and the screen lock",
    },
    Page {
        section: "services",
        title: "Services",
        about: "optional services, on or off",
    },
    Page {
        section: "updates",
        title: "Updates",
        about: "when updates are installed",
    },
    Page {
        section: "apps",
        title: "Apps",
        about: "the apps installed from Flathub",
    },
    Page {
        section: "addons",
        title: "Add-ons",
        about: "signed extras to the system",
    },
    Page {
        section: "system",
        title: "System",
        about: "developer mode and profiles",
    },
];

/// The page holding `section`.
pub fn page(section: &str) -> Option<&'static Page> {
    PAGES.iter().find(|p| p.section == section)
}

/// What a key's value must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Text in quotes
    Text,
    /// `true` or `false`
    Flag,
    /// A list of texts
    Texts,
    /// A whole number, 0 or more
    Whole,
    /// A number such as `1.25`
    Number,
    /// Two whole numbers, `[x, y]`
    Pair,
    /// One of these texts
    OneOf(&'static [&'static str]),
    /// One of these whole numbers
    WholeOf(&'static [i64]),
    /// A hostname: letters, digits and hyphens, up to 63
    Hostname,
    /// A login shell: an absolute path with no `:` and no control
    /// characters, which `/etc/passwd` could not hold
    Shell,
    /// Keys for a shortcut, such as `"Super+Q"`, or `""` for none
    /// ([`crate::shortcuts::parse`])
    Keys,
    /// Panels, as a preset's `[[panels]]` (M5.4e,
    /// [`crate::presets::check_panels`])
    Panels,
    /// A screen's resolution, `WIDTHxHEIGHT` such as `"1920x1080"`
    Resolution,
    /// Keyboard layouts as xkb names them, such as `"us,ru"` or
    /// `"de(nodeadkeys)"` ([`crate::keyboard::normalize`], M5.21)
    Keyboard,
}

/// One key of the settings file. `*` in a path stands for any name, such as a
/// user or an output.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub path: &'static str,
    pub kind: Kind,
    /// Whether this release acts on the key. Keys that later steps fill in
    /// are parsed now, so adding them is not a format bump; `check` refuses
    /// them with "not supported yet" and the boot apply skips them.
    pub supported: bool,
}

const fn now(path: &'static str, kind: Kind) -> Key {
    Key {
        path,
        kind,
        supported: true,
    }
}

const fn later(path: &'static str, kind: Kind) -> Key {
    Key {
        path,
        kind,
        supported: false,
    }
}

/// The layout presets (ADR-002).
pub const PRESETS: &[&str] = &[
    "classic",
    "mac-like",
    "windows-like",
    "hive",
    "tablet",
    "phone",
];

/// Every key of format 1. Append only; `tests/keys.txt` must list each one.
pub const KEYS: &[Key] = &[
    later(
        "system.variant",
        Kind::OneOf(&["container", "server", "desktop", "phone"]),
    ),
    now("system.developer_mode", Kind::Flag),
    later("system.profiles", Kind::Texts),
    now("users.*.admin", Kind::Flag),
    now("users.*.ssh_keys", Kind::Texts),
    now("users.*.login_shell", Kind::Shell),
    now("network.hostname", Kind::Hostname),
    later("region.language", Kind::Text),
    now("region.keyboard", Kind::Keyboard),
    later("region.timezone", Kind::Text),
    now("layout.preset", Kind::OneOf(crate::presets::NAMES)),
    now("layout.tiling", Kind::Flag),
    now(
        "layout.title_bars",
        Kind::OneOf(&["always", "floating-only"]),
    ),
    later(
        "layout.device_type",
        Kind::OneOf(&["desktop", "tablet", "phone"]),
    ),
    now("layout.window_buttons", Kind::OneOf(&["left", "right"])),
    now("layout.panels", Kind::Panels),
    now("displays.*.position", Kind::Pair),
    now("displays.*.scale", Kind::Number),
    now("displays.*.resolution", Kind::Resolution),
    now("displays.*.refresh_rate", Kind::Number),
    now("displays.*.enabled", Kind::Flag),
    later("displays.*.rotation", Kind::WholeOf(&[0, 90, 180, 270])),
    later("appearance.wallpaper", Kind::Text),
    now("appearance.mode", Kind::OneOf(&["light", "dark", "auto"])),
    later("appearance.accent", Kind::Text),
    later("appearance.font", Kind::Text),
    later("appearance.font_size", Kind::Number),
    later("appearance.cursor_size", Kind::Whole),
    later("appearance.icon_size", Kind::Whole),
    now(
        "appearance.animations",
        Kind::OneOf(&["full", "reduced", "off"]),
    ),
    now("shortcuts.*", Kind::Keys),
    later("default_apps.browser", Kind::Text),
    later("default_apps.files", Kind::Text),
    later("default_apps.editor", Kind::Text),
    later("default_apps.terminal", Kind::Text),
    later("default_apps.mail", Kind::Text),
    later("startup.apps", Kind::Texts),
    later(
        "power.lid_close",
        Kind::OneOf(&["suspend", "lock", "nothing", "poweroff"]),
    ),
    later("power.lock_after_minutes", Kind::Whole),
    later(
        "power.power_button",
        Kind::OneOf(&["suspend", "poweroff", "ask", "nothing"]),
    ),
    later(
        "power.on_battery",
        Kind::OneOf(&["power-saver", "balanced", "performance"]),
    ),
    later("services.*", Kind::Flag),
    later("updates.channel", Kind::Text),
    later("updates.version", Kind::Text),
    later(
        "updates.automatic",
        Kind::OneOf(&["off", "check", "install", "install-and-restart"]),
    ),
    later("updates.restart_window", Kind::Text),
    later("apps.installed", Kind::Texts),
    later("addons.installed", Kind::Texts),
];

/// A whole machine. Every key is optional: an absent key means the release
/// decides (ADR-008, section 3), so nothing here has a default of its own.
/// Each section is a page of the Settings app ([`PAGES`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SystemFile {
    pub format: i64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub layout: Layout,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub displays: BTreeMap<String, Display>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub appearance: Appearance,
    /// Action name to keys, such as `close_window = "Super+Q"`
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub shortcuts: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub region: Region,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub users: BTreeMap<String, User>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub network: Network,
    #[serde(default, skip_serializing_if = "is_default")]
    pub default_apps: DefaultApps,
    #[serde(default, skip_serializing_if = "is_default")]
    pub startup: Startup,
    #[serde(default, skip_serializing_if = "is_default")]
    pub power: Power,
    /// Feature name to on or off, such as `ssh = false`
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, bool>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub updates: Updates,
    #[serde(default, skip_serializing_if = "is_default")]
    pub apps: Apps,
    #[serde(default, skip_serializing_if = "is_default")]
    pub addons: Addons,
    #[serde(default, skip_serializing_if = "is_default")]
    pub system: System,
}

impl Default for SystemFile {
    /// A file in this release's format with every key absent.
    fn default() -> Self {
        SystemFile {
            format: FORMAT,
            layout: Layout::default(),
            displays: BTreeMap::new(),
            appearance: Appearance::default(),
            shortcuts: BTreeMap::new(),
            region: Region::default(),
            users: BTreeMap::new(),
            network: Network::default(),
            default_apps: DefaultApps::default(),
            startup: Startup::default(),
            power: Power::default(),
            services: BTreeMap::new(),
            updates: Updates::default(),
            apps: Apps::default(),
            addons: Addons::default(),
            system: System::default(),
        }
    }
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// The System page: what kind of system this is, developer mode
/// (ADR-007) and profiles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct System {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub developer_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Vec<String>>,
}

/// One person on the Users page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct User {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_keys: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_shell: Option<String>,
}

/// The Network page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Network {
    /// The row "Device name (hostname)"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

/// The Region page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Region {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyboard: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// The Layout page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiling: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_bars: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_type: Option<String>,
    /// The title bar buttons' side (M5.4b); absent is the preset's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_buttons: Option<String>,
    /// The panels in place of the preset's (M5.4e); absent is the
    /// preset's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panels: Option<Vec<crate::presets::Panel>>,
}

/// One screen on the Displays page, by its connector's name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Display {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<[i64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// `WIDTHxHEIGHT`, such as `1920x1080`; absent is the screen's own
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// In Hz, such as `60` or `59.94`; absent is the resolution's best
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<u32>,
}

impl Display {
    /// The mode to ask the screen for, `WIDTHxHEIGHT` or
    /// `WIDTHxHEIGHT@HZ`, from the resolution and the refresh rate.
    pub fn mode(&self) -> Option<String> {
        let resolution = self.resolution.as_deref()?;
        Some(match self.refresh_rate {
            Some(hz) => format!("{resolution}@{hz}"),
            None => resolution.to_string(),
        })
    }
}

/// The Appearance page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<String>,
    /// Light, dark or automatic
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_size: Option<u32>,
    /// Full, reduced or off
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animations: Option<String>,
}

/// The Default apps page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DefaultApps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mail: Option<String>,
}

/// The Startup page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Startup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<Vec<String>>,
}

/// The Power page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Power {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lid_close: Option<String>,
    /// Minutes without input before the screen locks; 0 never locks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_after_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_button: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_battery: Option<String>,
}

/// The Updates page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Updates {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_window: Option<String>,
}

/// The Apps page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Apps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed: Option<Vec<String>>,
}

/// The Add-ons page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Addons {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed: Option<Vec<String>>,
}

/// Something a reader left out: an unknown key, or a value of the wrong
/// kind. The key's default is used instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub key: String,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key, self.message)
    }
}

/// A file as a lenient reader sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    /// Every key that could be used
    pub file: SystemFile,
    /// What was left out, and why
    pub problems: Vec<Problem>,
    /// Keys that were read but that no part of this release acts on yet
    pub later: Vec<String>,
}

/// Reads a settings file leniently: unknown keys and values of the wrong
/// kind are left out and reported, and every other key is kept. Only a file
/// that is not TOML, or not in this release's format, is refused.
pub fn read(text: &str) -> Result<Read> {
    let table: Table = toml::from_str(text).context("the file is not valid TOML")?;
    let format = format_of(&table)?;
    if format != FORMAT {
        bail!("the file is format {format}, and this release reads format {FORMAT}");
    }
    read_table(&table)
}

/// `edel settings check`: what a strict checker refuses in a file, one line
/// per problem; empty when the file is fine. Keys no part acts on yet are
/// refused too, so a file never promises what the machine will not do.
pub fn check(text: &str) -> Result<Vec<String>> {
    let read = read(text)?;
    let mut lines: Vec<String> = read.problems.iter().map(|p| p.to_string()).collect();
    lines.extend(
        read.later
            .iter()
            .map(|key| format!("{key}: not supported yet")),
    );
    // Across keys: unknown actions, two on one key, a way out unbound.
    lines.extend(crate::shortcuts::check(&read.file.shortcuts));
    Ok(lines)
}

/// Reads the machine's settings file the way an unattended reader must
/// (ADR-008): a file in a newer format is not read; the file's `.v<N>`
/// beside it, left by `edel migrate` for this release's format, is read
/// instead. Without one this fails, and the caller applies nothing.
pub fn read_on_machine(path: &Path) -> Result<Read> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let table: Table = toml::from_str(&text)
        .with_context(|| format!("{} is not valid TOML; nothing is applied", path.display()))?;
    let format = format_of(&table)?;
    if format <= FORMAT {
        return read(&text);
    }
    let older = versioned(path, FORMAT);
    let Ok(text) = fs::read_to_string(&older) else {
        bail!(
            "{} is format {format}, newer than this release reads, and there is no {} beside it; nothing is applied",
            path.display(),
            older.display()
        );
    };
    let mut read = read(&text).with_context(|| format!("reading {}", older.display()))?;
    read.problems.insert(
        0,
        Problem {
            key: "format".into(),
            message: format!(
                "format {format} is newer than this release, so {} was read instead",
                older.display()
            ),
        },
    );
    Ok(read)
}

/// The `format` of a settings file's text.
pub fn format(text: &str) -> Result<i64> {
    let table: Table = toml::from_str(text).context("the file is not valid TOML")?;
    format_of(&table)
}

/// Where a key's value comes from, in the order every reader resolves it
/// (ADR-008): the person's own file, then the machine's, else the release
/// decides. Settings shows it on each row (M5.6b), as `edel settings
/// get` does.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// The person's own file sets it.
    Person(Value),
    /// The machine's file sets it, and the person's does not.
    Machine(Value),
    /// Neither: the preset or the release decides.
    Release,
}

/// Where `key` (such as `layout.preset`) comes from, given the machine's
/// and the person's files' text; a file that is missing or not TOML sets
/// nothing.
pub fn source(key: &str, machine: Option<&str>, person: Option<&str>) -> Source {
    let find = |text: Option<&str>| -> Option<Value> {
        let table: Table = toml::from_str(text?).ok()?;
        let mut value = Value::Table(table);
        for part in key.split('.') {
            value = value.as_table()?.get(part)?.clone();
        }
        Some(value)
    };
    match (find(person), find(machine)) {
        (Some(v), _) => Source::Person(v),
        (None, Some(v)) => Source::Machine(v),
        (None, None) => Source::Release,
    }
}

/// `edel settings set KEY=VALUE` on a file's text (ADR-008, writers): checks
/// the key and value as strictly as `check`, then changes that one value in
/// place, so comments, order and keys this release does not know survive
/// byte for byte. VALUE is TOML when it reads as TOML (`true`, `2`,
/// `["a"]`, `"x"`) and plain text otherwise, so `hostname=lab-1` works.
pub fn set(text: &str, key: &str, value: &str) -> Result<String> {
    let path: Vec<&str> = key.split('.').collect();
    let entry = known_key(key, &path)?;
    let value = normalize(entry.kind, &value_from_arg(value)).map_err(|m| anyhow!("{key}: {m}"))?;
    if !entry.supported {
        bail!("{key}: not supported yet");
    }
    let new = toml_edit_value(&value)?;
    let mut doc: DocumentMut = text.parse().context("the file is not valid TOML")?;
    let (last, parents) = path.split_last().context("no key given")?;
    let mut table = doc.as_table_mut();
    for (i, part) in parents.iter().enumerate() {
        let item = table.entry(part).or_insert_with(|| {
            let mut new = toml_edit::Table::new();
            // Only the table holding the key gets a [header].
            new.set_implicit(i + 1 < parents.len());
            toml_edit::Item::Table(new)
        });
        table = item.as_table_mut().with_context(|| {
            format!(
                "{} is not a table in the file; change {key} by hand",
                path[..=i].join(".")
            )
        })?;
    }
    match table.get_mut(last).and_then(toml_edit::Item::as_value_mut) {
        Some(old) => {
            let decor = old.decor().clone();
            *old = new;
            *old.decor_mut() = decor;
        }
        None => {
            table.insert(last, toml_edit::Item::Value(new));
        }
    }
    let text = doc.to_string();
    // A shortcut must leave the table whole: no two actions on one key.
    if path.first() == Some(&"shortcuts") {
        let lines = crate::shortcuts::check(&read(&text)?.file.shortcuts);
        if !lines.is_empty() {
            bail!("{}", lines.join("; "));
        }
    }
    Ok(text)
}

/// `edel settings reset KEY` on a file's text: removes the key, and tables
/// left empty by it, so the release decides again (ADR-008). Every other
/// byte stays. A key this release does not know can be removed too. A
/// user's table stays when its last key goes, so apply still looks after
/// the user and takes back what the key gave, such as admin.
pub fn unset(text: &str, key: &str) -> Result<String> {
    let path: Vec<&str> = key.split('.').collect();
    let mut doc: DocumentMut = text.parse().context("the file is not valid TOML")?;
    let removed = match path.as_slice() {
        ["users", name, last] => doc
            .get_mut("users")
            .and_then(|users| users.as_table_like_mut()?.get_mut(name))
            .and_then(|user| user.as_table_like_mut())
            .is_some_and(|user| user.remove(last).is_some()),
        _ => remove_path(doc.as_table_mut(), &path)?,
    };
    if !removed {
        bail!("{key} is not in the file");
    }
    Ok(doc.to_string())
}

fn remove_path(table: &mut toml_edit::Table, path: &[&str]) -> Result<bool> {
    let [first, rest @ ..] = path else {
        return Ok(false);
    };
    if rest.is_empty() {
        return Ok(table.remove(first).is_some());
    }
    let Some(inner) = table.get_mut(first) else {
        return Ok(false);
    };
    let inner = inner
        .as_table_mut()
        .with_context(|| format!("{first} is not a table in the file; change it by hand"))?;
    let removed = remove_path(inner, rest)?;
    if removed && inner.is_empty() {
        table.remove(first);
    }
    Ok(removed)
}

/// The key table's entry for `path`, or why a writer refuses it.
fn known_key(key: &str, path: &[&str]) -> Result<&'static Key> {
    let names: Vec<String> = path.iter().map(|p| p.to_string()).collect();
    if path.first() == Some(&"users") && path.len() > 1 && !is_user_name(path[1]) {
        bail!("{key}: {:?} is not a user name", path[1]);
    }
    KEYS.iter()
        .find(|k| matches(k.path, &names, false))
        .ok_or_else(|| match nearest_key(key) {
            Some(near) => anyhow!("{key}: unknown key; did you mean {near}?"),
            None => anyhow!("{key}: unknown key; edel settings lists the pages, and edel settings get PAGE their keys"),
        })
}

/// The known key nearest to the mistyped `key`, such as `layout.preset` for
/// `shell.presset`, when one is close enough to be meant; a name standing
/// for `*`, such as a user's or a screen's, is kept as typed.
pub fn nearest_key(key: &str) -> Option<String> {
    let typed: Vec<&str> = key.split('.').collect();
    KEYS.iter()
        .filter_map(|k| {
            let parts: Vec<&str> = k.path.split('.').collect();
            (parts.len() == typed.len()).then(|| {
                parts
                    .iter()
                    .zip(&typed)
                    .map(|(p, t)| if *p == "*" { *t } else { *p })
                    .collect::<Vec<_>>()
                    .join(".")
            })
        })
        .chain(PAGES.iter().map(|p| p.section.to_string()))
        .map(|candidate| (distance(key, &candidate), candidate))
        .filter(|(d, candidate)| *d > 0 && *d <= (candidate.len() / 4).max(2))
        .min_by_key(|(d, _)| *d)
        .map(|(_, candidate)| candidate)
}

/// The value among `allowed` nearest to the mistyped `value`.
fn nearest_value<'a>(value: &str, allowed: &[&'a str]) -> Option<&'a str> {
    allowed
        .iter()
        .map(|a| (distance(value, a), *a))
        .filter(|(d, a)| *d > 0 && *d <= (a.len() / 3).max(2))
        .min_by_key(|(d, _)| *d)
        .map(|(_, a)| a)
}

/// How many letters to add, drop or change to turn `a` into `b`
/// (Levenshtein's distance).
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let replaced = previous + usize::from(ca != *cb);
            previous = row[j + 1];
            row[j + 1] = replaced.min(row[j] + 1).min(previous + 1);
        }
    }
    row[b.len()]
}

fn value_from_arg(raw: &str) -> Value {
    toml::from_str::<Table>(&format!("v = {raw}"))
        .ok()
        .and_then(|mut t| t.remove("v"))
        .unwrap_or_else(|| Value::String(raw.to_string()))
}

fn toml_edit_value(value: &Value) -> Result<toml_edit::Value> {
    let mut table = Table::new();
    table.insert("v".into(), value.clone());
    let doc: DocumentMut = toml::to_string(&table)?.parse()?;
    // A list of tables, such as layout.panels, comes back as [[v]]
    // tables; it is written inline, on the key's one line.
    doc.get("v")
        .cloned()
        .and_then(|item| item.into_value().ok())
        .map(|mut v| {
            if let toml_edit::Value::Array(items) = &mut v {
                items.fmt();
                for item in items.iter_mut() {
                    if let toml_edit::Value::InlineTable(table) = item {
                        table.fmt();
                    }
                }
            }
            v.decor_mut().clear();
            v
        })
        .context("cannot write the value")
}

/// The settings file `path` with `.v1` added, for format 1.
pub fn versioned(path: &Path, format: i64) -> PathBuf {
    PathBuf::from(format!("{}.v{format}", path.display()))
}

fn format_of(table: &Table) -> Result<i64> {
    match table.get("format") {
        Some(Value::Integer(format)) if *format >= 1 => Ok(*format),
        Some(other) => bail!("format must be a whole number from 1, not {other}"),
        None => bail!("the file has no format; add format = {FORMAT} at the top"),
    }
}

fn read_table(table: &Table) -> Result<Read> {
    let mut problems = Vec::new();
    let mut later = Vec::new();
    let kept = clean(table, &[], &mut problems, &mut later);
    let mut ignored = Vec::new();
    let file: SystemFile =
        serde_ignored::deserialize(Value::Table(kept), |path| ignored.push(path.to_string()))
            .context("the key table and the structs disagree")?;
    // The key table already left out unknown keys; anything ignored here is
    // a key the table lists but the structs lack, which a test prevents.
    problems.extend(ignored.into_iter().map(|key| Problem {
        key,
        message: "unknown key".into(),
    }));
    Ok(Read {
        file,
        problems,
        later,
    })
}

/// Keeps the keys of `table` (found at `at`) that the key table knows with
/// a value of the right kind, and reports the rest.
fn clean(
    table: &Table,
    at: &[String],
    problems: &mut Vec<Problem>,
    later: &mut Vec<String>,
) -> Table {
    let mut kept = Table::new();
    for (name, value) in table {
        let mut path = at.to_vec();
        path.push(name.clone());
        let shown = path.join(".");
        if at.is_empty() && name == "format" {
            kept.insert(name.clone(), value.clone());
        } else if at == ["users"] && !is_user_name(name) {
            problems.push(Problem {
                key: shown,
                message: "not a user name; use up to 32 lowercase letters, digits, - and _, starting with a letter or _".into(),
            });
        } else if let Some(key) = KEYS.iter().find(|k| matches(k.path, &path, false)) {
            match normalize(key.kind, value) {
                Ok(value) => {
                    if !key.supported {
                        later.push(shown);
                    }
                    kept.insert(name.clone(), value);
                }
                Err(message) => problems.push(Problem {
                    key: shown,
                    message,
                }),
            }
        } else if KEYS.iter().any(|k| matches(k.path, &path, true)) {
            match value {
                Value::Table(inner) => {
                    let inner = clean(inner, &path, problems, later);
                    kept.insert(name.clone(), Value::Table(inner));
                }
                _ => problems.push(Problem {
                    key: shown,
                    message: "expected a table".into(),
                }),
            }
        } else {
            problems.push(Problem {
                key: shown,
                message: "unknown key".into(),
            });
        }
    }
    kept
}

/// Whether `path` is the key `pattern` or, with `prefix`, a table on the
/// way to it.
fn matches(pattern: &str, path: &[String], prefix: bool) -> bool {
    let parts: Vec<&str> = pattern.split('.').collect();
    let fits = if prefix {
        parts.len() > path.len()
    } else {
        parts.len() == path.len()
    };
    fits && parts
        .iter()
        .zip(path)
        .all(|(p, name)| *p == "*" || p == name)
}

/// `value` if it is of `kind` (a whole number becomes a number where one
/// is expected), else what is wrong with it.
fn normalize(kind: Kind, value: &Value) -> Result<Value, String> {
    let fail = |expected: &str| Err(format!("expected {expected}, not {value}"));
    match (kind, value) {
        (Kind::Text, Value::String(_)) | (Kind::Flag, Value::Boolean(_)) => Ok(value.clone()),
        (Kind::Text, _) => fail("text in quotes"),
        (Kind::Flag, _) => fail("true or false"),
        (Kind::Texts, Value::Array(items)) if items.iter().all(Value::is_str) => Ok(value.clone()),
        (Kind::Texts, _) => fail("a list of texts in quotes"),
        (Kind::Whole, Value::Integer(n)) if u32::try_from(*n).is_ok() => Ok(value.clone()),
        (Kind::Whole, _) => fail("a whole number from 0"),
        (Kind::Number, Value::Float(n)) if n.is_finite() => Ok(value.clone()),
        (Kind::Number, Value::Integer(n)) => Ok(Value::Float(*n as f64)),
        (Kind::Number, _) => fail("a number"),
        (Kind::Pair, Value::Array(items))
            if items.len() == 2 && items.iter().all(Value::is_integer) =>
        {
            Ok(value.clone())
        }
        (Kind::Pair, _) => fail("two whole numbers, such as [0, 0]"),
        (Kind::OneOf(allowed), Value::String(s)) if allowed.contains(&s.as_str()) => {
            Ok(value.clone())
        }
        (Kind::OneOf(allowed), Value::String(s)) => Err(match nearest_value(s, allowed) {
            Some(near) => format!(
                "unknown value {s:?}; did you mean {near}? Use {}",
                or_list(allowed)
            ),
            None => format!("unknown value {s:?}; use {}", or_list(allowed)),
        }),
        (Kind::OneOf(allowed), _) => fail(&or_list(allowed)),
        (Kind::WholeOf(allowed), Value::Integer(n)) if allowed.contains(n) => Ok(value.clone()),
        (Kind::WholeOf(allowed), _) => {
            let allowed: Vec<String> = allowed.iter().map(|n| n.to_string()).collect();
            let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
            fail(&or_list(&allowed))
        }
        (Kind::Hostname, Value::String(s)) if is_hostname(s) => Ok(value.clone()),
        (Kind::Hostname, _) => fail("a hostname: letters, digits and hyphens, up to 63"),
        (Kind::Shell, Value::String(s)) if is_shell_path(s) => Ok(value.clone()),
        (Kind::Shell, _) => fail("a login shell's full path, such as \"/bin/sh\""),
        (Kind::Keys, Value::String(s)) => crate::shortcuts::normalize(s).map(Value::String),
        (Kind::Keys, _) => fail("keys in quotes, such as \"Super+Q\""),
        (Kind::Panels, Value::Array(_)) => {
            let panels: Vec<crate::presets::Panel> =
                value.clone().try_into().map_err(|e: toml::de::Error| {
                    format!("expected panels as a preset writes them: {}", e.message())
                })?;
            crate::presets::check_panels(&panels).map_err(|e| e.to_string())?;
            Ok(value.clone())
        }
        (Kind::Resolution, Value::String(r)) if is_resolution(r) => Ok(value.clone()),
        (Kind::Resolution, _) => fail("a resolution such as \"1920x1080\""),
        (Kind::Keyboard, Value::String(s)) => {
            let rules = std::fs::read_to_string(crate::keyboard::RULES).ok();
            crate::keyboard::normalize(s, rules.as_deref()).map(Value::String)
        }
        (Kind::Keyboard, _) => fail("keyboard layouts in quotes, such as \"us\" or \"us,ru\""),
        (Kind::Panels, _) => {
            fail("a list of panels, such as [{ edge = \"bottom\", end = [\"clock\"] }]")
        }
    }
}

/// A single DNS label, as `hostname` and `/etc/hostname` take it.
pub fn is_hostname(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A login shell `/etc/passwd` can hold: an absolute path of at most 255
/// bytes, with no `:` (the field separator) and no control characters.
pub fn is_shell_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 255
        && !path.contains(':')
        && !path.chars().any(char::is_control)
}

/// `WIDTHxHEIGHT` in pixels, such as `1920x1080`.
fn is_resolution(text: &str) -> bool {
    text.split_once('x').is_some_and(|(w, h)| {
        [w, h].iter().all(|n| {
            !n.is_empty()
                && n.len() <= 5
                && n.bytes().all(|b| b.is_ascii_digit())
                && !n.starts_with('0')
        })
    })
}

/// A name busybox `adduser` accepts and every tool handles.
pub fn is_user_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'_'))
        && name.len() <= 32
        && bytes.all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
}

/// "light, dark or auto"
fn or_list(items: &[&str]) -> String {
    match items.split_last() {
        Some((last, [])) => (*last).to_string(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example in ADR-006, so the decision record and the parser agree.
    fn adr_006_example() -> &'static str {
        let adr = include_str!("../../../docs/ADR-006-atomic-updates-and-replication.md");
        let start = adr.find("```toml\n").expect("ADR-006 has a toml example") + 8;
        let len = adr[start..].find("```").expect("the example ends");
        &adr[start..start + len]
    }

    #[test]
    fn a_value_comes_from_the_person_then_the_machine_then_the_release() {
        let machine = "format = 1\n[layout]\npreset = \"hive\"\n";
        let person = "format = 1\n[layout]\ntiling = false\n";
        assert_eq!(
            source("layout.preset", Some(machine), Some(person)),
            Source::Machine(Value::String("hive".into()))
        );
        assert_eq!(
            source("layout.tiling", Some(machine), Some(person)),
            Source::Person(Value::Boolean(false))
        );
        assert_eq!(
            source("layout.panels", Some(machine), None),
            Source::Release
        );
        assert_eq!(
            source("layout.preset", Some("not toml ["), None),
            Source::Release
        );
    }

    fn sample(kind: Kind) -> Value {
        match kind {
            Kind::Text => Value::String("x".into()),
            Kind::Flag => Value::Boolean(true),
            Kind::Texts => Value::Array(vec![Value::String("x".into())]),
            Kind::Whole => Value::Integer(1),
            Kind::Number => Value::Float(1.5),
            Kind::Pair => Value::Array(vec![Value::Integer(0), Value::Integer(-1)]),
            Kind::OneOf(allowed) => Value::String(allowed[0].into()),
            Kind::WholeOf(allowed) => Value::Integer(allowed[0]),
            Kind::Hostname => Value::String("x".into()),
            Kind::Shell => Value::String("/bin/sh".into()),
            Kind::Keys => Value::String("Super+W".into()),
            Kind::Panels => value_from_arg(r#"[{ edge = "bottom", end = ["clock"] }]"#),
            Kind::Resolution => Value::String("1920x1080".into()),
            Kind::Keyboard => Value::String("us".into()),
        }
    }

    /// A table that sets each of `paths` to a sample value, with `*` as `x`.
    fn with_keys<'a>(paths: impl IntoIterator<Item = &'a str>) -> Table {
        let mut root = Table::new();
        root.insert("format".into(), Value::Integer(FORMAT));
        for path in paths {
            let key = KEYS.iter().find(|k| k.path == path).expect("a known key");
            let parts: Vec<&str> = path
                .split('.')
                .map(|p| if p == "*" { "x" } else { p })
                .collect();
            let mut table = &mut root;
            for part in &parts[..parts.len() - 1] {
                table = table
                    .entry(part.to_string())
                    .or_insert_with(|| Value::Table(Table::new()))
                    .as_table_mut()
                    .expect("a table");
            }
            table.insert(parts[parts.len() - 1].into(), sample(key.kind));
        }
        root
    }

    fn keys_txt() -> Vec<&'static str> {
        include_str!("../tests/keys.txt")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect()
    }

    const MIXED: &str = r#"
format = 1

[network]
hostname = "lab-1"

[appearance]
acent = "red"
mode = "sepia"
font_size = 11
"#;

    #[test]
    fn parses_the_adr_006_example() {
        let read = read(adr_006_example()).unwrap();
        assert_eq!(read.problems, []);
        assert_eq!(read.file.network.hostname.as_deref(), Some("ali-laptop"));
        assert_eq!(read.file.users["ali"].admin, Some(true));
        assert_eq!(
            read.file.addons.installed,
            Some(vec!["virtualization".into()])
        );
        assert_eq!(read.file.layout.preset.as_deref(), Some("classic"));
    }

    #[test]
    fn check_names_an_unknown_key() {
        let lines = check(MIXED).unwrap();
        assert!(lines.contains(&"appearance.acent: unknown key".to_string()));
    }

    #[test]
    fn check_lists_the_allowed_values() {
        let lines = check(MIXED).unwrap();
        assert!(lines.contains(
            &"appearance.mode: unknown value \"sepia\"; use light, dark or auto".to_string()
        ));
        assert!(lines.contains(&"appearance.font_size: not supported yet".to_string()));
        assert!(!lines.iter().any(|line| line.starts_with("network.")));
    }

    #[test]
    fn check_refuses_a_newer_format() {
        let error = check("format = 2\n").unwrap_err().to_string();
        assert!(error.contains("format 2"), "{error}");
        assert!(check("[network]\nhostname = \"a\"\n").is_err());
    }

    #[test]
    fn a_lenient_read_keeps_the_rest_and_reports() {
        let read = read(MIXED).unwrap();
        assert_eq!(read.file.network.hostname.as_deref(), Some("lab-1"));
        assert_eq!(read.file.appearance.mode, None);
        assert_eq!(read.file.appearance.font_size, Some(11.0));
        let keys: Vec<&str> = read.problems.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["appearance.acent", "appearance.mode"]);
    }

    #[test]
    fn values_of_the_wrong_kind_fall_back() {
        let read = read(
            "format = 1\n[users.ci]\nadmin = \"yes\"\nlogin_shell = \"/bin/sh\"\n[displays.DP-1]\nrotation = 45\n[network]\nhostname = [1]\n",
        )
        .unwrap();
        assert_eq!(read.file.users["ci"].admin, None);
        assert_eq!(
            read.file.users["ci"].login_shell.as_deref(),
            Some("/bin/sh")
        );
        assert_eq!(read.file.network.hostname, None);
        let shown: Vec<String> = read.problems.iter().map(|p| p.to_string()).collect();
        assert!(shown.contains(&"users.ci.admin: expected true or false, not \"yes\"".into()));
        assert!(
            shown.contains(&"displays.DP-1.rotation: expected 0, 90, 180 or 270, not 45".into())
        );
    }

    /// A preset this release lacks (M5.4a): check and set refuse it and
    /// list the ones it has, while a reader on a machine leaves it out and
    /// says so, so the desktop starts Classic (ADR-008).
    #[test]
    fn panels_are_checked_as_a_presets_and_read_as_tables_or_inline() {
        let set_one = set(
            "format = 1\n",
            "layout.panels",
            r#"[{ edge = "bottom", end = ["clock"] }]"#,
        )
        .unwrap();
        assert_eq!(
            set_one,
            "format = 1\n\n[layout]\npanels = [{ edge = \"bottom\", end = [\"clock\"] }]\n"
        );
        let tables = "format = 1\n[[layout.panels]]\nedge = \"top\"\nstart = [\"menu\"]\n\n[[layout.panels]]\nedge = \"bottom\"\nstyle = \"dock\"\ncentre = [\"apps\"]\n";
        assert!(check(tables).unwrap().is_empty());
        let panels = read(tables).unwrap().file.layout.panels.unwrap();
        assert_eq!(panels.len(), 2);
        assert_eq!(panels[1].style, crate::presets::Style::Dock);
        // Two along one edge, a name no widget could have, a key a panel
        // lacks: refused by set, left out and reported by a lenient read.
        for (bad, why) in [
            (
                r#"[{ edge = "top" }, { edge = "top" }]"#,
                "two panels along the top edge",
            ),
            (
                r#"[{ edge = "top", end = ["Clock!"] }]"#,
                "is not a widget name",
            ),
            (
                r#"[{ edge = "top", colour = "red" }]"#,
                "expected panels as a preset writes them",
            ),
            (r#""clock""#, "expected a list of panels"),
        ] {
            let error = set("format = 1\n", "layout.panels", bad).unwrap_err();
            assert!(error.to_string().contains(why), "{bad}: {error}");
            let file = format!("format = 1\n[layout]\npanels = {bad}\n");
            let read = read(&file).unwrap();
            assert_eq!(read.file.layout.panels, None, "{bad}");
            assert_eq!(read.problems[0].key, "layout.panels");
        }
    }

    #[test]
    fn a_preset_this_release_lacks_is_refused_or_reported() {
        let file = "format = 1\n[layout]\npreset = \"tablet\"\n";
        let lines = check(file).unwrap();
        assert_eq!(
            lines,
            [
                "layout.preset: unknown value \"tablet\"; use classic, hive, mac-like or windows-like"
            ]
        );
        let error = set("format = 1\n", "layout.preset", "tablet").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("use classic, hive, mac-like or windows-like"),
            "{error}"
        );
        let read = read(file).unwrap();
        assert_eq!(read.file.layout.preset, None);
        assert_eq!(read.problems[0].key, "layout.preset");
        let set_hive = set("format = 1\n", "layout.preset", "hive").unwrap();
        assert_eq!(set_hive, "format = 1\n\n[layout]\npreset = \"hive\"\n");
        assert!(check(&set_hive).unwrap().is_empty());
    }

    const EDITED: &str = "format = 1\nfuture.key = 1 # kept\n\n[network]\nhostname = \"ci-seeded\"  # mine\n\n# The person who runs CI\n[users.ci]\nadmin = true\n";

    #[test]
    fn set_changes_one_value_and_keeps_every_other_byte() {
        let changed = set(EDITED, "network.hostname", "other").unwrap();
        assert_eq!(changed, EDITED.replace("\"ci-seeded\"", "\"other\""));
        let added = set(&changed, "users.ali.admin", "true").unwrap();
        assert!(added.starts_with(&changed), "{added}");
        assert!(added.ends_with("\n[users.ali]\nadmin = true\n"), "{added}");
        let developer = set(EDITED, "system.developer_mode", "false").unwrap();
        assert!(
            developer.ends_with("[system]\ndeveloper_mode = false\n"),
            "{developer}"
        );
    }

    #[test]
    fn shortcuts_are_written_one_way_and_never_clash() {
        let text = "format = 1\n";
        let set_one = set(text, "shortcuts.close_window", "super+w").unwrap();
        assert_eq!(
            set_one,
            "format = 1\n\n[shortcuts]\nclose_window = \"Super+W\"\n"
        );
        assert!(check(&set_one).unwrap().is_empty());
        let clash = set(text, "shortcuts.open_terminal", "Super+Q")
            .unwrap_err()
            .to_string();
        assert_eq!(
            clash,
            "shortcuts.open_terminal: Super+Q is already close_window's"
        );
        let unbound = set(text, "shortcuts.lock_screen", "")
            .unwrap_err()
            .to_string();
        assert!(
            unbound.contains("a way out must keep its keys"),
            "{unbound}"
        );
        let unknown = set(text, "shortcuts.cloze", "Super+W")
            .unwrap_err()
            .to_string();
        assert!(unknown.contains("unknown action"), "{unknown}");
        assert!(set(text, "shortcuts.close_window", "Super+Q+W").is_err());
        // check says the same of a file written by hand.
        let lines = check("format = 1\n[shortcuts]\nopen_terminal = \"super+q\"\n").unwrap();
        assert_eq!(
            lines,
            ["shortcuts.open_terminal: Super+Q is already close_window's"]
        );
    }

    #[test]
    fn set_refuses_what_check_refuses() {
        let error = |key, value| set(EDITED, key, value).unwrap_err().to_string();
        assert_eq!(
            error("network.hostnme", "a"),
            "network.hostnme: unknown key; did you mean network.hostname?"
        );
        assert!(error("nothing.near", "a").contains("edel settings lists the pages"));
        assert!(error("appearance.mode", "drak").contains("did you mean dark?"));
        assert_eq!(
            nearest_key("layout.presset").as_deref(),
            Some("layout.preset")
        );
        assert_eq!(
            nearest_key("displays.eDP-1.scal").as_deref(),
            Some("displays.eDP-1.scale")
        );
        assert_eq!(nearest_key("network.hostname"), None);
        // Every section is one page of the Settings app, and back.
        for key in KEYS {
            let section = key.path.split('.').next().unwrap_or_default();
            assert!(page(section).is_some(), "{section} is on no page");
        }
        assert!(error("users.ci.admin", "yes").contains("expected true or false"));
        assert!(error("network.hostname", "not valid").contains("expected a hostname"));
        assert_eq!(
            error("appearance.font_size", "11"),
            "appearance.font_size: not supported yet"
        );
        assert!(error("users.Ali.admin", "true").contains("not a user name"));
    }

    #[test]
    fn unset_removes_the_key_and_empty_tables() {
        let unset_once = unset(EDITED, "network.hostname").unwrap();
        assert_eq!(
            unset_once,
            "format = 1\nfuture.key = 1 # kept\n\n# The person who runs CI\n[users.ci]\nadmin = true\n"
        );
        assert!(unset(&unset_once, "network.hostname").is_err());
        assert_eq!(
            unset(EDITED, "future.key").unwrap(),
            EDITED.replace("future.key = 1 # kept\n", "")
        );
    }

    #[test]
    fn unset_keeps_a_user_whose_last_key_goes() {
        assert_eq!(
            unset("format = 1\n[users.bob]\nadmin = true\n", "users.bob.admin").unwrap(),
            "format = 1\n[users.bob]\n"
        );
        let kept = unset(EDITED, "users.ci.admin").unwrap();
        assert!(
            kept.ends_with("# The person who runs CI\n[users.ci]\n"),
            "{kept}"
        );
        assert!(read(&kept).unwrap().file.users.contains_key("ci"));
    }

    #[test]
    fn a_login_shell_is_a_full_path_passwd_can_hold() {
        let error = |value: &str| {
            set("format = 1\n", "users.ali.login_shell", value)
                .unwrap_err()
                .to_string()
        };
        assert!(set("format = 1\n", "users.ali.login_shell", "/bin/ash").is_ok());
        assert!(error("bash").contains("full path"));
        assert!(error("/bin/sh:x").contains("full path"));
        assert!(error("\"/bin/sh\\n\"").contains("full path"));
        let read = read("format = 1\n[users.ali]\nlogin_shell = \"/bin/a:b\"\n").unwrap();
        assert_eq!(read.file.users["ali"].login_shell, None);
        assert_eq!(read.problems.len(), 1);
    }

    #[test]
    fn hostnames_and_user_names_are_checked() {
        let read = read(
            "format = 1\n[network]\nhostname = \"not valid\"\n[users.Ali]\nadmin = true\n[users.ali-2]\nadmin = true\n",
        )
        .unwrap();
        assert_eq!(read.file.network.hostname, None);
        assert_eq!(read.file.users.keys().collect::<Vec<_>>(), ["ali-2"]);
        let keys: Vec<&str> = read.problems.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["network.hostname", "users.Ali"]);
        assert!(is_hostname("lab-1") && !is_hostname("-lab") && !is_hostname(""));
    }

    #[test]
    fn a_newer_file_is_read_through_its_versioned_sibling() {
        let dir = std::env::temp_dir().join(format!("edel-versioned-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = crate::places::settings_in(&dir);
        fs::write(&path, "format = 2\n[network]\nhostname = \"new\"\n").unwrap();
        let error = read_on_machine(&path).unwrap_err().to_string();
        assert!(error.contains("nothing is applied"), "{error}");
        fs::write(
            versioned(&path, 1),
            "format = 1\n[network]\nhostname = \"old\"\n",
        )
        .unwrap();
        let read = read_on_machine(&path).unwrap();
        assert_eq!(read.file.network.hostname.as_deref(), Some("old"));
        assert_eq!(read.problems[0].key, "format");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_key_round_trips_through_to_string() {
        let table = with_keys(KEYS.iter().map(|k| k.path));
        let first = read(&toml::to_string(&table).unwrap()).unwrap();
        assert_eq!(first.problems, []);
        assert_eq!(
            first.later.len(),
            KEYS.iter().filter(|k| !k.supported).count()
        );
        let again = read(&toml::to_string(&first.file).unwrap()).unwrap();
        assert_eq!(again.file, first.file);
    }

    #[test]
    fn the_structs_have_every_key_in_keys_txt() {
        for line in keys_txt() {
            assert!(
                KEYS.iter().any(|k| k.path == line),
                "{line} is in tests/keys.txt but not in KEYS; a key is never removed within a format"
            );
            let mut ignored = Vec::new();
            let _: SystemFile =
                serde_ignored::deserialize(Value::Table(with_keys([line])), |path| {
                    ignored.push(path.to_string())
                })
                .unwrap();
            assert_eq!(ignored, Vec::<String>::new(), "the structs lack {line}");
        }
    }

    #[test]
    fn keys_txt_lists_every_key() {
        let listed = keys_txt();
        for key in KEYS {
            assert!(
                listed.contains(&key.path),
                "add {} to tests/keys.txt",
                key.path
            );
        }
    }

    #[test]
    fn the_reference_page_lists_every_key() {
        let page = include_str!("../../../docs/settings.md");
        for key in KEYS {
            assert!(
                page.contains(&format!("`{}`", key.path)),
                "document {} in docs/settings.md",
                key.path
            );
        }
    }
}
