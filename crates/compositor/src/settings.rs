//! What the compositor takes from the settings file (roadmap M4.5): the
//! machine's and the person's, the same schema, found through
//! `edel::places` and read with `edel::settings`
//! so the defaults and the leniency are `edel`'s (ADR-008). A key the
//! person's file sets wins; a key neither sets is the preset's (M5.1c),
//! then the default. Reading is
//! lenient: a missing file is the defaults, and a broken one is reported
//! and read as missing, never fatal.

use std::collections::BTreeMap;
use std::path::Path;

use edel::presets::{self, Side};
use edel::settings::{self, SettingsFile};
pub use edel::tokens::Scheme;

use crate::animation::Motion;
use crate::frame::Shown;
use crate::tiling::Style;

/// Whether the compositor draws title bars in tiling too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TitleBars {
    #[default]
    Always,
    FloatingOnly,
}

/// The settings the compositor follows, all live.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Settings {
    /// `layout.tiling`: workspaces tile; absent means the preset's policy.
    pub tiling: Option<bool>,
    /// `layout.preset`: absent means Classic.
    pub preset: Option<String>,
    /// `layout.title_bars`: absent means always.
    pub title_bars: TitleBars,
    /// `layout.window_buttons` (M5.4b): absent means the preset's side.
    pub window_buttons: Option<Side>,
    /// `layout.close_button`, `minimize_button` and `maximize_button`
    /// (M5.18a): absent shows each.
    pub buttons: Shown,
    /// `panels.list` (M5.4e): absent means the preset's; shell-ui reads
    /// them, and the compositor only restarts it when they change.
    pub panels: Option<Vec<edel::presets::Panel>>,
    /// `[displays.NAME]`, by output name.
    pub outputs: BTreeMap<String, OutputSettings>,
    /// `appearance.animations` (M5.11b): absent means full.
    pub motion: Motion,
    /// `appearance.mode` (M5.5c): absent, or `auto`, means light (M5.12a).
    pub color_scheme: Scheme,
    /// `[shortcuts]`, action to keys as written, the person's over the
    /// machine's; `edel::shortcuts::resolve` lays them over the defaults
    /// (M5.13a).
    pub shortcuts: BTreeMap<String, String>,
    /// `region.keyboard` (M5.21), as `edel::keyboard` writes it: absent
    /// means xkb's default, the US layout.
    pub keyboard: Option<String>,
    /// `region.language` (M5.24a): shell-ui reads its words in it, and
    /// the compositor only restarts it when it changes.
    pub language: Option<String>,
    /// `layout.tiling_style` (M5.16a): absent means stack.
    pub tiling_style: Style,
    /// `workspaces.count` (M5.2i): absent means the preset's count.
    pub workspace_count: Option<usize>,
    /// `workspaces.dynamic` (M5.2i): absent means off.
    pub dynamic_workspaces: bool,
    /// `workspaces.per_screen` (M5.2k): absent means off, every
    /// screen switching together.
    pub workspaces_per_screen: bool,
    /// `workspaces.names` (M5.2i), the first workspace's first; an
    /// empty text is no name.
    pub workspace_names: Vec<String>,
    /// `workspaces.apps` (M5.2l): each app id and the workspace it
    /// opens on, from 0, in the file's order; absent opens every app on
    /// the workspace in use.
    pub app_workspaces: Vec<(String, usize)>,
}

/// One screen's keys; each absent one means the screen decides.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutputSettings {
    /// Else [`crate::layout::auto_scale`].
    pub scale: Option<f64>,
    /// Its top left in the layout, logical; else to the right of the
    /// screens before it.
    pub position: Option<(i32, i32)>,
    /// `WIDTHxHEIGHT` or `WIDTHxHEIGHT@HZ`; else its preferred mode.
    pub mode: Option<String>,
    /// Off leaves the screen dark; absent or on uses it.
    pub enabled: Option<bool>,
}

impl Settings {
    /// The machine's file, then the person's over it.
    pub fn from_files(machine: Option<&SettingsFile>, person: Option<&SettingsFile>) -> Settings {
        let mut settings = Settings::default();
        for file in [machine, person].into_iter().flatten() {
            if file.layout.tiling.is_some() {
                settings.tiling = file.layout.tiling;
            }
            if file.layout.preset.is_some() {
                settings.preset.clone_from(&file.layout.preset);
            }
            match file.layout.title_bars.as_deref() {
                Some("always") => settings.title_bars = TitleBars::Always,
                Some("floating-only") => settings.title_bars = TitleBars::FloatingOnly,
                _ => {}
            }
            if let Some(side) = file.layout.window_buttons.as_deref().and_then(Side::parse) {
                settings.window_buttons = Some(side);
            }
            for (value, shown) in [
                (file.layout.close_button, &mut settings.buttons.close),
                (file.layout.minimize_button, &mut settings.buttons.minimize),
                (file.layout.maximize_button, &mut settings.buttons.maximize),
            ] {
                if let Some(value) = value {
                    *shown = value;
                }
            }
            if file.panels.list.is_some() {
                settings.panels.clone_from(&file.panels.list);
            }
            if let Some(style) = file.layout.tiling_style.as_deref().and_then(Style::parse) {
                settings.tiling_style = style;
            }
            if let Some(count) = file.workspaces.count {
                settings.workspace_count = usize::try_from(count).ok();
            }
            if let Some(dynamic) = file.workspaces.dynamic {
                settings.dynamic_workspaces = dynamic;
            }
            if let Some(per_screen) = file.workspaces.per_screen {
                settings.workspaces_per_screen = per_screen;
            }
            if let Some(names) = &file.workspaces.names {
                settings.workspace_names.clone_from(names);
            }
            if let Some(apps) = &file.workspaces.apps {
                // The person's table replaces the machine's whole, as the
                // other keys do; a number out of range is left out.
                settings.app_workspaces = apps
                    .iter()
                    .filter(|(_, n)| (1..=edel::presets::MOST_WORKSPACES as i64).contains(n))
                    .filter_map(|(app, n)| Some((app.clone(), usize::try_from(n - 1).ok()?)))
                    .collect();
            }
            if let Some(motion) = file
                .appearance
                .animations
                .as_deref()
                .and_then(Motion::parse)
            {
                settings.motion = motion;
            }
            if let Some(scheme) = file.appearance.mode.as_deref().and_then(Scheme::parse) {
                settings.color_scheme = scheme;
            }
            settings.shortcuts.extend(file.shortcuts.clone());
            if file.region.keyboard.is_some() {
                settings.keyboard.clone_from(&file.region.keyboard);
            }
            if file.region.language.is_some() {
                settings.language.clone_from(&file.region.language);
            }
            for (name, output) in &file.displays {
                let into = settings.outputs.entry(name.clone()).or_default();
                if let Some(scale) = output.scale.filter(|s| s.is_finite() && *s > 0.0) {
                    into.scale = Some(scale);
                }
                if let Some([x, y]) = output.position {
                    if let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) {
                        into.position = Some((x, y));
                    }
                }
                if let Some(mode) = output.mode() {
                    into.mode = Some(mode);
                }
                if output.enabled.is_some() {
                    into.enabled = output.enabled;
                }
            }
        }
        settings
    }

    /// The policy workspaces start in, or switch to when `layout.tiling`
    /// or the preset changes: `layout.tiling` if set, else the preset's.
    pub fn policy(&self) -> &'static str {
        match self.tiling {
            Some(true) => "tiling",
            Some(false) => "floating",
            None => presets::named(self.preset.as_deref())
                .0
                .windows
                .policy
                .name(),
        }
    }

    /// The side of the title bars their buttons sit on (M5.4b):
    /// `layout.window_buttons` if set, else the preset's.
    pub fn button_side(&self) -> Side {
        self.window_buttons
            .unwrap_or_else(|| presets::named(self.preset.as_deref()).0.windows.buttons)
    }

    /// The commands the session starts (M5.7b): the preset's
    /// `[session] start`, else the release's list.
    pub fn session_start(&self) -> Vec<String> {
        presets::named(self.preset.as_deref()).0.session.start
    }

    /// How many workspaces there are (M5.2a): `workspaces.count` if set,
    /// else the preset's count. Dynamic workspaces ignore it (M5.2i).
    pub fn workspaces(&self) -> usize {
        self.workspace_count
            .unwrap_or_else(|| presets::named(self.preset.as_deref()).0.workspaces.count)
    }

    /// Whether empty workspaces come and go as the windows do (M5.2i).
    pub fn dynamic(&self) -> bool {
        self.dynamic_workspaces
    }

    /// Whether each screen shows its own workspace (M5.2k), so Super+N
    /// switches only the screen the pointer is on.
    pub fn per_screen(&self) -> bool {
        self.workspaces_per_screen
    }

    /// The workspaces' names, the first workspace's first (M5.2i).
    pub fn names(&self) -> &[String] {
        &self.workspace_names
    }

    /// The workspace, from 0, that `app_id`'s windows open on
    /// (`workspaces.apps`, M5.2l); `None` when the app has none.
    pub fn app_workspace(&self, app_id: &str) -> Option<usize> {
        self.app_workspaces
            .iter()
            .find(|(id, _)| id == app_id)
            .map(|(_, workspace)| *workspace)
    }

    /// Whether `other` lays the desktop out by another preset (M5.4a): a
    /// name this release lacks is Classic, so it differs from Classic only
    /// in name and restarts nothing.
    pub fn preset_differs(&self, other: &Settings) -> bool {
        presets::named(self.preset.as_deref()).0 != presets::named(other.preset.as_deref()).0
    }

    /// Whether a dock hides while a window covers it (M5.4f): from
    /// `panels.list` if set, else the preset's panels.
    pub fn dock_hides(&self) -> bool {
        match &self.panels {
            Some(panels) => presets::dock_hides(panels),
            None => presets::dock_hides(&presets::named(self.preset.as_deref()).0.panels),
        }
    }

    /// Whether windows under `policy` get the compositor's title bars.
    pub fn bars_in(&self, policy: &str) -> bool {
        self.title_bars == TitleBars::Always || policy == "floating"
    }
}

/// The person's settings file, by its name or a former one.
pub fn person_file() -> Option<std::path::PathBuf> {
    edel::places::person_settings().map(|p| edel::places::found(&p))
}

/// The machine's settings file, by its name or a former one.
pub fn machine_file() -> std::path::PathBuf {
    edel::places::found(&edel::places::machine_settings())
}

/// Reads one file the way an unattended reader must: missing is nothing,
/// what cannot be used is left out and noted, a file it cannot read at all
/// is noted and read as missing.
pub fn read(path: &Path, notes: &mut Vec<String>) -> Option<SettingsFile> {
    if !path.exists() {
        return None;
    }
    match settings::read_on_machine(path) {
        Ok(read) => {
            notes.extend(
                read.problems
                    .iter()
                    .map(|p| format!("{}: {p}", path.display())),
            );
            Some(read.file)
        }
        Err(e) => {
            notes.push(format!("{e:#}; its settings are left at their defaults"));
            None
        }
    }
}

/// Both files, read now; the notes say what was left out.
pub fn load(machine: &Path, person: Option<&Path>) -> (Settings, Vec<String>) {
    let mut notes = Vec::new();
    let machine = read(machine, &mut notes);
    let person = person.and_then(|p| read(p, &mut notes));
    let settings = Settings::from_files(machine.as_ref(), person.as_ref());
    notes.extend(presets::named(settings.preset.as_deref()).1);
    (settings, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> SettingsFile {
        settings::read(text).unwrap().file
    }

    #[test]
    fn the_defaults_are_floating_with_title_bars_always() {
        let settings = Settings::from_files(None, None);
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.policy(), "floating");
        assert!(settings.bars_in("tiling"));
        let empty = file("format = 1\n");
        assert_eq!(
            Settings::from_files(Some(&empty), Some(&empty)),
            Settings::default()
        );
    }

    #[test]
    fn the_persons_file_wins_key_by_key() {
        let machine = file("format = 1\n[layout]\ntiling = true\ntitle_bars = \"floating-only\"\n");
        let person = file("format = 1\n[layout]\ntiling = false\n");
        let settings = Settings::from_files(Some(&machine), Some(&person));
        assert_eq!(settings.tiling, Some(false), "the person's tiling wins");
        assert_eq!(
            settings.title_bars,
            TitleBars::FloatingOnly,
            "the machine's stays"
        );
        assert!(!settings.bars_in("tiling"));
        assert!(settings.bars_in("floating"));
        let only_machine = Settings::from_files(Some(&machine), None);
        assert_eq!(only_machine.policy(), "tiling");
    }

    #[test]
    fn without_shell_tiling_the_presets_policy_holds() {
        // Classic floats, named or not; a preset this release lacks is
        // Classic too.
        for text in [
            "format = 1\n",
            "format = 1\n[layout]\npreset = \"classic\"\n",
            "format = 1\n[layout]\npreset = \"mac-like\"\n",
        ] {
            let settings = Settings::from_files(Some(&file(text)), None);
            assert_eq!(settings.tiling, None);
            assert_eq!(settings.policy(), "floating", "{text}");
        }
        let preset = file("format = 1\n[layout]\npreset = \"classic\"\n");
        let person = file("format = 1\n[layout]\ntiling = true\n");
        let settings = Settings::from_files(Some(&preset), Some(&person));
        assert_eq!(settings.preset.as_deref(), Some("classic"));
        assert_eq!(
            settings.policy(),
            "tiling",
            "layout.tiling wins over the preset"
        );
    }

    #[test]
    fn the_session_starts_the_sound_system_whatever_the_preset() {
        // M5.7b: no preset names its own list, so every preset, and a name
        // this release lacks, starts the release's.
        for name in [None, Some("classic"), Some("hive"), Some("cinnamon")] {
            let settings = Settings {
                preset: name.map(str::to_string),
                ..Settings::default()
            };
            assert_eq!(
                settings.session_start(),
                edel::presets::SESSION_START,
                "{name:?}"
            );
        }
    }

    #[test]
    fn the_buttons_side_is_the_files_else_the_presets() {
        assert_eq!(Settings::default().button_side(), Side::Right);
        let left = file("format = 1\n[layout]\nwindow_buttons = \"left\"\n");
        let settings = Settings::from_files(None, Some(&left));
        assert_eq!(settings.button_side(), Side::Left);
        let odd = file("format = 1\n[layout]\nwindow_buttons = \"top\"\n");
        assert_eq!(
            Settings::from_files(Some(&left), Some(&odd)).button_side(),
            Side::Left
        );
    }

    #[test]
    fn each_button_shows_unless_a_file_hides_it() {
        assert_eq!(Settings::default().buttons, Shown::default());
        let machine = file("format = 1\n[layout]\nminimize_button = false\nclose_button = false\n");
        let person = file("format = 1\n[layout]\nclose_button = true\n");
        let settings = Settings::from_files(Some(&machine), Some(&person));
        assert_eq!(
            settings.buttons,
            Shown {
                close: true,
                minimize: false,
                maximize: true,
            }
        );
    }

    #[test]
    fn only_another_preset_counts_as_a_change_of_preset() {
        let named = |name: Option<&str>| Settings {
            preset: name.map(str::to_string),
            ..Settings::default()
        };
        assert!(named(None).preset_differs(&named(Some("hive"))));
        assert!(!named(None).preset_differs(&named(Some("classic"))));
        assert!(!named(Some("classic")).preset_differs(&named(Some("tablet"))));
        assert!(named(Some("hive")).preset_differs(&named(Some("tablet"))));
    }

    #[test]
    fn the_persons_panels_win() {
        let machine =
            file("format = 1\n[panels]\nlist = [{ edge = \"top\", end = [\"clock\"] }]\n");
        let person = file("format = 1\n[[panels.list]]\nedge = \"bottom\"\nstart = [\"menu\"]\n");
        let both = Settings::from_files(Some(&machine), Some(&person));
        let panels = both.panels.clone().unwrap();
        assert_eq!(panels[0].edge, edel::presets::Edge::Bottom);
    }

    #[test]
    fn motion_is_full_unless_a_file_says_otherwise() {
        assert_eq!(Settings::from_files(None, None).motion, Motion::Full);
        let machine = file("format = 1\n[appearance]\nanimations = \"off\"\n");
        let person = file("format = 1\n[appearance]\nanimations = \"reduced\"\n");
        assert_eq!(
            Settings::from_files(Some(&machine), None).motion,
            Motion::Off
        );
        assert_eq!(
            Settings::from_files(Some(&machine), Some(&person)).motion,
            Motion::Reduced,
            "the person's wins"
        );
    }

    #[test]
    fn the_colour_scheme_is_light_unless_a_file_says_dark() {
        assert_eq!(Settings::from_files(None, None).color_scheme, Scheme::Light);
        let machine = file("format = 1\n[appearance]\nmode = \"dark\"\n");
        let person = file("format = 1\n[appearance]\nmode = \"auto\"\n");
        assert_eq!(
            Settings::from_files(Some(&machine), None).color_scheme,
            Scheme::Dark
        );
        assert_eq!(
            Settings::from_files(Some(&machine), Some(&person)).color_scheme,
            Scheme::Light,
            "the person's wins, and auto is light in this release"
        );
    }

    #[test]
    fn shortcuts_merge_action_by_action() {
        let machine =
            file("format = 1\n[shortcuts]\nclose = \"Super+W\"\nterminal = \"Super+Return\"\n");
        let person = file("format = 1\n[shortcuts]\nclose = \"Super+X\"\n");
        let shortcuts = Settings::from_files(Some(&machine), Some(&person)).shortcuts;
        assert_eq!(shortcuts["close"], "Super+X", "the person's wins");
        assert_eq!(shortcuts["terminal"], "Super+Return", "the machine's stays");
    }

    #[test]
    fn output_scales_come_by_name_and_the_persons_win() {
        let machine =
            file("format = 1\n[displays.eDP-1]\nscale = 1.5\n[displays.HDMI-A-1]\nscale = 1\n");
        let person = file("format = 1\n[displays.eDP-1]\nscale = 2\n[displays.DP-1]\nscale = -1\n");
        let outputs = Settings::from_files(Some(&machine), Some(&person)).outputs;
        assert_eq!(outputs["eDP-1"].scale, Some(2.0));
        assert_eq!(outputs["HDMI-A-1"].scale, Some(1.0));
        assert_eq!(outputs["DP-1"].scale, None, "a scale below 0 is left out");
    }

    #[test]
    fn a_screens_keys_merge_one_by_one() {
        let machine = file(
            "format = 1\n[displays.HDMI-A-1]\nposition = [1920, 0]\nresolution = \"2560x1440\"\nrefresh_rate = 60\n",
        );
        let person = file("format = 1\n[displays.HDMI-A-1]\nenabled = false\n");
        let hdmi = &Settings::from_files(Some(&machine), Some(&person)).outputs["HDMI-A-1"];
        assert_eq!(hdmi.position, Some((1920, 0)));
        assert_eq!(hdmi.mode.as_deref(), Some("2560x1440@60"));
        assert_eq!(
            hdmi.enabled,
            Some(false),
            "the person's key added to the machine's"
        );
        assert_eq!(hdmi.scale, None);
    }

    #[test]
    fn the_tiling_style_is_the_persons_over_the_machines_and_scroll_waits() {
        let machine = file("format = 1\n[layout]\ntiling_style = \"split\"\n");
        assert_eq!(
            Settings::from_files(Some(&machine), None).tiling_style,
            Style::Split
        );
        let person = file("format = 1\n[layout]\ntiling_style = \"stack\"\n");
        let settings = Settings::from_files(Some(&machine), Some(&person));
        assert_eq!(settings.tiling_style, Style::Stack);
        assert_eq!(Settings::default().tiling_style, Style::Stack);
        // The styles the compositor knows are the key's values, its owner.
        let key = edel::settings::KEYS
            .iter()
            .find(|k| k.path == "layout.tiling_style")
            .unwrap();
        let edel::settings::Kind::OneOf(values) = key.kind else {
            panic!("layout.tiling_style is not a choice");
        };
        let names: Vec<&str> = Style::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(names, values);
    }

    #[test]
    fn the_workspace_keys_are_the_persons_over_the_machines_and_the_preset_counts() {
        assert_eq!(Settings::default().workspaces(), 4, "Classic has four");
        assert!(!Settings::default().dynamic());
        let machine = file(
            "format = 1\n[workspaces]\ncount = 2\ndynamic = true\nper_screen = true\nnames = [\"Mail\"]\n",
        );
        let person = file("format = 1\n[workspaces]\ncount = 6\n");
        let both = Settings::from_files(Some(&machine), Some(&person));
        assert_eq!(both.workspaces(), 6, "the person's count wins");
        assert!(both.dynamic());
        assert!(
            both.per_screen(),
            "the machine's key stays when the person's file is silent"
        );
        assert!(
            !Settings::default().per_screen(),
            "absent is every screen together"
        );
        assert_eq!(both.names(), ["Mail"]);
        assert_eq!(Settings::from_files(Some(&machine), None).workspaces(), 2);
        // Apps with a workspace of their own (M5.2l).
        let machine = file(
            "format = 1\n[workspaces]\napps = { \"org.mozilla.firefox\" = 2, \"org.gnome.Nautilus\" = 4 }\n",
        );
        let settings = Settings::from_files(Some(&machine), None);
        assert_eq!(settings.app_workspace("org.mozilla.firefox"), Some(1));
        assert_eq!(settings.app_workspace("org.gnome.Nautilus"), Some(3));
        assert_eq!(settings.app_workspace("foot"), None);
        assert_eq!(
            Settings::default().app_workspace("org.mozilla.firefox"),
            None
        );
        // The person's table replaces the machine's whole, so Nautilus
        // no longer has a rule.
        let person = file("format = 1\n[workspaces]\napps = { \"foot\" = 9 }\n");
        let both = Settings::from_files(Some(&machine), Some(&person));
        assert_eq!(both.app_workspace("org.gnome.Nautilus"), None);
        assert_eq!(both.app_workspace("foot"), Some(8));
        assert_eq!(both.app_workspaces, [("foot".to_string(), 8)]);
    }

    #[test]
    fn a_missing_or_broken_file_is_the_defaults_with_a_note() {
        let dir = std::env::temp_dir().join(format!("edel-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let broken = dir.join("broken.toml");
        std::fs::write(&broken, "format = 1\n[shell\n").unwrap();
        let odd = dir.join("odd.toml");
        std::fs::write(
            &odd,
            "format = 1\n[layout]\ntiling = \"yes\"\ntitle_bars = \"never\"\n",
        )
        .unwrap();
        let (settings, notes) = load(&dir.join("missing.toml"), Some(&broken));
        assert_eq!(settings, Settings::default());
        assert_eq!(notes.len(), 1, "{notes:?}");
        let (settings, notes) = load(&odd, None);
        assert_eq!(settings, Settings::default());
        assert_eq!(notes.len(), 2, "{notes:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
