//! What the compositor takes from the system file (roadmap M4.5): the
//! machine's `/data/edel/system.toml` and the person's
//! `~/.config/edel/system.toml`, the same schema, read with `edel::system`
//! so the defaults and the leniency are `edel`'s (ADR-008). A key the
//! person's file sets wins; a key neither sets is the default. Reading is
//! lenient: a missing file is the defaults, and a broken one is reported
//! and read as missing, never fatal.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use edel::system::{self, SystemFile};

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
    /// `shell.tiling`: workspaces tile; absent means floating.
    pub tiling: bool,
    /// `shell.title_bars`: absent means always.
    pub title_bars: TitleBars,
    /// `[outputs.NAME]`, by output name.
    pub outputs: BTreeMap<String, OutputSettings>,
    /// `appearance.motion` (M5.11b): absent means full.
    pub motion: Motion,
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
            if let Some(tiling) = file.shell.tiling {
                settings.tiling = tiling;
            }
            match file.shell.title_bars.as_deref() {
                Some("always") => settings.title_bars = TitleBars::Always,
                Some("floating-only") => settings.title_bars = TitleBars::FloatingOnly,
                _ => {}
            }
            if let Some(motion) = file.appearance.motion.as_deref().and_then(Motion::parse) {
                settings.motion = motion;
            }
            for (name, output) in &file.outputs {
                let into = settings.outputs.entry(name.clone()).or_default();
                if let Some(scale) = output.scale.filter(|s| s.is_finite() && *s > 0.0) {
                    into.scale = Some(scale);
                }
                if let Some([x, y]) = output.position {
                    if let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) {
                        into.position = Some((x, y));
                    }
                }
                if output.mode.is_some() {
                    into.mode.clone_from(&output.mode);
                }
                if output.enabled.is_some() {
                    into.enabled = output.enabled;
                }
            }
        }
        settings
    }

    /// The policy workspaces start in, or switch to when `shell.tiling`
    /// changes.
    pub fn policy(&self) -> &'static str {
        if self.tiling { "tiling" } else { "floating" }
    }

    /// Whether windows under `policy` get the compositor's title bars.
    pub fn bars_in(&self, policy: &str) -> bool {
        self.title_bars == TitleBars::Always || policy == "floating"
    }
}

/// The person's file: `$XDG_CONFIG_HOME/edel/system.toml`, else
/// `~/.config/edel/system.toml`.
pub fn person_file() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(config.join("edel/system.toml"))
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
    (
        Settings::from_files(machine.as_ref(), person.as_ref()),
        notes,
    )
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
        let machine = file("format = 1\n[shell]\ntiling = true\ntitle_bars = \"floating-only\"\n");
        let person = file("format = 1\n[shell]\ntiling = false\n");
        let settings = Settings::from_files(Some(&machine), Some(&person));
        assert!(!settings.tiling, "the person's tiling wins");
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
    fn motion_is_full_unless_a_file_says_otherwise() {
        assert_eq!(Settings::from_files(None, None).motion, Motion::Full);
        let machine = file("format = 1\n[appearance]\nmotion = \"off\"\n");
        let person = file("format = 1\n[appearance]\nmotion = \"reduced\"\n");
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
    fn output_scales_come_by_name_and_the_persons_win() {
        let machine =
            file("format = 1\n[outputs.eDP-1]\nscale = 1.5\n[outputs.HDMI-A-1]\nscale = 1\n");
        let person = file("format = 1\n[outputs.eDP-1]\nscale = 2\n[outputs.DP-1]\nscale = -1\n");
        let outputs = Settings::from_files(Some(&machine), Some(&person)).outputs;
        assert_eq!(outputs["eDP-1"].scale, Some(2.0));
        assert_eq!(outputs["HDMI-A-1"].scale, Some(1.0));
        assert_eq!(outputs["DP-1"].scale, None, "a scale below 0 is left out");
    }

    #[test]
    fn a_screens_keys_merge_one_by_one() {
        let machine =
            file("format = 1\n[outputs.HDMI-A-1]\nposition = [1920, 0]\nmode = \"2560x1440@60\"\n");
        let person = file("format = 1\n[outputs.HDMI-A-1]\nenabled = false\n");
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
            "format = 1\n[shell]\ntiling = \"yes\"\ntitle_bars = \"never\"\n",
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
