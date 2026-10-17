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
    /// The group the page stands under (a `GROUPS` name)
    pub group: &'static str,
}

/// A heading the pages stand under, in the sidebar and in `edel settings`
/// (M5.32b): few pages to a group, so the list reads at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    /// The word the code and the tests use
    pub name: &'static str,
    /// What the heading says
    pub title: &'static str,
}

/// The groups of the Settings app, in its order; each page names one.
pub const GROUPS: &[Group] = &[
    Group {
        name: "desktop",
        title: n_("Desktop"),
    },
    Group {
        name: "connections",
        title: n_("Connections"),
    },
    Group {
        name: "devices",
        title: n_("Devices"),
    },
    Group {
        name: "personal",
        title: n_("Personal"),
    },
    Group {
        name: "system",
        title: n_("System"),
    },
];

/// The Settings app's pages, in its order (the mockups' order, Alimardon
/// 2026-10-08, and their groups, M5.32b: the page people open most first,
/// the desktop's pages, then the connections, the devices, the person's
/// own and the system's). Every section of the file is one of them, and a
/// page may have no section of the file (Sound, whose state is the sound
/// system's own). Settings (M5.6) reads the same table.
pub const PAGES: &[Page] = &[
    Page {
        section: "appearance",
        title: n_("Appearance"),
        about: n_("colours, accent, shape, fonts and motion"),
        group: "desktop",
    },
    Page {
        section: "layout",
        title: n_("Layout"),
        about: n_("the preset, tiling and title bars"),
        group: "desktop",
    },
    Page {
        section: "panels",
        title: n_("Panels"),
        about: n_("the panels and docks, the tray and the pinned apps"),
        group: "desktop",
    },
    Page {
        section: "workspaces",
        title: n_("Workspaces"),
        about: n_("how many, their names and the apps that open on each"),
        group: "desktop",
    },
    Page {
        section: "lock_screen",
        title: n_("Lock screen"),
        about: n_("its look, parts and background"),
        group: "desktop",
    },
    Page {
        // The connections and Wi-Fi passwords are NetworkManager's own
        // state, never in the file (ADR-006, M5.8a); the file holds the
        // device's name.
        section: "network",
        title: n_("Network"),
        about: n_("wired and Wi-Fi connections, and the device's name"),
        group: "connections",
    },
    Page {
        // BlueZ's own state, like Sound's: no key, and `edel settings get
        // bluetooth` says so (M5.8a).
        section: "bluetooth",
        title: n_("Bluetooth"),
        about: n_("pairing and connecting headphones, mice and other devices"),
        group: "connections",
    },
    Page {
        section: "displays",
        title: n_("Displays"),
        about: n_("each screen's place, scale and resolution"),
        group: "devices",
    },
    Page {
        // The sound system's own state, not a section of the file: the
        // volume and the chosen devices are WirePlumber's (M5.7b), so this
        // page has no key, and `edel settings get sound` says so.
        section: "sound",
        title: n_("Sound"),
        about: n_("the output and input devices and their volume"),
        group: "devices",
    },
    Page {
        section: "power",
        title: n_("Power"),
        about: n_("the lid, the power button and the screen lock"),
        group: "devices",
    },
    Page {
        // Banners and the notification centre are shell-ui's (M5.9b).
        section: "notifications",
        title: n_("Notifications"),
        about: n_("banners and do not disturb"),
        group: "personal",
    },
    Page {
        section: "shortcuts",
        title: n_("Shortcuts"),
        about: n_("the keys for each action"),
        group: "personal",
    },
    Page {
        section: "region",
        title: n_("Region"),
        about: n_("language, keyboard and time zone"),
        group: "personal",
    },
    Page {
        section: "default_apps",
        title: n_("Default apps"),
        about: n_("the browser, files, editor, terminal and mail"),
        group: "personal",
    },
    Page {
        section: "startup",
        title: n_("Startup"),
        about: n_("apps that start after login"),
        group: "personal",
    },
    Page {
        section: "apps",
        title: n_("Apps"),
        about: n_("what each app may use"),
        group: "personal",
    },
    Page {
        // Where apps may find this computer (M6.14c).
        section: "location",
        title: n_("Location"),
        about: n_("where apps may find this computer"),
        group: "personal",
    },
    Page {
        section: "addons",
        title: n_("Add-ons"),
        about: n_("signed extras to the system"),
        group: "system",
    },
    Page {
        section: "services",
        title: n_("Services"),
        about: n_("optional services, on or off"),
        group: "system",
    },
    Page {
        section: "users",
        title: n_("Users"),
        about: n_("the people who log in, and their ssh keys"),
        group: "system",
    },
    Page {
        section: "updates",
        title: n_("Updates"),
        about: n_("when updates are installed"),
        group: "system",
    },
    Page {
        section: "system",
        title: n_("System"),
        about: n_("developer mode and profiles"),
        group: "system",
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
    /// The lock screen's parts, as `[[lock_screen.parts]]` tables
    /// ([`crate::lock_screen::check`], M5.10c5)
    LockParts,
    /// A screen's resolution, `WIDTHxHEIGHT` such as `"1920x1080"`
    Resolution,
    /// Keyboard layouts as xkb names them, such as `"us,ru"` or
    /// `"de(nodeadkeys)"` ([`crate::keyboard::normalize`], M5.21)
    Keyboard,
    /// A time zone tzdata names, such as `Europe/Berlin`, checked against the
    /// zone files when the machine has them ([`crate::timezones::known_in`])
    TimeZone,
    /// A list of Flatpak app ids, such as `["io.mpv.Mpv"]`
    /// ([`crate::apps::is_flatpak_id`], M6.1)
    AppIds,
    /// A list of add-on names, such as `["fonts-cjk"]`: feature names
    /// ([`crate::features::is_name`], M7.2a)
    Addons,
    /// A list of host names, such as `["pool.ntp.org"]`: dot-separated
    /// names of letters, digits and hyphens, at most [`MOST_HOSTS`]
    /// (M1.13)
    Hosts,
    /// A release channel's name, such as `"stable"` ([`is_channel`], M5.8c)
    Channel,
    /// An xcursor theme's folder name, one this machine has under
    /// [`crate::places::ICON_THEME_DIRS`] (M5.12b5, [`cursor_themes`])
    CursorTheme,
    /// Apps and the workspace each opens on, such as `{ "org.mozilla.firefox" = 2 }` (M5.2l)
    AppWorkspaces,
    /// Flatpak apps and one of these choices each, such as
    /// `{ "org.gnome.Snapshot" = "never" }` ([`crate::app_permissions`], M6.14a)
    AppChoices(&'static [&'static str]),
    /// A wallpaper: a wallpaper's name such as `"hills"`, a picture's
    /// path, a colour `"#rrggbb"` or `"none"` ([`crate::wallpapers`],
    /// M5.10c4b)
    Wallpaper,
    /// A lock screen's look: a built-in look's name or one of the file's
    /// own `[lock_screen.looks]`, checked against the file by [`check`] and
    /// [`set`] (M5.10c5c)
    LockLook,
    /// A lock screen's background, as [`crate::lock_screen::parse_background`]
    /// reads it (M5.10c5c)
    LockBackground,
    /// The person's own looks, as `[lock_screen.looks.NAME]` tables
    /// ([`crate::lock_screen::check_looks`], M5.10c5c)
    LockLooks,
    /// An accent: one of the release's named accents, `wallpaper`, or a
    /// colour `#rrggbb` ([`crate::tokens::is_accent`], M5.12b1)
    Accent,
    /// A colour, `#rrggbb` or `#rrggbbaa` (M5.12b1)
    Colour,
    /// A theme's name: a built-in theme's or one of the file's own
    /// `[themes]`, checked against the file by [`check`] and [`set`]
    /// (M5.12b6a)
    Theme,
    /// The person's pictures on top of the theme in use: a list of
    /// wallpaper references, each as `appearance.wallpaper` takes it, at
    /// most [`crate::themes::MOST_PICTURES`], none twice (M5.12b6, part c2)
    Pictures,
    /// A person's own theme, as a `[themes.NAME]` table: its title, its
    /// pictures and the appearance values it sets ([`crate::themes`],
    /// M5.12b6a)
    OwnTheme,
    /// A layout preset's name: a built-in preset's, or one of the file's own
    /// `[presets]`, checked against the file by [`crate::presets::problems`]
    /// (M5.17)
    Preset,
    /// A person's own layout preset, as a `[presets.NAME]` table: the built-in
    /// preset it starts from and the `[layout]` keys it sets ([`crate::presets`],
    /// M5.17)
    OwnPreset,
    /// One wallpaper per workspace, in order: each entry a wallpaper as
    /// `appearance.wallpaper` takes it, or `""` for Appearance's
    /// (M5.12b5, `workspaces.wallpapers`)
    WallpaperList,
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
    "zen",
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
    now("region.timezone", Kind::TimeZone),
    // The servers the clock is set from (M1.13); `pool.ntp.org` without.
    now("region.time_servers", Kind::Hosts),
    now("layout.preset", Kind::Preset),
    now("layout.tiling", Kind::Flag),
    now(
        "layout.title_bars",
        Kind::OneOf(&["always", "floating-only", "never"]),
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
    // One wallpaper per workspace (M5.12b5): `""` is Appearance's.
    now(WORKSPACE_WALLPAPERS, Kind::WallpaperList),
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
    // The theme in use (M5.12b6a): a built-in theme's name or one of the
    // file's own `[themes]`; the theme's values apply under the ones here.
    now("appearance.theme", Kind::Theme),
    now("appearance.wallpaper", Kind::Wallpaper),
    // What lies behind the windows in dark mode, when it differs from
    // `appearance.wallpaper` (M5.10c4d): a person's own pair is two paths.
    now("appearance.wallpaper_dark", Kind::Wallpaper),
    // The person's pictures on top of the theme in use (M5.12b6, part c2):
    // references as `appearance.wallpaper` takes them, never copies.
    now("appearance.pictures", Kind::Pictures),
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
    // The high-contrast look (M6.9): strong edges and the plainest colours in
    // both schemes, laid over every colour the person chose; absent is normal.
    now("appearance.contrast", Kind::OneOf(&["normal", "high"])),
    now("appearance.accent", Kind::Accent),
    // A person's own colours (M5.12b1): any colour token by name, in both
    // schemes, or the light or the dark scheme's alone on top.
    now("appearance.colours.*", Kind::Colour),
    now("appearance.colours.light.*", Kind::Colour),
    now("appearance.colours.dark.*", Kind::Colour),
    // The windows' shape (M5.12b2): the corners, the gaps between tiled
    // windows and at the screen's edge (when it differs), the border, the
    // title bar's height and the panels' opacity, each in its range.
    now("appearance.radius", Kind::WholeFromTo(0, 24)),
    now("appearance.gaps", Kind::WholeFromTo(0, 64)),
    now("appearance.gaps_outer", Kind::WholeFromTo(0, 64)),
    now("appearance.border_width", Kind::WholeFromTo(0, 10)),
    now("appearance.title_bar_height", Kind::WholeFromTo(24, 48)),
    now("appearance.panel_opacity", Kind::WholeFromTo(20, 100)),
    // The windows' depth (M5.12b3): the opacity of the focused window and of
    // the others, and the shadow's blur and darkness, each in its range.
    now("appearance.window_opacity", Kind::WholeFromTo(20, 100)),
    now(
        "appearance.window_opacity_unfocused",
        Kind::WholeFromTo(20, 100),
    ),
    now("appearance.shadow_size", Kind::WholeFromTo(0, 64)),
    now("appearance.shadow_darkness", Kind::WholeFromTo(0, 100)),
    // The glass behind see-through panels (M5.12b3): how strongly the
    // wallpaper blurs there, 0 showing it sharp.
    now("appearance.blur", Kind::WholeFromTo(0, 100)),
    // The text (M5.12b5): the interface and monospace fonts by family name,
    // and every text size in percent of the release's.
    now(FONT, Kind::Text),
    now(MONOSPACE_FONT, Kind::Text),
    now(TEXT_SIZE, Kind::WholeFromTo(80, 150)),
    // The pointer and the icons (M5.12b5 part A): the cursor's size in
    // logical pixels, its theme, and the icons' size in percent.
    now(CURSOR_SIZE, Kind::WholeFromTo(16, 96)),
    now(CURSOR_THEME, Kind::CursorTheme),
    now(ICON_SIZE, Kind::WholeFromTo(75, 150)),
    now(
        "appearance.animations",
        Kind::OneOf(&["full", "reduced", "off"]),
    ),
    // How fast everything moves, as a share of the release's speed (M5.12b4):
    // 200 halves every length, 50 doubles it. One curve for every kind of
    // animation, windows and popups alike; there are no per-kind keys, as
    // the one setting is simpler and everything still follows it.
    now("appearance.animation_speed", Kind::WholeFromTo(25, 400)),
    now("appearance.animation_curve", Kind::OneOf(CURVES)),
    // The lock and login screens' parts, each placed by an anchor and an
    // offset (M5.10c5); absent is the classic arrangement.
    now("lock_screen.parts", Kind::LockParts),
    // The look that picks every part and the background at once (M5.10c5c),
    // the background behind them, whether the count of unread notifications
    // shows, and the person's own looks made by Save as.
    now("lock_screen.look", Kind::LockLook),
    now("lock_screen.background", Kind::LockBackground),
    // How much the background is darkened, in percent (M5.10c7g): absent is
    // the look's own, else the background's default.
    now(
        "lock_screen.dim",
        Kind::WholeFromTo(0, crate::lock_screen::MOST_DIM as i64),
    ),
    now(
        "lock_screen.notifications",
        Kind::OneOf(crate::lock_screen::NOTIFICATIONS),
    ),
    now("lock_screen.looks", Kind::LockLooks),
    // The person's own themes (M5.12b6a), each a table of pictures and
    // appearance values, checked as a whole by `normalize`.
    now("themes.*", Kind::OwnTheme),
    // The person's own layout presets (M5.17), each a table of `[layout]`'s keys
    // and the built-in preset it starts from.
    now("presets.*", Kind::OwnPreset),
    now("shortcuts.*", Kind::Keys),
    now("default_apps.browser", Kind::Text),
    now("default_apps.files", Kind::Text),
    now("default_apps.editor", Kind::Text),
    now("default_apps.terminal", Kind::Text),
    now("default_apps.mail", Kind::Text),
    now("startup.apps", Kind::Texts),
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
    now("services.*", Kind::Flag),
    // The release lists `edel update` takes (M3.8); absent, the image's.
    now("updates.channel", Kind::Channel),
    later("updates.version", Kind::Text),
    later(
        "updates.automatic",
        Kind::OneOf(&["off", "check", "install", "install-and-restart"]),
    ),
    later("updates.restart_window", Kind::Text),
    now("apps.installed", Kind::AppIds),
    // What each app may use (M6.14a), the person's, laid onto Flatpak's
    // own state by shell-ui.
    now(
        "apps.camera",
        Kind::AppChoices(crate::app_permissions::CAMERA_CHOICES),
    ),
    now(
        "apps.microphone",
        Kind::AppChoices(crate::app_permissions::ALLOW_OR_NEVER),
    ),
    now(
        "apps.screenshots",
        Kind::AppChoices(crate::app_permissions::ALLOW_OR_NEVER),
    ),
    now(
        "apps.notifications",
        Kind::AppChoices(crate::app_permissions::ALLOW_OR_NEVER),
    ),
    now(
        "apps.files",
        Kind::AppChoices(crate::app_permissions::FILES_CHOICES),
    ),
    now("addons.installed", Kind::Addons),
    // Banners stay away while the notification list still fills (M5.9b).
    now("notifications.do_not_disturb", Kind::Flag),
    // Where banners appear (M5.9j): by the status area, or a corner or the
    // top's middle.
    now(
        "notifications.position",
        Kind::OneOf(&[
            "auto",
            "top-right",
            "top-left",
            "bottom-right",
            "bottom-left",
            "top",
        ]),
    ),
    // What the notifications button shows while some are unread (M5.9k).
    now(
        "notifications.badge",
        Kind::OneOf(&["dot", "count", "none"]),
    ),
    // Where the notification list opens (M5.9l): hung on the button that
    // opened it, or a sheet along the left or the right side.
    now(
        "notifications.list",
        Kind::OneOf(&["button", "left", "right"]),
    ),
    // The input method for typing languages a keyboard has no keys for
    // (M6.8): fcitx5, started with the session; absent or none runs none.
    now("region.input_method", Kind::OneOf(&["none", "fcitx5"])),
    // What apps may know of this computer's place (M6.14c): each app's
    // choice, then the person's switch, the machine's place and its
    // online lookup.
    now(
        "apps.location",
        Kind::AppChoices(crate::app_permissions::CAMERA_CHOICES),
    ),
    // Location (M6.14c): the person's switch, the machine's place and its
    // online lookup.
    now("location.enabled", Kind::Flag),
    now("location.place", Kind::TimeZone),
    now("location.online", Kind::Flag),
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
    #[serde(default, skip_serializing_if = "is_default")]
    pub lock_screen: LockScreen,
    /// The person's own themes, each a `[themes.NAME]` table (M5.12b6a)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub themes: BTreeMap<String, OwnTheme>,
    /// The person's own layout presets, each a `[presets.NAME]` table (M5.17)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub presets: BTreeMap<String, OwnPreset>,
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
    #[serde(default, skip_serializing_if = "is_default")]
    pub location: Location,
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
            lock_screen: LockScreen::default(),
            themes: BTreeMap::new(),
            presets: BTreeMap::new(),
            shortcuts: BTreeMap::new(),
            region: Region::default(),
            users: BTreeMap::new(),
            network: Network::default(),
            default_apps: DefaultApps::default(),
            startup: Startup::default(),
            power: Power::default(),
            notifications: Notifications::default(),
            location: Location::default(),
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
    /// The input method, `fcitx5` or `none` (M6.8); absent runs none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_method: Option<String>,
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

/// One of the person's own layout presets (M5.17): a `[presets.NAME]` table,
/// the built-in preset it starts from (`preset`, absent is Classic) and the
/// `[layout]` keys it sets, as the table's own keys. The keys a reader
/// applies are [`Layout`]'s, checked the same way ([`crate::presets`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OwnPreset {
    #[serde(default = "own_preset_base")]
    pub preset: String,
    #[serde(flatten)]
    pub layout: Layout,
}

fn own_preset_base() -> String {
    crate::presets::DEFAULT.to_string()
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
    /// The wallpaper of each workspace, the first workspace's first, each
    /// as `appearance.wallpaper` takes it and `""` for Appearance's
    /// (M5.12b5); absent is Appearance's for every workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallpapers: Option<Vec<String>>,
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
    /// The theme in use (M5.12b6a): a built-in theme's name, or one of the
    /// file's own `[themes]`; absent is the release's own, `edel`. The
    /// values the theme sets apply under the ones written here
    /// ([`crate::themes::themed`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<String>,
    /// What lies behind the windows in dark mode, when it differs from
    /// `wallpaper`: a person's own pair is two paths (M5.10c4d).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallpaper_dark: Option<String>,
    /// The person's pictures on top of the theme in use, as references
    /// (M5.12b6, part c2): a wallpaper's name, a picture's path or a colour,
    /// each as `wallpaper` takes it. Absent is the theme's own list, and an
    /// empty list shows no picture ([`crate::themes::pictures_of`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pictures: Option<Vec<String>>,
    /// Light, dark or automatic
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// `normal` or `high` (M6.9): high contrast lays the high-contrast colours
    /// of `design/tokens.toml` over every colour, in both schemes; absent is
    /// normal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    /// A person's own colours, any colour token by name (M5.12b1); absent
    /// is the release's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colours: Option<Colours>,
    /// The windows' corner radius in logical pixels, 0 to 24 (M5.12b2); the
    /// other corners scale with it. Absent is the release's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<u32>,
    /// The gap between tiled windows, logical pixels, 0 to 64 (M5.12b2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gaps: Option<u32>,
    /// The gap between tiled windows and the screen's edge, 0 to 64 (M5.12b2);
    /// absent is `gaps`' value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gaps_outer: Option<u32>,
    /// The border round each window with a title bar, 0 to 10 logical pixels
    /// (M5.12b2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_width: Option<u32>,
    /// The title bar's height, 24 to 48 logical pixels (M5.12b2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_bar_height: Option<u32>,
    /// The panels' opacity in percent, 20 to 100 (M5.12b2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_opacity: Option<u32>,
    /// The focused window's opacity in percent, 20 to 100, its title bar and
    /// border with it (M5.12b3); absent is 100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_opacity: Option<u32>,
    /// Every other window's opacity in percent, 20 to 100 (M5.12b3); absent
    /// is 100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_opacity_unfocused: Option<u32>,
    /// The shadow's blur in logical pixels, 0 to 64 (M5.12b3); 0 draws no
    /// shadow. Absent is the release's 24.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_size: Option<u32>,
    /// The shadow's darkness in percent, 0 to 100, in both schemes (M5.12b3);
    /// absent is the scheme's own colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_darkness: Option<u32>,
    /// How strongly the wallpaper blurs behind see-through panels, 0 to 100
    /// (M5.12b3); 0 shows it sharp. Absent is the release's 50.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blur: Option<u32>,
    /// The interface font's family (M5.12b5); absent is the tokens' own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// The monospace font's family (M5.12b5); absent is
    /// [`crate::tokens::MONOSPACE`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monospace_font: Option<String>,
    /// Every text size in percent, 80 to 150 (M5.12b5); absent is 100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_size: Option<u32>,
    /// The cursor's height in logical pixels, 16 to 96 (M5.12b5 part A);
    /// absent is [`CURSOR_SIZE_DEFAULT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_size: Option<u32>,
    /// The cursor's xcursor theme (M5.12b5 part A); absent is
    /// [`CURSOR_THEME_DEFAULT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_theme: Option<String>,
    /// The icons' size in percent, 75 to 150 (M5.12b5 part A); absent is
    /// [`ICON_SIZE_DEFAULT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_size: Option<u32>,
    /// Full, reduced or off
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animations: Option<String>,
    /// How fast things move, percent of the release's speed (M5.12b4); absent
    /// is 100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation_speed: Option<u32>,
    /// How each animation eases, `smooth`, `snappy` or `springy` (M5.12b4);
    /// absent is smooth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation_curve: Option<String>,
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

/// A person's own colours (M5.12b1): any colour token by name, for both
/// schemes, and the light and dark scheme's own on top of them. The shared
/// names are the flattened table, so `[appearance.colours]` holds
/// `panel = "#1e1e2e"` and `[appearance.colours.dark]` holds the dark one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Colours {
    #[serde(flatten)]
    pub both: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<BTreeMap<String, String>>,
}

/// One of the person's own themes (M5.12b6a): a `[themes.NAME]` table, its
/// pictures and the appearance values it sets, as the keys of the table
/// itself (`accent = "wallpaper"`, `[themes.NAME.colours]`). A built-in
/// theme is read the same way from its file ([`crate::themes`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OwnTheme {
    /// Shown instead of the name made readable; absent is that name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The wallpapers of the theme, each as `appearance.wallpaper` takes it;
    /// a copy is never made, so a line here is the picture.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pictures: Vec<String>,
    #[serde(flatten)]
    pub values: Appearance,
}

/// `over`'s colours laid over `under`'s, name by name, in the shared table
/// and in each scheme's own (M5.12b1); none when neither names one.
fn merged(
    under: Option<&BTreeMap<String, String>>,
    over: Option<&BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    match (under, over) {
        (None, None) => None,
        (under, over) => {
            let mut colours = under.cloned().unwrap_or_default();
            colours.extend(
                over.into_iter()
                    .flatten()
                    .map(|(n, v)| (n.clone(), v.clone())),
            );
            Some(colours)
        }
    }
}

/// The person's colours over the machine's (M5.12b1): each colour name
/// the person sets wins, the others stay the machine's, in `both`, `light`
/// and `dark` alike. None when neither file sets colours.
pub fn colours_over(machine: Option<&Colours>, person: Option<&Colours>) -> Option<Colours> {
    if machine.is_none() && person.is_none() {
        return None;
    }
    let empty = Colours::default();
    let (machine, person) = (machine.unwrap_or(&empty), person.unwrap_or(&empty));
    Some(Colours {
        both: merged(Some(&machine.both), Some(&person.both)).unwrap_or_default(),
        light: merged(machine.light.as_ref(), person.light.as_ref()),
        dark: merged(machine.dark.as_ref(), person.dark.as_ref()),
    })
}

/// The six keys of the windows' shape (M5.12b2), each a whole number in its
/// key table range ([`whole_range`]).
pub const SHAPE_KEYS: &[&str] = &[
    "appearance.radius",
    "appearance.gaps",
    "appearance.gaps_outer",
    "appearance.border_width",
    "appearance.title_bar_height",
    "appearance.panel_opacity",
];

/// The five keys of the windows' depth (M5.12b3), each a whole number in its
/// key table range: the focused and the other windows' opacity, the shadow's
/// blur and darkness, and the glass behind see-through panels. Not in
/// [`SHAPE_KEYS`], whose grid Settings draws for the six shape keys alone.
pub const DEPTH_KEYS: &[&str] = &[
    "appearance.window_opacity",
    "appearance.window_opacity_unfocused",
    "appearance.shadow_size",
    "appearance.shadow_darkness",
    "appearance.blur",
];

/// The interface font's family, as fontconfig names it (M5.12b5).
pub const FONT: &str = "appearance.font";
/// The font apps use for code and terminals (M5.12b5); absent is
/// [`crate::tokens::MONOSPACE`].
pub const MONOSPACE_FONT: &str = "appearance.monospace_font";
/// Every text size in percent of the release's, 80 to 150 (M5.12b5).
pub const TEXT_SIZE: &str = "appearance.text_size";
/// The cursor's height in logical pixels, 16 to 96; absent is
/// [`CURSOR_SIZE_DEFAULT`] (M5.12b5 part A).
pub const CURSOR_SIZE: &str = "appearance.cursor_size";
/// The cursor's xcursor theme, a folder name [`cursor_themes`] lists;
/// absent is [`CURSOR_THEME_DEFAULT`] (M5.12b5 part A).
pub const CURSOR_THEME: &str = "appearance.cursor_theme";
/// The icons' size in percent of the release's, 75 to 150; absent is 100
/// (M5.12b5 part A).
pub const ICON_SIZE: &str = "appearance.icon_size";
/// The cursor's size when no file sets it, in logical pixels.
pub const CURSOR_SIZE_DEFAULT: u32 = 24;
/// The cursor theme when no file sets it: the xcursor standard's own.
pub const CURSOR_THEME_DEFAULT: &str = "default";
/// The icons' size when no file sets it, in percent of the release's.
pub const ICON_SIZE_DEFAULT: u32 = 100;

/// The three keys of the text (M5.12b5), read by [`crate::tokens::Text`].
pub const TEXT_KEYS: &[&str] = &[FONT, MONOSPACE_FONT, TEXT_SIZE];

/// The cursor's theme names this machine has, `default` first, then the
/// others under [`crate::places::ICON_THEME_DIRS`] that have a `cursors`
/// folder, sorted and without repeats (M5.12b5 part A). Settings lists them
/// and the checker takes a theme only from this list.
pub fn cursor_themes() -> Vec<String> {
    let mut names: Vec<String> = crate::places::ICON_THEME_DIRS
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .filter(|entry| entry.path().join("cursors").is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != CURSOR_THEME_DEFAULT)
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names.insert(0, CURSOR_THEME_DEFAULT.to_string());
    names
}

/// What is wrong with `name` as a cursor theme on this machine, if anything:
/// a name with a `/` or a leading dot is never a folder name; a theme this
/// machine lacks is refused with the ones it has (the names come from
/// [`cursor_themes`]). `default` is always taken, as the release's own.
pub fn cursor_theme_problem(name: &str) -> Option<String> {
    if name == CURSOR_THEME_DEFAULT || cursor_themes().iter().any(|t| t == name) {
        return None;
    }
    let names = cursor_themes().join(", ");
    Some(trf(
        "{name} is not a cursor theme this machine has; the ones it has are {names} (to use the release's, run edel settings reset on this key)",
        &[("name", &format!("{name:?}")), ("names", &names)],
    ))
}

/// The least and most a whole-number key takes, from its entry in [`KEYS`]
/// (M5.12b2): the one place a reader finds a range, so the tokens and the
/// file's own check agree. None for a key that is not a whole number range.
pub fn whole_range(key: &str) -> Option<(i64, i64)> {
    KEYS.iter()
        .find(|k| k.path == key)
        .and_then(|k| match k.kind {
            Kind::WholeFromTo(least, most) => Some((least, most)),
            _ => None,
        })
}

/// Whether any of the text keys differ between two appearances (M5.12b5),
/// for the compositor and shell-ui to read the text again.
pub fn text_changed(old: &Appearance, new: &Appearance) -> bool {
    old.font != new.font
        || old.monospace_font != new.monospace_font
        || old.text_size != new.text_size
}

/// Whether any of the windows' shape keys differ between two appearances
/// (M5.12b2), for the compositor to re-lay its windows when the file changes.
pub fn shape_changed(old: &Appearance, new: &Appearance) -> bool {
    old.radius != new.radius
        || old.gaps != new.gaps
        || old.gaps_outer != new.gaps_outer
        || old.border_width != new.border_width
        || old.title_bar_height != new.title_bar_height
        || old.panel_opacity != new.panel_opacity
        || old.window_opacity != new.window_opacity
        || old.window_opacity_unfocused != new.window_opacity_unfocused
        || old.shadow_size != new.shadow_size
        || old.shadow_darkness != new.shadow_darkness
        || old.blur != new.blur
}

/// The Lock screen page (M5.10c5): the parts the lock and login screens
/// show and where each stands.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LockScreen {
    /// The parts in place of the classic arrangement, each with its anchor
    /// and offset ([`crate::lock_screen::check`]); absent is the classic one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<crate::lock_screen::Placed>>,
    /// A look: `classic`, `magazine`, `minimal` or one of `looks` (M5.10c5c);
    /// absent is `classic`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look: Option<String>,
    /// What lies behind the parts: `backdrop`, `blurred`, `sharp`, a colour
    /// `#rrggbb` or a picture's full path ([`crate::lock_screen::parse_background`]);
    /// absent is the look's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// How much the background is darkened, in percent, 0 to 80 (M5.10c7g);
    /// absent is the look's own, else the background's default
    /// ([`crate::lock_screen::default_dim`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dim: Option<u32>,
    /// `count`, the unread notifications' number, or `none`; absent is `count`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notifications: Option<String>,
    /// The person's own looks, each a `[lock_screen.looks.NAME]` table
    /// (M5.10c5c), made by Save as on the Lock screen page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looks: Option<BTreeMap<String, OwnLook>>,
}

/// One of the person's own looks: the parts it places and what lies behind
/// them ([`LockScreen::looks`]). A look is a preset of the lock: choosing it
/// brings both (Alimardon, 2026-10-11).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OwnLook {
    pub parts: Vec<crate::lock_screen::Placed>,
    /// As `lock_screen.background` takes it; absent is the blurred
    /// wallpaper, as for every look but classic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// As `lock_screen.dim` takes it (M5.10c7g); absent is the background's
    /// default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dim: Option<u32>,
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

/// The default apps a file sets (M6.10), as (role, desktop file id) pairs in
/// [`apps::DEFAULT_APP_KEYS`]' order; an empty id is none. Shared by
/// `edel settings apply` (the system's `mimeapps.list`) and the compositor
/// (the shortcuts that open them).
pub fn default_app_roles(defaults: &DefaultApps) -> Vec<(&'static str, &str)> {
    let values = [
        ("files", &defaults.files),
        ("browser", &defaults.browser),
        ("editor", &defaults.editor),
        ("terminal", &defaults.terminal),
        ("mail", &defaults.mail),
    ];
    crate::apps::DEFAULT_APP_KEYS
        .iter()
        .filter_map(|(role, _)| {
            let (_, id) = values.iter().find(|(r, _)| r == role)?;
            let id = id.as_deref().filter(|id| !id.is_empty())?;
            Some((*role, id))
        })
        .collect()
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
    /// Where banners appear: `auto` (by the status area), a corner, or
    /// `top`; absent is `auto` (M5.9j).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    /// What the notifications button shows while some are unread: `dot`,
    /// `count` or `none`; absent is `dot` (M5.9k).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// Where the notification list opens: `button` (hung on the button
    /// that opened it), `left` or `right` (a sheet along that side); absent
    /// is `button` (M5.9l).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<String>,
}

/// The Location page (M6.14c): whether apps may know where this computer
/// is, the place it is in and whether it is found from the Wi-Fi networks
/// around it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Location {
    /// Whether apps may know where this computer is at all; absent is on.
    /// Off gives every app nothing, whatever each app's choice is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// The city apps are told this computer is in, named by its time zone
    /// such as `Asia/Tashkent`; absent is the time zone's city.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// Whether the place is found from the Wi-Fi networks around the
    /// computer, through BeaconDB; absent is off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online: Option<bool>,
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
    /// What each app may use (M6.14a): app id and choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub microphone: Option<BTreeMap<String, String>>,
    /// Which apps may know where this computer is (M6.14c): app id and
    /// choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshots: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notifications: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<BTreeMap<String, String>>,
}

impl Apps {
    /// The choices the file makes, by key, as
    /// [`crate::app_permissions::entries`] takes them.
    pub fn choices(&self) -> crate::app_permissions::Choices {
        [
            ("apps.camera", &self.camera),
            ("apps.microphone", &self.microphone),
            ("apps.location", &self.location),
            ("apps.screenshots", &self.screenshots),
            ("apps.notifications", &self.notifications),
            ("apps.files", &self.files),
        ]
        .into_iter()
        .filter_map(|(key, table)| table.clone().map(|t| (key, t)))
        .collect()
    }
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
    // A key a release stopped using is still read; its note says so (M6.11).
    lines.extend(retired_notes(text, crate::key_help::find));
    // Across keys: unknown actions, two on one key, a way out unbound.
    lines.extend(crate::shortcuts::check(&read.file.shortcuts));
    // Across keys: a look that is neither built in nor the file's own (M5.10c5c).
    lines.extend(look_problem(&read.file).map(|m| format!("lock_screen.look: {m}")));
    // Across keys: the colours name colour tokens and keep every pair readable (M5.12b1).
    lines.extend(colour_problems(&read.file));
    // Across keys: each theme is a name of its own that no built-in takes,
    // and the theme in use is one of them (M5.12b6a).
    lines.extend(crate::themes::problems(&read.file));
    // Across keys: each own preset names a name of its own and a built-in
    // preset to start from, and the preset in use is one of them (M5.17).
    lines.extend(crate::presets::problems(&read.file));
    Ok(lines)
}

/// Why the file's `lock_screen.look` names no look (M5.10c5c), if it does
/// not: a built-in one, or one of its own `[lock_screen.looks]`.
fn look_problem(file: &SettingsFile) -> Option<String> {
    let look = file.lock_screen.look.as_deref()?;
    crate::lock_screen::check_look(look, file.lock_screen.looks.as_ref()).err()
}

/// The problems the file's accent and colours cause (M5.12b1), one line
/// each, the key first. A name that is no colour token is said with the
/// nearest one (`"panl" is not a colour; did you mean panel?`). Then each
/// pair of the release that the colours leave under its ratio, in either
/// scheme: `appearance.accent` when the accent alone leaves it, else
/// `appearance.colours`, with [`crate::tokens::Tokens::contrast_problems`]'s
/// words.
fn colour_problems(file: &SettingsFile) -> Vec<String> {
    let mut lines = Vec::new();
    let built = crate::tokens::Tokens::built_in();
    if let Some(colours) = &file.appearance.colours {
        let tables = [
            ("appearance.colours", Some(&colours.both)),
            ("appearance.colours.light", colours.light.as_ref()),
            ("appearance.colours.dark", colours.dark.as_ref()),
        ];
        for (section, table) in tables {
            for name in table.into_iter().flat_map(|table| table.keys()) {
                if built.colour(name).is_some() {
                    continue;
                }
                let message = match nearest_value(name, crate::tokens::COLOUR_NAMES) {
                    Some(near) => trf(
                        "{name} is not a colour; did you mean {near}?",
                        &[("name", &format!("{name:?}")), ("near", near)],
                    ),
                    None => trf(
                        "{name} is not a colour; try a colour such as {names}, or see edel settings get appearance",
                        &[
                            ("name", &format!("{name:?}")),
                            (
                                "names",
                                &crate::tokens::COLOUR_NAMES[..COLOUR_EXAMPLES].join(", "),
                            ),
                        ],
                    ),
                };
                lines.push(format!("{section}.{name}: {message}"));
            }
        }
    }
    let accent = file.appearance.accent.as_deref();
    let colours = file.appearance.colours.as_ref();
    for scheme in [crate::tokens::Scheme::Light, crate::tokens::Scheme::Dark] {
        let base = crate::tokens::Tokens::built_in_scheme(scheme);
        let accent_alone = base
            .clone()
            .with_appearance(accent, None, scheme, None)
            .0
            .contrast_problems();
        let (tokens, _) = base.with_appearance(accent, colours, scheme, None);
        for problem in tokens.contrast_problems() {
            let key = if accent_alone.contains(&problem) {
                "appearance.accent"
            } else {
                "appearance.colours"
            };
            let line = format!("{key}: {problem}");
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
    }
    lines
}

/// How many colour names a problem line shows, as examples (M5.12b1).
const COLOUR_EXAMPLES: usize = 3;

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
        // The built-in presets; the person's own come from the file (M5.17).
        Some(Key {
            kind: Kind::Preset, ..
        }) => crate::presets::NAMES,
        _ => &[],
    }
}

/// The value at the dotted `path` of `part`'s table form (`window_buttons`,
/// `colours.panel`); none when a name on the way is missing.
pub fn value_at(part: &impl Serialize, path: &str) -> Option<Value> {
    let mut value = Value::try_from(part).ok()?;
    for name in path.split('.') {
        value = value.as_table()?.get(name)?.clone();
    }
    Some(value)
}

/// The file as the desktop applies it (M5.17, M5.12b6a): its own layout
/// preset in use laid under its keys ([`crate::presets::applied`]), then its
/// theme ([`crate::themes::themed`]). Every reader of a file's layout or
/// appearance values calls this on each file it reads, before it reads
/// (ADR-010); the writers never do, so they write the file as it is.
pub fn in_use(file: &SettingsFile) -> SettingsFile {
    crate::themes::themed(&crate::presets::applied(file))
}

/// `key`'s value as the files say it now: the person's file over the
/// machine's, as a string; none when neither sets it.
pub fn chosen(key: &str, machine: Option<&str>, person: Option<&str>) -> Option<String> {
    // A value a theme gives comes first, as the tokens take it (M5.12b6a);
    // with no theme in the files this is the value each file says.
    if let Some(value) = crate::themes::chosen(key, machine, person) {
        return Some(value);
    }
    // A layout value an own preset gives, as the preset in use applies it (M5.17).
    if let Some(value) = crate::presets::chosen(key, machine, person) {
        return Some(value);
    }
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

/// The key that says whether apps may know where this computer is (M6.14c),
/// the one name shell-ui, Settings and `edel settings` share.
pub const LOCATION_ENABLED: &str = "location.enabled";

/// Whether apps may know where this computer is: the file's choice, and on
/// when the file names none (M6.14c).
pub fn location_enabled(file: &SettingsFile) -> bool {
    file.location.enabled.unwrap_or(true)
}

/// The key that says where banners appear (M5.9j), the one name shell-ui,
/// Settings and `edel settings` share; its values are [`choices`]'.
pub const NOTIFICATION_POSITION: &str = "notifications.position";

/// The key that says what the notifications button shows while some are
/// unread (M5.9k), the one name shell-ui, Settings and `edel settings`
/// share; its values are [`choices`]'.
pub const NOTIFICATION_BADGE: &str = "notifications.badge";

/// The key that says where the notification list opens (M5.9l), the one
/// name shell-ui, Settings and `edel settings` share; its values are
/// [`choices`]'.
pub const NOTIFICATION_LIST: &str = "notifications.list";

/// The key that says how much things move (M5.11b): `full`, the default,
/// `reduced` or `off`; the lock screen's field shakes only at `full`
/// (M5.10c).
pub const ANIMATIONS: &str = "appearance.animations";

/// The key that says how fast things move (M5.12b4), as a percent of the
/// release's speed: 100, the default, is as made; 200 is twice as fast.
pub const ANIMATION_SPEED: &str = "appearance.animation_speed";

/// The key that says how each animation eases (M5.12b4): `smooth`, the
/// default, `snappy` or `springy`; its values are [`CURVES`].
pub const ANIMATION_CURVE: &str = "appearance.animation_curve";

/// The easing curves `appearance.animation_curve` takes (M5.12b4).
pub const CURVES: &[&str] = &["smooth", "snappy", "springy"];

/// The speed `appearance.animation_speed` is when the file does not say,
/// as a percent of the release's (M5.12b4): as made.
pub const ANIMATION_SPEED_DEFAULT: u32 = 100;

/// The motion `appearance.animations` is when the file does not say (M5.12b4).
pub const ANIMATIONS_DEFAULT: &str = "full";

/// The curve `appearance.animation_curve` is when the file does not say
/// (M5.12b4).
pub const ANIMATION_CURVE_DEFAULT: &str = "smooth";

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

/// The key each workspace's own wallpaper is kept under, one per place
/// (M5.12b5), which moves with its workspace as the names do.
pub const WORKSPACE_WALLPAPERS: &str = "workspaces.wallpapers";

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
    write_text(path, &edited)
}

/// Sets or takes out several keys in the settings file at `path` in one
/// write, in the order given, each checked as [`set`] and [`unset`] check
/// it, so a change that clashes with one still to come must come after it.
/// The Shortcuts page moves a chord and clears the action that held it this
/// way (M5.13b).
pub fn write_all(path: &Path, edits: &[(&str, Option<&str>)]) -> Result<()> {
    let mut text = fs::read_to_string(path).unwrap_or_else(|_| format!("format = {FORMAT}\n"));
    for (key, value) in edits {
        text = match value {
            Some(value) => set(&text, key, value)?,
            None => unset(&text, key)?,
        };
    }
    write_text(path, &text)
}

/// Writes a settings file's whole text to `path` (its folder made first),
/// through a file beside it that is renamed over it, so a reader never sees
/// half a file. The writers above and Settings' theme edits use it.
pub fn write_text(path: &Path, edited: &str) -> Result<()> {
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
    if let Some(message) = retired_message(key, crate::key_help::find(key)) {
        bail!("{message}");
    }
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
    // A look names a built-in look or one of the file's own (M5.10c5c).
    if key == "lock_screen.look" {
        if let Some(message) = look_problem(&read(&text)?.file) {
            bail!("{key}: {message}");
        }
    }
    // A colour names a colour token and keeps every pair readable (M5.12b1).
    if key == "appearance.accent" || key.starts_with("appearance.colours") {
        let lines = colour_problems(&read(&text)?.file);
        if !lines.is_empty() {
            bail!("{}", lines.join("; "));
        }
    }
    // A theme is named, and the theme in use is one of them (M5.12b6a).
    if key == "appearance.theme" || key.starts_with("themes.") {
        let lines = crate::themes::problems(&read(&text)?.file);
        if !lines.is_empty() {
            bail!("{}", lines.join("; "));
        }
    }
    // A preset is named, and the preset in use is one of them (M5.17).
    if key == "layout.preset" || key.starts_with("presets.") {
        let lines = crate::presets::problems(&read(&text)?.file);
        if !lines.is_empty() {
            bail!("{}", lines.join("; "));
        }
    }
    Ok(text)
}

/// Why `set` refuses `key` because a release stopped using it (M6.11), if
/// it did: `line` is the key's line in [`crate::key_help`].
fn retired_message(key: &str, line: Option<&crate::key_help::Help>) -> Option<String> {
    let version = line?.retired?;
    Some(trf(
        "{key}: no longer used since {version}; edel settings reset {key} takes it out",
        &[("key", key), ("version", version)],
    ))
}

/// The notes `check` gives for the keys a file holds that a release stopped
/// using (M6.11): `find` gives each key's line, as [`crate::key_help::find`].
fn retired_notes(
    text: &str,
    find: impl Fn(&str) -> Option<&'static crate::key_help::Help>,
) -> Vec<String> {
    let Ok(table) = toml::from_str::<Table>(text) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    present_keys(&table, "", &mut keys);
    keys.iter()
        .filter_map(|key| {
            let version = find(key)?.retired?;
            Some(trf(
                "{key}: no longer used since {version}",
                &[("key", key), ("version", version)],
            ))
        })
        .collect()
}

/// Every key a table holds, dotted, the tables' own names left out.
fn present_keys(table: &Table, prefix: &str, out: &mut Vec<String>) {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            Value::Table(inner) => present_keys(inner, &key, out),
            _ => out.push(key),
        }
    }
}

/// Whether `kind` takes a list, which `KEY+=VALUE` and `KEY-=VALUE` edit
/// (M6.11): a list of texts, of Flatpak ids, of host names or of pictures.
fn is_list(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Texts | Kind::AppIds | Kind::Addons | Kind::Hosts | Kind::Pictures
    )
}

/// The value at `path` in `table`, if the file has it.
fn list_value_at<'a>(table: &'a Table, path: &[&str]) -> Option<&'a Value> {
    let (last, parents) = path.split_last()?;
    let mut here = table;
    for part in parents {
        here = here.get(*part)?.as_table()?;
    }
    here.get(*last)
}

/// `edel settings set KEY+=VALUE` (adds) and `KEY-=VALUE` (removes) on a
/// file's text (M6.11). Adding puts VALUE at the end of the list unless it is
/// there already; removing takes out its one item, which must be there. The
/// whole new list is written through [`set`], so it is checked as
/// `KEY=[...]` would be, and an empty list removes the key, as [`unset`]
/// does.
pub fn edit_list(text: &str, key: &str, value: &str, add: bool) -> Result<String> {
    let path: Vec<&str> = key.split('.').collect();
    let entry = known_key(key, &path)?;
    if !is_list(entry.kind) {
        bail!(
            "{}",
            trf(
                "{key}: += and -= work on lists only; this key holds one value, so use {key}=VALUE",
                &[("key", key)]
            )
        );
    }
    // A list item reads as the arguments of `set` do, so a quoted text and a
    // bare word both give the text.
    let item = match value_from_arg(value) {
        Value::String(text) => text,
        _ => value.to_string(),
    };
    let table: Table = toml::from_str(text).context(tr("the file is not valid TOML"))?;
    let mut list: Vec<String> = match list_value_at(&table, &path) {
        None => Vec::new(),
        Some(Value::Array(items)) if items.iter().all(Value::is_str) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(_) => bail!(
            "{}",
            trf(
                "{key} is not a list in the file; change it by hand",
                &[("key", key)]
            )
        ),
    };
    if add {
        if !list.contains(&item) {
            list.push(item);
        }
    } else {
        if !list.contains(&item) {
            bail!(
                "{}",
                trf(
                    "{key}: {item} is not in the list",
                    &[("key", key), ("item", &format!("{item:?}"))]
                )
            );
        }
        list.retain(|kept| *kept != item);
    }
    if list.is_empty() {
        return unset(text, key);
    }
    let array = Value::Array(list.into_iter().map(Value::String).collect());
    set(text, key, &array.to_string())
}

/// `edel settings reset KEY` on a file's text: removes the key, and tables
/// left empty by it, so the release decides again (ADR-008). Every other
/// byte stays. A key this release does not know can be removed too. A
/// user's table stays when its last key goes, so apply still looks after
/// the user and takes back what the key gave, such as admin.
pub fn unset(text: &str, key: &str) -> Result<String> {
    unset_all(text, &[key])
}

/// `edel settings reset KEY...` on a file's text: removes each key the
/// file holds, as `unset` does. A key already at its default is passed
/// over while another one is removed, so resetting a group needs no care
/// for which of its keys are set; only when none is in the file does it
/// refuse, as there is nothing to reset.
pub fn unset_all(text: &str, keys: &[&str]) -> Result<String> {
    let mut doc: DocumentMut = text.parse().context(tr("the file is not valid TOML"))?;
    let mut removed = false;
    for key in keys {
        removed |= remove_key(&mut doc, key)?;
    }
    if !removed {
        match keys {
            [key] => bail!(
                "{}",
                trf(
                    "{key} is not in the file, so it already has its default; there is nothing to reset",
                    &[("key", key)]
                )
            ),
            _ => bail!(
                "{}",
                trf(
                    "none of {keys} is in the file, so they already have their defaults; there is nothing to reset",
                    &[("keys", &keys.join(", "))]
                )
            ),
        }
    }
    Ok(doc.to_string())
}

/// Removes `key` from `doc`; whether it was there.
fn remove_key(doc: &mut DocumentMut, key: &str) -> Result<bool> {
    let path: Vec<&str> = key.split('.').collect();
    Ok(match path.as_slice() {
        ["users", name, last] => doc
            .get_mut("users")
            .and_then(|users| users.as_table_like_mut()?.get_mut(name))
            .and_then(|user| user.as_table_like_mut())
            .is_some_and(|user| user.remove(last).is_some()),
        _ => remove_path(doc.as_table_mut(), &path)?,
    })
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
    // A scheme's table holds colours, not one colour (M5.12b1).
    if path.len() == 3
        && path[..2] == ["appearance", "colours"]
        && matches!(path[2], "light" | "dark")
    {
        bail!(
            "{}",
            trf(
                "{key} is a table of colours; set one colour in it, such as {key}.panel",
                &[("key", key)]
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
pub(crate) fn nearest_value<'a>(value: &str, allowed: &[&'a str]) -> Option<&'a str> {
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
    // A preset starting from a built-in preset this release lacks is read as
    // Classic, and the file says so (M5.17).
    for (name, own) in &file.presets {
        if !crate::presets::is_built_in(&own.preset) && !file.presets.contains_key(&own.preset) {
            problems.push(Problem {
                key: format!("presets.{name}.preset"),
                message: crate::presets::lacks_note(&own.preset),
            });
        }
    }
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
        } else if at.is_empty() && name == "presets" {
            // The person's own layout presets, each checked as its own table (M5.17).
            match value {
                Value::Table(inner) => {
                    let mut own = Table::new();
                    for (preset, value) in inner {
                        match value {
                            Value::Table(table) => {
                                let table = clean_own(preset, table, problems, later);
                                own.insert(preset.clone(), Value::Table(table));
                            }
                            _ => problems.push(Problem {
                                key: format!("presets.{preset}"),
                                message: tr("expected a table").into(),
                            }),
                        }
                    }
                    kept.insert(name.clone(), Value::Table(own));
                }
                _ => problems.push(Problem {
                    key: shown,
                    message: tr("expected a table").into(),
                }),
            }
        } else if at == ["users"] && !is_user_name(name) {
            problems.push(Problem {
                key: shown,
                message: tr("not a user name; use up to 32 lowercase letters, digits, - and _, starting with a letter or _").into(),
            });
        } else if at == ["appearance", "colours"] && matches!(name.as_str(), "light" | "dark") {
            // A scheme's own table of colours, read as its own keys (M5.12b1).
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

/// Keeps the keys of one of the person's own presets (`[presets.NAME]`, M5.17):
/// `preset`, the built-in it starts from, and the `[layout]` keys, which
/// [`clean`] checks as the layout's own, so a preset's value gets the same
/// check a layout's does. `name` names the table in the problems' keys.
fn clean_own(
    name: &str,
    table: &Table,
    problems: &mut Vec<Problem>,
    later: &mut Vec<String>,
) -> Table {
    let mut kept = Table::new();
    let mut layout = Table::new();
    for (key, value) in table {
        if key != "preset" {
            layout.insert(key.clone(), value.clone());
        } else if value.as_str().is_some_and(crate::themes::is_name) {
            kept.insert(key.clone(), value.clone());
        } else {
            problems.push(Problem {
                key: format!("presets.{name}.preset"),
                message: tr("a built-in preset's name in quotes, such as \"hive\"").into(),
            });
        }
    }
    let mut wrapped = Table::new();
    wrapped.insert("layout".into(), Value::Table(layout));
    let mut found = Vec::new();
    if let Some(Value::Table(cleaned)) = clean(&wrapped, &[], &mut found, later).remove("layout") {
        for (key, value) in cleaned {
            kept.insert(key, value);
        }
    }
    problems.extend(found.into_iter().map(|problem| {
        Problem {
            key: problem
                .key
                .replacen("layout.", &format!("presets.{name}."), 1),
            message: problem.message,
        }
    }));
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
        (Kind::LockParts, Value::Array(_)) => {
            let parts: Vec<crate::lock_screen::Placed> =
                value.clone().try_into().map_err(|e: toml::de::Error| {
                    trf(
                        "expected parts as the lock screen writes them: {why}",
                        &[("why", e.message())],
                    )
                })?;
            crate::lock_screen::check(&parts)?;
            Ok(value.clone())
        }
        (Kind::LockParts, _) => fail(tr(
            "a list of parts, such as [{ part = \"person\", at = \"centre\" }]",
        )),
        (Kind::LockLook, Value::String(s)) if crate::lock_screen::is_look_name(s) => {
            Ok(value.clone())
        }
        (Kind::LockLook, Value::String(s)) => Err(trf(
            "{value} is not a look's name; use letters, digits and hyphens, such as \"minimal\"",
            &[("value", &format!("{s:?}"))],
        )),
        (Kind::LockLook, _) => fail(tr("a look's name in quotes, such as \"minimal\"")),
        (Kind::LockBackground, Value::String(s))
            if crate::lock_screen::parse_background(s).is_some() =>
        {
            Ok(value.clone())
        }
        (Kind::LockBackground, Value::String(s)) => Err(trf(
            "{value} is not a background; give backdrop, blurred, sharp, a colour such as \"#1f2b48\", or a picture's full path such as \"/home/you/lock.png\" (to show the look's, run edel settings reset on this key)",
            &[("value", &format!("{s:?}"))],
        )),
        (Kind::LockBackground, _) => fail(tr(
            "a background in quotes, such as \"blurred\" or \"#1f2b48\"",
        )),
        (Kind::LockLooks, Value::Table(_)) => {
            let looks: BTreeMap<String, OwnLook> =
                value.clone().try_into().map_err(|e: toml::de::Error| {
                    trf(
                        "expected looks as the lock screen writes them: {why}",
                        &[("why", e.message())],
                    )
                })?;
            crate::lock_screen::check_looks(&looks)?;
            Ok(value.clone())
        }
        (Kind::LockLooks, _) => fail(tr(
            "looks as tables, such as { home = { parts = [{ part = \"person\", at = \"centre\" }] } }",
        )),
        (Kind::Theme, Value::String(s)) if crate::themes::is_name(s) => Ok(value.clone()),
        (Kind::Theme, Value::String(s)) => Err(trf(
            "{value} is not a theme's name; use letters, digits and hyphens, such as \"aurora\"",
            &[("value", &format!("{s:?}"))],
        )),
        (Kind::Theme, _) => fail(tr("a theme's name in quotes, such as \"aurora\"")),
        (Kind::OwnTheme, Value::Table(table)) => {
            crate::themes::check_table(table)?;
            // The values are checked as the appearance keys are, so a
            // theme's radius is the same whole number a file's would be.
            let mut values = Table::new();
            for (key, value) in table {
                if key != "title" && key != "pictures" {
                    values.insert(key.clone(), value.clone());
                }
            }
            let mut wrapped = Table::new();
            wrapped.insert("appearance".into(), Value::Table(values));
            let (mut problems, mut later) = (Vec::new(), Vec::new());
            clean(&wrapped, &[], &mut problems, &mut later);
            match problems.first() {
                Some(problem) => Err(problem.to_string()),
                None => Ok(value.clone()),
            }
        }
        (Kind::OwnTheme, _) => fail(tr(
            "a theme as a table, such as { pictures = [\"aurora\"], accent = \"wallpaper\" }",
        )),
        (Kind::Preset, Value::String(s)) if crate::themes::is_name(s) => Ok(value.clone()),
        (Kind::Preset, Value::String(s)) => Err(trf(
            "{value} is not a preset's name; use letters, digits and hyphens, such as \"my-hive\"",
            &[("value", &format!("{s:?}"))],
        )),
        (Kind::Preset, _) => fail(tr("a preset's name in quotes, such as \"hive\"")),
        (Kind::OwnPreset, Value::Table(table)) => {
            let mut problems = Vec::new();
            clean_own("", table, &mut problems, &mut Vec::new());
            match problems.first() {
                Some(problem) => Err(problem.message.clone()),
                None => Ok(value.clone()),
            }
        }
        (Kind::OwnPreset, _) => fail(tr(
            "a preset as a table, such as { preset = \"hive\", window_buttons = \"left\" }",
        )),
        (Kind::Resolution, Value::String(r)) if is_resolution(r) => Ok(value.clone()),
        (Kind::Resolution, _) => fail(tr("a resolution such as \"1920x1080\"")),
        (Kind::Keyboard, Value::String(s)) => {
            let rules = std::fs::read_to_string(crate::keyboard::RULES).ok();
            crate::keyboard::normalize(s, rules.as_deref()).map(Value::String)
        }
        (Kind::Keyboard, _) => fail(tr(
            "keyboard layouts in quotes, such as \"us\" or \"us,ru\"",
        )),
        (Kind::TimeZone, Value::String(s))
            if crate::timezones::known_in(Path::new(crate::places::ZONEINFO_DIR), s) =>
        {
            Ok(value.clone())
        }
        (Kind::TimeZone, Value::String(s)) => Err(trf(
            "{value} is not a time zone this machine knows; time zones are names such as \"Europe/Berlin\" or \"Asia/Tashkent\"",
            &[("value", &format!("{s:?}"))],
        )),
        (Kind::TimeZone, _) => fail(tr("a time zone in quotes, such as \"Europe/Berlin\"")),
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
        (Kind::AppIds, Value::Array(items)) => {
            for item in items {
                match item.as_str() {
                    Some(id) if crate::apps::is_flatpak_id(id) => {}
                    _ => {
                        return Err(trf(
                            "{item} is not a Flatpak app id; an id is the app's reverse-DNS name, three or more parts joined by dots, such as \"io.mpv.Mpv\"",
                            &[("item", &item.to_string())],
                        ));
                    }
                }
            }
            Ok(value.clone())
        }
        (Kind::AppIds, _) => fail(tr("a list of Flatpak app ids, such as [\"io.mpv.Mpv\"]")),
        (Kind::Addons, Value::Array(items)) => {
            for item in items {
                match item.as_str() {
                    Some(name) if crate::features::is_name(name) => {}
                    _ => {
                        return Err(trf(
                            "{item} is not an add-on name; a name is lowercase letters, digits and hyphens, such as \"fonts-cjk\" (edel addon list shows them)",
                            &[("item", &item.to_string())],
                        ));
                    }
                }
            }
            Ok(value.clone())
        }
        (Kind::Addons, _) => fail(tr("a list of add-on names, such as [\"fonts-cjk\"]")),
        (Kind::Channel, Value::String(s)) if is_channel(s) => Ok(value.clone()),
        (Kind::Channel, Value::String(s)) => Err(trf(
            "{value} is not a channel name; use lowercase letters, digits and hyphens, such as \"stable\" or \"preview\" (to follow the channel this image was built for, run edel settings reset on this key)",
            &[("value", &format!("{s:?}"))],
        )),
        (Kind::Channel, _) => fail(tr("a channel name in quotes, such as \"stable\"")),
        (Kind::CursorTheme, Value::String(s)) => match cursor_theme_problem(s) {
            None => Ok(value.clone()),
            Some(problem) => Err(problem),
        },
        (Kind::CursorTheme, _) => fail(tr("a cursor theme's name in quotes, such as \"default\"")),
        (Kind::Wallpaper, Value::String(s)) if crate::wallpapers::is_form(s) => Ok(value.clone()),
        (Kind::Wallpaper, Value::String(s)) => Err(wallpaper_problem(s)),
        (Kind::Wallpaper, _) => fail(tr(
            "a wallpaper's name, a picture's path or a colour in quotes, such as \"hills\"",
        )),
        // Each entry is checked as `appearance.wallpaper` is, with its
        // number; `""` is Appearance's and needs no check.
        (Kind::WallpaperList, Value::Array(items)) => {
            for (at, item) in items.iter().enumerate() {
                match item.as_str() {
                    Some("") => {}
                    Some(s) if crate::wallpapers::is_form(s) => {}
                    Some(s) => {
                        return Err(trf(
                            "entry {number}: {problem}",
                            &[
                                ("number", &(at + 1).to_string()),
                                ("problem", &wallpaper_problem(s)),
                            ],
                        ));
                    }
                    None => {
                        return fail(tr(
                            "a list of wallpapers in quotes, one per workspace, such as [\"\", \"#336699\"]",
                        ));
                    }
                }
            }
            Ok(value.clone())
        }
        (Kind::WallpaperList, _) => fail(tr(
            "a list of wallpapers in quotes, one per workspace, such as [\"\", \"#336699\"]",
        )),
        (Kind::Pictures, Value::Array(items)) => {
            let mut pictures = Vec::new();
            for item in items {
                match item.as_str() {
                    Some(picture) => pictures.push(picture.to_string()),
                    None => return fail(tr("a list of pictures in quotes, such as [\"aurora\"]")),
                }
            }
            crate::themes::check_pictures(&pictures)?;
            Ok(value.clone())
        }
        (Kind::Pictures, _) => fail(tr(
            "a list of pictures in quotes, such as [\"aurora\", \"/home/you/sea.jpg\"]",
        )),
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
        (Kind::AppChoices(allowed), Value::Table(apps)) => {
            for (app, choice) in apps {
                if !crate::apps::is_flatpak_id(app) {
                    return Err(trf(
                        "{app} is not a Flatpak app id; use one such as \"org.mozilla.firefox\"",
                        &[("app", &format!("{app:?}"))],
                    ));
                }
                match choice {
                    Value::String(s) if allowed.contains(&s.as_str()) => {}
                    _ => {
                        return Err(trf(
                            "{app} has {value}; choose {choices}",
                            &[
                                ("app", &format!("{app:?}")),
                                ("value", &choice.to_string()),
                                ("choices", &or_list(allowed)),
                            ],
                        ));
                    }
                }
            }
            Ok(value.clone())
        }
        (Kind::AppChoices(allowed), _) => fail(&trf(
            "apps and their choice, such as { \"org.mozilla.firefox\" = \"{first}\" }",
            &[("first", allowed[0])],
        )),
        (Kind::Accent, Value::String(s)) if crate::tokens::is_accent(s) => Ok(value.clone()),
        (Kind::Accent, Value::String(s)) => Err(trf(
            "{value} is not an accent; choose {choices} or a colour such as \"#d0363f\"",
            &[("value", &format!("{s:?}")), ("choices", &accent_choices())],
        )),
        (Kind::Accent, _) => fail(tr(
            "an accent's name or a colour in quotes, such as \"blue\"",
        )),
        (Kind::Colour, Value::String(s)) if crate::tokens::Colour::parse(s).is_some() => {
            Ok(value.clone())
        }
        (Kind::Colour, Value::String(s)) => Err(trf(
            "{value} is not a colour; use #rrggbb or #rrggbbaa, such as \"#1e1e2e\"",
            &[("value", &format!("{s:?}"))],
        )),
        (Kind::Colour, _) => fail(tr("a colour in quotes, such as \"#1e1e2e\"")),
    }
}

/// Why `value` is no wallpaper, in the words `appearance.wallpaper` gives
/// (M5.12b5): the one message both that key and each entry of
/// `workspaces.wallpapers` show.
fn wallpaper_problem(value: &str) -> String {
    trf(
        "{value} is not a wallpaper; give a wallpaper's name such as \"hills\", a picture's path, a colour such as \"#1f2b48\", or \"none\" (to show the release's, run edel settings reset on this key)",
        &[("value", &format!("{value:?}"))],
    )
}

/// The accents a person may choose, as an accent's message names them: the
/// release's names in the file's order, then `wallpaper` (M5.12b1).
fn accent_choices() -> String {
    let mut names: Vec<String> = crate::tokens::accents()
        .into_iter()
        .map(|(name, _, _)| name)
        .collect();
    names.push(crate::tokens::WALLPAPER_ACCENT.to_string());
    names.join(", ")
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
pub(crate) fn or_list(items: &[&str]) -> String {
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

    #[test]
    fn default_app_roles_leave_out_the_unset_and_the_empty_ones() {
        let file = read(
            "format = 1\n[default_apps]\nbrowser = \"org.mozilla.firefox\"\nfiles = \"\"\nmail = \"org.example.Mail\"\n",
        )
        .unwrap()
        .file;
        assert_eq!(
            default_app_roles(&file.default_apps),
            [
                ("browser", "org.mozilla.firefox"),
                ("mail", "org.example.Mail")
            ]
        );
    }

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
            Kind::LockParts => value_from_arg(r#"[{ part = "person", at = "centre" }]"#),
            Kind::Resolution => Value::String("1920x1080".into()),
            Kind::Keyboard => Value::String("us".into()),
            Kind::TimeZone => Value::String("Europe/Berlin".into()),
            Kind::Hosts => Value::Array(vec![Value::String("pool.ntp.org".into())]),
            Kind::AppIds => value_from_arg(r#"["io.mpv.Mpv"]"#),
            Kind::Addons => value_from_arg(r#"["fonts-cjk"]"#),
            Kind::Channel => Value::String("stable".into()),
            Kind::CursorTheme => Value::String("default".into()),
            Kind::Wallpaper => Value::String("hills".into()),
            Kind::WallpaperList => value_from_arg(r##"["", "#336699"]"##),
            Kind::Pictures => value_from_arg(r#"["hills"]"#),
            Kind::AppWorkspaces => value_from_arg(r#"{ "org.mozilla.firefox" = 2 }"#),
            Kind::AppChoices(allowed) => value_from_arg(&format!(
                r#"{{ "org.mozilla.firefox" = "{}" }}"#,
                allowed[0]
            )),
            Kind::LockLook => Value::String("minimal".into()),
            Kind::LockBackground => Value::String("blurred".into()),
            Kind::Accent => Value::String("blue".into()),
            Kind::Colour => Value::String("#1e1e2e".into()),
            Kind::Theme => Value::String("edel".into()),
            Kind::OwnTheme => value_from_arg(r#"{ pictures = ["hills"], accent = "blue" }"#),
            Kind::Preset => Value::String("hive".into()),
            Kind::OwnPreset => value_from_arg(r#"{ preset = "hive", window_buttons = "left" }"#),
            Kind::LockLooks => {
                value_from_arg(r#"{ home = { parts = [{ part = "person", at = "centre" }] } }"#)
            }
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
cursor_size = 11

[power]
lock_after_minutes = 11
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
        assert!(lines.contains(&"power.lock_after_minutes: not supported yet".to_string()));
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
        assert_eq!(read.file.appearance.cursor_size, None);
        let keys: Vec<&str> = read.problems.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "appearance.acent",
                "appearance.cursor_size",
                "appearance.mode"
            ]
        );
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

    /// The lock screen's parts (M5.10c5) are checked like the panels: a good
    /// list is set and read back as tables, and a bad one is refused by set
    /// and left out, with its message, by a lenient read.
    #[test]
    fn lock_parts_are_checked_and_read_as_tables_or_inline() {
        let set_one = set(
            "format = 1\n",
            "lock_screen.parts",
            r#"[{ part = "person", at = "centre" }]"#,
        )
        .unwrap();
        assert_eq!(
            set_one,
            "format = 1\n\n[lock_screen]\nparts = [{ at = \"centre\", part = \"person\" }]\n"
        );
        let tables = "format = 1\n[[lock_screen.parts]]\npart = \"clock\"\nat = \"top\"\ny = 0.14\n\n[[lock_screen.parts]]\npart = \"person\"\nat = \"centre\"\ny = 0.21\n\n[[lock_screen.parts]]\npart = \"message\"\nat = \"bottom_right\"\ntext = \"Hello\"\n";
        assert!(check(tables).unwrap().is_empty());
        let parts = read(tables).unwrap().file.lock_screen.parts.unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].part, crate::lock_screen::Part::Clock);
        assert_eq!(parts[2].text.as_deref(), Some("Hello"));
        // No person, two persons, an offset past the screen, a message with
        // no words, a picture with a relative path: each refused by set with
        // its message, and left out by a lenient read.
        for (bad, why) in [
            (
                r#"[{ part = "clock", at = "top" }]"#,
                "the lock screen needs the person",
            ),
            (
                r#"[{ part = "person", at = "centre" }, { part = "person", at = "bottom" }]"#,
                "the person stands once; take out the second",
            ),
            (
                r#"[{ part = "person", at = "centre", x = 2 }]"#,
                "the person's offset x is 2",
            ),
            (
                r#"[{ part = "message", at = "bottom" }, { part = "person", at = "centre" }]"#,
                "the message needs text",
            ),
            (
                r#"[{ part = "person", at = "centre" }, { part = "picture", at = "top", path = "lock.png" }]"#,
                "\"lock.png\" is not a picture path",
            ),
            (r#""clock""#, "expected a list of parts"),
        ] {
            let error = set("format = 1\n", "lock_screen.parts", bad).unwrap_err();
            assert!(error.to_string().contains(why), "{bad}: {error}");
            let file = format!("format = 1\n[lock_screen]\nparts = {bad}\n");
            let read = read(&file).unwrap();
            assert_eq!(read.file.lock_screen.parts, None, "{bad}");
            assert_eq!(read.problems[0].key, "lock_screen.parts");
        }
    }

    /// The lock screen's looks, background and count (M5.10c5c) are set and
    /// read back, and a bad value is refused by set with its message and
    /// left out by a lenient read.
    #[test]
    fn lock_looks_are_set_read_back_and_checked() {
        let set_then_read = |text: &str, key: &str, value: &str| {
            let text = set(text, key, value).unwrap();
            assert!(check(&text).unwrap().is_empty(), "{text}");
            read(&text).unwrap().file.lock_screen
        };
        let look = set_then_read("format = 1\n", "lock_screen.look", "\"minimal\"");
        assert_eq!(look.look.as_deref(), Some("minimal"));
        let background = set_then_read("format = 1\n", "lock_screen.background", "\"#1f2b48\"");
        assert_eq!(background.background.as_deref(), Some("#1f2b48"));
        let picture = set_then_read(
            "format = 1\n",
            "lock_screen.background",
            "\"/home/you/lock.jpg\"",
        );
        assert_eq!(picture.background.as_deref(), Some("/home/you/lock.jpg"));
        let dim = set_then_read("format = 1\n", "lock_screen.dim", "40");
        assert_eq!(dim.dim, Some(40));
        let count = set_then_read("format = 1\n", "lock_screen.notifications", "none");
        assert_eq!(count.notifications.as_deref(), Some("none"));
        let own = set_then_read(
            "format = 1\n",
            "lock_screen.looks",
            r#"{ home = { parts = [{ part = "person", at = "centre" }] } }"#,
        );
        assert_eq!(own.looks.unwrap().len(), 1);
        // A look of the file's own is set, once the file has it.
        let file = "format = 1\n[lock_screen.looks.home]\nparts = [{ part = \"person\", at = \"centre\" }]\n";
        let chosen = set(file, "lock_screen.look", "\"home\"").unwrap();
        assert_eq!(
            read(&chosen).unwrap().file.lock_screen.look.as_deref(),
            Some("home")
        );
        // Refused by set, with its message.
        for (key, bad, why) in [
            ("lock_screen.look", "\"my look\"", "is not a look's name"),
            (
                "lock_screen.look",
                "\"gallery\"",
                "\"gallery\" is not a look; choose classic, magazine or minimal",
            ),
            ("lock_screen.look", "\"home\"", "\"home\" is not a look"),
            (
                "lock_screen.background",
                "lock.png",
                "lock.png\" is not a background",
            ),
            (
                "lock_screen.background",
                "\"navy\"",
                "\"navy\" is not a background",
            ),
            (
                "lock_screen.notifications",
                "some",
                "unknown value \"some\"",
            ),
            (
                "lock_screen.looks",
                r#"{ home = { parts = [{ part = "clock", at = "top" }] } }"#,
                "the lock screen needs the person",
            ),
            (
                "lock_screen.looks",
                r#"{ minimal = { parts = [{ part = "person", at = "centre" }] } }"#,
                "\"minimal\" is a built-in look",
            ),
            ("lock_screen.dim", "81", "a whole number from 0 to 80"),
            ("lock_screen.dim", "\"40\"", "a whole number from 0 to 80"),
            ("lock_screen.looks", "\"home\"", "looks as tables"),
        ] {
            let error = set("format = 1\n", key, bad).unwrap_err();
            assert!(error.to_string().contains(why), "{key} {bad}: {error}");
        }
        // A look the file does not have is reported by check, and a lenient
        // read keeps no bad background.
        let lines = check("format = 1\n[lock_screen]\nlook = \"gallery\"\n").unwrap();
        assert_eq!(
            lines,
            ["lock_screen.look: \"gallery\" is not a look; choose classic, magazine or minimal"]
        );
        let read = read("format = 1\n[lock_screen]\nbackground = \"lock.png\"\n").unwrap();
        assert_eq!(read.file.lock_screen.background, None);
        assert_eq!(read.problems[0].key, "lock_screen.background");
    }

    #[test]
    fn the_title_bars_key_takes_never_and_refuses_the_rest() {
        // M5.18b: never is one of the key's values, as always and
        // floating-only are, and any other word is refused with them.
        let never = set("format = 1\n", "layout.title_bars", "never").unwrap();
        assert_eq!(never, "format = 1\n\n[layout]\ntitle_bars = \"never\"\n");
        assert!(check(&never).unwrap().is_empty());
        let refused = set("format = 1\n", "layout.title_bars", "sometimes").unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "layout.title_bars: unknown value \"sometimes\"; use always, floating-only or never"
        );
        assert_eq!(
            choices("layout.title_bars"),
            crate::presets::TitleBars::ALL.map(|t| t.name())
        );
    }

    #[test]
    fn a_preset_this_release_lacks_is_refused_or_reported() {
        let file = "format = 1\n[layout]\npreset = \"tablet\"\n";
        let lines = check(file).unwrap();
        assert_eq!(
            lines,
            [
                "layout.preset: \"tablet\" is not a preset; choose classic, mac-like, windows-like, hive or zen"
            ]
        );
        let error = set("format = 1\n", "layout.preset", "tablet").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("choose classic, mac-like, windows-like, hive or zen"),
            "{error}"
        );
        // A name is read as written; the check names the preset it is not (M5.17).
        let read = read(file).unwrap();
        assert_eq!(read.file.layout.preset.as_deref(), Some("tablet"));
        assert!(read.problems.is_empty(), "{:?}", read.problems);
        let set_hive = set("format = 1\n", "layout.preset", "hive").unwrap();
        assert_eq!(set_hive, "format = 1\n\n[layout]\npreset = \"hive\"\n");
        assert!(check(&set_hive).unwrap().is_empty());
    }

    static RETIRED: crate::key_help::Help = crate::key_help::Help {
        key: "old.key",
        help: "an old key",
        default: "1",
        from: crate::key_help::DecidedBy::Release,
        belongs: crate::key_help::Belongs::Device,
        retired: Some("0.9"),
    };

    static CURRENT: crate::key_help::Help = crate::key_help::Help {
        key: "new.key",
        help: "a key",
        default: "1",
        from: crate::key_help::DecidedBy::Release,
        belongs: crate::key_help::Belongs::Device,
        retired: None,
    };

    #[test]
    fn a_retired_key_is_refused_by_set_with_the_release_that_stopped_it() {
        assert_eq!(
            retired_message("old.key", Some(&RETIRED)).as_deref(),
            Some("old.key: no longer used since 0.9; edel settings reset old.key takes it out")
        );
        assert_eq!(retired_message("new.key", Some(&CURRENT)), None);
        assert_eq!(retired_message("network.hostname", None), None);
    }

    #[test]
    fn check_notes_a_retired_key_the_file_holds() {
        let find = |key: &str| (key == "old.key").then_some(&RETIRED);
        let notes = retired_notes("format = 1\n[old]\nkey = 2\n", find);
        assert_eq!(notes, ["old.key: no longer used since 0.9"]);
        assert!(retired_notes("format = 1\n[new]\nkey = 2\n", find).is_empty());
        assert!(retired_notes("not toml [", find).is_empty());
    }

    #[test]
    fn plus_and_minus_add_to_and_remove_from_a_list() {
        let added = edit_list("format = 1\n", "startup.apps", "org.mozilla.firefox", true).unwrap();
        assert_eq!(
            added,
            "format = 1\n\n[startup]\napps = [\"org.mozilla.firefox\"]\n"
        );
        // Adding the same item again changes nothing.
        assert_eq!(
            edit_list(&added, "startup.apps", "org.mozilla.firefox", true).unwrap(),
            added
        );
        let two = edit_list(&added, "startup.apps", "org.example.Mail", true).unwrap();
        assert!(
            two.ends_with("apps = [\"org.mozilla.firefox\", \"org.example.Mail\"]\n"),
            "{two}"
        );
        let one = edit_list(&two, "startup.apps", "org.mozilla.firefox", false).unwrap();
        assert!(one.ends_with("apps = [\"org.example.Mail\"]\n"), "{one}");
        // Removing the last item removes the key, and the table it left empty.
        let gone = edit_list(&one, "startup.apps", "org.example.Mail", false).unwrap();
        assert_eq!(gone, "format = 1\n");
    }

    #[test]
    fn plus_and_minus_refuse_a_key_that_is_not_a_list() {
        let error = edit_list(EDITED, "network.hostname", "lab", true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("lists only"), "{error}");
        assert!(error.contains("network.hostname=VALUE"), "{error}");
    }

    #[test]
    fn minus_refuses_an_item_the_list_does_not_hold() {
        let error = edit_list("format = 1\n", "startup.apps", "org.example.Mail", false)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "startup.apps: \"org.example.Mail\" is not in the list"
        );
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
    fn a_moved_chord_and_its_cleared_holder_are_one_write() {
        let dir = std::env::temp_dir().join(format!("edel-write-all-{}", std::process::id()));
        let path = dir.join(crate::places::SETTINGS);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "format = 1\n").unwrap();
        // The holder is cleared first, so the chord it held is free to move to.
        write_all(
            &path,
            &[
                ("shortcuts.show_workspaces", Some("")),
                ("shortcuts.close_window", Some("Super+W")),
            ],
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "format = 1\n\n[shortcuts]\nshow_workspaces = \"\"\nclose_window = \"Super+W\"\n"
        );
        // Taking one out again leaves the other line as it is.
        write_all(&path, &[("shortcuts.close_window", None)]).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("show_workspaces = \"\"")
        );
        // A clash that is not cleared first is refused, and nothing is written.
        assert!(write_all(&path, &[("shortcuts.open_terminal", Some("Super+Q"))]).is_err());
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("open_terminal")
        );
        std::fs::remove_dir_all(&dir).ok();
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
            // Themes have no page of their own: they are the Appearance
            // page's, as the roadmap's M5.12b6 says (M5.12b6a).
            // Presets are the Layout page's, as themes are the Appearance page's.
            let section = match key.path.split('.').next().unwrap_or_default() {
                "themes" => "appearance",
                "presets" => "layout",
                section => section,
            };
            assert!(page(section).is_some(), "{section} is on no page");
        }
        assert!(error("users.ci.admin", "yes").contains("expected true or false"));
        assert!(error("network.hostname", "not valid").contains("expected a hostname"));
        assert_eq!(
            error("power.lock_after_minutes", "11"),
            "power.lock_after_minutes: not supported yet; this release does not act on it, so it cannot be set"
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
    fn unset_all_passes_over_keys_at_their_default() {
        let unset_once = unset_all(EDITED, &["future.key", "network.hostname"]).unwrap();
        assert_eq!(
            unset_all(EDITED, &["layout.preset", "network.hostname", "future.key"]).unwrap(),
            unset_once
        );
        assert_eq!(
            unset_all(&unset_once, &["network.hostname", "future.key"])
                .unwrap_err()
                .to_string(),
            "none of network.hostname, future.key is in the file, so they already have their defaults; there is nothing to reset"
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
    fn apps_installed_takes_flatpak_ids_and_names_the_bad_one() {
        // A good id is kept; a bad one is refused with its own text, and
        // the reader leaves the whole list out, as it does for hosts (M6.1).
        let ok = set(
            "format = 1
",
            "apps.installed",
            r#"["io.mpv.Mpv"]"#,
        )
        .unwrap();
        assert_eq!(
            read(&ok).unwrap().file.apps.installed,
            Some(vec!["io.mpv.Mpv".into()])
        );
        let bad = |value: &str| {
            set(
                "format = 1
",
                "apps.installed",
                value,
            )
            .unwrap_err()
        };
        let message = bad(r#"["io.mpv.Mpv", "mpv"]"#).to_string();
        assert!(
            message.contains("\"mpv\" is not a Flatpak app id"),
            "{message}"
        );
        assert!(message.contains("io.mpv.Mpv"), "{message}");
        assert!(bad("[1]").to_string().contains("not a Flatpak app id"));
        assert!(
            bad("io.mpv.Mpv")
                .to_string()
                .contains("a list of Flatpak app ids")
        );
        let file = "format = 1\n[apps]\ninstalled = [\"io.mpv.Mpv\", \"mpv\"]\n";
        let read = read(file).unwrap();
        assert_eq!(read.file.apps.installed, None);
        assert!(
            read.problems
                .iter()
                .any(|p| p.key == "apps.installed" && p.to_string().contains("\"mpv\"")),
            "{:?}",
            read.problems
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
    fn every_page_names_a_group_that_exists() {
        for page in PAGES {
            assert!(
                GROUPS.iter().any(|g| g.name == page.group),
                "{} names the group {:?}, which GROUPS does not hold",
                page.section,
                page.group
            );
        }
    }

    #[test]
    fn a_groups_pages_stand_together_in_their_groups_order() {
        // The groups appear in GROUPS' order, each as one run of pages.
        let runs: Vec<&str> = PAGES
            .iter()
            .enumerate()
            .filter_map(|(at, page)| {
                (at == 0 || PAGES[at - 1].group != page.group).then_some(page.group)
            })
            .collect();
        let order: Vec<&str> = GROUPS.iter().map(|g| g.name).collect();
        assert_eq!(
            runs, order,
            "each group's pages in one run, in GROUPS' order"
        );
    }

    #[test]
    fn about_lines_fit_one_line_of_the_sidebar() {
        for page in PAGES {
            assert!(
                page.about.chars().count() <= 60,
                "the about line of {} is longer than 60 characters: {:?}",
                page.section,
                page.about
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
    fn the_location_keys_read_back_and_check() {
        let text =
            "format = 1\n[location]\nenabled = false\nplace = \"Asia/Tashkent\"\nonline = true\n";
        let parsed = read(text).unwrap();
        assert_eq!(parsed.file.location.enabled, Some(false));
        assert_eq!(parsed.file.location.place.as_deref(), Some("Asia/Tashkent"));
        assert_eq!(parsed.file.location.online, Some(true));
        assert!(!location_enabled(&parsed.file));
        assert!(location_enabled(&SettingsFile::default()));
        assert!(check(text).unwrap().is_empty());
        // A place is a time zone's name, and `check` names the key that is wrong.
        let refused = check("format = 1\n[location]\nplace = \"not a zone\"\n").unwrap();
        assert!(
            refused
                .iter()
                .any(|problem| problem.contains("location.place"))
        );
        // The switch is set and reset like any flag.
        let written = set("format = 1\n", LOCATION_ENABLED, "false").unwrap();
        assert!(written.contains("[location]\nenabled = false\n"));
        assert!(
            !unset(&written, LOCATION_ENABLED)
                .unwrap()
                .contains("location")
        );
    }

    #[test]
    fn an_apps_location_choice_reads_into_the_choices() {
        let text = "format = 1\n[apps.location]\n\"org.example.App\" = \"ask\"\n";
        let parsed = read(text).unwrap();
        assert_eq!(
            parsed.file.apps.choices()["apps.location"]["org.example.App"],
            "ask"
        );
        assert!(check(text).unwrap().is_empty());
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
    fn each_workspace_wallpaper_is_checked_as_appearance_wallpaper_is() {
        let ok = set(
            "format = 1\n",
            "workspaces.wallpapers",
            r##"["", "#336699", "hills"]"##,
        )
        .unwrap();
        assert!(check(&ok).unwrap().is_empty());
        let refused = set("format = 1\n", "workspaces.wallpapers", r##"["", "Sea"]"##).unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "workspaces.wallpapers: entry 2: \"Sea\" is not a wallpaper; give a wallpaper's name such as \"hills\", a picture's path, a colour such as \"#1f2b48\", or \"none\" (to show the release's, run edel settings reset on this key)"
        );
        let refused = set("format = 1\n", "workspaces.wallpapers", r##"["#12"]"##).unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "workspaces.wallpapers: entry 1: \"#12\" is not a wallpaper; give a wallpaper's name such as \"hills\", a picture's path, a colour such as \"#1f2b48\", or \"none\" (to show the release's, run edel settings reset on this key)"
        );
        let refused = set("format = 1\n", "workspaces.wallpapers", "[1]").unwrap_err();
        assert_eq!(
            format!("{refused:#}"),
            "workspaces.wallpapers: expected a list of wallpapers in quotes, one per workspace, such as [\"\", \"#336699\"], not [1]"
        );
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

    #[test]
    fn an_accent_is_a_named_accent_wallpaper_or_an_opaque_colour() {
        let accent = |value: &str| set("format = 1\n", "appearance.accent", value);
        assert!(accent("red").unwrap().contains("accent = \"red\""));
        assert!(accent("wallpaper").is_ok());
        assert!(accent("#ff0000").is_ok());
        assert_eq!(
            accent("crimson").unwrap_err().to_string(),
            "appearance.accent: \"crimson\" is not an accent; choose blue, purple, pink, red, orange, yellow, green, teal, graphite, wallpaper or a colour such as \"#d0363f\""
        );
        assert!(accent("#ff000080").is_err());
        let read = read("format = 1\n[appearance]\naccent = \"crimson\"\n").unwrap();
        assert_eq!(read.problems.len(), 1, "{:?}", read.problems);
    }

    #[test]
    fn a_colour_is_read_set_and_reset_per_token_and_per_scheme() {
        let text = "format = 1\n";
        let dark = set(text, "appearance.colours.dark.panel", "#11111b").unwrap();
        let file = read(&dark).unwrap().file;
        let colours = file.appearance.colours.unwrap();
        assert!(colours.both.is_empty());
        assert_eq!(colours.dark.unwrap()["panel"], "#11111b");
        // A colour for both schemes must hold in each: the background is in no pair.
        let shared = set(text, "appearance.colours.background", "#202020").unwrap();
        let colours = read(&shared).unwrap().file.appearance.colours.unwrap();
        assert_eq!(colours.both["background"], "#202020");
        assert_eq!(colours.light, None);
        let reset = unset(&shared, "appearance.colours.background").unwrap();
        assert_eq!(read(&reset).unwrap().file.appearance.colours, None);
        let reset = unset(&dark, "appearance.colours.dark.panel").unwrap();
        assert_eq!(read(&reset).unwrap().file.appearance.colours, None);
        // A scheme's table is not one colour, and a colour is a colour.
        let table = set(text, "appearance.colours.light", "#ffffff").unwrap_err();
        assert!(
            table.to_string().contains(
                "appearance.colours.light is a table of colours; set one colour in it, such as appearance.colours.light.panel"
            ),
            "{table}"
        );
        let not = set(text, "appearance.colours.background", "fff").unwrap_err();
        assert!(not.to_string().contains("is not a colour"), "{not}");
    }

    #[test]
    fn a_colour_name_is_a_token_and_the_nearest_one_is_named() {
        assert_eq!(
            check("format = 1\n[appearance.colours]\npanl = \"#ffffff\"\n").unwrap(),
            ["appearance.colours.panl: \"panl\" is not a colour; did you mean panel?"]
        );
        assert_eq!(
            check("format = 1\n[appearance.colours.dark]\npanl = \"#ffffff\"\n").unwrap(),
            ["appearance.colours.dark.panl: \"panl\" is not a colour; did you mean panel?"]
        );
        let far = check("format = 1\n[appearance.colours]\nzzzzzzzz = \"#ffffff\"\n").unwrap();
        assert!(
            far[0].contains("try a colour such as background, title_bar, title_bar_focused"),
            "{far:?}"
        );
        assert_eq!(
            check("format = 1\n[appearance]\naccent = \"red\"\n[appearance.colours.dark]\npanel = \"#11111b\"\n").unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_colour_that_breaks_a_pair_is_refused_with_its_line() {
        let grey = set("format = 1\n", "appearance.colours.panel", "#777777")
            .unwrap_err()
            .to_string();
        assert!(
            grey.starts_with("appearance.colours: panel_text #1c1e22 on panel #777777 reads at 3.7:1; text needs 4.5:1, such as #"),
            "{grey}"
        );
        let pale = set("format = 1\n", "appearance.accent", "#c0d0ff")
            .unwrap_err()
            .to_string();
        assert!(
            pale.starts_with("appearance.accent: accent #c0d0ff on panel #f6f7f9 reads at 1.4:1; an icon or mark needs 3.0:1, such as #"),
            "{pale}"
        );
        assert!(set("format = 1\n", "appearance.accent", "red").is_ok());
        assert!(set("format = 1\n", "appearance.colours.dark.panel", "#11111b").is_ok());
    }

    #[test]
    fn the_persons_colours_lie_over_the_machines_name_by_name() {
        let machine = Colours {
            both: BTreeMap::from([("panel".to_string(), "#111111".to_string())]),
            light: Some(BTreeMap::from([(
                "window".to_string(),
                "#222222".to_string(),
            )])),
            dark: None,
        };
        let person = Colours {
            both: BTreeMap::from([("accent".to_string(), "#ff0000".to_string())]),
            light: None,
            dark: Some(BTreeMap::from([(
                "panel".to_string(),
                "#333333".to_string(),
            )])),
        };
        let merged = colours_over(Some(&machine), Some(&person)).unwrap();
        assert_eq!(merged.both["panel"], "#111111");
        assert_eq!(merged.both["accent"], "#ff0000");
        assert_eq!(merged.light.as_ref().unwrap()["window"], "#222222");
        assert_eq!(merged.dark.as_ref().unwrap()["panel"], "#333333");
        assert_eq!(colours_over(None, None), None);
        assert_eq!(colours_over(Some(&machine), None).unwrap(), machine);
    }

    #[test]
    fn the_shape_keys_take_their_range_and_refuse_outside_it() {
        let radius = |value: &str| set("format = 1\n", "appearance.radius", value);
        assert!(radius("0").unwrap().contains("radius = 0"));
        assert!(radius("24").is_ok());
        let wide = radius("40").unwrap_err().to_string();
        assert!(wide.contains("a whole number from 0 to 24"), "{wide}");
        assert!(set("format = 1\n", "appearance.title_bar_height", "23").is_err());
        assert!(set("format = 1\n", "appearance.title_bar_height", "48").is_ok());
        assert!(set("format = 1\n", "appearance.panel_opacity", "19").is_err());
        assert!(set("format = 1\n", "appearance.panel_opacity", "20").is_ok());
        assert!(set("format = 1\n", "appearance.gaps_outer", "65").is_err());
        assert!(set("format = 1\n", "appearance.border_width", "10").is_ok());
        assert_eq!(whole_range("appearance.gaps"), Some((0, 64)));
        assert_eq!(whole_range("appearance.mode"), None);
    }

    #[test]
    fn the_cursor_and_icon_sizes_take_their_ranges() {
        let cursor = |value: &str| set("format = 1\n", "appearance.cursor_size", value);
        assert!(cursor("48").unwrap().contains("cursor_size = 48"));
        assert!(cursor("16").is_ok() && cursor("96").is_ok());
        assert!(cursor("15").is_err() && cursor("97").is_err());
        let icon = |value: &str| set("format = 1\n", "appearance.icon_size", value);
        assert!(icon("150").unwrap().contains("icon_size = 150"));
        assert!(icon("75").is_ok());
        let wide = icon("151").unwrap_err().to_string();
        assert!(wide.contains("a whole number from 75 to 150"), "{wide}");
        assert_eq!(whole_range("appearance.cursor_size"), Some((16, 96)));
        assert_eq!(whole_range("appearance.icon_size"), Some((75, 150)));
    }

    #[test]
    fn a_cursor_theme_is_one_this_machine_has_and_default_always_is() {
        let themes = cursor_themes();
        assert_eq!(themes.first().map(String::as_str), Some("default"));
        assert!(cursor_theme_problem("default").is_none());
        let theme = |value: &str| set("format = 1\n", "appearance.cursor_theme", value);
        let refused = theme("no-such-cursor-theme").unwrap_err().to_string();
        assert!(
            refused.contains("is not a cursor theme this machine has"),
            "{refused}"
        );
        assert!(refused.contains("the ones it has are default"), "{refused}");
        assert!(
            theme("default")
                .unwrap()
                .contains("cursor_theme = \"default\"")
        );
    }

    #[test]
    fn the_animation_speed_takes_its_range_and_the_curve_its_names() {
        let speed = |value: &str| set("format = 1\n", "appearance.animation_speed", value);
        assert!(speed("200").unwrap().contains("animation_speed = 200"));
        let fast = speed("500").unwrap_err().to_string();
        assert!(fast.contains("a whole number from 25 to 400"), "{fast}");
        assert!(set("format = 1\n", "appearance.animation_curve", "springy").is_ok());
        assert!(set("format = 1\n", "appearance.animation_curve", "bouncy").is_err());
        assert_eq!(whole_range("appearance.animation_speed"), Some((25, 400)));
    }

    #[test]
    fn a_changed_shape_is_seen_and_a_colour_change_is_not() {
        let shape = |text: &str| read(text).unwrap().file.appearance;
        let old = shape("format = 1\n[appearance]\nradius = 10\n");
        let new = shape("format = 1\n[appearance]\nradius = 12\n");
        assert!(shape_changed(&old, &new));
        assert!(!shape_changed(&old, &old));
        let coloured = shape("format = 1\n[appearance]\nradius = 10\naccent = \"red\"\n");
        assert!(!shape_changed(&old, &coloured));
        assert!(shape_changed(
            &Appearance::default(),
            &shape("format = 1\n[appearance]\npanel_opacity = 50\n")
        ));
    }

    #[test]
    fn the_text_keys_take_their_range_and_a_changed_text_is_seen() {
        let text_size = |value: &str| set("format = 1\n", "appearance.text_size", value);
        assert!(text_size("80").unwrap().contains("text_size = 80"));
        assert!(text_size("150").is_ok());
        let wide = text_size("151").unwrap_err().to_string();
        assert!(wide.contains("a whole number from 80 to 150"), "{wide}");
        assert!(text_size("79").is_err());
        assert!(set("format = 1\n", "appearance.font", "Noto Sans").is_ok());
        assert_eq!(whole_range("appearance.text_size"), Some((80, 150)));
        let appearance = |text: &str| read(text).unwrap().file.appearance;
        let old = appearance("format = 1\n[appearance]\nfont = \"Inter Variable\"\n");
        let new = appearance("format = 1\n[appearance]\nfont = \"Noto Sans\"\n");
        assert!(text_changed(&old, &new));
        assert!(!text_changed(&old, &old));
        let bigger = appearance("format = 1\n[appearance]\ntext_size = 120\n");
        assert!(text_changed(&old, &bigger));
        assert!(!shape_changed(&old, &bigger));
    }
}
