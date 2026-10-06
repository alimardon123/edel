//! What the compositor takes from the settings file (roadmap M4.5): the
//! machine's and the person's, the same schema, found through
//! `edel::places` and read with `edel::system`
//! so the defaults and the leniency are `edel`'s (ADR-008). A key the
//! person's file sets wins; a key neither sets is the preset's (M5.1c),
//! then the default. Reading is
//! lenient: a missing file is the defaults, and a broken one is reported
//! and read as missing, never fatal.

use std::collections::BTreeMap;
use std::path::Path;

use edel::presets::{self, Side};
use edel::system::{self, SystemFile};
pub use edel::tokens::Scheme;

use crate::animation::Motion;

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
    /// `layout.panels` (M5.4e): absent means the preset's; shell-ui reads
    /// them, and the compositor only restarts it when they change.
    pub panels: Option<Vec<edel::presets::Panel>>,
    /// `[displays.NAME]`, by output name.
    pub outputs: BTreeMap<String, OutputSettings>,
    /// `appearance.animations` (M5.11b): absent means full.
    pub motion: Motion,
    /// `appearance.mode` (M5.5c): absent, or `auto`, means dark.
    pub color_scheme: Scheme,
    /// `[shortcuts]`, action to keys as written, the person's over the
    /// machine's; `edel::shortcuts::resolve` lays them over the defaults
    /// (M5.13a).
    pub shortcuts: BTreeMap<String, String>,
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
    pub fn from_files(machine: Option<&SystemFile>, person: Option<&SystemFile>) -> Settings {
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
            if file.layout.panels.is_some() {
                settings.panels.clone_from(&file.layout.panels);
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

    /// How many workspaces the preset has (M5.2a).
    pub fn workspaces(&self) -> usize {
        presets::named(self.preset.as_deref()).0.workspaces.count
    }

    /// Whether `other` lays the desktop out by another preset (M5.4a): a
    /// name this release lacks is Classic, so it differs from Classic only
    /// in name and restarts nothing.
    pub fn preset_differs(&self, other: &Settings) -> bool {
        presets::named(self.preset.as_deref()).0 != presets::named(other.preset.as_deref()).0
    }

    /// Whether a dock hides while a window covers it (M5.4f): from
    /// `layout.panels` if set, else the preset's panels.
    pub fn dock_hides(&self) -> bool {
        match &self.panels {
            Some(panels) => presets::dock_hides(panels),
            None => presets::dock_hides(&presets::named(self.preset.as_deref()).0.panels),
        }
    }

    /// Whether `layout.panels` changed (M5.4e), which shell-ui follows by
    /// starting again, as for a preset.
    pub fn panels_differ(&self, other: &Settings) -> bool {
        self.panels != other.panels
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
pub fn read(path: &Path, notes: &mut Vec<String>) -> Option<SystemFile> {
    if !path.exists() {
        return None;
    }
    match system::read_on_machine(path) {
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

    fn file(text: &str) -> SystemFile {
        system::read(text).unwrap().file
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
    fn the_persons_panels_win_and_only_a_change_of_them_counts() {
        let machine =
            file("format = 1\n[layout]\npanels = [{ edge = \"top\", end = [\"clock\"] }]\n");
        let person = file("format = 1\n[[layout.panels]]\nedge = \"bottom\"\nstart = [\"menu\"]\n");
        let both = Settings::from_files(Some(&machine), Some(&person));
        let panels = both.panels.clone().unwrap();
        assert_eq!(panels[0].edge, edel::presets::Edge::Bottom);
        let only_machine = Settings::from_files(Some(&machine), None);
        assert!(both.panels_differ(&only_machine));
        assert!(!both.panels_differ(&both.clone()));
        assert!(Settings::default().panels_differ(&only_machine));
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
    fn the_colour_scheme_is_dark_unless_a_file_says_light() {
        assert_eq!(Settings::from_files(None, None).color_scheme, Scheme::Dark);
        let machine = file("format = 1\n[appearance]\nmode = \"light\"\n");
        let person = file("format = 1\n[appearance]\nmode = \"auto\"\n");
        assert_eq!(
            Settings::from_files(Some(&machine), None).color_scheme,
            Scheme::Light
        );
        assert_eq!(
            Settings::from_files(Some(&machine), Some(&person)).color_scheme,
            Scheme::Dark,
            "the person's wins, and auto is dark in this release"
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
