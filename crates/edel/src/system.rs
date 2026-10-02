//! The system file, `system.toml`: one TOML file that describes a whole
//! machine (ADR-006). This is the one parser for it: `edel` today, and the
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

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

/// The system file format this release reads and writes. A key is never
/// removed or renamed within a format: `tests/keys.txt` lists every key a
/// format has had, and a cargo test holds the structs to it.
pub const FORMAT: i64 = 1;

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
}

/// One key of the system file. `*` in a path stands for any name, such as a
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
    "tiling",
    "tablet",
    "phone",
];

/// Every key of format 1. Append only; `tests/keys.txt` must list each one.
pub const KEYS: &[Key] = &[
    later("system.channel", Kind::Text),
    later("system.version", Kind::Text),
    later(
        "system.variant",
        Kind::OneOf(&["container", "server", "desktop", "phone"]),
    ),
    now("system.developer", Kind::Flag),
    later("system.profiles", Kind::Texts),
    now("users.*.admin", Kind::Flag),
    now("users.*.ssh_keys", Kind::Texts),
    now("users.*.shell", Kind::Text),
    now("network.hostname", Kind::Hostname),
    later("locale.language", Kind::Text),
    later("locale.keyboard", Kind::Text),
    later("locale.timezone", Kind::Text),
    later("shell.preset", Kind::OneOf(PRESETS)),
    later("shell.tiling", Kind::Flag),
    later("shell.title_bars", Kind::Flag),
    later(
        "shell.form_factor",
        Kind::OneOf(&["desktop", "tablet", "phone"]),
    ),
    later("outputs.*.position", Kind::Pair),
    later("outputs.*.scale", Kind::Number),
    later("outputs.*.mode", Kind::Text),
    later("outputs.*.enabled", Kind::Flag),
    later("outputs.*.transform", Kind::WholeOf(&[0, 90, 180, 270])),
    later("appearance.wallpaper", Kind::Text),
    later(
        "appearance.color_scheme",
        Kind::OneOf(&["light", "dark", "auto"]),
    ),
    later("appearance.accent", Kind::Text),
    later("appearance.font", Kind::Text),
    later("appearance.font_size", Kind::Number),
    later("appearance.cursor_size", Kind::Whole),
    later("appearance.icon_size", Kind::Whole),
    later(
        "appearance.motion",
        Kind::OneOf(&["full", "reduced", "off"]),
    ),
    later("shortcuts.*", Kind::Text),
    later("defaults.browser", Kind::Text),
    later("defaults.files", Kind::Text),
    later("defaults.editor", Kind::Text),
    later("defaults.terminal", Kind::Text),
    later("defaults.mail", Kind::Text),
    later("startup.apps", Kind::Texts),
    later(
        "power.lid",
        Kind::OneOf(&["suspend", "lock", "nothing", "poweroff"]),
    ),
    later("power.idle", Kind::Whole),
    later(
        "power.power_button",
        Kind::OneOf(&["suspend", "poweroff", "ask", "nothing"]),
    ),
    later(
        "power.on_battery",
        Kind::OneOf(&["power-saver", "balanced", "performance"]),
    ),
    later("services.*", Kind::Flag),
    later(
        "updates.auto",
        Kind::OneOf(&["off", "check", "install", "boot"]),
    ),
    later("updates.window", Kind::Text),
    later("apps.flatpak", Kind::Texts),
    later("addons.add", Kind::Texts),
];

/// A whole machine. Every key is optional: an absent key means the release
/// decides (ADR-008, section 3), so nothing here has a default of its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SystemFile {
    pub format: i64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub system: System,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub users: BTreeMap<String, User>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub network: Network,
    #[serde(default, skip_serializing_if = "is_default")]
    pub locale: Locale,
    #[serde(default, skip_serializing_if = "is_default")]
    pub shell: Shell,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, Output>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub appearance: Appearance,
    /// Action name to keys, such as `close = "Super+Q"`
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub shortcuts: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub defaults: Defaults,
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
}

impl Default for SystemFile {
    /// A file in this release's format with every key absent.
    fn default() -> Self {
        SystemFile {
            format: FORMAT,
            system: System::default(),
            users: BTreeMap::new(),
            network: Network::default(),
            locale: Locale::default(),
            shell: Shell::default(),
            outputs: BTreeMap::new(),
            appearance: Appearance::default(),
            shortcuts: BTreeMap::new(),
            defaults: Defaults::default(),
            startup: Startup::default(),
            power: Power::default(),
            services: BTreeMap::new(),
            updates: Updates::default(),
            apps: Apps::default(),
            addons: Addons::default(),
        }
    }
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct System {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub developer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct User {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_keys: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Network {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Locale {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyboard: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Shell {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiling: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_bars: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form_factor: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Output {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<[i64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Defaults {
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Startup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Power {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lid: Option<String>,
    /// Minutes without input before the screen locks; 0 never locks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_button: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_battery: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Updates {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Apps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flatpak: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Addons {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub add: Option<Vec<String>>,
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

/// Reads a system file leniently: unknown keys and values of the wrong
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

/// `edel system check`: what a strict checker refuses in a file, one line
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
    Ok(lines)
}

/// Reads the machine's system file the way an unattended reader must
/// (ADR-008): a file in a newer format is not read; `system.toml.v<N>`
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

/// `system.toml.v1` for `system.toml` and format 1.
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
        (Kind::OneOf(allowed), Value::String(s)) => {
            Err(format!("unknown value {s:?}; use {}", or_list(allowed)))
        }
        (Kind::OneOf(allowed), _) => fail(&or_list(allowed)),
        (Kind::WholeOf(allowed), Value::Integer(n)) if allowed.contains(n) => Ok(value.clone()),
        (Kind::WholeOf(allowed), _) => {
            let allowed: Vec<String> = allowed.iter().map(|n| n.to_string()).collect();
            let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
            fail(&or_list(&allowed))
        }
        (Kind::Hostname, Value::String(s)) if is_hostname(s) => Ok(value.clone()),
        (Kind::Hostname, _) => fail("a hostname: letters, digits and hyphens, up to 63"),
    }
}

/// A single DNS label, as `hostname` and `/etc/hostname` take it.
pub fn is_hostname(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
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
color_scheme = "sepia"
font_size = 11
"#;

    #[test]
    fn parses_the_adr_006_example() {
        let read = read(adr_006_example()).unwrap();
        assert_eq!(read.problems, []);
        assert_eq!(read.file.network.hostname.as_deref(), Some("ali-laptop"));
        assert_eq!(read.file.users["ali"].admin, Some(true));
        assert_eq!(read.file.addons.add, Some(vec!["virtualization".into()]));
        assert_eq!(read.file.shell.preset.as_deref(), Some("classic"));
    }

    #[test]
    fn check_names_an_unknown_key() {
        let lines = check(MIXED).unwrap();
        assert!(lines.contains(&"appearance.acent: unknown key".to_string()));
    }

    #[test]
    fn check_lists_the_allowed_values() {
        let lines = check(MIXED).unwrap();
        assert!(
            lines.contains(
                &"appearance.color_scheme: unknown value \"sepia\"; use light, dark or auto"
                    .to_string()
            )
        );
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
        assert_eq!(read.file.appearance.color_scheme, None);
        assert_eq!(read.file.appearance.font_size, Some(11.0));
        let keys: Vec<&str> = read.problems.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["appearance.acent", "appearance.color_scheme"]);
    }

    #[test]
    fn values_of_the_wrong_kind_fall_back() {
        let read = read(
            "format = 1\n[users.ci]\nadmin = \"yes\"\nshell = \"/bin/sh\"\n[outputs.DP-1]\ntransform = 45\n[network]\nhostname = [1]\n",
        )
        .unwrap();
        assert_eq!(read.file.users["ci"].admin, None);
        assert_eq!(read.file.users["ci"].shell.as_deref(), Some("/bin/sh"));
        assert_eq!(read.file.network.hostname, None);
        let shown: Vec<String> = read.problems.iter().map(|p| p.to_string()).collect();
        assert!(shown.contains(&"users.ci.admin: expected true or false, not \"yes\"".into()));
        assert!(
            shown.contains(&"outputs.DP-1.transform: expected 0, 90, 180 or 270, not 45".into())
        );
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
        let path = dir.join("system.toml");
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
        let page = include_str!("../../../docs/system-file.md");
        for key in KEYS {
            assert!(
                page.contains(&format!("`{}`", key.path)),
                "document {} in docs/system-file.md",
                key.path
            );
        }
    }
}
