//! Layout presets (ADR-002, M5.1c): each one file under `presets/`, built
//! into the library, so the compositor, shell-ui and Settings read the same
//! preset with the same code. A preset gives the values the settings file
//! leaves out; a key the settings file sets wins (ADR-008). Presets are part
//! of the release, never written on a machine, so [`check`] reads them
//! strictly and the tests check every one; a name this release does not
//! have (a preset a later release dropped, or a typo in a hand-edited
//! file) gives Classic, with a note.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::i18n::{n_, tr, trf};

/// The only preset format so far.
pub const FORMAT: i64 = 1;

/// The presets this release has, by name, the default first, in the order
/// Settings shows them and messages list them (ADR-002's: Classic,
/// Mac-like, Windows-like, Hive, then Zen, Tablet and Phone as they
/// come). A new one is a file under `presets/`, a line here and its name
/// in [`NAMES`], in the same order; its name must be in
/// [`crate::settings::PRESETS`].
pub const BUILT_IN: &[(&str, &str)] = &[
    ("classic", include_str!("../../../presets/classic.toml")),
    ("mac-like", include_str!("../../../presets/mac-like.toml")),
    (
        "windows-like",
        include_str!("../../../presets/windows-like.toml"),
    ),
    ("hive", include_str!("../../../presets/hive.toml")),
];

/// The names in [`BUILT_IN`]: what `layout.preset` may be on this release,
/// so `edel settings check` and `set` refuse any other and list these.
pub const NAMES: &[&str] = &["classic", "mac-like", "windows-like", "hive"];

/// The preset a missing `layout.preset` means.
pub const DEFAULT: &str = "classic";

/// The most workspaces a preset can ask for: one for each of Super+1 to
/// Super+9.
pub const MOST_WORKSPACES: usize = 9;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub format: i64,
    pub windows: Windows,
    pub workspaces: Workspaces,
    pub launcher: Launcher,
    /// The apps the apps widget pins (M5.4c).
    #[serde(default)]
    pub apps: Apps,
    #[serde(default)]
    pub panels: Vec<Panel>,
    /// What the compositor starts with the session (M5.7b).
    #[serde(default)]
    pub session: Session,
}

/// The programs a person's session starts (M5.7b): the sound system, so
/// that apps and Settings find it running, each as a command, a program
/// found on the `PATH` or an absolute path, with its arguments after it.
/// The compositor starts them one after the other, once, after shell-ui;
/// one that keeps failing is given up on, as its log says. A preset that
/// says nothing starts [`SESSION_START`], so the list is written once and
/// a preset names its own only to start others (a phone's, say).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    #[serde(default = "session_start")]
    pub start: Vec<String>,
}

impl Default for Session {
    fn default() -> Session {
        Session {
            start: session_start(),
        }
    }
}

/// What a session starts when its preset names nothing: PipeWire, which
/// also serves the PulseAudio apps speak (the `sound` feature loads that
/// server inside it), then WirePlumber, which manages its devices (M5.7b).
pub const SESSION_START: &[&str] = &["pipewire", "wireplumber"];

fn session_start() -> Vec<String> {
    SESSION_START.iter().map(|c| c.to_string()).collect()
}

/// The apps widget's pinned apps (M5.4c), from its start: each a role
/// (`files`, `browser`, `mail`, `editor`, `terminal`, `music`,
/// `settings`), meaning the installed app for it, or a desktop file id;
/// those this machine lacks are left out.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apps {
    #[serde(default)]
    pub pinned: Vec<String>,
}

/// The launcher (M5.3b), which Super or the menu button opens.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launcher {
    pub style: LauncherStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LauncherStyle {
    /// A search line over a list of apps, opening beside the panel's
    /// start, where the menu button is, as Cinnamon's menu. A full-screen
    /// grid joins with the preset that first asks for it (M5.4).
    Menu,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Windows {
    /// The policy a workspace starts in when `layout.tiling` is not set.
    pub policy: Policy,
    /// The side of the title bar the buttons sit on when
    /// `layout.window_buttons` is not set (M5.4b); absent is the right.
    #[serde(default)]
    pub buttons: Side,
}

/// A side of the title bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    #[default]
    Right,
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }

    /// `left` or `right`, as `layout.window_buttons` writes it; anything
    /// else is none.
    pub fn parse(name: &str) -> Option<Side> {
        match name {
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            _ => None,
        }
    }
}

/// The workspace model (M5.2a): a fixed number of workspaces. A dynamic
/// model, one more workspace whenever the last fills, joins with the
/// preset that first asks for it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspaces {
    /// How many, 1 to [`MOST_WORKSPACES`].
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    Floating,
    Tiling,
}

impl Policy {
    pub fn name(self) -> &'static str {
        match self {
            Policy::Floating => "floating",
            Policy::Tiling => "tiling",
        }
    }
}

/// One panel: a layer-shell surface along a screen's edge holding
/// shell-ui's widgets, by name, from its start, in its centre and towards
/// its end.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    pub edge: Edge,
    /// A bar along the whole edge, or a dock (M5.4d).
    #[serde(default, skip_serializing_if = "Style::is_bar")]
    pub style: Style,
    /// Whether a dock hides while a window covers it (M5.4f).
    #[serde(default, skip_serializing_if = "Hide::is_never")]
    pub hide: Hide,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub start: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub centre: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub end: Vec<String>,
}

impl Panel {
    /// Every widget the panel names, start to end.
    pub fn widgets(&self) -> impl Iterator<Item = &str> {
        self.start
            .iter()
            .chain(&self.centre)
            .chain(&self.end)
            .map(String::as_str)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Top,
    Bottom,
}

impl Edge {
    pub fn name(self) -> &'static str {
        match self {
            Edge::Top => "top",
            Edge::Bottom => "bottom",
        }
    }
}

/// What a panel looks like (M5.4d): a bar along the whole edge, keeping
/// that much of the screen free of windows, or a dock, a card as wide as
/// what it holds, centred along the edge a little away from it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    #[default]
    Bar,
    Dock,
}

impl Style {
    /// Whether it is the default, which a written panel leaves out.
    pub fn is_bar(&self) -> bool {
        *self == Style::Bar
    }
}

/// When a dock steps aside (M5.4f): never, keeping its height free of
/// windows, or while a window covers it, keeping nothing free; the
/// compositor then stops drawing it until the pointer reaches its edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Hide {
    #[default]
    Never,
    Covered,
}

impl Hide {
    /// Whether it is the default, which a written panel leaves out.
    pub fn is_never(&self) -> bool {
        *self == Hide::Never
    }
}

/// Reads a preset strictly: the format this release knows, every key
/// known, 1 to [`MOST_WORKSPACES`] workspaces, at most one panel along
/// each edge, widget names that could be names. Whether shell-ui has each
/// widget is its own test's to say.
pub fn check(text: &str) -> Result<Preset> {
    let preset: Preset = toml::from_str(text).context("parsing the preset")?;
    if preset.format != FORMAT {
        bail!(
            "format {} is not one this release reads; it reads {FORMAT}",
            preset.format
        );
    }
    if !(1..=MOST_WORKSPACES).contains(&preset.workspaces.count) {
        bail!(
            "{} workspaces is not 1 to {MOST_WORKSPACES}",
            preset.workspaces.count
        );
    }
    check_panels(&preset.panels)?;
    for command in &preset.session.start {
        if command.trim().is_empty() || command.chars().any(char::is_control) {
            bail!(
                "{}",
                trf(
                    "{command} is not a command the session can start; write a program and its arguments on one line",
                    &[("command", &format!("{command:?}"))]
                )
            );
        }
    }
    Ok(preset)
}

/// What a preset's panels, or `[[layout.panels]]` in a settings file
/// (M5.4e), must be: at most one along each edge, holding widget names
/// that could be names.
pub fn check_panels(panels: &[Panel]) -> Result<()> {
    for (i, panel) in panels.iter().enumerate() {
        if panels[..i].iter().any(|p| p.edge == panel.edge) {
            bail!(
                "{}",
                trf(
                    "two panels along the {edge} edge",
                    &[("edge", panel.edge.name())]
                )
            );
        }
        if let Some(bad) = panel.widgets().find(|w| !crate::features::is_name(w)) {
            bail!(
                "{}",
                trf(
                    "{widget} is not a widget name",
                    &[("widget", &format!("{bad:?}"))]
                )
            );
        }
        if panel.hide != Hide::Never && panel.style != Style::Dock {
            bail!(
                "{}",
                trf(
                    "only a dock hides; the {edge} panel is a bar",
                    &[("edge", panel.edge.name())]
                )
            );
        }
    }
    Ok(())
}

/// Whether any of `panels` is a dock that hides while a window covers it.
pub fn dock_hides(panels: &[Panel]) -> bool {
    panels
        .iter()
        .any(|p| p.style == Style::Dock && p.hide == Hide::Covered)
}

/// A preset's name as people read it, in their language: `mac-like` is
/// Mac-like. A person's own preset (`[presets.NAME]`) is its name with a
/// capital. These words are `edel`'s to translate, as the Settings app
/// shows them too (`po/edel.pot` owns them).
pub fn title(name: &str) -> String {
    match name {
        "classic" => tr(n_("Classic")).to_string(),
        "mac-like" => tr(n_("Mac-like")).to_string(),
        "windows-like" => tr(n_("Windows-like")).to_string(),
        "hive" => tr(n_("Hive")).to_string(),
        _ => {
            let mut chars = name.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    }
}

/// The preset `name` names, else Classic and a note saying why. A missing
/// name is Classic without a note.
pub fn named(name: Option<&str>) -> (Preset, Option<String>) {
    let wanted = name.unwrap_or(DEFAULT);
    let found = BUILT_IN.iter().find(|(n, _)| *n == wanted);
    let note = found.is_none().then(|| {
        trf(
            "this release has no preset {name}; the Classic preset is used",
            &[("name", &format!("{wanted:?}"))],
        )
    });
    let (_, text) = found.unwrap_or(&BUILT_IN[0]);
    // Every built-in preset is checked by the tests.
    let preset = check(text).expect("the built-in presets are checked by the tests");
    (preset, note)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_preset_is_valid_and_known_to_the_system_file() {
        assert_eq!(BUILT_IN[0].0, DEFAULT, "the default comes first");
        let names: Vec<&str> = BUILT_IN.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, NAMES, "NAMES lists BUILT_IN's names in order");
        for (name, text) in BUILT_IN {
            check(text).unwrap_or_else(|e| panic!("presets/{name}.toml: {e:#}"));
            assert!(
                crate::settings::PRESETS.contains(name),
                "{name} is not in settings::PRESETS, so layout.preset could not name it"
            );
        }
    }

    #[test]
    fn a_session_starts_the_sound_system_unless_its_preset_says_otherwise() {
        // No preset names its own: the list is written once, here.
        for (name, text) in BUILT_IN {
            assert!(
                !text.lines().any(|l| l.starts_with("[session]")),
                "{name} repeats the default"
            );
            assert_eq!(named(Some(name)).0.session.start, SESSION_START);
        }
        assert_eq!(
            SESSION_START,
            ["pipewire", "wireplumber"],
            "PipeWire first: the others connect to it"
        );
        let classic = BUILT_IN[0].1;
        let own = format!("{classic}\n[session]\nstart = [\"pipewire\", \"foot --server\"]\n");
        assert_eq!(
            check(&own).unwrap().session.start,
            ["pipewire", "foot --server"]
        );
        // An empty list starts nothing, for a preset that wants no sound.
        let none = format!("{classic}\n[session]\nstart = []\n");
        assert!(check(&none).unwrap().session.start.is_empty());
        for bad in ["\"\"", "\"  \"", "\"a\\nb\""] {
            let text = format!("{classic}\n[session]\nstart = [{bad}]\n");
            let e = format!("{:#}", check(&text).unwrap_err());
            assert!(e.contains("is not a command the session can start"), "{e}");
        }
        let text = format!("{classic}\n[session]\nrestart = true\n");
        assert!(format!("{:#}", check(&text).unwrap_err()).contains("unknown field"));
    }

    #[test]
    fn classic_floats_on_four_workspaces_with_one_panel_along_the_bottom() {
        let (classic, note) = named(None);
        assert_eq!(note, None);
        assert_eq!(classic.windows.policy, Policy::Floating);
        assert_eq!(classic.workspaces.count, 4);
        assert_eq!(classic.panels.len(), 1);
        let panel = &classic.panels[0];
        assert_eq!(panel.edge, Edge::Bottom);
        assert_eq!(
            panel.widgets().collect::<Vec<_>>(),
            [
                "menu",
                "separator",
                "windows",
                "workspaces",
                "layout",
                "separator",
                "tray",
                "clock"
            ]
        );
    }

    #[test]
    fn windows_like_has_a_taskbar_with_search_and_the_apps_in_its_centre() {
        let (windows, note) = named(Some("windows-like"));
        assert_eq!(note, None);
        assert_eq!(windows.windows.policy, Policy::Floating);
        assert_eq!(windows.windows.buttons, Side::Right);
        assert_eq!(
            windows.apps.pinned,
            ["files", "browser", "terminal", "mail", "music"]
        );
        let panel = &windows.panels[0];
        assert_eq!(panel.edge, Edge::Bottom);
        assert_eq!(panel.start, ["menu", "search"]);
        assert_eq!(panel.centre, ["apps"]);
        // Classic and Hive pin nothing: they have no apps widget.
        assert!(named(None).0.apps.pinned.is_empty());
    }

    #[test]
    fn mac_like_has_a_bar_on_top_a_dock_below_and_buttons_on_the_left() {
        let (mac, note) = named(Some("mac-like"));
        assert_eq!(note, None);
        assert_eq!(mac.windows.buttons, Side::Left);
        let edges: Vec<(Edge, Style)> = mac.panels.iter().map(|p| (p.edge, p.style)).collect();
        assert_eq!(
            edges,
            [(Edge::Top, Style::Bar), (Edge::Bottom, Style::Dock)]
        );
        assert_eq!(mac.panels[1].centre, ["apps"]);
        // A panel names its style only when it is a dock.
        assert!(named(None).0.panels.iter().all(|p| p.style == Style::Bar));
    }

    #[test]
    fn an_unknown_name_is_classic_with_a_note() {
        let (preset, note) = named(Some("cinnamon"));
        assert_eq!(preset, named(None).0);
        assert!(note.unwrap().contains("no preset \"cinnamon\""));
    }

    #[test]
    fn check_refuses_what_this_release_does_not_read() {
        let classic = BUILT_IN[0].1;
        let refused = |text: &str, says: &str| {
            let e = format!("{:#}", check(text).unwrap_err());
            assert!(e.contains(says), "{e}");
        };
        refused(
            &classic.replace("format = 1", "format = 2"),
            "format 2 is not one this release reads",
        );
        refused(
            &classic.replace("[windows]", "[windows]\ncorners = \"round\""),
            "unknown field `corners`",
        );
        refused(
            &classic.replace("policy = \"floating\"", "policy = \"stacking\""),
            "unknown variant `stacking`",
        );
        refused(
            &classic.replace("count = 4", "count = 10"),
            "10 workspaces is not 1 to 9",
        );
        refused(
            &classic.replace("count = 4", "count = 0"),
            "0 workspaces is not 1 to 9",
        );
        refused(
            &format!("{classic}\n[[panels]]\nedge = \"bottom\"\n"),
            "two panels along the bottom edge",
        );
        refused(
            &classic.replace("edge = \"bottom\"", "edge = \"bottom\"\nhide = \"covered\""),
            "only a dock hides; the bottom panel is a bar",
        );
        let hiding = classic.replace(
            "edge = \"bottom\"",
            "edge = \"bottom\"\nstyle = \"dock\"\nhide = \"covered\"",
        );
        assert!(dock_hides(&check(&hiding).unwrap().panels));
        assert!(!dock_hides(&check(classic).unwrap().panels));
        refused(
            &classic.replace("\"clock\"", "\"Clock Widget\""),
            "\"Clock Widget\" is not a widget name",
        );
    }
}
