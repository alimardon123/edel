//! Every row of Settings with its key (M5.6b): the one list a test holds
//! to `edel::settings`'s key table, and what each row says about where its
//! value comes from (ADR-008).

use edel::i18n::{n_, tr, trf};
use edel::settings::Source;

/// One row: the settings file key it shows and changes.
pub struct Row {
    pub key: &'static str,
    pub title: &'static str,
}

/// The rows of every page, page by page.
pub const ROWS: &[Row] = &[
    Row {
        key: "layout.preset",
        title: n_("Preset"),
    },
    Row {
        key: "layout.tiling",
        title: n_("Tile windows"),
    },
    Row {
        key: "layout.tiling_style",
        title: n_("Tiling style"),
    },
    // The panels that apply, and the editor that changes them (M5.31d).
    Row {
        key: "panels.list",
        title: n_("Panels"),
    },
    // The apps the dock's apps widget pins, listed under Panels; the drag
    // on the panel changes them (M5.31d).
    Row {
        key: "panels.pinned",
        title: n_("Pinned apps"),
    },
    Row {
        key: "layout.title_bars",
        title: n_("Title bars"),
    },
    Row {
        key: "layout.window_buttons",
        title: n_("Window buttons"),
    },
    // The title bar's buttons (M5.18a).
    Row {
        key: "layout.minimize_button",
        title: n_("Minimize button"),
    },
    Row {
        key: "layout.maximize_button",
        title: n_("Maximize button"),
    },
    Row {
        key: "layout.close_button",
        title: n_("Close button"),
    },
    // The tray's apps in the panel (M5.9h): the card lists each app.
    Row {
        key: "panels.tray",
        title: n_("Tray"),
    },
    // The Workspaces page (M5.2n, its own page since M5.2q): the count,
    // dynamic and per-screen ones, the names and the apps that open on
    // their own workspace.
    Row {
        key: "workspaces.count",
        title: n_("Number of workspaces"),
    },
    Row {
        key: "workspaces.dynamic",
        title: n_("Dynamic workspaces"),
    },
    Row {
        key: "workspaces.per_screen",
        title: n_("Workspaces on each screen"),
    },
    Row {
        key: "workspaces.names",
        title: n_("Workspace names"),
    },
    // One wallpaper per workspace (M5.12b5): its group sits under the names.
    Row {
        key: "workspaces.wallpapers",
        title: n_("Wallpaper for each workspace"),
    },
    Row {
        key: "workspaces.apps",
        title: n_("Apps on their workspace"),
    },
    Row {
        key: "workspaces.overview_strip",
        title: n_("Side of the strip in the workspace overview"),
    },
    // The Appearance page (M5.2q): the wallpaper first (M5.10c4c), then
    // light or dark, then the workspace switcher's look, shown count and
    // ends.
    // The theme card (M5.12b6b): the theme in use, its pictures on top of
    // the theme's own, and a dark style's picture of a pair (M5.10c4d).
    Row {
        key: "appearance.theme",
        title: n_("Theme"),
    },
    Row {
        key: "appearance.pictures",
        title: n_("Pictures"),
    },
    Row {
        key: "appearance.wallpaper",
        title: n_("Wallpaper"),
    },
    Row {
        key: "appearance.wallpaper_dark",
        title: n_("Dark style wallpaper"),
    },
    Row {
        key: "appearance.mode",
        title: n_("Style"),
    },
    // The accent and the colours (M5.12b1c): the swatches, then each colour
    // a person sets; `*` is a colour token's name.
    Row {
        key: "appearance.accent",
        title: n_("Accent"),
    },
    Row {
        key: "appearance.colours.*",
        title: n_("Colours"),
    },
    // A colour for one style only, Light or Dark in the Add a colour row.
    Row {
        key: "appearance.colours.light.*",
        title: n_("Light style colours"),
    },
    Row {
        key: "appearance.colours.dark.*",
        title: n_("Dark style colours"),
    },
    // The high-contrast look (M6.9): a switch under the accent, for strong
    // edges and the plainest colours.
    Row {
        key: "appearance.contrast",
        title: n_("High contrast"),
    },
    // The Shape group (M5.12b2c): the corners, the gaps, the border, the
    // title bar's height and the panels' opacity, each a slider.
    Row {
        key: "appearance.radius",
        title: n_("Corners"),
    },
    Row {
        key: "appearance.gaps",
        title: n_("Gaps"),
    },
    Row {
        key: "appearance.gaps_outer",
        title: n_("Gaps at the edge"),
    },
    Row {
        key: "appearance.border_width",
        title: n_("Border"),
    },
    Row {
        key: "appearance.title_bar_height",
        title: n_("Title bar height"),
    },
    Row {
        key: "appearance.panel_opacity",
        title: n_("Panels' opacity"),
    },
    // The Depth group (M5.12b3): how see-through the windows are, the
    // shadow under them and the glass behind panels, shown after the shape.
    Row {
        key: "appearance.window_opacity",
        title: n_("Window opacity"),
    },
    Row {
        key: "appearance.window_opacity_unfocused",
        title: n_("Unfocused windows"),
    },
    Row {
        key: "appearance.shadow_size",
        title: n_("Shadow size"),
    },
    Row {
        key: "appearance.shadow_darkness",
        title: n_("Shadow darkness"),
    },
    Row {
        key: "appearance.blur",
        title: n_("Glass blur"),
    },
    // The text (M5.12b5): the fonts and the size, shown above the shape.
    Row {
        key: "appearance.font",
        title: n_("Interface font"),
    },
    Row {
        key: "appearance.monospace_font",
        title: n_("Monospace font"),
    },
    Row {
        key: "appearance.text_size",
        title: n_("Text size"),
    },
    // The pointer and the icons (M5.12b5 part A), after the text.
    Row {
        key: "appearance.cursor_size",
        title: n_("Cursor size"),
    },
    Row {
        key: "appearance.cursor_theme",
        title: n_("Cursor"),
    },
    Row {
        key: "appearance.icon_theme",
        title: n_("Icons"),
    },
    Row {
        key: "appearance.icon_size",
        title: n_("Icon size"),
    },
    // The Motion group (M5.12b4): how much things move, how fast, and the
    // feel, whose values (Springy and the rest) search finds too.
    Row {
        key: "appearance.animations",
        title: n_("Motion"),
    },
    Row {
        key: "appearance.animation_speed",
        title: n_("Animation speed"),
    },
    Row {
        key: "appearance.animation_curve",
        title: n_("Feel"),
    },
    Row {
        key: "appearance.switcher_look",
        title: n_("Switcher look"),
    },
    Row {
        key: "appearance.switcher_shown",
        title: n_("Numbers shown"),
    },
    Row {
        key: "appearance.switcher_ends",
        title: n_("Switcher ends"),
    },
    // The Desktop group (M8.21 part 21j1): the clock and the date over the
    // wallpaper, under the windows.
    Row {
        key: "appearance.widgets",
        title: n_("Desktop widgets"),
    },
    // The Lock screen page (M5.10c5c): the look, what lies behind it, the
    // clock and the date, and the count of unread notifications.
    Row {
        key: "lock_screen.look",
        title: n_("Look"),
    },
    Row {
        key: "lock_screen.wallpaper",
        title: n_("Wallpaper"),
    },
    Row {
        key: "lock_screen.widgets",
        title: n_("Clock and date"),
    },
    Row {
        key: "lock_screen.notifications",
        title: n_("Unread count"),
    },
    // The Users page (M5.8b): whether the person at the machine is an
    // administrator, shown, and changed by an administrator.
    Row {
        key: "users.*.admin",
        title: n_("Administrator"),
    },
    // The Displays page (M5.7a); the `*` is a screen's name.
    Row {
        key: "displays.*.resolution",
        title: n_("Resolution"),
    },
    Row {
        key: "displays.*.scale",
        title: n_("Scale"),
    },
    Row {
        key: "displays.*.position",
        title: n_("Position"),
    },
    Row {
        key: "displays.*.enabled",
        title: n_("On"),
    },
    // The Sound page's one key (M7.6b); the volume and the devices are the
    // sound system's own state.
    Row {
        key: edel::settings::LOW_LATENCY,
        title: n_("Low latency"),
    },
    // The Power page (M7.8): the lid, the power button, the screen lock and
    // the battery's profile, in the page's order.
    Row {
        key: edel::settings::LID_CLOSE,
        title: n_("When the lid closes"),
    },
    Row {
        key: edel::settings::POWER_BUTTON,
        title: n_("Power button"),
    },
    Row {
        key: edel::settings::LOCK_AFTER_MINUTES,
        title: n_("Lock the screen after"),
    },
    Row {
        key: edel::settings::ON_BATTERY,
        title: n_("On battery"),
    },
    Row {
        key: edel::settings::GAME_MODE,
        title: n_("Game mode"),
    },
    // The Notifications page (M5.9b).
    Row {
        key: edel::settings::DO_NOT_DISTURB,
        title: n_("Do not disturb"),
    },
    // Where banners appear (M5.9j).
    Row {
        key: edel::settings::NOTIFICATION_POSITION,
        title: n_("Where notifications appear"),
    },
    // What the notifications button shows while some are unread (M5.9k).
    Row {
        key: edel::settings::NOTIFICATION_BADGE,
        title: n_("Unread badge"),
    },
    // Where the notification list opens (M5.9l).
    Row {
        key: edel::settings::NOTIFICATION_LIST,
        title: n_("Notification list"),
    },
    // The Region page (M6.11): the five `[region]` keys, in the page's order.
    // The language, the layouts and the input method are the person's; the
    // time zone and the servers are the machine's, set as root.
    Row {
        key: "region.language",
        title: n_("Language"),
    },
    Row {
        key: "region.keyboard",
        title: n_("Keyboard layouts"),
    },
    Row {
        key: "region.input_method",
        title: n_("Input method"),
    },
    Row {
        key: "region.timezone",
        title: n_("Time zone"),
    },
    Row {
        key: "region.time_servers",
        title: n_("Time servers"),
    },
    // The Location page (M6.14c): the person's switch, then the machine's
    // city and its online lookup.
    Row {
        key: "location.enabled",
        title: n_("Location"),
    },
    Row {
        key: "location.place",
        title: n_("Place"),
    },
    Row {
        key: "location.online",
        title: n_("Look online"),
    },
    // The System page's profile (M7.6), the machine's.
    Row {
        key: "system.profiles",
        title: n_("Profile"),
    },
    // The Default apps page (M6.10): the app each role opens, one row each,
    // in the order `edel::apps::DEFAULT_APP_KEYS` gives them.
    Row {
        key: "default_apps.browser",
        title: n_("Web browser"),
    },
    Row {
        key: "default_apps.files",
        title: n_("Files"),
    },
    Row {
        key: "default_apps.editor",
        title: n_("Text editor"),
    },
    Row {
        key: "default_apps.terminal",
        title: n_("Terminal"),
    },
    Row {
        key: "default_apps.mail",
        title: n_("Mail"),
    },
    // The Startup page (M6.10): the apps that start with the desktop.
    Row {
        key: "startup.apps",
        title: n_("Startup apps"),
    },
    // The Apps page (M6.14a): what each Flatpak app may use, one row for each
    // permission of `edel::app_permissions`.
    Row {
        key: "apps.camera",
        title: n_("Camera"),
    },
    Row {
        key: "apps.microphone",
        title: n_("Microphone"),
    },
    Row {
        key: "apps.location",
        title: n_("Location"),
    },
    Row {
        key: "apps.screenshots",
        title: n_("Screenshots"),
    },
    Row {
        key: "apps.notifications",
        title: n_("Notifications"),
    },
    Row {
        key: "apps.files",
        title: n_("Files"),
    },
    // The Add-ons page (M7.2b): one switch for each add-on the image was
    // built with, under the add-on's own name.
    Row {
        key: "addons.installed",
        title: n_("Add-ons"),
    },
    // The Updates page (M5.8c).
    Row {
        key: "updates.channel",
        title: n_("Channel"),
    },
    // The System page's developer mode (M7.1): a switch that changes the
    // machine's key as root, as the Region page's machine keys do.
    Row {
        key: "system.developer_mode",
        title: n_("Developer mode"),
    },
    // The Shortcuts page (M5.13b): one row per action of `edel::shortcuts`,
    // so search finds each by its name; the keys are the page's own.
    Row {
        key: "shortcuts.close_window",
        title: n_("Close window"),
    },
    Row {
        key: "shortcuts.minimize_window",
        title: n_("Minimize window"),
    },
    Row {
        key: "shortcuts.toggle_maximize",
        title: n_("Maximize window"),
    },
    Row {
        key: "shortcuts.toggle_fullscreen",
        title: n_("Fullscreen"),
    },
    Row {
        key: "shortcuts.switch_windows",
        title: n_("Switch windows"),
    },
    Row {
        key: "shortcuts.toggle_tiling",
        title: n_("Floating or tiling"),
    },
    Row {
        key: "shortcuts.choose_tiling_style",
        title: n_("Tiling styles menu"),
    },
    Row {
        key: "shortcuts.focus_window_left",
        title: n_("Focus window to the left"),
    },
    Row {
        key: "shortcuts.focus_window_right",
        title: n_("Focus window to the right"),
    },
    Row {
        key: "shortcuts.focus_window_up",
        title: n_("Focus window up"),
    },
    Row {
        key: "shortcuts.focus_window_down",
        title: n_("Focus window down"),
    },
    Row {
        key: "shortcuts.move_window_left",
        title: n_("Move window left"),
    },
    Row {
        key: "shortcuts.move_window_right",
        title: n_("Move window right"),
    },
    Row {
        key: "shortcuts.move_window_up",
        title: n_("Move window up"),
    },
    Row {
        key: "shortcuts.move_window_down",
        title: n_("Move window down"),
    },
    Row {
        key: "shortcuts.cycle_column_width",
        title: n_("Column width"),
    },
    Row {
        key: "shortcuts.open_launcher",
        title: n_("Open launcher"),
    },
    Row {
        key: "shortcuts.open_terminal",
        title: n_("Open terminal"),
    },
    Row {
        key: "shortcuts.open_files",
        title: n_("Open files"),
    },
    Row {
        key: "shortcuts.open_browser",
        title: n_("Open browser"),
    },
    Row {
        key: "shortcuts.take_screenshot",
        title: n_("Take screenshot"),
    },
    Row {
        key: "shortcuts.show_workspaces",
        title: n_("Workspace overview"),
    },
    Row {
        key: "shortcuts.next_workspace",
        title: n_("Next workspace"),
    },
    Row {
        key: "shortcuts.previous_workspace",
        title: n_("Previous workspace"),
    },
    Row {
        key: "shortcuts.move_to_next_workspace",
        title: n_("Move window to next workspace"),
    },
    Row {
        key: "shortcuts.move_to_previous_workspace",
        title: n_("Move window to previous workspace"),
    },
    Row {
        key: "shortcuts.go_to_workspace_1",
        title: n_("Workspace 1"),
    },
    Row {
        key: "shortcuts.go_to_workspace_2",
        title: n_("Workspace 2"),
    },
    Row {
        key: "shortcuts.go_to_workspace_3",
        title: n_("Workspace 3"),
    },
    Row {
        key: "shortcuts.go_to_workspace_4",
        title: n_("Workspace 4"),
    },
    Row {
        key: "shortcuts.go_to_workspace_5",
        title: n_("Workspace 5"),
    },
    Row {
        key: "shortcuts.go_to_workspace_6",
        title: n_("Workspace 6"),
    },
    Row {
        key: "shortcuts.go_to_workspace_7",
        title: n_("Workspace 7"),
    },
    Row {
        key: "shortcuts.go_to_workspace_8",
        title: n_("Workspace 8"),
    },
    Row {
        key: "shortcuts.go_to_workspace_9",
        title: n_("Workspace 9"),
    },
    Row {
        key: "shortcuts.move_to_workspace_1",
        title: n_("Move window to workspace 1"),
    },
    Row {
        key: "shortcuts.move_to_workspace_2",
        title: n_("Move window to workspace 2"),
    },
    Row {
        key: "shortcuts.move_to_workspace_3",
        title: n_("Move window to workspace 3"),
    },
    Row {
        key: "shortcuts.move_to_workspace_4",
        title: n_("Move window to workspace 4"),
    },
    Row {
        key: "shortcuts.move_to_workspace_5",
        title: n_("Move window to workspace 5"),
    },
    Row {
        key: "shortcuts.move_to_workspace_6",
        title: n_("Move window to workspace 6"),
    },
    Row {
        key: "shortcuts.move_to_workspace_7",
        title: n_("Move window to workspace 7"),
    },
    Row {
        key: "shortcuts.move_to_workspace_8",
        title: n_("Move window to workspace 8"),
    },
    Row {
        key: "shortcuts.move_to_workspace_9",
        title: n_("Move window to workspace 9"),
    },
    Row {
        key: "shortcuts.lock_screen",
        title: n_("Lock screen"),
    },
    Row {
        key: "shortcuts.next_keyboard_layout",
        title: n_("Next keyboard layout"),
    },
    Row {
        key: "shortcuts.toggle_game_mode",
        title: n_("Game mode on or off"),
    },
    Row {
        key: "shortcuts.show_tray",
        title: n_("Show tray"),
    },
    Row {
        key: "shortcuts.clipboard_history",
        title: n_("Clipboard history"),
    },
    // Whether the clipboard keeps what is copied (M7.13), beside its shortcut.
    Row {
        key: edel::settings::CLIPBOARD_HISTORY,
        title: n_("Keep clipboard history"),
    },
];

/// What the Panels group says under its heading (M5.31d).
pub const PANELS_WHAT: &str = n_("Which panels and docks show, where, and what they hold");

/// The key of the row `name` means, by its key or its title, whatever the
/// case (`panels.list` or `Panels`), if one does.
pub fn row_named(name: &str) -> Option<&'static str> {
    let name = name.trim().to_lowercase();
    ROWS.iter()
        .find(|r| r.key.to_lowercase() == name || title(r.key).to_lowercase() == name)
        .map(|r| r.key)
}

/// The title of `key`'s row, in the person's language.
pub fn title(key: &str) -> &'static str {
    ROWS.iter()
        .find(|r| r.key == key)
        .map_or("", |r| tr(r.title))
}

/// A key's row name in plain words, in lower case, for a sentence such as
/// the theme line's "Changed: accent": the row of the key, or of its family
/// (`appearance.colours.*` for `appearance.colours.panel`), else the key's
/// last part with spaces for underscores.
pub fn plain(key: &str) -> String {
    let family = |row: &&Row| {
        row.key
            .strip_suffix(".*")
            .is_some_and(|prefix| key.starts_with(&format!("{prefix}.")))
    };
    match ROWS
        .iter()
        .find(|r| r.key == key)
        .or_else(|| ROWS.iter().find(family))
    {
        Some(row) => tr(row.title).to_lowercase(),
        None => key.rsplit('.').next().unwrap_or(key).replace('_', " "),
    }
}

/// The rows on the page for `section`, such as `layout`, which search
/// looks through as well as the pages' titles.
pub fn on_page(section: &str) -> impl Iterator<Item = &'static Row> {
    ROWS.iter()
        .filter(move |r| edel::settings::page_of(r.key).is_some_and(|page| page.section == section))
}

/// The values `key` may take, as the key table lists them, so a row's
/// choices are always the ones `edel settings set` takes.
pub fn values(key: &str) -> &'static [&'static str] {
    edel::settings::choices(key)
}

/// A value as a row shows it: `floating-only` is Floating only, in the
/// person's language for the values the key table lists.
pub fn label(value: &str) -> String {
    let word = match value {
        "always" => n_("Always"),
        "floating-only" => n_("Floating only"),
        "never" => n_("Never"),
        "left" => n_("Left"),
        "right" => n_("Right"),
        "top" => n_("Top"),
        "bottom" => n_("Bottom"),
        "numbers" => n_("Numbers"),
        "button" => n_("Button"),
        "fade" => n_("Fade"),
        "arrows" => n_("Arrows"),
        "counts" => n_("Counts"),
        "light" => n_("Light"),
        "dark" => n_("Dark"),
        "auto" => n_("Automatic"),
        "stack" => n_("Stack"),
        "split" => n_("Split"),
        "scroll" => n_("Scroll"),
        "dot" => n_("Dot"),
        "count" => n_("Count"),
        "none" => n_("None"),
        "poweroff" => n_("Shut down"),
        "nothing" => n_("Do nothing"),
        "ask" => n_("Ask"),
        "lock" => n_("Lock the screen"),
        "power-saver" => n_("Power saver"),
        "plugged-in" => n_("When plugged in"),
        _ => return crate::files::title(&value.replace('-', " ")),
    };
    tr(word).to_string()
}

/// What a row says under its title about where its value comes from:
/// the person's own choice, the machine's, or `Automatic` and the value
/// the release gives it.
pub fn describe(source: &Source, current: &str) -> String {
    match source {
        Source::Person(_) => tr("Your choice").to_string(),
        Source::Machine(_) => tr("Set by this machine").to_string(),
        Source::Release => trf("Automatic ({current})", &[("current", current)]),
    }
}

/// What a row's line adds about where its value comes from, as the
/// mockup's rows say it: nothing when the release decides, `From the
/// preset` when the preset does, `Your choice` or `Set by this machine`.
pub fn note(source: &Source, from_preset: bool) -> Option<&'static str> {
    match source {
        Source::Person(_) => Some(tr("Your choice")),
        Source::Machine(_) => Some(tr("Set by this machine")),
        Source::Release if from_preset => Some(tr("From the preset")),
        Source::Release => None,
    }
}

/// The command that sets each of `pairs` on any machine, as Copy as
/// command gives it (ADR-008: every setting is a row, a command and a
/// line of the file); `edel settings set` takes several `KEY=VALUE` at
/// once, for a row of several keys.
pub fn command(pairs: &[(&str, String)]) -> String {
    let assignments: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", quoted(v)))
        .collect();
    format!("edel settings set {}", assignments.join(" "))
}

/// `value` as a shell takes it: as it is when it holds only letters,
/// digits and `. , : / @ % + -`, else in single quotes, so a pair such as
/// `[0, 0]` is one word and nothing in it is a glob.
fn quoted(value: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || ".,:/@%+-_".contains(c);
    if !value.is_empty() && value.chars().all(plain) {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// `Version 2026.10.5 · Stable`, or what is known of it: the version alone,
/// the channel alone, or nothing; one line for the Updates page's card and
/// About's.
pub fn version_line(version: Option<&str>, channel: Option<&str>) -> String {
    match (version, channel) {
        (Some(v), Some(c)) => trf(
            "Version {version} · {channel}",
            &[("version", v), ("channel", c)],
        ),
        (Some(v), None) => trf("Version {version}", &[("version", v)]),
        (None, Some(c)) => c.to_string(),
        (None, None) => String::new(),
    }
}

/// A size as people read it: `412 MB`, `1.4 GB`. Decimal units, as the
/// stores and the disk makers use, so a size here matches the one on the box.
pub fn bytes_text(bytes: u64) -> String {
    const MB: f64 = 1_000_000.0;
    let mb = bytes as f64 / MB;
    if mb < 1000.0 {
        trf("{n} MB", &[("n", &format!("{:.0}", mb.max(1.0)))])
    } else if mb < 10_000.0 {
        trf("{n} GB", &[("n", &format!("{:.1}", mb / 1000.0))])
    } else {
        trf("{n} GB", &[("n", &format!("{:.0}", mb / 1000.0))])
    }
}

/// Whether the row can be reset: only the person's own value can, as
/// Settings writes only the person's file.
pub fn resettable(source: &Source) -> bool {
    matches!(source, Source::Person(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::settings::{self, KEYS, Source};
    use toml::Value;

    /// The keys Settings has no row of their own for, each with why (M6.11):
    /// another control writes them whole, or the command line and the file
    /// are the ways to change them. A key a later step brings (`supported:
    /// false`) needs neither, as no page shows a row that does not work yet,
    /// and a retired key needs neither either.
    const NO_ROW: &[(&str, &str)] = &[
        (
            "users.*.ssh_keys",
            "a key is pasted on the command line or written in the file, as a server's administrator does",
        ),
        (
            "users.*.login_shell",
            "the shell a terminal starts, for people who use one; set in the file or with edel settings set",
        ),
        (
            "network.hostname",
            "the machine's key, and Settings writes only the person's file; About shows the name",
        ),
        (
            "displays.*.refresh_rate",
            "the compositor's state file lists modes without their rates, so the Displays page cannot offer them; a screen runs at its mode's best rate",
        ),
        (
            "lock_screen.dim",
            "changed in the lock screen's own editor, which the Lock screen page's Customize opens",
        ),
        (
            "lock_screen.looks",
            "a table the Lock screen page's gallery writes whole (New look, Rename, Delete), so a row's Undo would rewrite it",
        ),
        (
            "themes.*",
            "tables the Appearance page's theme card writes whole (Keep as new, Rename, Delete, Import), as edel theme does",
        ),
        (
            "presets.*",
            "tables the Layout page writes whole (Save as preset, Delete), as edel preset does",
        ),
        (
            "services.*",
            "the machine's switches for its features' services, which an administrator changes as root with edel settings set services.NAME=false",
        ),
        (
            "apps.installed",
            "the machine's list of apps installed for everyone, which an administrator changes as root with edel settings set apps.installed+=ID",
        ),
        (
            "install.disk",
            "an instruction to a stick that installs itself, read only by edel install --unattended (docs/FLEET.md)",
        ),
        (
            "install.erase",
            "an instruction to a stick that installs itself, read only by edel install --unattended (docs/FLEET.md)",
        ),
        (
            "install.then",
            "an instruction to a stick that installs itself, read only by edel install --unattended (docs/FLEET.md)",
        ),
        (
            "system.profiles_without",
            "written by taking out or adding back what a profile brings, on the Add-ons page or with -= and +=; the Profile row's line names it",
        ),
    ];

    #[test]
    fn every_row_is_a_key_this_release_supports() {
        for row in ROWS {
            // A family key such as `shortcuts.*` holds each of its rows.
            let same_shape = |pattern: &str| {
                let (p, k): (Vec<&str>, Vec<&str>) =
                    (pattern.split('.').collect(), row.key.split('.').collect());
                p.len() == k.len() && p.iter().zip(&k).all(|(p, k)| *p == "*" || p == k)
            };
            let key = KEYS
                .iter()
                .find(|k| k.path == row.key)
                .or_else(|| KEYS.iter().find(|k| same_shape(k.path)))
                .unwrap_or_else(|| panic!("{} is not a settings file key", row.key));
            assert!(key.supported, "{} is not supported yet", row.key);
        }
    }

    #[test]
    fn a_row_offers_the_values_the_key_table_lists() {
        assert_eq!(
            values("layout.title_bars"),
            ["always", "floating-only", "never"]
        );
        assert_eq!(values("layout.window_buttons"), ["left", "right"]);
        assert_eq!(label("floating-only"), "Floating only");
        assert_eq!(label("never"), "Never");
        let titles: Vec<&str> = on_page("layout").map(|r| r.title).collect();
        assert_eq!(titles.len(), 8);
        assert_eq!(on_page("panels").count(), 3);
        assert_eq!(on_page("workspaces").count(), 7);
        assert_eq!(
            values("workspaces.overview_strip"),
            ["left", "right", "top", "bottom"]
        );
        assert_eq!(label("top"), "Top");
        assert_eq!(on_page("appearance").count(), 35);
        assert_eq!(values("layout.tiling_style"), ["stack", "split", "scroll"]);
        assert_eq!(on_page("about").count(), 0);
    }

    #[test]
    fn a_row_says_where_its_value_comes_from() {
        assert_eq!(describe(&Source::Release, "Classic"), "Automatic (Classic)");
        let set = Source::Machine(Value::String("hive".into()));
        assert_eq!(describe(&set, "Hive"), "Set by this machine");
        assert!(!resettable(&set));
        let own = Source::Person(Value::Boolean(true));
        assert_eq!(describe(&own, "on"), "Your choice");
        assert!(resettable(&own));
        assert_eq!(note(&Source::Release, true), Some("From the preset"));
        assert_eq!(note(&Source::Release, false), None);
        assert_eq!(note(&own, true), Some("Your choice"));
        assert_eq!(note(&set, false), Some("Set by this machine"));
    }

    #[test]
    fn the_version_line_says_what_is_known() {
        assert_eq!(
            version_line(Some("2026.10.5"), Some("Stable")),
            "Version 2026.10.5 · Stable"
        );
        assert_eq!(version_line(Some("2026.10.5"), None), "Version 2026.10.5");
        assert_eq!(version_line(None, Some("Stable")), "Stable");
        assert_eq!(version_line(None, None), "");
    }

    #[test]
    fn sizes_read_in_megabytes_then_gigabytes() {
        assert_eq!(bytes_text(412_000_000), "412 MB");
        assert_eq!(bytes_text(1_400_000_000), "1.4 GB");
        assert_eq!(bytes_text(128_000_000_000), "128 GB");
        assert_eq!(bytes_text(200), "1 MB");
    }

    #[test]
    fn a_value_with_spaces_is_one_word_for_the_shell() {
        let line = command(&[("displays.eDP-1.position", "[1280, 0]".into())]);
        assert_eq!(
            line,
            "edel settings set displays.eDP-1.position='[1280, 0]'"
        );
        assert_eq!(quoted("2.5"), "2.5");
        assert_eq!(quoted("1920x1080"), "1920x1080");
        assert_eq!(quoted("it's"), "'it'\\''s'");
        assert_eq!(quoted(""), "''");
    }

    #[test]
    fn the_displays_rows_are_found_by_search_and_belong_to_the_page() {
        let titles: Vec<&str> = on_page("displays").map(|r| r.title).collect();
        assert_eq!(titles, ["Resolution", "Scale", "Position", "On"]);
        assert_eq!(title("displays.*.scale"), "Scale");
    }

    #[test]
    fn the_sound_page_has_its_one_row() {
        let titles: Vec<&str> = on_page("sound").map(|r| r.title).collect();
        assert_eq!(titles, ["Low latency"]);
    }

    #[test]
    fn the_pinned_apps_row_is_titled_as_the_panels_group_says() {
        assert_eq!(title("panels.pinned"), "Pinned apps");
        assert_eq!(row_named("Pinned apps"), Some("panels.pinned"));
    }

    #[test]
    fn the_panels_row_is_found_by_its_key_or_its_title() {
        assert_eq!(row_named("panels.list"), Some("panels.list"));
        assert_eq!(row_named("Panels"), Some("panels.list"));
        assert_eq!(row_named("PANELS"), Some("panels.list"));
        assert_eq!(row_named("panel"), None, "a start is not a row");
        assert_eq!(row_named("scale"), Some("displays.*.scale"));
        assert_eq!(title("panels.list"), "Panels");
    }

    #[test]
    fn copy_as_command_is_the_line_edel_settings_takes() {
        let line = command(&[("layout.preset", "mac-like".into())]);
        assert_eq!(line, "edel settings set layout.preset=mac-like");
        let (key, value) = line
            .strip_prefix("edel settings set ")
            .and_then(|a| a.split_once('='))
            .unwrap();
        assert!(settings::set("format = 1\n", key, value).is_ok());
        let both = command(&[
            ("layout.close_button", "true".into()),
            ("layout.minimize_button", "false".into()),
        ]);
        assert_eq!(
            both,
            "edel settings set layout.close_button=true layout.minimize_button=false"
        );
    }

    /// Whether `pattern` (a key, or a family such as `shortcuts.*`) holds
    /// `key`, a `*` in either matching any one part.
    fn holds(pattern: &str, key: &str) -> bool {
        let (p, k): (Vec<&str>, Vec<&str>) =
            (pattern.split('.').collect(), key.split('.').collect());
        p.len() == k.len()
            && p.iter()
                .zip(&k)
                .all(|(p, k)| *p == "*" || *k == "*" || p == k)
    }

    #[test]
    fn every_key_has_a_row_or_says_why_not() {
        let missing: Vec<&str> = KEYS
            .iter()
            .filter(|key| key.supported)
            .filter(|key| edel::key_help::find(key.path).is_none_or(|line| line.retired.is_none()))
            .filter(|key| !ROWS.iter().any(|row| holds(row.key, key.path)))
            .filter(|key| !NO_ROW.iter().any(|(k, _)| *k == key.path))
            .map(|key| key.path)
            .collect();
        assert!(
            missing.is_empty(),
            "these keys have no row in rows.rs and are not in NO_ROW with a reason: {missing:?}"
        );
    }

    #[test]
    fn no_row_names_only_keys_without_a_row_and_says_why() {
        for (key, why) in NO_ROW {
            let entry = KEYS
                .iter()
                .find(|k| k.path == *key)
                .unwrap_or_else(|| panic!("{key} in NO_ROW is not a settings file key"));
            assert!(
                entry.supported,
                "{key} in NO_ROW is not supported yet, so it needs no reason"
            );
            assert!(
                !ROWS.iter().any(|row| holds(row.key, key)),
                "{key} is in NO_ROW but has a row"
            );
            assert!(why.len() > 20, "{key}'s reason says too little: {why:?}");
            assert_eq!(
                NO_ROW.iter().filter(|(k, _)| k == key).count(),
                1,
                "{key} twice"
            );
        }
    }
}
