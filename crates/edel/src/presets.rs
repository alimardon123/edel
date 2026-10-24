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
use toml::{Table, Value};

use crate::i18n::{n_, tr, trf};
use crate::settings::{self, Layout, OwnPreset, SettingsFile};
use crate::themes;

/// The only preset format so far.
pub const FORMAT: i64 = 1;

/// The presets this release has, by name, the default first, in the order
/// Settings shows them and messages list them (ADR-002's: Classic,
/// Mac-like, Windows-like, Hive, Zen, then Tablet and Phone as they
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
    ("zen", include_str!("../../../presets/zen.toml")),
];

/// The names in [`BUILT_IN`]: what `layout.preset` may be on this release,
/// so `edel settings check` and `set` refuse any other and list these.
pub const NAMES: &[&str] = &["classic", "mac-like", "windows-like", "hive", "zen"];

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
    /// The tiles of the panel's quick settings (M5.9a).
    #[serde(default)]
    pub quick: Quick,
}

/// The tiles quick settings can hold (M5.9a), the names a preset's
/// `[quick] tiles` may list; shell-ui has one tile for each, and its tests
/// fail when this list and its tiles differ. A new tile is a name here and
/// a tile there.
pub const TILES: &[&str] = &[
    "wifi",
    "bluetooth",
    "airplane",
    "do_not_disturb",
    "location",
    "dark_style",
    "battery_saver",
];

/// The tiles a preset that names none shows, in this order: the seven
/// every built-in preset shows (Location only where its feature is there,
/// Battery saver only where a battery is).
pub const DEFAULT_TILES: &[&str] = &[
    "wifi",
    "bluetooth",
    "airplane",
    "do_not_disturb",
    "location",
    "dark_style",
    "battery_saver",
];

/// Quick settings (M5.9a): the tiles a preset shows, in two columns, in
/// this order. A tile whose feature or hardware this machine lacks is left
/// out by shell-ui, not here. A person's own list, a key beside
/// `panels.list`, comes with M5.31's editor.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quick {
    #[serde(default = "default_tiles")]
    pub tiles: Vec<String>,
}

impl Default for Quick {
    fn default() -> Quick {
        Quick {
            tiles: default_tiles(),
        }
    }
}

fn default_tiles() -> Vec<String> {
    DEFAULT_TILES.iter().map(|t| t.to_string()).collect()
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
    /// Whether windows get title bars when `layout.title_bars` is not set
    /// (M5.18b); absent is always.
    #[serde(default)]
    pub title_bars: Option<TitleBars>,
    /// The style tiling starts in when `layout.tiling_style` is not set
    /// (M5.18b), one of the key's values, as `layout.tiling_style` names
    /// them; absent is stack. Checked against the key table by [`check`].
    #[serde(default)]
    pub tiling_style: Option<String>,
}

/// Whether windows get the compositor's title bars (M5.18), which
/// `layout.title_bars` and a preset's `[windows] title_bars` name: always,
/// in floating and tiling; floating only, as before; or never, which
/// Zen uses. Its names are the key table's values (`settings::KEYS`), and a
/// test holds the two to each other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum TitleBars {
    #[default]
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "floating-only")]
    FloatingOnly,
    #[serde(rename = "never")]
    Never,
}

impl TitleBars {
    /// Every value, in the key table's order.
    pub const ALL: [TitleBars; 3] = [TitleBars::Always, TitleBars::FloatingOnly, TitleBars::Never];

    /// The value as the settings file writes it.
    pub fn name(self) -> &'static str {
        match self {
            TitleBars::Always => "always",
            TitleBars::FloatingOnly => "floating-only",
            TitleBars::Never => "never",
        }
    }

    /// The value a name is, `always`, `floating-only` or `never`; anything
    /// else is none.
    pub fn parse(name: &str) -> Option<TitleBars> {
        TitleBars::ALL.into_iter().find(|t| t.name() == name)
    }
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

/// The title bars' buttons as the files and the preset give them (M6.16):
/// the side they sit on and which of the three are shown. The compositor
/// draws its bars with it and shell-ui tells apps' own title bars the same
/// rule, through [`gnome_button_layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Buttons {
    pub side: Side,
    pub close: bool,
    pub minimize: bool,
    pub maximize: bool,
}

/// The buttons the person's file over the machine's give (M6.16):
/// `layout.window_buttons` for the side, else the preset's side, the preset
/// being the one `layout.preset` names in the files, and each of
/// `layout.close_button`, `minimize_button` and `maximize_button` unless a
/// file sets it false. Each file's own preset is applied first
/// ([`settings::in_use`]), as the compositor reads them.
pub fn buttons_applying(machine: Option<&SettingsFile>, person: Option<&SettingsFile>) -> Buttons {
    let machine = machine.map(settings::in_use);
    let person = person.map(settings::in_use);
    let (mut side, mut preset) = (None, None);
    let mut shown = [true; 3];
    for file in [machine.as_ref(), person.as_ref()].into_iter().flatten() {
        if file.layout.preset.is_some() {
            preset.clone_from(&file.layout.preset);
        }
        if let Some(chosen) = file.layout.window_buttons.as_deref().and_then(Side::parse) {
            side = Some(chosen);
        }
        for (value, index) in [
            (file.layout.close_button, 0),
            (file.layout.minimize_button, 1),
            (file.layout.maximize_button, 2),
        ] {
            if let Some(value) = value {
                shown[index] = value;
            }
        }
    }
    Buttons {
        side: side.unwrap_or_else(|| named(preset.as_deref()).0.windows.buttons),
        close: shown[0],
        minimize: shown[1],
        maximize: shown[2],
    }
}

/// The buttons in GNOME's `button-layout` form (M6.16), which GTK and
/// libadwaita apps read from the settings portal for their own header bars:
/// the shown buttons of the side, close first on the left and last on the
/// right, then a colon on the side the bar is on. A hidden button is left
/// out, so a bar with none is `:`: `close,minimize,maximize:` on the left,
/// `:minimize,maximize,close` on the right, `:maximize,close` with minimize
/// hidden on the right.
pub fn gnome_button_layout(buttons: &Buttons) -> String {
    let shown = |on: bool, name: &'static str| on.then_some(name);
    let names: Vec<&str> = match buttons.side {
        Side::Left => [
            shown(buttons.close, "close"),
            shown(buttons.minimize, "minimize"),
            shown(buttons.maximize, "maximize"),
        ],
        Side::Right => [
            shown(buttons.minimize, "minimize"),
            shown(buttons.maximize, "maximize"),
            shown(buttons.close, "close"),
        ],
    }
    .into_iter()
    .flatten()
    .collect();
    match buttons.side {
        Side::Left => format!("{}:", names.join(",")),
        Side::Right => format!(":{}", names.join(",")),
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
    /// How tall the panel is (M5.31c): small, medium or large, from the
    /// tokens `size.panel_small`, `size.panel` and `size.panel_large`.
    #[serde(default, skip_serializing_if = "Size::is_medium")]
    pub size: Size,
    /// A bar that floats: a gap (`size.gap`) round it and rounded corners,
    /// as a dock has (M5.31c).
    #[serde(default, skip_serializing_if = "is_false")]
    pub floating: bool,
    /// The panel on the main screen only, or on every screen (M5.31c).
    #[serde(default, skip_serializing_if = "Screens::is_main")]
    pub screens: Screens,
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

/// How tall a panel is (M5.31c), from the tokens: `size.panel_small` (34),
/// `size.panel` (40, the default) or `size.panel_large` (48). A dock adds
/// its own room to any of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    /// Whether it is the default, which a written panel leaves out.
    pub fn is_medium(&self) -> bool {
        *self == Size::Medium
    }
}

/// Which screens a panel is on (M5.31c): the main one, the compositor's
/// choice (the default), or every screen, one panel on each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Screens {
    #[default]
    Main,
    Every,
}

impl Screens {
    /// Whether it is the default, which a written panel leaves out.
    pub fn is_main(&self) -> bool {
        *self == Screens::Main
    }
}

/// Whether a flag is off, which a written panel leaves out (M5.31c).
fn is_false(flag: &bool) -> bool {
    !*flag
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
    check_quick(&preset.quick)?;
    if let Some(style) = &preset.windows.tiling_style {
        let styles = crate::settings::choices("layout.tiling_style");
        if !styles.contains(&style.as_str()) {
            bail!(
                "{}",
                trf(
                    "{style} is not a tiling style; the styles are {styles}",
                    &[
                        ("style", &format!("{style:?}")),
                        ("styles", &styles.join(", ")),
                    ]
                )
            );
        }
    }
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

/// What a preset's `[quick] tiles` must be: names of tiles this release has
/// ([`TILES`]), none twice.
pub fn check_quick(quick: &Quick) -> Result<()> {
    for (i, tile) in quick.tiles.iter().enumerate() {
        if !TILES.contains(&tile.as_str()) {
            bail!(
                "{}",
                trf(
                    "{tile} is not a quick settings tile; the tiles are {tiles}",
                    &[("tile", &format!("{tile:?}")), ("tiles", &TILES.join(", "))]
                )
            );
        }
        if quick.tiles[..i].contains(tile) {
            bail!(
                "{}",
                trf(
                    "the quick settings tile {tile} is listed twice",
                    &[("tile", tile)]
                )
            );
        }
    }
    Ok(())
}

/// What a preset's panels, or `[[panels.list]]` in a settings file
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
        if panel.floating && panel.style == Style::Dock {
            bail!(
                "{}",
                trf(
                    "a dock floats already; take floating out of the {edge} panel",
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
        "zen" => tr(n_("Zen")).to_string(),
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
    let note = found.is_none().then(|| lacks_note(wanted));
    let (_, text) = found.unwrap_or(&BUILT_IN[0]);
    // Every built-in preset is checked by the tests.
    let preset = check(text).expect("the built-in presets are checked by the tests");
    (preset, note)
}

/// The words for a preset this release lacks, as the reader and the
/// compositor give them: Classic is used instead.
pub fn lacks_note(name: &str) -> String {
    trf(
        "this release has no preset {name}; the Classic preset is used",
        &[("name", &format!("{name:?}"))],
    )
}

/// Whether `name` is one of [`BUILT_IN`]'s, which never changes.
pub fn is_built_in(name: &str) -> bool {
    BUILT_IN.iter().any(|(built_in, _)| *built_in == name)
}

/// The value a built-in preset gives `key` while the settings file does
/// not set it, as the file would write it (`true`, `"right"`, `4`), for
/// `edel settings get` (M6.11); None for a key no preset decides, or one
/// whose value is more than one word, such as the panels.
pub fn value_of(key: &str, preset: &Preset) -> Option<String> {
    let text = |word: &str| toml::Value::String(word.to_string()).to_string();
    Some(match key {
        "layout.tiling" => (preset.windows.policy == Policy::Tiling).to_string(),
        "layout.window_buttons" => text(preset.windows.buttons.name()),
        "layout.title_bars" => text(preset.windows.title_bars.unwrap_or_default().name()),
        "layout.tiling_style" => text(preset.windows.tiling_style.as_deref().unwrap_or("stack")),
        "workspaces.count" => preset.workspaces.count.to_string(),
        _ => return None,
    })
}

/// Why `name` is no preset's name, in the words every reader shows (the
/// checks, save and the commands say it the same way).
pub fn name_message(name: &str) -> String {
    trf(
        "{name} is not a preset's name; use letters, digits and hyphens, such as \"my-hive\"",
        &[("name", &format!("{name:?}"))],
    )
}

/// The built-in preset an own preset starts from: its `preset` when this
/// release has it, else Classic (the note of [`lacks_note`] says so).
fn base_of(own: &OwnPreset) -> String {
    if is_built_in(&own.preset) {
        own.preset.clone()
    } else {
        DEFAULT.to_string()
    }
}

/// The name of the preset in use: `layout.preset`, else the default.
fn in_use_name(file: &SettingsFile) -> &str {
    file.layout.preset.as_deref().unwrap_or(DEFAULT)
}

/// The `[layout]` keys of `file` as a table, `preset` left out.
fn layout_table(layout: &Layout) -> Result<Table> {
    let Value::Table(mut table) = Value::try_from(layout)? else {
        bail!("{}", tr("a layout could not be written as a table"));
    };
    table.remove("preset");
    Ok(table)
}

/// The file with its own preset in use applied (M5.17): when `layout.preset`
/// names one of the file's own, each `[layout]` key the file leaves unset is
/// the own preset's, and `layout.preset` becomes its base built-in preset, a
/// base this release lacks becoming Classic. A file that names no own preset
/// comes back unchanged. Pure, and read through [`crate::settings::in_use`]
/// (ADR-010).
pub fn applied(file: &SettingsFile) -> SettingsFile {
    let Some(own) = file
        .layout
        .preset
        .as_deref()
        .and_then(|n| file.presets.get(n))
    else {
        return file.clone();
    };
    let (Ok(mut layout), Ok(values)) = (layout_table(&file.layout), layout_table(&own.layout))
    else {
        return file.clone();
    };
    themes::merge(&mut layout, values);
    layout.insert("preset".into(), Value::String(base_of(own)));
    let mut out = file.clone();
    if let Ok(layout) = Value::Table(layout).try_into() {
        out.layout = layout;
    }
    out
}

/// The value a preset in use gives `key` (a `layout.*` key), as the desktop
/// applies it, the person's file over the machine's; None when no file names
/// an own preset, so the caller reads each file's own value as before.
pub fn chosen(key: &str, machine: Option<&str>, person: Option<&str>) -> Option<String> {
    let path = key.strip_prefix("layout.")?;
    // Most files name no own preset, and then nothing is parsed: this is asked
    // at each drawing of a menu.
    let names_some_preset = |text: Option<&str>| text.is_some_and(|text| text.contains("presets"));
    if !names_some_preset(machine) && !names_some_preset(person) {
        return None;
    }
    let read = |text: Option<&str>| text.and_then(|text| settings::read(text).ok());
    let (machine, person) = (read(machine), read(person));
    let names_one = [&machine, &person]
        .into_iter()
        .flatten()
        .any(|read| !read.file.presets.is_empty());
    if !names_one {
        return None;
    }
    let person = person.map(|read| settings::in_use(&read.file));
    let machine = machine.map(|read| settings::in_use(&read.file));
    let value = [person, machine]
        .into_iter()
        .flatten()
        .find_map(|file| settings::value_at(&file.layout, path))?;
    Some(match value {
        Value::String(text) => text,
        other => other.to_string(),
    })
}

/// What the file's own presets and the preset in use say wrong, one line each,
/// as `edel settings check` gives them: a preset's name, a built-in name taken
/// by a person's own preset, a preset based on another of the person's own
/// (a preset is one level deep), and a `layout.preset` that names no preset.
/// A base this release lacks is a note of the reader, not a problem here.
pub fn problems(file: &SettingsFile) -> Vec<String> {
    let mut lines = Vec::new();
    for (name, own) in &file.presets {
        let shown = format!("presets.{name}");
        if !themes::is_name(name) {
            lines.push(format!("{shown}: {}", name_message(name)));
        } else if is_built_in(name) {
            let message = trf(
                "{name} is a built-in preset; name yours another way, such as \"my-hive\"",
                &[("name", &format!("{name:?}"))],
            );
            lines.push(format!("{shown}: {message}"));
        }
        if !is_built_in(&own.preset) && file.presets.contains_key(&own.preset) {
            let message = trf(
                "{name} starts from {base}, which is one of yours; start from a built-in preset: {builtins}",
                &[
                    ("name", &format!("{name:?}")),
                    ("base", &format!("{:?}", own.preset)),
                    ("builtins", &settings::or_list(NAMES)),
                ],
            );
            lines.push(format!("{shown}: {message}"));
        }
    }
    let in_use = in_use_name(file);
    if !is_built_in(in_use) && !file.presets.contains_key(in_use) {
        let builtins = settings::or_list(NAMES);
        let message = if let Some(near) = settings::nearest_value(in_use, NAMES) {
            trf(
                "{name} is not a preset; did you mean {near}?",
                &[("name", &format!("{in_use:?}")), ("near", near)],
            )
        } else if file.presets.is_empty() {
            trf(
                "{name} is not a preset; choose {builtins}",
                &[("name", &format!("{in_use:?}")), ("builtins", &builtins)],
            )
        } else {
            let own: Vec<&str> = file.presets.keys().map(String::as_str).collect();
            trf(
                "{name} is not a preset; choose {builtins}, or one of yours ({own})",
                &[
                    ("name", &format!("{in_use:?}")),
                    ("builtins", &builtins),
                    ("own", &settings::or_list(&own)),
                ],
            )
        };
        lines.push(format!("layout.preset: {message}"));
    }
    lines
}

/// One preset as `edel preset list` shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub title: String,
    pub built_in: bool,
    pub in_use: bool,
    /// The built-in preset it lays out from: its own name for a built-in one.
    pub base: String,
}

/// The presets in the order `edel preset list` and Settings show them: the
/// built-in ones in [`NAMES`]' order, then the person's own by name, the one
/// in use marked. An own preset's title is its name.
pub fn rows(file: &SettingsFile) -> Vec<Row> {
    let in_use = in_use_name(file);
    let built_in = BUILT_IN
        .iter()
        .map(|(name, _)| (name.to_string(), title(name), true, name.to_string()));
    let own = file
        .presets
        .iter()
        .filter(|(name, _)| !is_built_in(name))
        .map(|(name, own)| (name.clone(), name.clone(), false, base_of(own)));
    built_in
        .chain(own)
        .map(|(name, title, built_in, base)| Row {
            in_use: name == in_use,
            name,
            title,
            built_in,
            base,
        })
        .collect()
}

/// The layout keys a file sets besides its preset, as `layout.KEY` (M5.17):
/// what Save as preset moves into the preset of the person's own.
pub fn changed(file: &SettingsFile) -> Vec<String> {
    layout_table(&file.layout)
        .map(|table| table.keys().map(|key| format!("layout.{key}")).collect())
        .unwrap_or_default()
}

/// `edel preset save NAME`, on a file's text (Save as, M5.17): makes
/// `[presets.NAME]` from the layout in use, its keys the file's own on top
/// of the preset in use, and its base the built-in preset in use (an own
/// preset in use keeps its base), then sets `layout.preset` to it and takes
/// the moved keys out of `[layout]`, so the screen is the same. A name of
/// the person's own that exists is replaced; a built-in's is refused.
pub fn save_text(text: &str, name: &str) -> Result<String> {
    let file = settings::read(text)?.file;
    if !themes::is_name(name) {
        bail!("{}", name_message(name));
    }
    if is_built_in(name) {
        bail!(
            "{}",
            trf(
                "{name} is a built-in preset and stays as it is; name yours another way, such as \"my-hive\"",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    let in_use = in_use_name(&file);
    let (base, mut table) = match file.presets.get(in_use) {
        Some(own) => (base_of(own), layout_table(&file.layout)?),
        None => (
            if is_built_in(in_use) { in_use } else { DEFAULT }.to_string(),
            layout_table(&file.layout)?,
        ),
    };
    if let Some(own) = file.presets.get(in_use) {
        themes::merge(&mut table, layout_table(&own.layout)?);
    }
    table.insert("preset".into(), Value::String(base));
    let own: OwnPreset = Value::Table(table).try_into()?;
    let Value::Table(table) = Value::try_from(&own)? else {
        bail!("{}", tr("a preset could not be written as a table"));
    };
    let mut text = settings::set(
        text,
        &format!("presets.{name}"),
        &Value::Table(table).to_string(),
    )?;
    text = settings::set(&text, "layout.preset", &format!("{name:?}"))?;
    let moved = changed(&file);
    if !moved.is_empty() {
        let moved: Vec<&str> = moved.iter().map(String::as_str).collect();
        text = settings::unset_all(&text, &moved)?;
    }
    Ok(text)
}

/// `edel preset delete NAME`, on a file's text: removes `[presets.NAME]`. A
/// built-in preset, a name that is no preset of the person's and the preset in
/// use are refused, the last with the way to pick another first.
pub fn delete_text(text: &str, name: &str) -> Result<String> {
    let file = settings::read(text)?.file;
    if is_built_in(name) {
        bail!(
            "{}",
            trf(
                "{name} is a built-in preset, which stays as it is",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    if !file.presets.contains_key(name) {
        bail!(
            "{}",
            trf(
                "there is no preset of yours called {name}; edel preset list shows them",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    if in_use_name(&file) == name {
        bail!(
            "{}",
            trf(
                "{name} is the preset in use; pick another preset first: edel settings set layout.preset=classic",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    settings::unset(text, &format!("presets.{name}"))
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
                "apps",
                "workspaces",
                "layout",
                "separator",
                "tray",
                "keyboard",
                "status",
                "clock"
            ]
        );
    }

    #[test]
    fn gnome_button_layout_lists_the_shown_buttons_on_their_side() {
        let all = |side| Buttons {
            side,
            close: true,
            minimize: true,
            maximize: true,
        };
        assert_eq!(
            gnome_button_layout(&all(Side::Left)),
            "close,minimize,maximize:"
        );
        assert_eq!(
            gnome_button_layout(&all(Side::Right)),
            ":minimize,maximize,close"
        );
    }

    #[test]
    fn gnome_button_layout_leaves_a_hidden_button_out() {
        let right = Buttons {
            side: Side::Right,
            close: true,
            minimize: false,
            maximize: true,
        };
        assert_eq!(gnome_button_layout(&right), ":maximize,close");
        let left = Buttons {
            side: Side::Left,
            close: false,
            minimize: true,
            maximize: true,
        };
        assert_eq!(gnome_button_layout(&left), "minimize,maximize:");
    }

    #[test]
    fn gnome_button_layout_with_every_button_hidden_is_a_bare_colon() {
        for side in [Side::Left, Side::Right] {
            let none = Buttons {
                side,
                close: false,
                minimize: false,
                maximize: false,
            };
            assert_eq!(gnome_button_layout(&none), ":");
        }
    }

    #[test]
    fn the_persons_buttons_win_over_the_machines() {
        let machine =
            file("format = 1\n[layout]\nwindow_buttons = \"left\"\nminimize_button = false\n");
        let person = file("format = 1\n[layout]\nwindow_buttons = \"right\"\n");
        let both = buttons_applying(Some(&machine), Some(&person));
        assert_eq!(both.side, Side::Right, "the person's side");
        assert!(!both.minimize, "the machine's hidden minimize still holds");
        let alone = buttons_applying(Some(&machine), None);
        assert_eq!(alone.side, Side::Left);
        assert!(!alone.minimize);
        assert_eq!(gnome_button_layout(&alone), "close,maximize:");
    }

    #[test]
    fn the_buttons_side_falls_back_to_the_preset_the_files_name() {
        let mac = file("format = 1\n[layout]\npreset = \"mac-like\"\n");
        assert_eq!(buttons_applying(None, Some(&mac)).side, Side::Left);
        let nothing = buttons_applying(None, None);
        assert_eq!(nothing.side, Side::Right, "Classic's side");
        assert!(nothing.close && nothing.minimize && nothing.maximize);
        assert_eq!(gnome_button_layout(&nothing), ":minimize,maximize,close");
    }

    #[test]
    fn windows_like_has_a_taskbar_with_search_and_the_apps_in_its_centre() {
        let (windows, note) = named(Some("windows-like"));
        assert_eq!(note, None);
        assert_eq!(windows.windows.policy, Policy::Floating);
        assert_eq!(windows.windows.buttons, Side::Right);
        assert_eq!(
            windows.apps.pinned,
            ["files", "browser", "terminal", "mail", "music", "store"]
        );
        let panel = &windows.panels[0];
        assert_eq!(panel.edge, Edge::Bottom);
        assert_eq!(panel.start, ["menu", "search"]);
        assert_eq!(panel.centre, ["apps"]);
        // Classic and Hive pin nothing: they have no apps widget.
        assert_eq!(
            named(None).0.apps.pinned,
            ["browser", "files", "editor", "terminal", "store"]
        );
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
    fn every_preset_lists_quick_tiles_this_release_has_and_none_twice() {
        for (name, text) in BUILT_IN {
            let preset = check(text).unwrap();
            assert!(!preset.quick.tiles.is_empty(), "{name} lists no tiles");
            check_quick(&preset.quick).unwrap();
        }
        assert_eq!(named(None).0.quick.tiles, DEFAULT_TILES);
        // A preset that says nothing shows the default tiles.
        let classic = BUILT_IN[0].1;
        let bare: String = classic
            .lines()
            .filter(|l| !l.starts_with("tiles =") && !l.starts_with("[quick]"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(check(&bare).unwrap().quick, Quick::default());
        let own = format!("{bare}\n[quick]\ntiles = [\"dark_style\", \"wifi\"]\n");
        assert_eq!(check(&own).unwrap().quick.tiles, ["dark_style", "wifi"]);
    }

    #[test]
    fn a_quick_tile_this_release_lacks_or_lists_twice_is_refused() {
        let classic = BUILT_IN[0].1;
        // Classic names no tiles, so each text gets its own `[quick]` table.
        let with = |tiles: &str| format!("{classic}\n[quick]\ntiles = {tiles}\n");
        let e = format!(
            "{:#}",
            check(&with("[\"wifi\", \"night_light\"]")).unwrap_err()
        );
        assert!(
            e.contains("\"night_light\" is not a quick settings tile; the tiles are wifi, bluetooth, airplane, do_not_disturb, location, dark_style, battery_saver"),
            "{e}"
        );
        let e = format!("{:#}", check(&with("[\"wifi\", \"wifi\"]")).unwrap_err());
        assert!(
            e.contains("the quick settings tile wifi is listed twice"),
            "{e}"
        );
        let text = format!("{classic}\n[quick]\nrows = 3\n");
        assert!(format!("{:#}", check(&text).unwrap_err()).contains("unknown field `rows`"));
        assert!(check(&with("[]")).is_ok(), "an empty list shows no tiles");
    }

    #[test]
    fn an_unknown_name_is_classic_with_a_note() {
        let (preset, note) = named(Some("cinnamon"));
        assert_eq!(preset, named(None).0);
        assert!(note.unwrap().contains("no preset \"cinnamon\""));
    }

    #[test]
    fn a_panel_reads_its_size_floating_and_screens_and_writes_them_only_when_set() {
        let classic = BUILT_IN[0].1;
        let set = classic.replace(
            "edge = \"bottom\"",
            "edge = \"bottom\"\nsize = \"large\"\nfloating = true\nscreens = \"every\"",
        );
        let panel = &check(&set).unwrap().panels[0];
        assert_eq!(panel.size, Size::Large);
        assert!(panel.floating);
        assert_eq!(panel.screens, Screens::Every);
        let written = toml::to_string(panel).unwrap();
        assert!(written.contains("size = \"large\""), "{written}");
        assert!(written.contains("floating = true"), "{written}");
        assert!(written.contains("screens = \"every\""), "{written}");
        // The defaults are left out, so a preset's own file stays short.
        let plain = &check(classic).unwrap().panels[0];
        assert_eq!(
            (plain.size, plain.floating, plain.screens),
            (Size::Medium, false, Screens::Main)
        );
        let written = toml::to_string(plain).unwrap();
        assert!(
            !written.contains("size") && !written.contains("floating"),
            "{written}"
        );
        assert!(!written.contains("screens"), "{written}");
        // Each size is one word, and no other.
        for (word, size) in [
            ("small", Size::Small),
            ("medium", Size::Medium),
            ("large", Size::Large),
        ] {
            let text = classic.replace(
                "edge = \"bottom\"",
                &format!("edge = \"bottom\"\nsize = \"{word}\""),
            );
            assert_eq!(check(&text).unwrap().panels[0].size, size);
        }
        let huge = classic.replace("edge = \"bottom\"", "edge = \"bottom\"\nsize = \"huge\"");
        let e = format!("{:#}", check(&huge).unwrap_err());
        assert!(
            e.contains("unknown variant `huge`, expected one of `small`, `medium`, `large`"),
            "{e}"
        );
    }

    #[test]
    fn a_dock_that_floats_is_refused_as_a_dock_already_floats() {
        let classic = BUILT_IN[0].1;
        let dock = classic.replace(
            "edge = \"bottom\"",
            "edge = \"bottom\"\nstyle = \"dock\"\nfloating = true",
        );
        let e = format!("{:#}", check(&dock).unwrap_err());
        assert!(
            e.contains("a dock floats already; take floating out of the bottom panel"),
            "{e}"
        );
        // A floating bar is fine, and so is a dock that does not float.
        let bar = classic.replace("edge = \"bottom\"", "edge = \"bottom\"\nfloating = true");
        assert!(check(&bar).unwrap().panels[0].floating);
        let plain_dock =
            classic.replace("edge = \"bottom\"", "edge = \"bottom\"\nstyle = \"dock\"");
        assert!(check(&plain_dock).is_ok());
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

    #[test]
    fn zen_tiles_in_the_split_style_with_no_title_bars_and_a_slim_bar_on_top() {
        // M5.18b: the style and the bars come from the file, not the code.
        let (zen, note) = named(Some("zen"));
        assert_eq!(note, None);
        assert_eq!(zen.windows.policy, Policy::Tiling);
        assert_eq!(zen.windows.title_bars, Some(TitleBars::Never));
        assert_eq!(zen.windows.tiling_style.as_deref(), Some("split"));
        assert_eq!(zen.workspaces.count, 4, "as Hive's count");
        assert_eq!(zen.launcher.style, LauncherStyle::Menu);
        assert_eq!(zen.quick, named(Some("hive")).0.quick, "as Hive's tiles");
        assert_eq!(zen.panels.len(), 1);
        let panel = &zen.panels[0];
        assert_eq!(panel.edge, Edge::Top);
        assert_eq!(panel.size, Size::Small);
        assert_eq!(panel.start, ["menu"]);
        assert!(panel.centre.is_empty());
        assert_eq!(panel.end, ["workspaces", "clock"]);
        // The presets that say nothing keep the bars and the stack.
        for name in ["classic", "mac-like", "windows-like", "hive"] {
            let (preset, _) = named(Some(name));
            assert_eq!(preset.windows.title_bars, None, "{name}");
            assert_eq!(preset.windows.tiling_style, None, "{name}");
        }
    }

    #[test]
    fn a_preset_reads_its_title_bars_and_tiling_style_and_refuses_the_rest() {
        let classic = BUILT_IN[0].1;
        let set = classic.replace(
            "buttons = \"right\"",
            "buttons = \"right\"\ntitle_bars = \"floating-only\"\ntiling_style = \"scroll\"",
        );
        let windows = check(&set).unwrap().windows;
        assert_eq!(windows.title_bars, Some(TitleBars::FloatingOnly));
        assert_eq!(windows.tiling_style.as_deref(), Some("scroll"));
        let bars = classic.replace(
            "buttons = \"right\"",
            "buttons = \"right\"\ntitle_bars = \"sometimes\"",
        );
        let e = format!("{:#}", check(&bars).unwrap_err());
        assert!(e.contains("unknown variant `sometimes`"), "{e}");
        let style = classic.replace(
            "buttons = \"right\"",
            "buttons = \"right\"\ntiling_style = \"spiral\"",
        );
        let e = format!("{:#}", check(&style).unwrap_err());
        assert!(
            e.contains("\"spiral\" is not a tiling style; the styles are stack, split, scroll"),
            "{e}"
        );
    }

    #[test]
    fn the_title_bars_values_are_the_key_tables() {
        let names: Vec<&str> = TitleBars::ALL.iter().map(|t| t.name()).collect();
        assert_eq!(names, crate::settings::choices("layout.title_bars"));
        for t in TitleBars::ALL {
            assert_eq!(TitleBars::parse(t.name()), Some(t));
        }
        assert_eq!(TitleBars::parse("sometimes"), None);
        assert_eq!(TitleBars::default(), TitleBars::Always);
    }

    fn file(text: &str) -> SettingsFile {
        settings::read(text).unwrap().file
    }

    /// The screen a file gives: the preset it lays out from and its layout
    /// keys, the preset's name left out of the keys.
    fn screen(text: &str) -> (Preset, Layout) {
        let applied = settings::in_use(&file(text));
        let mut layout = applied.layout.clone();
        layout.preset = None;
        (named(applied.layout.preset.as_deref()).0, layout)
    }

    const MINE: &str = "format = 1\n# my desk\n[appearance]\naccent = \"blue\"\n\n[layout]\npreset = \"mine\"\ntiling = true\n\n[presets.mine]\npreset = \"hive\"\nwindow_buttons = \"left\"\n";

    #[test]
    fn an_own_preset_lays_its_keys_under_the_files_and_names_its_base() {
        let laid = applied(&file(MINE));
        assert_eq!(laid.layout.preset.as_deref(), Some("hive"));
        assert_eq!(laid.layout.window_buttons.as_deref(), Some("left"));
        assert_eq!(laid.layout.tiling, Some(true), "the file's own key stays");
        let over = file(&MINE.replace(
            "tiling = true\n",
            "tiling = true\nwindow_buttons = \"right\"\n",
        ));
        assert_eq!(
            applied(&over).layout.window_buttons.as_deref(),
            Some("right"),
            "the person's value wins over the preset's"
        );
        // A file naming no own preset comes back unchanged.
        let plain = file("format = 1\n[layout]\npreset = \"zen\"\n");
        assert_eq!(applied(&plain), plain);
    }

    #[test]
    fn save_moves_the_layout_keys_and_keeps_every_other_byte() {
        let text = "format = 1\n# my desk\n[appearance]\naccent = \"blue\"\n\n[layout]\nwindow_buttons = \"left\"\ntiling = true # keep\n\n[network]\nhostname = \"lab\"\n";
        let saved = save_text(text, "mine").unwrap();
        for line in [
            "# my desk\n",
            "[appearance]\naccent = \"blue\"\n",
            "[network]\nhostname = \"lab\"\n",
        ] {
            assert!(saved.contains(line), "{saved}");
        }
        let read = file(&saved);
        assert_eq!(read.layout.preset.as_deref(), Some("mine"));
        assert_eq!(read.layout.window_buttons, None);
        assert_eq!(read.presets["mine"].preset, "classic");
        assert_eq!(
            read.presets["mine"].layout.window_buttons.as_deref(),
            Some("left")
        );
        assert_eq!(read.presets["mine"].layout.tiling, Some(true));
    }

    #[test]
    fn saved_shows_the_same_screen() {
        let text = "format = 1\n[layout]\nwindow_buttons = \"left\"\ntiling = true\n";
        let saved = save_text(text, "mine").unwrap();
        assert_eq!(screen(text), screen(&saved));
        assert_eq!(
            screen(&saved).0,
            named(None).0,
            "plain Classic under the name"
        );
        // Picking Classic afterwards gives plain Classic, as it did before.
        let back = settings::set(&saved, "layout.preset", "\"classic\"").unwrap();
        assert_eq!(screen(&back).0, named(None).0);
    }

    #[test]
    fn save_refuses_a_built_in_name_and_a_bad_one() {
        let text = "format = 1\n[layout]\npreset = \"hive\"\n";
        let refused = save_text(text, "hive").unwrap_err().to_string();
        assert!(refused.contains("built-in preset"), "{refused}");
        assert!(save_text(text, "My Desk").is_err());
    }

    #[test]
    fn save_from_an_own_preset_keeps_its_base_and_keys() {
        let saved = save_text(MINE, "other").unwrap();
        let read = file(&saved);
        assert_eq!(read.presets["other"].preset, "hive");
        assert_eq!(
            read.presets["other"].layout.window_buttons.as_deref(),
            Some("left")
        );
        assert_eq!(read.layout.preset.as_deref(), Some("other"));
        assert_eq!(screen(MINE), screen(&saved));
        // Saving over the one in use updates it, and keeps its base.
        let updated = save_text(MINE, "mine").unwrap();
        assert_eq!(file(&updated).presets["mine"].preset, "hive");
    }

    #[test]
    fn delete_refuses_the_preset_in_use_and_built_ins() {
        let refused = delete_text(MINE, "mine").unwrap_err().to_string();
        assert!(refused.contains("preset in use"), "{refused}");
        assert!(
            refused.contains("edel settings set layout.preset=classic"),
            "{refused}"
        );
        assert!(delete_text(MINE, "hive").is_err());
        let other = MINE.replace("preset = \"mine\"", "preset = \"classic\"");
        let gone = file(&delete_text(&other, "mine").unwrap());
        assert!(gone.presets.is_empty());
    }

    #[test]
    fn a_base_this_release_lacks_gives_classic_and_a_report() {
        let text = "format = 1\n[layout]\npreset = \"old\"\n[presets.old]\npreset = \"tablet\"\nwindow_buttons = \"left\"\n";
        let read = settings::read(text).unwrap();
        assert!(
            read.problems
                .iter()
                .any(|p| p.key == "presets.old.preset"
                    && p.message.contains("has no preset \"tablet\"")),
            "{:?}",
            read.problems
        );
        assert_eq!(
            applied(&read.file).layout.preset.as_deref(),
            Some("classic")
        );
        assert_eq!(screen(text).0, named(None).0);
        assert_eq!(screen(text).1.window_buttons.as_deref(), Some("left"));
        // The strict checker refuses nothing a reader only notes.
        assert!(problems(&read.file).is_empty());
    }

    #[test]
    fn a_key_this_release_lacks_in_an_own_preset_is_left_out_and_reported() {
        let text = "format = 1\n[presets.mine]\npreset = \"hive\"\nwindow_colour = \"red\"\nwindow_buttons = \"left\"\n";
        let read = settings::read(text).unwrap();
        assert!(
            read.problems
                .iter()
                .any(|p| p.key == "presets.mine.window_colour")
        );
        assert_eq!(
            read.file.presets["mine"].layout.window_buttons.as_deref(),
            Some("left")
        );
    }

    #[test]
    fn layout_preset_names_an_own_preset_only_when_its_table_exists() {
        let named_only = "format = 1\n[layout]\npreset = \"mine\"\n";
        let lines = problems(&file(named_only));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("layout.preset: \"mine\" is not a preset;"),
            "{lines:?}"
        );
        let set_it = settings::set(named_only, "layout.preset", "\"mine\"");
        assert!(set_it.is_err());
        let with_table = format!("{named_only}\n[presets.mine]\n");
        assert!(problems(&file(&with_table)).is_empty());
        assert!(settings::set(&with_table, "layout.preset", "\"mine\"").is_ok());
    }

    #[test]
    fn the_checker_refuses_an_own_preset_named_as_a_built_in_or_based_on_another() {
        let hive = problems(&file(
            "format = 1\n[presets.hive]\nwindow_buttons = \"left\"\n",
        ));
        assert!(
            hive.iter()
                .any(|line| line.contains("is a built-in preset")),
            "{hive:?}"
        );
        let based = problems(&file(
            "format = 1\n[presets.a]\npreset = \"hive\"\n[presets.b]\npreset = \"a\"\n",
        ));
        assert!(
            based
                .iter()
                .any(|line| line.starts_with("presets.b: ") && line.contains("starts from")),
            "{based:?}"
        );
    }

    #[test]
    fn rows_list_the_built_ins_then_the_own_with_the_one_in_use() {
        let rows = rows(&file(MINE));
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(
            names,
            ["classic", "mac-like", "windows-like", "hive", "zen", "mine"]
        );
        assert!(rows[5].in_use && !rows[5].built_in);
        assert_eq!(rows[5].title, "mine");
        assert_eq!(rows[5].base, "hive");
        assert!(rows[0].built_in && !rows[0].in_use);
    }

    #[test]
    fn chosen_gives_an_own_presets_layout_value_and_none_without_one() {
        assert_eq!(
            chosen("layout.window_buttons", None, Some(MINE)).as_deref(),
            Some("left")
        );
        assert_eq!(
            chosen("layout.preset", None, Some(MINE)).as_deref(),
            Some("hive")
        );
        assert_eq!(
            chosen(
                "layout.window_buttons",
                None,
                Some("format = 1\n[layout]\nwindow_buttons = \"right\"\n")
            ),
            None
        );
    }

    #[test]
    fn a_built_in_preset_gives_the_values_get_shows() {
        let (classic, _) = named(Some("classic"));
        assert_eq!(
            value_of("layout.tiling", &classic).as_deref(),
            Some("false")
        );
        assert_eq!(
            value_of("layout.window_buttons", &classic).as_deref(),
            Some("\"right\"")
        );
        assert_eq!(value_of("workspaces.count", &classic).as_deref(), Some("4"));
        let (hive, _) = named(Some("hive"));
        assert_eq!(value_of("layout.tiling", &hive).as_deref(), Some("true"));
        assert_eq!(value_of("panels.list", &classic), None);
    }
}
