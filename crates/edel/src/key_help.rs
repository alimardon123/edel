//! What every settings file key means, its default and who decides it
//! (M6.11, ADR-008): one line per key of [`crate::settings::KEYS`], the
//! one owner of the help `edel settings get` prints, the suggestion
//! `set` and `check` give, and the two generated docs,
//! `docs/settings.md`'s key list and `docs/defaults.md`. A test fails
//! when a key has no line here or a line names no key.

use crate::i18n::n_;
use crate::settings::{
    GAME_MODE_DEFAULT, LID_CLOSE_DEFAULT, LOCK_AFTER_MINUTES_DEFAULT_TEXT, ON_BATTERY_DEFAULT,
    PAGES, POWER_BUTTON_DEFAULT, page_of,
};

/// Who decides a key's value while the settings file does not set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecidedBy {
    /// The release's own value, the same on every machine.
    Release,
    /// The layout preset in use (`layout.preset`), so it differs by preset.
    Preset,
    /// A design token in `design/tokens.toml`.
    Token,
    /// The image the machine was made from (a desktop, a server).
    Image,
    /// The hardware: the screen, the keyboard, the disk.
    Hardware,
}

impl DecidedBy {
    /// The word `edel settings get` and the docs show for it.
    pub fn word(self) -> &'static str {
        match self {
            DecidedBy::Release => "release",
            DecidedBy::Preset => "preset",
            DecidedBy::Token => "token",
            DecidedBy::Image => "image",
            DecidedBy::Hardware => "hardware",
        }
    }
}

/// Whose a key is (Seam, ADR-011): a person's follows them to another of
/// their devices (M7.10f), a device's stays with the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Belongs {
    Person,
    Device,
}

/// One key's line.
#[derive(Clone, Copy, Debug)]
pub struct Help {
    /// The key's path as [`crate::settings::KEYS`] writes it, `*` and all.
    pub key: &'static str,
    /// One sentence for people, marked for translation.
    pub help: &'static str,
    /// The value that applies while the file does not set the key, as the
    /// file would write it, or a short phrase when it depends ("the
    /// preset's", "every screen's own").
    pub default: &'static str,
    /// Who decides that default.
    pub from: DecidedBy,
    pub belongs: Belongs,
    /// The release that stopped using the key, if one did: it is still
    /// read, reported as no longer used and refused by `set`.
    pub retired: Option<&'static str>,
}

/// Every key's line, in the key table's order.
pub const HELP: &[Help] = &[
    Help {
        key: "system.variant",
        help: n_(
            "What kind of system this is: a container, a server, a desktop or a phone; this release does not act on it yet.",
        ),
        default: "the image's",
        from: DecidedBy::Image,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "system.developer_mode",
        help: n_(
            "Developer mode, which lets people change the system's own parts, such as its kernel; off unless a person turns it on.",
        ),
        default: "false",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "system.profiles",
        help: n_(
            "The hardware profiles applied to this machine, in order; this release does not act on them yet.",
        ),
        default: "[]",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "users.*.admin",
        help: n_("Whether this person is an administrator, who may change the whole machine."),
        default: "false",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "users.*.ssh_keys",
        help: n_(
            "The public keys that may log in to this person's account over ssh, one line each.",
        ),
        default: "the file as it is",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "users.*.login_shell",
        help: n_("The program this person's login starts, given as a full path such as /bin/ash."),
        default: r#""/bin/sh""#,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "network.hostname",
        help: n_(
            "The name this computer has on the network: letters, digits and hyphens, up to 63.",
        ),
        default: r#""edel""#,
        from: DecidedBy::Image,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "region.language",
        help: n_(
            "The language the desktop's own words and Settings are shown in, such as de or pt_BR.",
        ),
        default: "English",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "region.keyboard",
        help: n_(
            "The keyboard layouts, up to four, such as us or us,ru; Super+Space goes to the next one.",
        ),
        default: r#""us""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "region.timezone",
        help: n_("The time zone the clock shows, such as Europe/London."),
        default: r#""UTC""#,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "region.time_servers",
        help: n_(
            "The servers the clock is set from, one to four host names, such as pool.ntp.org.",
        ),
        default: r#"["pool.ntp.org"]"#,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "layout.preset",
        help: n_(
            "The layout preset that arranges the panel, windows and workspaces, such as classic, mac-like or zen.",
        ),
        default: r#""classic""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "layout.tiling",
        help: n_(
            "Whether windows tile across the screen (true) or float freely (false); the preset decides when this is missing.",
        ),
        default: "the preset's",
        from: DecidedBy::Preset,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "layout.title_bars",
        help: n_(
            "Where windows get a title bar: always, only on floating windows, or never (Zen has none).",
        ),
        default: "the preset's",
        from: DecidedBy::Preset,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "layout.device_type",
        help: n_(
            "What kind of device this is: a desktop, a tablet or a phone; this release does not act on it yet.",
        ),
        default: "detected",
        from: DecidedBy::Hardware,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "layout.window_buttons",
        help: n_(
            "The side of each title bar where close, minimize and maximize sit: left or right.",
        ),
        default: "the preset's",
        from: DecidedBy::Preset,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "layout.close_button",
        help: n_(
            "Whether each title bar shows its close button; Super+Q closes a window whether or not it shows.",
        ),
        default: "true",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "layout.minimize_button",
        help: n_("Whether each title bar shows its minimize button."),
        default: "true",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "layout.maximize_button",
        help: n_(
            "Whether each title bar shows its maximize button; a hidden button's room goes to the title.",
        ),
        default: "true",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "layout.tiling_style",
        help: n_(
            "How tiling lays windows out: stack, split (as Hyprland tiles) or scroll (as niri tiles).",
        ),
        default: "the preset's",
        from: DecidedBy::Preset,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "panels.list",
        help: n_(
            "The panels along the screen's edges, each with its widgets, in place of the preset's.",
        ),
        default: "the preset's",
        from: DecidedBy::Preset,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "panels.tray",
        help: n_(
            "The tray apps kept in the panel, by their tray id; the other apps wait behind the tray's arrow.",
        ),
        default: "[]",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "panels.pinned",
        help: n_("The apps the apps widget pins, in order, by app id or role such as browser."),
        default: "the preset's",
        from: DecidedBy::Preset,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "workspaces.count",
        help: n_("How many workspaces there are, from 1 to 9, one for each of Super+1 to Super+9."),
        default: "the preset's",
        from: DecidedBy::Preset,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "workspaces.dynamic",
        help: n_("Whether empty workspaces come and go, so the count follows the windows."),
        default: "false",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "workspaces.per_screen",
        help: n_(
            "Whether each screen shows its own workspace, so Super+1 to Super+9 switch only the screen under the pointer.",
        ),
        default: "false",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "workspaces.names",
        help: n_(
            "The names of the workspaces, the first one's first; an empty text leaves a workspace as its number.",
        ),
        default: "[]",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "workspaces.wallpapers",
        help: n_(
            "A wallpaper for each workspace, the first one's first; an empty text uses Appearance's wallpaper.",
        ),
        default: "[]",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "workspaces.apps",
        help: n_(
            "The apps that always open on one workspace, by app id and number, such as Firefox on 2.",
        ),
        default: "{}",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "workspaces.overview_strip",
        help: n_(
            "The side of the screen where the workspace overview's strip lies: left, right, top or bottom.",
        ),
        default: r#""left""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "displays.*.position",
        help: n_("Where this screen sits among the others, as [x, y] in whole numbers."),
        default: "each screen's own",
        from: DecidedBy::Hardware,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "displays.*.scale",
        help: n_("How big things are on this screen, such as 1.25 for 125 percent."),
        default: "each screen's own",
        from: DecidedBy::Hardware,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "displays.*.resolution",
        help: n_("The resolution of this screen as WIDTHxHEIGHT, such as 1920x1080."),
        default: "each screen's own",
        from: DecidedBy::Hardware,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "displays.*.refresh_rate",
        help: n_("How many times a second this screen redraws, such as 60."),
        default: "the resolution's best",
        from: DecidedBy::Hardware,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "displays.*.enabled",
        help: n_("Whether this screen is on; a screen that is off shows nothing."),
        default: "true",
        from: DecidedBy::Hardware,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "displays.*.rotation",
        help: n_(
            "How far this screen is turned: 0, 90, 180 or 270 degrees; this release does not act on it yet.",
        ),
        default: "each screen's own",
        from: DecidedBy::Hardware,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "appearance.theme",
        help: n_(
            "The theme in use: a built-in one (edel, aurora, earth or ocean) or one of your own.",
        ),
        default: r#""edel""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.wallpaper",
        help: n_(
            "The wallpaper behind the windows: a name such as hills, a picture's path, a colour or none.",
        ),
        default: r#""hills""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.wallpaper_dark",
        help: n_("The wallpaper for dark mode, when it differs from the light one."),
        default: "the wallpaper's",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.pictures",
        help: n_(
            "The pictures of the theme in use, shown on top of its own; at most 32, none twice.",
        ),
        default: "the theme's",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.switcher_look",
        help: n_(
            "How the workspace switcher looks: round numbers, or a button with a line for each workspace.",
        ),
        default: r#""numbers""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.switcher_shown",
        help: n_("How many workspace numbers the switcher shows at once, from 1 to 9."),
        default: "3",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.switcher_ends",
        help: n_(
            "What the switcher's ends show when more workspaces lie beyond them: fade, arrows or counts.",
        ),
        default: r#""arrows""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.mode",
        help: n_(
            "Light or dark colours for title bars, panels and apps; auto is the release's choice, light in this one.",
        ),
        default: r#""auto""#,
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.contrast",
        help: n_(
            "High contrast gives strong edges and plain colours in both schemes; normal is the usual look.",
        ),
        default: r#""normal""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.accent",
        help: n_(
            "The accent colour for the shown workspace, buttons and marks: a named accent, wallpaper, or a colour.",
        ),
        default: r#""blue""#,
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.colours.*",
        help: n_("One colour of the look, by its token name, in both schemes."),
        default: "the token's",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.colours.light.*",
        help: n_("One colour of the light scheme alone, over the colours both schemes share."),
        default: "the token's",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.colours.dark.*",
        help: n_("One colour of the dark scheme alone, over the colours both schemes share."),
        default: "the token's",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.radius",
        help: n_(
            "The corner radius of windows, in logical pixels from 0 to 24; 0 makes every corner square.",
        ),
        default: "the token size.radius_window",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.gaps",
        help: n_("The gap between tiled windows, in logical pixels from 0 to 64."),
        default: "the token size.gap",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.gaps_outer",
        help: n_(
            "The gap between tiled windows and the screen's edge, in logical pixels from 0 to 64.",
        ),
        default: "the token size.gap_outer",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.border_width",
        help: n_("The border round each window with a title bar, in logical pixels from 0 to 10."),
        default: "the token size.border",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.title_bar_height",
        help: n_("The height of each title bar, in logical pixels from 24 to 48."),
        default: "the token size.title_bar",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.panel_opacity",
        help: n_("How solid the panels are, in percent from 20 to 100."),
        default: "the token size.panel_opacity",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.window_opacity",
        help: n_("How solid the focused window is, in percent from 20 to 100."),
        default: "the token size.window_opacity",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.window_opacity_unfocused",
        help: n_("How solid every other window is, in percent from 20 to 100."),
        default: "the token size.window_opacity_unfocused",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.shadow_size",
        help: n_(
            "How far the shadow under each window spreads, in logical pixels from 0 to 64; 0 is none.",
        ),
        default: "the token size.shadow_blur",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.shadow_darkness",
        help: n_("How dark the shadow under each window is, in percent from 0 to 100."),
        default: "the alpha of colour.shadow",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.blur",
        help: n_(
            "How strongly the wallpaper blurs behind see-through panels, in percent; 0 shows it sharp.",
        ),
        default: "the token size.blur",
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.font",
        help: n_("The font for the interface's text, by its family name."),
        default: r#""Inter Variable""#,
        from: DecidedBy::Token,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.monospace_font",
        help: n_("The font apps use for code and terminals, by its family name."),
        default: r#""Noto Sans Mono""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.text_size",
        help: n_("How big every text is, in percent of the release's size, from 80 to 150."),
        default: "100",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.cursor_size",
        help: n_("The pointer's height in logical pixels, from 16 to 96."),
        default: "24",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.cursor_theme",
        help: n_(
            "The pointer's theme, by the folder name of an installed cursor theme; default is the drawn arrow.",
        ),
        default: r#""default""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.icon_theme",
        help: n_(
            "The apps' icon theme, by the folder name of an installed icon theme; the panel and apps draw app icons from it, and the shell's own icons stay Edel's.",
        ),
        default: r#""Edel""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.icon_size",
        help: n_(
            "How big the icons in the panel and the bar are, in percent of the release's, from 75 to 150.",
        ),
        default: "100",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.animations",
        help: n_("How much things move: full, reduced (fades only) or off."),
        default: r#""full""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.animation_speed",
        help: n_(
            "How fast things move, in percent of the release's speed, from 25 to 400; 200 is twice as fast.",
        ),
        default: "100",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "appearance.animation_curve",
        help: n_(
            "How animations feel: smooth, snappy (quicker) or springy (windows and pop-ups overshoot a little).",
        ),
        default: r#""smooth""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "lock_screen.parts",
        help: n_(
            "The parts of the lock and login screens and where each one stands, as a list of tables.",
        ),
        default: "the look's",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "lock_screen.look",
        help: n_(
            "The look of the lock and login screens: classic, magazine, minimal or one of your own.",
        ),
        default: r#""classic""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "lock_screen.background",
        help: n_(
            "What lies behind the lock screen's parts: the backdrop, the wallpaper blurred or sharp, a colour or a picture.",
        ),
        default: "the look's",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "lock_screen.dim",
        help: n_(
            "How much the lock screen's background is darkened, in percent from 0 to 80, so the words read.",
        ),
        default: "30, or 40 for a sharp background or picture",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "lock_screen.notifications",
        help: n_(
            "Whether the lock screen shows how many notifications are unread: count, or none to hide it.",
        ),
        default: r#""count""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "lock_screen.looks",
        help: n_("Your own lock screen looks, each with its parts and background, chosen by name."),
        default: "none",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "themes.*",
        help: n_(
            "One of your own themes: its title, its pictures and the appearance values it sets.",
        ),
        default: "none",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "presets.*",
        help: n_(
            "One of your own layout presets: the built-in preset it starts from and the layout keys it sets.",
        ),
        default: "none",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "shortcuts.*",
        help: n_(
            "The keys for one action, such as Super+X, or empty for none; the Shortcuts page lists the actions.",
        ),
        default: "each action's own keys, docs/SHORTCUTS.md",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "clipboard.history",
        help: n_(
            "Keeps the last texts and pictures you copied, in memory only, for Super+V; false keeps nothing.",
        ),
        default: "true",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "default_apps.browser",
        help: n_(
            "The app that opens web pages and web links, by its desktop file id without .desktop.",
        ),
        default: "the first installed browser",
        from: DecidedBy::Image,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "default_apps.files",
        help: n_("The app that opens folders, by its desktop file id without .desktop."),
        default: "the first installed file manager",
        from: DecidedBy::Image,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "default_apps.editor",
        help: n_("The app that opens plain text files, by its desktop file id without .desktop."),
        default: "the first installed editor",
        from: DecidedBy::Image,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "default_apps.terminal",
        help: n_("The app the terminal shortcut opens, by its desktop file id without .desktop."),
        default: "the first installed terminal, else foot",
        from: DecidedBy::Image,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "default_apps.mail",
        help: n_(
            "The app that opens mail and mail links, by its desktop file id without .desktop.",
        ),
        default: "the first installed mail app",
        from: DecidedBy::Image,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "startup.apps",
        help: n_(
            "The apps that start once the desktop is on screen, in the order listed, by desktop file id.",
        ),
        default: "[]",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "power.lid_close",
        help: n_(
            "What closing the lid does: suspend (locking first), lock, nothing or power off; nothing happens while two or more screens are lit.",
        ),
        default: LID_CLOSE_DEFAULT,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "power.lock_after_minutes",
        help: n_("How many minutes without input before the screen locks, 0 for never."),
        default: LOCK_AFTER_MINUTES_DEFAULT_TEXT,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "power.power_button",
        help: n_(
            "What the power button does: suspend (the screen locks first), power off, ask (the screen locks and its power menu opens) or nothing.",
        ),
        default: POWER_BUTTON_DEFAULT,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "power.on_battery",
        help: n_(
            "How hard the machine works on battery: power-saver, balanced or performance; plugged in, it is balanced, or performance when chosen here.",
        ),
        default: ON_BATTERY_DEFAULT,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "power.game_mode",
        help: n_(
            "When a fullscreen game gets variable refresh: plugged-in (or with no battery), always or off; Super+G turns it on or off until you log out.",
        ),
        default: GAME_MODE_DEFAULT,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "services.*",
        help: n_("Turns one switchable feature, such as ssh or bluetooth, on or off by its name."),
        default: "the image's",
        from: DecidedBy::Image,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "updates.channel",
        help: n_(
            "The release channel whose updates this machine takes, such as stable or preview.",
        ),
        default: "the image's",
        from: DecidedBy::Image,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "updates.version",
        help: n_(
            "A release to stay on instead of following the channel; this release does not act on it yet.",
        ),
        default: "follows the channel",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "updates.automatic",
        help: n_(
            "Whether updates are checked, installed, or installed and restarted by themselves, or off; this release does not act on it yet.",
        ),
        default: "not set yet",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "updates.restart_window",
        help: n_(
            "The hours when a restart may happen by itself, such as 02:00-04:00; this release does not act on it yet.",
        ),
        default: "none",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "apps.installed",
        help: n_(
            "The Flathub apps installed for each administrator, by app id such as io.mpv.Mpv.",
        ),
        default: "[]",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "apps.camera",
        help: n_(
            "Which apps may use the camera, by Flatpak app id: allow, ask every time or never.",
        ),
        default: "{}",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "apps.microphone",
        help: n_("Which apps may use the microphone, by Flatpak app id: allow or never."),
        default: "{}",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "apps.screenshots",
        help: n_(
            "Which apps may take a screenshot without asking, by Flatpak app id: allow or never.",
        ),
        default: "{}",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "apps.notifications",
        help: n_("Which apps may show notifications, by Flatpak app id: allow or never."),
        default: "{}",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "apps.files",
        help: n_(
            "Which apps reach your files by themselves (allow) or only the ones you pick (ask), by Flatpak app id.",
        ),
        default: "{}",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "addons.installed",
        help: n_(
            "The add-ons this machine has, by name; each arrives with the next update and is in edel addon list.",
        ),
        default: "[]",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "notifications.do_not_disturb",
        help: n_(
            "Whether banners stay away; notifications still arrive and are listed in the notification centre.",
        ),
        default: "false",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "notifications.position",
        help: n_("Where banners appear: by the status area, in a corner, or at the top's middle."),
        default: r#""auto""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "notifications.badge",
        help: n_(
            "What the notifications button shows while some are unread: a dot, the count, or nothing.",
        ),
        default: r#""dot""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "notifications.list",
        help: n_(
            "Where the notification list opens: hung on its button, or as a sheet on the left or right.",
        ),
        default: r#""button""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "region.input_method",
        help: n_(
            "The input method for typing languages a keyboard has no keys for: fcitx5 or none.",
        ),
        default: r#""none""#,
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "apps.location",
        help: n_(
            "Which apps may know where this computer is, by Flatpak app id: allow, ask every time or never.",
        ),
        default: "{}",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "location.enabled",
        help: n_("Off gives every app nothing, whatever each app may use."),
        default: "true",
        from: DecidedBy::Release,
        belongs: Belongs::Person,
        retired: None,
    },
    Help {
        key: "location.place",
        help: n_("The city apps are told, named by its time zone, such as Asia/Tashkent."),
        default: "the time zone's city",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "location.online",
        help: n_(
            "Find the place from nearby Wi-Fi networks through BeaconDB, a community service, which is sent their hardware addresses.",
        ),
        default: "false",
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "install.disk",
        help: n_(
            "The disk a stick installs itself on: auto, the one fixed disk large enough, or a /dev/disk/by-id/ name.",
        ),
        default: r#""auto""#,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "install.erase",
        help: n_(
            "Whether an unattended install erases only an empty disk, or any disk after a ten second countdown.",
        ),
        default: r#""empty-only""#,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
    Help {
        key: "install.then",
        help: n_("What the machine does once a stick has installed itself: power off or restart."),
        default: r#""poweroff""#,
        from: DecidedBy::Release,
        belongs: Belongs::Device,
        retired: None,
    },
];

/// The line for `key`, a concrete key (`users.ali.admin`) or a table path
/// (`users.*.admin`).
pub fn find(key: &str) -> Option<&'static Help> {
    HELP.iter().find(|line| fits(line.key, key))
}

/// Whether `key` is `path`, a `*` standing for any one part.
fn fits(path: &str, key: &str) -> bool {
    let (mut a, mut b) = (path.split('.'), key.split('.'));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(p), Some(k)) if p == "*" || p == k => {}
            _ => return false,
        }
    }
}

/// The Markdown of `docs/settings.md`'s list of every key (M6.11): a heading
/// for each page of the Settings app with its keys in a table, then a last
/// heading for the keys no page holds.
pub fn settings_markdown() -> String {
    let mut out = String::new();
    for page in PAGES {
        out.push_str(&format!("### `{}`\n\n", page.section));
        let lines: Vec<&Help> = HELP
            .iter()
            .filter(|line| page_of(line.key).map(|p| p.section) == Some(page.section))
            .collect();
        if lines.is_empty() {
            out.push_str("This page has no key in the file.\n\n");
        } else {
            out.push_str(&table(&lines));
        }
    }
    let other: Vec<&Help> = HELP
        .iter()
        .filter(|line| page_of(line.key).is_none())
        .collect();
    out.push_str("### Other keys\n\n");
    out.push_str(&table(&other));
    out
}

/// A default as the docs show it: a value the file would write (`"classic"`,
/// `false`, `4`, `[]`) in code style, a phrase ("the preset's") as words.
fn shown(default: &str) -> String {
    let value = default.starts_with('"')
        || default.starts_with('[')
        || default.starts_with('{')
        || matches!(default, "true" | "false")
        || default.parse::<f64>().is_ok();
    if value {
        format!("`{}`", cell(default))
    } else {
        cell(default)
    }
}

/// The whole of `docs/defaults.md` (M6.11): every key's default and who
/// decides it, in the order of [`HELP`].
pub fn defaults_markdown() -> String {
    let mut out = String::from(
        "# Defaults\n\nThis file is written by `EDEL_WRITE_DOCS=1 cargo test -p edel key_help` from `crates/edel/src/key_help.rs` and must not be edited by hand. It gives the value every settings key has while the settings file does not set it, and who decides that value. [The settings file](settings.md) says what each key does.\n\n| Key | Default | Decided by |\n|---|---|---|\n",
    );
    for line in HELP {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            line.key,
            shown(line.default),
            line.from.word()
        ));
    }
    out
}

/// Text for a table cell: a `|` would end the cell, so it is escaped.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

/// The table of `lines` under its header, with a blank line after it.
fn table(lines: &[&Help]) -> String {
    let mut out =
        String::from("| Key | What it does | Default | Decided by |\n|---|---|---|---|\n");
    for line in lines {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            line.key,
            cell(line.help),
            shown(line.default),
            line.from.word()
        ));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::KEYS;

    /// The two marker lines of `docs/settings.md`'s generated list of keys.
    const BEGIN: &str =
        "<!-- written by key_help.rs: EDEL_WRITE_DOCS=1 cargo test -p edel key_help -->";
    const END: &str = "<!-- end of the keys -->";

    /// The byte range of the generated keys in a settings page: from the
    /// line after `BEGIN` to the start of `END`.
    fn generated(page: &str) -> Option<std::ops::Range<usize>> {
        let start = page.find(BEGIN)? + BEGIN.len();
        let start = start + page[start..].find('\n')? + 1;
        let end = start + page[start..].find(END)?;
        Some(start..end)
    }

    #[test]
    fn every_key_has_one_line_in_its_order() {
        let lines: Vec<&str> = HELP.iter().map(|line| line.key).collect();
        let keys: Vec<&str> = KEYS.iter().map(|key| key.path).collect();
        assert_eq!(lines, keys);
    }

    #[test]
    fn each_help_is_one_short_sentence() {
        for line in HELP {
            assert!(line.help.ends_with('.'), "{}: no full stop", line.key);
            assert!(
                !line.help.contains('\n'),
                "{}: more than one line",
                line.key
            );
            assert!(
                line.help.chars().count() <= 150,
                "{}: longer than 150 characters",
                line.key
            );
            assert!(
                !line.help.contains('\u{2013}') && !line.help.contains('\u{2014}'),
                "{}: an en dash or an em dash",
                line.key
            );
        }
    }

    #[test]
    fn find_takes_a_concrete_key_for_a_star() {
        assert_eq!(
            find("users.ali.admin").map(|line| line.key),
            Some("users.*.admin")
        );
        assert_eq!(
            find("layout.preset").map(|line| line.key),
            Some("layout.preset")
        );
        assert!(find("no.such").is_none());
    }

    #[test]
    fn the_power_defaults_are_the_settings_ones() {
        use crate::settings::LOCK_AFTER_MINUTES_DEFAULT;
        assert_eq!(
            LOCK_AFTER_MINUTES_DEFAULT_TEXT.parse::<u32>().ok(),
            Some(LOCK_AFTER_MINUTES_DEFAULT)
        );
        assert_eq!(find("power.lid_close").map(|h| h.default), Some("suspend"));
        assert_eq!(
            find("power.power_button").map(|h| h.default),
            Some("suspend")
        );
        assert_eq!(
            find("power.on_battery").map(|h| h.default),
            Some("balanced")
        );
    }

    #[test]
    fn the_docs_are_what_the_table_writes() {
        let settings_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/settings.md");
        let defaults_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/defaults.md");
        if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
            let page = std::fs::read_to_string(settings_path).unwrap();
            let span = generated(&page).expect("docs/settings.md has no generated keys");
            let written = format!(
                "{}{}{}",
                &page[..span.start],
                settings_markdown(),
                &page[span.end..]
            );
            std::fs::write(settings_path, written).unwrap();
            std::fs::write(defaults_path, defaults_markdown()).unwrap();
        }
        let page = std::fs::read_to_string(settings_path).unwrap();
        let span = generated(&page).expect("docs/settings.md has no generated keys");
        assert!(
            page[span] == settings_markdown(),
            "docs/settings.md is not what key_help.rs writes; run EDEL_WRITE_DOCS=1 cargo test -p edel key_help"
        );
        let defaults = std::fs::read_to_string(defaults_path).unwrap_or_default();
        assert!(
            defaults == defaults_markdown(),
            "docs/defaults.md is not what key_help.rs writes; run EDEL_WRITE_DOCS=1 cargo test -p edel key_help"
        );
    }
}
