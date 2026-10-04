//! Layout presets (ADR-002, M5.1c): each one file under `presets/`, built
//! into the library, so the compositor, shell-ui and Settings read the same
//! preset with the same code. A preset gives the values the system file
//! leaves out; a key the system file sets wins (ADR-008). Presets are part
//! of the release, never written on a machine, so [`check`] reads them
//! strictly and the tests check every one; a name this release does not
//! have (a preset a later release dropped, or a typo in a hand-edited
//! file) gives Classic, with a note.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// The only preset format so far.
pub const FORMAT: i64 = 1;

/// The presets this release has, by name, the default first. A new one is
/// a file under `presets/`, a line here and its name in [`NAMES`], in the
/// same order; its name must be in [`crate::system::PRESETS`].
pub const BUILT_IN: &[(&str, &str)] = &[
    ("classic", include_str!("../../../presets/classic.toml")),
    ("hive", include_str!("../../../presets/hive.toml")),
    ("mac-like", include_str!("../../../presets/mac-like.toml")),
    (
        "windows-like",
        include_str!("../../../presets/windows-like.toml"),
    ),
];

/// The names in [`BUILT_IN`]: what `shell.preset` may be on this release,
/// so `edel system check` and `set` refuse any other and list these.
pub const NAMES: &[&str] = &["classic", "hive", "mac-like", "windows-like"];

/// The preset a missing `shell.preset` means.
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
    /// The policy a workspace starts in when `shell.tiling` is not set.
    pub policy: Policy,
    /// The side of the title bar the buttons sit on when
    /// `shell.window_buttons` is not set (M5.4b); absent is the right.
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

    /// `left` or `right`, as `shell.window_buttons` writes it; anything
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
    Ok(preset)
}

/// What a preset's panels, or `[[shell.panels]]` in a system file
/// (M5.4e), must be: at most one along each edge, holding widget names
/// that could be names.
pub fn check_panels(panels: &[Panel]) -> Result<()> {
    for (i, panel) in panels.iter().enumerate() {
        if panels[..i].iter().any(|p| p.edge == panel.edge) {
            bail!("two panels along the {} edge", panel.edge.name());
        }
        if let Some(bad) = panel.widgets().find(|w| !crate::features::is_name(w)) {
            bail!("{bad:?} is not a widget name");
        }
    }
    Ok(())
}

/// The preset `name` names, else Classic and a note saying why. A missing
/// name is Classic without a note.
pub fn named(name: Option<&str>) -> (Preset, Option<String>) {
    let wanted = name.unwrap_or(DEFAULT);
    let found = BUILT_IN.iter().find(|(n, _)| *n == wanted);
    let note = found
        .is_none()
        .then(|| format!("this release has no preset {wanted:?}; the Classic preset is used"));
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
                crate::system::PRESETS.contains(name),
                "{name} is not in system::PRESETS, so shell.preset could not name it"
            );
        }
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
            ["menu", "windows", "workspaces", "layout", "clock"]
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
            &classic.replace("\"clock\"", "\"Clock Widget\""),
            "\"Clock Widget\" is not a widget name",
        );
    }
}
