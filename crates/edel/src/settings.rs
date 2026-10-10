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

use crate::i18n::{n_, tr, trf};

/// The settings file format this release reads and writes. Once Edel OS is
/// released, a key is never removed or renamed within a format:
/// `tests/keys.txt` lists every key a format has had, and a cargo test
/// holds the structs to it. Until the first release, keys are renamed
/// freely, as nobody runs Edel OS yet (Alimardon, 2026-10-10).
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

/// The Settings app's pages, in its order (the mockups' order, Alimardon
/// 2026-10-08: the everyday pages first, people and the system last);
/// every section of the file is
/// one of them, and a page may have no section of the file (Sound, whose
/// state is the sound system's own). Settings (M5.6) reads the same table.
pub const PAGES: &[Page] = &[
    Page {
        section: "layout",
        title: n_("Layout"),
        about: n_("the preset, tiling and title bars"),
    },
    Page {
        section: "panels",
        title: n_("Panels"),
        about: n_("the panels and docks, the tray and the pinned apps"),
    },
    Page {
        section: "workspaces",
        title: n_("Workspaces"),
        about: n_("how many, their names and the apps that open on each"),
    },
    Page {
        section: "appearance",
        title: n_("Appearance"),
        about: n_("light or dark, the accent, fonts, animations and the workspace switcher"),
    },
    Page {
        section: "displays",
        title: n_("Displays"),
        about: n_("each screen's place, scale and resolution"),
    },
    Page {
        // The sound system's own state, not a section of the file: the
        // volume and the chosen devices are WirePlumber's (M5.7b), so this
        // page has no key, and `edel settings get sound` says so.
        section: "sound",
        title: n_("Sound"),
        about: n_("the output and input devices and their volume"),
    },
    Page {
        // The connections and Wi-Fi passwords are NetworkManager's own
        // state, never in the file (ADR-006, M5.8a); the file holds the
        // device's name.
        section: "network",
        title: n_("Network"),
        about: n_("wired and Wi-Fi connections, and the device's name"),
    },
    Page {
        // BlueZ's own state, like Sound's: no key, and `edel settings get
        // bluetooth` says so (M5.8a).
        section: "bluetooth",
        title: n_("Bluetooth"),
        about: n_("pairing and connecting headphones, mice and other devices"),
    },
    Page {
        section: "power",
        title: n_("Power"),
        about: n_("the lid, the power button and the screen lock"),
    },
    Page {
        // Banners and the notification centre are shell-ui's (M5.9b).
        section: "notifications",
        title: n_("Notifications"),
        about: n_("banners and do not disturb"),
    },
    Page {
        section: "region",
        title: n_("Region"),
        about: n_("language, keyboard and time zone"),
    },
    Page {
        section: "shortcuts",
        title: n_("Shortcuts"),
        about: n_("the keys for each action"),
    },
    Page {
        section: "default_apps",
        title: n_("Default apps"),
        about: n_("the browser, files, editor, terminal and mail"),
    },
    Page {
        section: "startup",
        title: n_("Startup"),
        about: n_("apps that start after login"),
    },
    Page {
        section: "apps",
        title: n_("Apps"),
        about: n_("the apps installed from Flathub"),
    },
    Page {
        section: "addons",
        title: n_("Add-ons"),
        about: n_("signed extras to the system"),
    },
    Page {
        section: "services",
        title: n_("Services"),
        about: n_("optional services, on or off"),
    },
    Page {
        section: "users",
        title: n_("Users"),
        about: n_("the people who log in, and their ssh keys"),
    },
    Page {
        section: "updates",
        title: n_("Updates"),
        about: n_("when updates are installed"),
    },
    Page {
        section: "system",
        title: n_("System"),
        about: n_("developer mode and profiles"),
    },
];

/// Whether the settings file has no key under `section`, as a page such as
/// Sound has none (M5.7b).
pub fn no_keys(section: &str) -> bool {
    !KEYS
        .iter()
        .any(|k| k.path.split('.').next() == Some(section))
}

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
    /// A whole number from the first to the last, both included
    /// (M5.2i, the workspace count)
    WholeFromTo(i64, i64),
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
    /// A list of host names, such as `["pool.ntp.org"]`: dot-separated
    /// names of letters, digits and hyphens, at most [`MOST_HOSTS`]
    /// (M1.13)
    Hosts,
    /// A release channel's name, such as `"stable"` ([`is_channel`], M5.8c)
    Channel,
    /// Apps and the workspace each opens on, such as `{ "org.mozilla.firefox" = 2 }` (M5.2l)
    AppWorkspaces,
}

/// The most host names a [`Kind::Hosts`] list holds.
pub const MOST_HOSTS: usize = 4;

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
    // The language people read the desktop in (M5.24a): a catalogue's
    // name, such as `de` or `pt_BR`; English without it.
    now("region.language", Kind::Text),
    now("region.keyboard", Kind::Keyboard),
    later("region.timezone", Kind::Text),
    // The servers the clock is set from (M1.13); `pool.ntp.org` without.
    now("region.time_servers", Kind::Hosts),
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
    // Each title bar button shown or hidden (M5.18a).
    now("layout.close_button", Kind::Flag),
    now("layout.minimize_button", Kind::Flag),
    now("layout.maximize_button", Kind::Flag),
    // How tiling lays windows out (M5.16a, scroll M5.16c).
    now(
        "layout.tiling_style",
        Kind::OneOf(&["stack", "split", "scroll"]),
    ),
    // The Panels page (M5.2q): the panels in place of the preset's
    // (M5.4e), the tray's apps kept in the panel by their item's Id, the
    // rest behind its arrow (M5.9g), and the apps the apps widget pins, in
    // order, by id or role (M5.31d).
    now("panels.list", Kind::Panels),
    now("panels.tray", Kind::Texts),
    now("panels.pinned", Kind::Texts),
    // The Workspaces page (M5.2q): their count, whether empty ones come
    // and go (M5.2i), whether each screen shows its own (M5.2k), their
    // names, the first workspace's first, and the apps that always open on
    // their own (M5.2l).
    now(
        "workspaces.count",
        Kind::WholeFromTo(1, crate::presets::MOST_WORKSPACES as i64),
    ),
    now("workspaces.dynamic", Kind::Flag),
    now("workspaces.per_screen", Kind::Flag),
    now("workspaces.names", Kind::Texts),
    now("workspaces.apps", Kind::AppWorkspaces),
    // The side of the screen the overview's strip of workspaces lies on
    // (M5.2j-b); the windows take the rest.
    now(
        "workspaces.overview_strip",
        Kind::OneOf(&["left", "right", "top", "bottom"]),
    ),
    now("displays.*.position", Kind::Pair),
    now("displays.*.scale", Kind::Number),
    now("displays.*.resolution", Kind::Resolution),
    now("displays.*.refresh_rate", Kind::Number),
    now("displays.*.enabled", Kind::Flag),
    later("displays.*.rotation", Kind::WholeOf(&[0, 90, 180, 270])),
    later("appearance.wallpaper", Kind::Text),
    // The panel's workspace switcher: its look, how many numbers show at
    // once and what the strip's ends show where more lie (M5.2m, on the
    // Appearance page since M5.2q).
    now(
        "appearance.switcher_look",
        Kind::OneOf(&["numbers", "button"]),
    ),
    now(
        "appearance.switcher_shown",
        Kind::WholeFromTo(1, crate::presets::MOST_WORKSPACES as i64),
    ),
    now(
        "appearance.switcher_ends",
        Kind::OneOf(&["fade", "arrows", "counts"]),
    ),
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
    // The release lists `edel update` takes (M3.8); absent, the image's.
    now("updates.channel", Kind::Channel),
    later("updates.version", Kind::Text),
    later(
        "updates.automatic",
        Kind::OneOf(&["off", "check", "install", "install-and-restart"]),
    ),
    later("updates.restart_window", Kind::Text),
    later("apps.installed", Kind::Texts),
    later("addons.installed", Kind::Texts),
    // Banners stay away while the notification list still fills (M5.9b).
    now("notifications.do_not_disturb", Kind::Flag),
];

/// A whole machine. Every key is optional: an absent key means the release
/// decides (ADR-008, section 3), so nothing here has a default of its own.
/// Each section is a page of the Settings app ([`PAGES`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingsFile {
    pub format: i64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub layout: Layout,
    #[serde(default, skip_serializing_if = "is_default")]
    pub panels: Panels,
    #[serde(default, skip_serializing_if = "is_default")]
    pub workspaces: Workspaces,
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
    #[serde(default, skip_serializing_if = "is_default")]
    pub notifications: Notifications,
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

impl Default for SettingsFile {
    /// A file in this release's format with every key absent.
    fn default() -> Self {
        SettingsFile {
            format: FORMAT,
            layout: Layout::default(),
            panels: Panels::default(),
            workspaces: Workspaces::default(),
            displays: BTreeMap::new(),
            appearance: Appearance::default(),
            shortcuts: BTreeMap::new(),
            region: Region::default(),
            users: BTreeMap::new(),
            network: Network::default(),
            default_apps: DefaultApps::default(),
            startup: Startup::default(),
            power: Power::default(),
            notifications: Notifications::default(),
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
    /// Host names the clock is set from (M1.13); absent is `pool.ntp.org`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_servers: Option<Vec<String>>,
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
    /// Whether every title bar shows its close, minimize and maximize
    /// buttons (M5.18a); absent shows each.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_button: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimize_button: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximize_button: Option<bool>,
    /// How tiling lays windows out (M5.16a); absent is `stack`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiling_style: Option<String>,
}

/// The Panels page (M5.2q).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Panels {
    /// The panels in place of the preset's (M5.4e); absent is the
    /// preset's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<Vec<crate::presets::Panel>>,
    /// The tray's apps kept in the panel, by their item's `Id`, in the
    /// panel's order; absent keeps none in the panel, so every app waits
    /// behind the tray's arrow (M5.9g).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tray: Option<Vec<String>>,
    /// The apps the apps widget pins, in order, each an app's id or a role
    /// such as `terminal` (M5.31d); absent is the layout preset's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<Vec<String>>,
}

/// The Workspaces page (M5.2q).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Workspaces {
    /// How many workspaces, 1 to the preset's most (M5.2i); absent is the
    /// preset's count. Dynamic workspaces ignore it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Whether an empty workspace always waits at the end and other empty
    /// ones close (M5.2i); absent is off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,
    /// Whether each screen shows its own workspace, so Super+1 to Super+9
    /// switch only the screen the pointer is on, instead of every screen
    /// switching together (M5.2k); absent is off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_screen: Option<bool>,
    /// The workspaces' names, the first workspace's first, an empty text
    /// meaning no name (M5.2i).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    /// Apps that always open on their own workspace, each app's id and
    /// the workspace's number from 1 (M5.2l); absent opens every app on
    /// the workspace in use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<BTreeMap<String, i64>>,
    /// The side of the screen the overview's strip of workspaces lies on,
    /// `left`, `right`, `top` or `bottom` (M5.2j-b); absent is
    /// [`OVERVIEW_STRIP_DEFAULT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overview_strip: Option<String>,
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
    /// The workspace switcher's look, `numbers` or `button` (M5.2m); absent
    /// is [`SWITCHER_LOOK_DEFAULT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switcher_look: Option<String>,
    /// How many workspaces the switcher shows at once, 1 to the preset's
    /// most (M5.2m); absent is [`SWITCHER_SHOWN_DEFAULT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switcher_shown: Option<u32>,
    /// What the switcher's strip shows where more workspaces lie: `fade`,
    /// `arrows` or `counts` (M5.2m); absent is [`SWITCHER_ENDS_DEFAULT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switcher_ends: Option<String>,
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

/// The Notifications page (M5.9b).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Notifications {
    /// Whether banners stay away while the notification centre still
    /// lists what arrives; absent is off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub do_not_disturb: Option<bool>,
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
    pub file: SettingsFile,
    /// What was left out, and why
    pub problems: Vec<Problem>,
    /// Keys that were read but that no part of this release acts on yet
    pub later: Vec<String>,
}

/// Reads a settings file leniently: unknown keys and values of the wrong
/// kind are left out and reported, and every other key is kept. Only a file
/// that is not TOML, or not in this release's format, is refused.
pub fn read(text: &str) -> Result<Read> {
    let table: Table = toml::from_str(text).context(tr("the file is not valid TOML"))?;
    let format = format_of(&table)?;
    if format != FORMAT {
        bail!(
            "{}",
            trf(
                "the file is format {format}, and this release reads format {current}",
                &[
                    ("format", &format.to_string()),
                    ("current", &FORMAT.to_string())
                ]
            )
        );
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
            .map(|key| format!("{key}: {}", tr("not supported yet"))),
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
    let text = fs::read_to_string(path).with_context(|| could_not_read(path))?;
    let table: Table = toml::from_str(&text).with_context(|| {
        trf(
            "{path} is not valid TOML; nothing is applied",
            &[("path", &path.display().to_string())],
        )
    })?;
    let format = format_of(&table)?;
    if format <= FORMAT {
        return read(&text);
    }
    let older = versioned(path, FORMAT);
    let Ok(text) = fs::read_to_string(&older) else {
        bail!(
            "{}",
            trf(
                "{path} is format {format}, newer than this release reads, and there is no {older} beside it; nothing is applied",
                &[
                    ("path", &path.display().to_string()),
                    ("format", &format.to_string()),
                    ("older", &older.display().to_string())
                ]
            )
        );
    };
    let mut read = read(&text).with_context(|| could_not_read(&older))?;
    read.problems.insert(
        0,
        Problem {
            key: "format".into(),
            message: trf(
                "format {format} is newer than this release, so {older} was read instead",
                &[
                    ("format", &format.to_string()),
                    ("older", &older.display().to_string()),
                ],
            ),
        },
    );
    Ok(read)
}

/// `could not read PATH`, the start of a failure to read a file.
fn could_not_read(path: &Path) -> String {
    trf(
        "could not read {path}",
        &[("path", &path.display().to_string())],
    )
}

/// The `format` of a settings file's text.
pub fn format(text: &str) -> Result<i64> {
    let table: Table = toml::from_str(text).context(tr("the file is not valid TOML"))?;
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

/// The values a choice key takes, as the key table lists them, so every
/// part that offers them (Settings' rows, the panel's menus) offers the
/// ones `edel settings set` takes; none for another kind of key.
pub fn choices(key: &str) -> &'static [&'static str] {
    match KEYS.iter().find(|k| k.path == key) {
        Some(Key {
            kind: Kind::OneOf(values),
            ..
        }) => values,
        _ => &[],
    }
}

/// `key`'s value as the files say it now: the person's file over the
/// machine's, as a string; none when neither sets it.
pub fn chosen(key: &str, machine: Option<&str>, person: Option<&str>) -> Option<String> {
    match source(key, machine, person) {
        Source::Person(value) | Source::Machine(value) => match value {
            Value::String(s) => Some(s),
            other => Some(other.to_string()),
        },
        Source::Release => None,
    }
}

/// The key banners and sounds are kept away with (M5.9b), the one name
/// shell-ui, Settings and `edel settings` share.
pub const DO_NOT_DISTURB: &str = "notifications.do_not_disturb";

/// The key the tray's apps kept in the panel are listed under (M5.9g), the
/// one name shell-ui, Settings and `edel settings` share.
pub const TRAY_IN_PANEL: &str = "panels.tray";

/// The keys the workspace switcher's look, shown count and ends are kept
/// under (M5.2m, on the Appearance page since M5.2q), the one names
/// shell-ui, Settings and `edel settings` share.
pub const SWITCHER_LOOK: &str = "appearance.switcher_look";
pub const SWITCHER_SHOWN: &str = "appearance.switcher_shown";
pub const SWITCHER_ENDS: &str = "appearance.switcher_ends";
/// What the switcher shows when neither file sets its keys (M5.2m): the
/// numbers, three at once, an arrow where more lie. shell-ui draws these
/// and Settings shows them.
pub const SWITCHER_LOOK_DEFAULT: &str = "numbers";
pub const SWITCHER_SHOWN_DEFAULT: u32 = 3;
pub const SWITCHER_ENDS_DEFAULT: &str = "arrows";

/// The key the overview's strip of workspaces is kept under (M5.2j-b), the
/// one name shell-ui, Settings and `edel settings` share, and what it shows
/// when no file sets it: the strip on the left.
pub const OVERVIEW_STRIP: &str = "workspaces.overview_strip";
pub const OVERVIEW_STRIP_DEFAULT: &str = "left";

/// The key the workspaces' names are kept under, one per place (M5.2p): the
/// names the switcher shows, which a drag of a switcher button reorders.
pub const WORKSPACE_NAMES: &str = "workspaces.names";

/// `key`'s value as a list of texts, the person's file over the machine's;
/// none when neither sets it, or what they say is not a list of texts. An
/// empty list is a list: `[]` over a machine's list says none.
pub fn texts(key: &str, machine: Option<&str>, person: Option<&str>) -> Option<Vec<String>> {
    let list = |value: Value| match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Value::String(s) => Some(s),
                _ => None,
            })
            .collect(),
        _ => None,
    };
    match source(key, machine, person) {
        Source::Person(value) | Source::Machine(value) => list(value),
        Source::Release => None,
    }
}

/// The apps kept in the panel after `app` is kept (appended, once) or taken
/// out of it (M5.9h): the one rule the panel's drag and Settings' Tray card
/// both follow.
pub fn tray_list_with(current: &[String], app: &str, keep: bool) -> Vec<String> {
    let mut list = current.to_vec();
    if keep {
        if !list.iter().any(|a| a == app) {
            list.push(app.to_string());
        }
    } else {
        list.retain(|a| a != app);
    }
    list
}

/// The value to write for the tray list `list` in the person's file, as a
/// TOML array; none when `list` is what applies without the person's file
/// (the machine's, or none), since writers never write a default (ADR-008).
pub fn tray_value(list: &[String], machine: Option<&str>) -> Option<String> {
    let without = texts(TRAY_IN_PANEL, machine, None).unwrap_or_default();
    (list != without.as_slice())
        .then(|| Value::Array(list.iter().map(|a| Value::String(a.clone())).collect()).to_string())
}

/// `key`'s value as a flag, the person's file over the machine's; none
/// when neither sets it, or what they say is not `true` or `false`.
pub fn flag(key: &str, machine: Option<&str>, person: Option<&str>) -> Option<bool> {
    match source(key, machine, person) {
        Source::Person(Value::Boolean(on)) | Source::Machine(Value::Boolean(on)) => Some(on),
        _ => None,
    }
}

/// Sets `key` to `value` in the settings file at `path`, or takes it out
/// with none, starting a file when there is none, through a rename so a
/// reader never sees half a file: what Settings and the panel write with.
pub fn write(path: &Path, key: &str, value: Option<&str>) -> Result<()> {
    let text = fs::read_to_string(path).unwrap_or_else(|_| format!("format = {FORMAT}\n"));
    let edited = match value {
        Some(value) => set(&text, key, value)?,
        None => unset(&text, key)?,
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| {
            trf(
                "could not make {path}",
                &[("path", &dir.display().to_string())],
            )
        })?;
    }
    let new = path.with_extension("toml.edel-new");
    fs::write(&new, edited)
        .and_then(|()| fs::rename(&new, path))
        .with_context(|| {
            trf(
                "could not write {path}",
                &[("path", &path.display().to_string())],
            )
        })
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
        bail!(
            "{}",
            trf(
                "{key}: not supported yet; this release does not act on it, so it cannot be set",
                &[("key", key)]
            )
        );
    }
    let new = toml_edit_value(&value)?;
    let mut doc: DocumentMut = text.parse().context(tr("the file is not valid TOML"))?;
    let (last, parents) = path.split_last().context(tr(
        "no key given; write KEY=VALUE, such as network.hostname=lab-1",
    ))?;
    let mut table = doc.as_table_mut();
    for (i, part) in parents.iter().enumerate() {
        let item = table.entry(part).or_insert_with(|| {
            let mut new = toml_edit::Table::new();
            // Only the table holding the key gets a [header].
            new.set_implicit(i + 1 < parents.len());
            toml_edit::Item::Table(new)
        });
        table = item.as_table_mut().with_context(|| {
            trf(
                "{table} is not a table in the file; change {key} by hand",
                &[("table", &path[..=i].join(".")), ("key", key)],
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
    let mut doc: DocumentMut = text.parse().context(tr("the file is not valid TOML"))?;
    let removed = match path.as_slice() {
        ["users", name, last] => doc
            .get_mut("users")
            .and_then(|users| users.as_table_like_mut()?.get_mut(name))
            .and_then(|user| user.as_table_like_mut())
            .is_some_and(|user| user.remove(last).is_some()),
        _ => remove_path(doc.as_table_mut(), &path)?,
    };
    if !removed {
        bail!(
            "{}",
            trf(
                "{key} is not in the file, so it already has its default; there is nothing to reset",
                &[("key", key)]
            )
        );
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
    let inner = inner.as_table_mut().with_context(|| {
        trf(
            "{table} is not a table in the file; change it by hand",
            &[("table", first)],
        )
    })?;
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
        bail!(
            "{}",
            trf(
                "{key}: {name} is not a user name",
                &[("key", key), ("name", &format!("{:?}", path[1]))]
            )
        );
    }
    KEYS.iter()
        .find(|k| matches(k.path, &names, false))
        .ok_or_else(|| anyhow!("{key}: {}", unknown_key(key)))
}

/// What is said of a key no page has, by `set`, `check` and `apply`: the
/// nearest key when one is close, else where the keys are listed.
fn unknown_key(key: &str) -> String {
    match nearest_key(key) {
        Some(near) => trf("unknown key; did you mean {near}?", &[("near", &near)]),
        None => {
            tr("unknown key; edel settings lists the pages, and edel settings get PAGE their keys")
                .to_string()
        }
    }
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
    // A list of tables, such as panels.list, comes back as [[v]]
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
        .context(tr("cannot write the value"))
}

/// The settings file `path` with `.v1` added, for format 1.
pub fn versioned(path: &Path, format: i64) -> PathBuf {
    PathBuf::from(format!("{}.v{format}", path.display()))
}

fn format_of(table: &Table) -> Result<i64> {
    match table.get("format") {
        Some(Value::Integer(format)) if *format >= 1 => Ok(*format),
        Some(other) => bail!(
            "{}",
            trf(
                "format must be a whole number from 1, not {value}",
                &[("value", &other.to_string())]
            )
        ),
        None => bail!(
            "{}",
            trf(
                "the file has no format; add format = {current} at the top",
                &[("current", &FORMAT.to_string())]
            )
        ),
    }
}

fn read_table(table: &Table) -> Result<Read> {
    let mut problems = Vec::new();
    let mut later = Vec::new();
    let kept = clean(table, &[], &mut problems, &mut later);
    let mut ignored = Vec::new();
    let file: SettingsFile =
        serde_ignored::deserialize(Value::Table(kept), |path| ignored.push(path.to_string()))
            .context(tr("the key table and the structs disagree"))?;
    // The key table already left out unknown keys; anything ignored here is
    // a key the table lists but the structs lack, which a test prevents.
    problems.extend(ignored.into_iter().map(|key| Problem {
        message: unknown_key(&key),
        key,
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
                message: tr("not a user name; use up to 32 lowercase letters, digits, - and _, starting with a letter or _").into(),
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
                    message: tr("expected a table").into(),
                }),
            }
        } else {
            problems.push(Problem {
                message: unknown_key(&shown),
                key: shown,
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
    let fail = |expected: &str| {
        Err(trf(
            "expected {expected}, not {value}",
            &[("expected", expected), ("value", &value.to_string())],
        ))
    };
    match (kind, value) {
        (Kind::Text, Value::String(_)) | (Kind::Flag, Value::Boolean(_)) => Ok(value.clone()),
        (Kind::Text, _) => fail(tr("text in quotes")),
        (Kind::Flag, _) => fail(tr("true or false")),
        (Kind::Texts, Value::Array(items)) if items.iter().all(Value::is_str) => Ok(value.clone()),
        (Kind::Texts, _) => fail(tr("a list of texts in quotes")),
        (Kind::Whole, Value::Integer(n)) if u32::try_from(*n).is_ok() => Ok(value.clone()),
        (Kind::Whole, _) => fail(tr("a whole number from 0")),
        (Kind::Number, Value::Float(n)) if n.is_finite() => Ok(value.clone()),
        (Kind::Number, Value::Integer(n)) => Ok(Value::Float(*n as f64)),
        (Kind::Number, _) => fail(tr("a number")),
        (Kind::Pair, Value::Array(items))
            if items.len() == 2 && items.iter().all(Value::is_integer) =>
        {
            Ok(value.clone())
        }
        (Kind::Pair, _) => fail(tr("two whole numbers, such as [0, 0]")),
        (Kind::OneOf(allowed), Value::String(s)) if allowed.contains(&s.as_str()) => {
            Ok(value.clone())
        }
        (Kind::OneOf(allowed), Value::String(s)) => Err(match nearest_value(s, allowed) {
            Some(near) => trf(
                "unknown value {value}; did you mean {near}? Use {allowed}",
                &[
                    ("value", &format!("{s:?}")),
                    ("near", near),
                    ("allowed", &or_list(allowed)),
                ],
            ),
            None => trf(
                "unknown value {value}; use {allowed}",
                &[("value", &format!("{s:?}")), ("allowed", &or_list(allowed))],
            ),
        }),
        (Kind::OneOf(allowed), _) => fail(&or_list(allowed)),
        (Kind::WholeOf(allowed), Value::Integer(n)) if allowed.contains(n) => Ok(value.clone()),
        (Kind::WholeFromTo(least, most), Value::Integer(n)) if (least..=most).contains(n) => {
            Ok(value.clone())
        }
        (Kind::WholeFromTo(least, most), _) => fail(&trf(
            "a whole number from {least} to {most}",
            &[("least", &least.to_string()), ("most", &most.to_string())],
        )),
        (Kind::WholeOf(allowed), _) => {
            let allowed: Vec<String> = allowed.iter().map(|n| n.to_string()).collect();
            let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
            fail(&or_list(&allowed))
        }
        (Kind::Hostname, Value::String(s)) if is_hostname(s) => Ok(value.clone()),
        (Kind::Hostname, _) => fail(tr("a hostname: letters, digits and hyphens, up to 63")),
        (Kind::Shell, Value::String(s)) if is_shell_path(s) => Ok(value.clone()),
        (Kind::Shell, _) => fail(tr("a login shell's full path, such as \"/bin/sh\"")),
        (Kind::Keys, Value::String(s)) => crate::shortcuts::normalize(s).map(Value::String),
        (Kind::Keys, _) => fail(tr("keys in quotes, such as \"Super+Q\"")),
        (Kind::Panels, Value::Array(_)) => {
            let panels: Vec<crate::presets::Panel> =
                value.clone().try_into().map_err(|e: toml::de::Error| {
                    trf(
                        "expected panels as a preset writes them: {why}",
                        &[("why", e.message())],
                    )
                })?;
            crate::presets::check_panels(&panels).map_err(|e| e.to_string())?;
            Ok(value.clone())
        }
        (Kind::Resolution, Value::String(r)) if is_resolution(r) => Ok(value.clone()),
        (Kind::Resolution, _) => fail(tr("a resolution such as \"1920x1080\"")),
        (Kind::Keyboard, Value::String(s)) => {
            let rules = std::fs::read_to_string(crate::keyboard::RULES).ok();
            crate::keyboard::normalize(s, rules.as_deref()).map(Value::String)
        }
        (Kind::Keyboard, _) => fail(tr(
            "keyboard layouts in quotes, such as \"us\" or \"us,ru\"",
        )),
        (Kind::Hosts, Value::Array(items)) => {
            let mut names = Vec::new();
            for item in items {
                match item.as_str() {
                    Some(name) if is_host(name) => names.push(name),
                    _ => {
                        return Err(trf(
                            "{item} is not a host name; use a name such as \"pool.ntp.org\" (letters, digits, hyphens and dots)",
                            &[("item", &item.to_string())],
                        ));
                    }
                }
            }
            if names.is_empty() || names.len() > MOST_HOSTS {
                return Err(trf(
                    "expected one to {most} host names, not {count} (to use the release's, run edel settings reset on this key)",
                    &[
                        ("most", &MOST_HOSTS.to_string()),
                        ("count", &names.len().to_string()),
                    ],
                ));
            }
            Ok(value.clone())
        }
        (Kind::Hosts, _) => fail(tr("a list of host names, such as [\"pool.ntp.org\"]")),
        (Kind::Channel, Value::String(s)) if is_channel(s) => Ok(value.clone()),
        (Kind::Channel, Value::String(s)) => Err(trf(
            "{value} is not a channel name; use lowercase letters, digits and hyphens, such as \"stable\" or \"preview\" (to follow the channel this image was built for, run edel settings reset on this key)",
            &[("value", &format!("{s:?}"))],
        )),
        (Kind::Channel, _) => fail(tr("a channel name in quotes, such as \"stable\"")),
        (Kind::Panels, _) => fail(tr(
            "a list of panels, such as [{ edge = \"bottom\", end = [\"clock\"] }]",
        )),
        (Kind::AppWorkspaces, Value::Table(apps)) => {
            for (app, number) in apps {
                if !is_app_id(app) {
                    return Err(trf(
                        "{app} is not an app id; use the id an app's window gives, such as \"org.mozilla.firefox\"",
                        &[("app", &format!("{app:?}"))],
                    ));
                }
                match number {
                    Value::Integer(n)
                        if (1..=crate::presets::MOST_WORKSPACES as i64).contains(n) => {}
                    _ => {
                        return Err(trf(
                            "{app} has workspace {value}; use a whole number from 1 to {most}",
                            &[
                                ("app", &format!("{app:?}")),
                                ("value", &number.to_string()),
                                ("most", &crate::presets::MOST_WORKSPACES.to_string()),
                            ],
                        ));
                    }
                }
            }
            Ok(value.clone())
        }
        (Kind::AppWorkspaces, _) => fail(tr(
            "apps and their workspace numbers, such as { \"org.mozilla.firefox\" = 2 }",
        )),
    }
}

/// An app id as a window gives it (M5.2l): text with no whitespace or
/// control characters, such as `org.mozilla.firefox`.
pub fn is_app_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| !c.is_whitespace() && !c.is_control())
}

/// A single DNS label, as `hostname` and `/etc/hostname` take it.
pub fn is_hostname(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The longest channel name.
pub const MOST_CHANNEL: usize = 32;

/// A release channel's name (M5.8c): it is written into the address of the
/// channel's release list, so only lowercase letters, digits and hyphens,
/// not starting or ending with a hyphen, up to [`MOST_CHANNEL`].
pub fn is_channel(name: &str) -> bool {
    (1..=MOST_CHANNEL).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The channels Edel OS publishes (`docs/RELEASE.md`): `stable` moves with
/// each tagged release, `preview` with each merge. Settings offers these;
/// a machine may follow another a fleet or a test serves, by name.
pub const CHANNELS: &[&str] = &["stable", "preview"];

/// The channel a machine with none chosen and none built in follows.
pub const DEFAULT_CHANNEL: &str = "stable";

/// The channel a machine follows (M5.8c): `updates.channel` in the
/// person's file over the machine's, else `image`, the one its image was
/// built for (os-release's `EDEL_CHANNEL`), else [`DEFAULT_CHANNEL`]. A
/// name [`is_channel`] refuses is skipped, as every reader on a machine
/// skips what it cannot use.
pub fn channel(machine: Option<&str>, person: Option<&str>, image: Option<&str>) -> String {
    chosen("updates.channel", machine, person)
        .filter(|c| is_channel(c))
        .or_else(|| image.filter(|c| !c.is_empty()).map(String::from))
        .unwrap_or_else(|| DEFAULT_CHANNEL.into())
}

/// A host name as a time server is named: dot-separated labels (each
/// as [`is_hostname`] takes one), at most 253 bytes.
pub fn is_host(name: &str) -> bool {
    name.len() <= 253 && name.split('.').all(is_hostname)
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
        Some((last, rest)) => trf(
            "{others} or {last}",
            &[("others", &rest.join(", ")), ("last", last)],
        ),
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
        assert_eq!(source("panels.list", Some(machine), None), Source::Release);
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
            Kind::WholeFromTo(least, _) => Value::Integer(least),
            Kind::Hostname => Value::String("x".into()),
            Kind::Shell => Value::String("/bin/sh".into()),
            Kind::Keys => Value::String("Super+X".into()),
            Kind::Panels => value_from_arg(r#"[{ edge = "bottom", end = ["clock"] }]"#),
            Kind::Resolution => Value::String("1920x1080".into()),
            Kind::Keyboard => Value::String("us".into()),
            Kind::Hosts => Value::Array(vec![Value::String("pool.ntp.org".into())]),
            Kind::Channel => Value::String("stable".into()),
            Kind::AppWorkspaces => value_from_arg(r#"{ "org.mozilla.firefox" = 2 }"#),
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
        assert!(
            lines.contains(
                &"appearance.acent: unknown key; did you mean appearance.accent?".to_string()
            ),
            "{lines:?}"
        );
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
            "panels.list",
            r#"[{ edge = "bottom", end = ["clock"] }]"#,
        )
        .unwrap();
        assert_eq!(
            set_one,
            "format = 1\n\n[panels]\nlist = [{ edge = \"bottom\", end = [\"clock\"] }]\n"
        );
        let tables = "format = 1\n[[panels.list]]\nedge = \"top\"\nstart = [\"menu\"]\n\n[[panels.list]]\nedge = \"bottom\"\nstyle = \"dock\"\ncentre = [\"apps\"]\n";
        assert!(check(tables).unwrap().is_empty());
        let panels = read(tables).unwrap().file.panels.list.unwrap();
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
            // A size this release lacks names the sizes it has (M5.31c).
            (
                r#"[{ edge = "bottom", size = "huge" }]"#,
                "unknown variant `huge`, expected one of `small`, `medium`, `large`",
            ),
            (
                r#"[{ edge = "bottom", style = "dock", floating = true }]"#,
                "a dock floats already; take floating out of the bottom panel",
            ),
        ] {
            let error = set("format = 1\n", "panels.list", bad).unwrap_err();
            assert!(error.to_string().contains(why), "{bad}: {error}");
            let file = format!("format = 1\n[panels]\nlist = {bad}\n");
            let read = read(&file).unwrap();
            assert_eq!(read.file.panels.list, None, "{bad}");
            assert_eq!(read.problems[0].key, "panels.list");
        }
    }

    #[test]
    fn a_preset_this_release_lacks_is_refused_or_reported() {
        let file = "format = 1\n[layout]\npreset = \"tablet\"\n";
        let lines = check(file).unwrap();
        assert_eq!(
            lines,
            [
                "layout.preset: unknown value \"tablet\"; use classic, mac-like, windows-like or hive"
            ]
        );
        let error = set("format = 1\n", "layout.preset", "tablet").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("use classic, mac-like, windows-like or hive"),
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
        let set_one = set(text, "shortcuts.close_window", "super+x").unwrap();
        assert_eq!(
            set_one,
            "format = 1\n\n[shortcuts]\nclose_window = \"Super+X\"\n"
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
        let unknown = set(text, "shortcuts.cloze", "Super+X")
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
            "appearance.font_size: not supported yet; this release does not act on it, so it cannot be set"
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
        assert_eq!(
            unset(&unset_once, "network.hostname")
                .unwrap_err()
                .to_string(),
            "network.hostname is not in the file, so it already has its default; there is nothing to reset"
        );
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
    fn a_channel_is_a_short_lowercase_name() {
        let ok = set("format = 1\n", "updates.channel", "preview").unwrap();
        assert_eq!(
            read(&ok).unwrap().file.updates.channel.as_deref(),
            Some("preview")
        );
        for good in ["stable", "preview", "ci", "long-2", "a"] {
            assert!(is_channel(good), "{good}");
        }
        for bad in ["", "-x", "x-", "Stable", "a b", "a/b", "../x", "x.y"] {
            assert!(!is_channel(bad), "{bad:?}");
        }
        assert!(!is_channel(&"x".repeat(MOST_CHANNEL + 1)));
        let error = set("format = 1\n", "updates.channel", "Beta").unwrap_err();
        assert_eq!(
            error.to_string(),
            "updates.channel: \"Beta\" is not a channel name; use lowercase letters, digits and \
             hyphens, such as \"stable\" or \"preview\" (to follow the channel this image was \
             built for, run edel settings reset on this key)"
        );
        let file = "format = 1\n[updates]\nchannel = \"a/b\"\n";
        assert_eq!(check(file).unwrap().len(), 1);
        assert_eq!(read(file).unwrap().file.updates.channel, None);
    }

    #[test]
    fn the_channel_is_the_persons_then_the_machines_then_the_images() {
        let file = |channel: &str| format!("format = 1\n[updates]\nchannel = \"{channel}\"\n");
        assert_eq!(channel(None, None, None), "stable");
        assert_eq!(channel(None, None, Some("ci")), "ci");
        assert_eq!(channel(None, None, Some("")), "stable");
        let (machine, person) = (file("preview"), file("beta"));
        assert_eq!(channel(Some(&machine), None, Some("ci")), "preview");
        assert_eq!(channel(Some(&machine), Some(&person), Some("ci")), "beta");
        let bad = file("A/B");
        assert_eq!(
            channel(Some(&bad), None, Some("ci")),
            "ci",
            "a name the address cannot hold is skipped"
        );
        assert!(CHANNELS.iter().all(|c| is_channel(c)));
        assert!(CHANNELS.contains(&DEFAULT_CHANNEL));
    }

    #[test]
    fn time_servers_are_host_names() {
        let ok = set(
            "format = 1\n",
            "region.time_servers",
            r#"["pool.ntp.org", "169.254.169.123"]"#,
        )
        .unwrap();
        let parsed = read(&ok).unwrap();
        assert_eq!(
            parsed.file.region.time_servers,
            Some(vec!["pool.ntp.org".into(), "169.254.169.123".into()])
        );
        let bad = |value: &str| set("format = 1\n", "region.time_servers", value).unwrap_err();
        assert_eq!(
            bad(r#"["pool.ntp.org", "bad name"]"#).to_string(),
            "region.time_servers: \"bad name\" is not a host name; use a name such as \
             \"pool.ntp.org\" (letters, digits, hyphens and dots)"
        );
        assert!(bad(r#"["-oops"]"#).to_string().contains("not a host name"));
        assert!(bad(r#"["a..b"]"#).to_string().contains("not a host name"));
        assert!(bad("[1]").to_string().contains("not a host name"));
        assert!(bad("[]").to_string().contains("one to 4 host names"));
        assert!(
            bad(r#"["a","b","c","d","e"]"#)
                .to_string()
                .contains("one to 4")
        );
        assert!(
            bad("pool.ntp.org")
                .to_string()
                .contains("a list of host names")
        );
        let file = "format = 1\n[region]\ntime_servers = [\"bad name\"]\n";
        assert_eq!(
            check(file).unwrap(),
            [format!(
                "region.time_servers: {}",
                normalize(Kind::Hosts, &value_from_arg(r#"["bad name"]"#)).unwrap_err()
            )]
        );
        assert_eq!(read(file).unwrap().file.region.time_servers, None);
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
            let _: SettingsFile =
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
    #[test]
    fn keeping_appends_once_and_taking_out_removes() {
        let now = vec!["nm-applet".to_string()];
        assert_eq!(
            tray_list_with(&now, "blueman", true),
            vec!["nm-applet".to_string(), "blueman".to_string()]
        );
        assert_eq!(
            tray_list_with(&now, "nm-applet", true),
            now,
            "no duplicates"
        );
        assert!(tray_list_with(&now, "nm-applet", false).is_empty());
        assert_eq!(tray_list_with(&now, "other", false), now);
    }

    #[test]
    fn the_list_is_written_as_toml_that_reads_back_as_it_was() {
        let list = vec!["plain".to_string(), "say \"hi\" \\ there".to_string()];
        let value = tray_value(&list, None).expect("a list the machine does not give is written");
        let text = format!("format = 1\n[panels]\ntray = {value}\n");
        assert_eq!(texts(TRAY_IN_PANEL, Some(text.as_str()), None), Some(list));
    }

    #[test]
    fn a_list_the_machine_already_gives_is_not_written() {
        let machine = "format = 1\n[panels]\ntray = [\"nm-applet\"]\n";
        let same = vec!["nm-applet".to_string()];
        assert_eq!(tray_value(&same, Some(machine)), None);
        // Taking the machine's app out is a choice, written as an empty list.
        assert_eq!(tray_value(&[], Some(machine)), Some("[]".to_string()));
        // With nothing from the machine, no list at all is what applies.
        assert_eq!(tray_value(&[], None), None);
    }

    #[test]
    fn the_tray_lists_are_read_from_the_person_over_the_machine() {
        let machine = "format = 1\n[panels]\ntray = [\"nm-applet\", \"edel-testclient\"]\n";
        let person = "format = 1\n[panels]\ntray = [\"blueman\"]\n";
        let none = "format = 1\n[panels]\ntray = []\n";
        let kept = |a: &str, b: &str| Some(vec![a.to_string(), b.to_string()]);
        // A machine list, and the person's over it.
        assert_eq!(
            texts(TRAY_IN_PANEL, Some(machine), None),
            kept("nm-applet", "edel-testclient")
        );
        assert_eq!(
            texts(TRAY_IN_PANEL, Some(machine), Some(person)),
            Some(vec!["blueman".to_string()])
        );
        // An empty list from the person keeps none, over the machine's.
        assert_eq!(
            texts(TRAY_IN_PANEL, Some(machine), Some(none)),
            Some(Vec::new())
        );
        // Neither sets it: none, so the release decides.
        assert_eq!(texts(TRAY_IN_PANEL, None, None), None);
        assert_eq!(texts(TRAY_IN_PANEL, Some("format = 1\n"), None), None);
        // Not a list of texts is no list.
        let text = "format = 1\n[panels]\ntray = \"nm-applet\"\n";
        assert_eq!(texts(TRAY_IN_PANEL, Some(text), None), None);
        let numbers = "format = 1\n[panels]\ntray = [1, 2]\n";
        assert_eq!(texts(TRAY_IN_PANEL, Some(numbers), None), None);
        // A bare name is refused with what to write.
        let refused = set("format = 1\n", TRAY_IN_PANEL, "nm-applet").unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "panels.tray: expected a list of texts in quotes, not \"nm-applet\""
        );
    }

    #[test]
    fn do_not_disturb_is_a_flag_whose_mistakes_explain_themselves() {
        let file = |on: &str| format!("format = 1\n[notifications]\ndo_not_disturb = {on}\n");
        let (on, off) = (file("true"), file("false"));
        // Absent, it is off: the release decides, and no flag is read.
        assert_eq!(flag(DO_NOT_DISTURB, None, None), None);
        assert_eq!(flag(DO_NOT_DISTURB, Some("format = 1\n"), None), None);
        // The person's file over the machine's.
        assert_eq!(flag(DO_NOT_DISTURB, Some(&on), None), Some(true));
        assert_eq!(flag(DO_NOT_DISTURB, Some(&on), Some(&off)), Some(false));
        assert_eq!(flag(DO_NOT_DISTURB, None, Some(&on)), Some(true));
        // `set` writes the one line, and `check` accepts the file.
        let written = set("format = 1\n", DO_NOT_DISTURB, "true").unwrap();
        assert_eq!(
            written,
            "format = 1\n\n[notifications]\ndo_not_disturb = true\n"
        );
        assert!(check(&written).unwrap().is_empty());
        assert!(
            !unset(&written, DO_NOT_DISTURB)
                .unwrap()
                .contains("notifications")
        );
        // A word that is no flag is refused with what to write.
        let refused = set("format = 1\n", DO_NOT_DISTURB, "maybe").unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "notifications.do_not_disturb: expected true or false, not \"maybe\""
        );
        // A near name is corrected, and `get notifications` knows the page.
        let near = set("format = 1\n", "notification.do_not_disturb", "true").unwrap_err();
        assert!(
            format!("{near:#}").contains("did you mean notifications.do_not_disturb?"),
            "{near:#}"
        );
        assert_eq!(page("notifications").unwrap().title, "Notifications");
        assert!(!no_keys("notifications"));
    }

    #[test]
    fn the_workspace_keys_are_checked_and_written_one_way() {
        let text = |line: &str| format!("format = 1\n[workspaces]\n{line}\n");
        assert!(check(&text("count = 9")).unwrap().is_empty());
        assert_eq!(
            read(&text("count = 3")).unwrap().file.workspaces.count,
            Some(3)
        );
        assert_eq!(
            check(&text("count = 10")).unwrap(),
            ["workspaces.count: expected a whole number from 1 to 9, not 10"]
        );
        assert_eq!(
            check(&text("count = 0")).unwrap(),
            ["workspaces.count: expected a whole number from 1 to 9, not 0"]
        );
        let refused = set("format = 1\n", "workspaces.count", "10").unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "workspaces.count: expected a whole number from 1 to 9, not 10"
        );
        assert_eq!(
            set("format = 1\n", "workspaces.count", "4").unwrap(),
            "format = 1\n\n[workspaces]\ncount = 4\n"
        );
        assert_eq!(
            set("format = 1\n", "workspaces.dynamic", "true").unwrap(),
            "format = 1\n\n[workspaces]\ndynamic = true\n"
        );
        assert_eq!(
            set("format = 1\n", "workspaces.per_screen", "true").unwrap(),
            "format = 1\n\n[workspaces]\nper_screen = true\n"
        );
        assert_eq!(
            set("format = 1\n", OVERVIEW_STRIP, "top").unwrap(),
            "format = 1\n\n[workspaces]\noverview_strip = \"top\"\n"
        );
        let refused = set("format = 1\n", OVERVIEW_STRIP, "diagonal").unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "workspaces.overview_strip: unknown value \"diagonal\"; use left, right, top or bottom"
        );
        let names = set("format = 1\n", "workspaces.names", r#"["Mail", ""]"#).unwrap();
        assert_eq!(
            texts("workspaces.names", None, Some(&names)),
            Some(vec!["Mail".to_string(), String::new()])
        );
        assert!(check(&names).unwrap().is_empty());
    }

    #[test]
    fn the_workspace_switcher_keys_are_checked_and_written() {
        let base = "format = 1\n";
        assert_eq!(
            set(base, SWITCHER_LOOK, "button").unwrap(),
            "format = 1\n\n[appearance]\nswitcher_look = \"button\"\n"
        );
        assert_eq!(
            set(base, SWITCHER_SHOWN, "5").unwrap(),
            "format = 1\n\n[appearance]\nswitcher_shown = 5\n"
        );
        assert_eq!(
            set(base, SWITCHER_ENDS, "counts").unwrap(),
            "format = 1\n\n[appearance]\nswitcher_ends = \"counts\"\n"
        );
        let refused = set(base, SWITCHER_ENDS, "dots").unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "appearance.switcher_ends: unknown value \"dots\"; use fade, arrows or counts"
        );
        let refused = set(base, SWITCHER_LOOK, "dots").unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "appearance.switcher_look: unknown value \"dots\"; use numbers or button"
        );
        let refused = set(base, SWITCHER_SHOWN, "10").unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "appearance.switcher_shown: expected a whole number from 1 to 9, not 10"
        );
        let written = set(base, SWITCHER_SHOWN, "5").unwrap();
        assert_eq!(
            read(&written).unwrap().file.appearance.switcher_shown,
            Some(5)
        );
        assert!(check(&written).unwrap().is_empty());
    }

    #[test]
    fn an_app_opens_on_its_workspace_checked_and_written_one_way() {
        let line = |value: &str| format!("format = 1\n[workspaces]\napps = {value}\n");
        let written = set(
            "format = 1\n",
            "workspaces.apps",
            "{ \"org.mozilla.firefox\" = 2 }",
        )
        .unwrap();
        assert_eq!(
            written,
            "format = 1\n\n[workspaces]\napps = { \"org.mozilla.firefox\" = 2 }\n"
        );
        assert!(check(&written).unwrap().is_empty());
        let apps = read(&written).unwrap().file.workspaces.apps.unwrap();
        assert_eq!(apps.get("org.mozilla.firefox"), Some(&2));
        assert_eq!(
            format!(
                "{:#}",
                set(
                    "format = 1\n",
                    "workspaces.apps",
                    "{ \"org.mozilla.firefox\" = 10 }"
                )
                .unwrap_err()
            ),
            "workspaces.apps: \"org.mozilla.firefox\" has workspace 10; use a whole number from 1 to 9"
        );
        assert_eq!(
            check(&line("{ \"org.mozilla.firefox\" = \"x\" }")).unwrap(),
            [
                "workspaces.apps: \"org.mozilla.firefox\" has workspace \"x\"; use a whole number from 1 to 9"
            ]
        );
        assert_eq!(
            check(&line("{ \"org firefox\" = 2 }")).unwrap(),
            [
                "workspaces.apps: \"org firefox\" is not an app id; use the id an app's window gives, such as \"org.mozilla.firefox\""
            ]
        );
        assert_eq!(
            check(&line("\"org.mozilla.firefox\"")).unwrap(),
            [
                "workspaces.apps: expected apps and their workspace numbers, such as { \"org.mozilla.firefox\" = 2 }, not \"org.mozilla.firefox\""
            ]
        );
        assert!(is_app_id("org.gnome.Nautilus"));
        assert!(!is_app_id(""));
        assert!(!is_app_id("a\tb"));
        assert_eq!(
            sample(Kind::AppWorkspaces).to_string(),
            "{ \"org.mozilla.firefox\" = 2 }"
        );
    }
}
