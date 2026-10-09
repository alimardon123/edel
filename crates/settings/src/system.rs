//! What the rows read and write (M5.6a): the machine's system file and
//! the person's, through `edel::system`, as the desktop reads them and as
//! `edel system set` writes them.

use std::path::PathBuf;

use edel::presets::{self, Policy};
use edel::system;

/// The two files the desktop reads, the person's over the machine's.
pub struct Files {
    pub machine: PathBuf,
    pub person: Option<PathBuf>,
}

/// The layout as the desktop has it now: the preset and whether its
/// windows tile.
#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    pub preset: String,
    pub tiling: bool,
}

impl Files {
    /// This machine's files.
    pub fn here() -> Files {
        Files {
            machine: PathBuf::from(system::MACHINE_FILE),
            person: system::person_file(),
        }
    }

    /// One file's `[shell]` keys, read leniently as the desktop reads
    /// them (ADR-008); nothing for a file that is missing or not TOML.
    fn shell_of(path: Option<&PathBuf>) -> system::Shell {
        path.and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| system::read(&text).ok())
            .map(|read| read.file.shell)
            .unwrap_or_default()
    }

    /// The layout from the machine's keys with `person`'s laid over them,
    /// the presets and their policies under both.
    fn layout_from(machine: &system::Shell, person: &system::Shell) -> Layout {
        let chosen = person.preset.clone().or(machine.preset.clone());
        let (preset, _) = presets::named(chosen.as_deref());
        let name = chosen
            .filter(|n| presets::NAMES.contains(&n.as_str()))
            .unwrap_or_else(|| presets::NAMES[0].to_string());
        Layout {
            preset: name,
            tiling: person
                .tiling
                .or(machine.tiling)
                .unwrap_or(preset.windows.policy == Policy::Tiling),
        }
    }

    /// The layout the desktop shows: the person's keys over the
    /// machine's, then the preset, Classic when none is chosen.
    pub fn layout(&self) -> Layout {
        let machine = Self::shell_of(Some(&self.machine));
        let person = Self::shell_of(self.person.as_ref());
        Self::layout_from(&machine, &person)
    }

    /// Chooses preset `name`: written to the person's file, or taken out
    /// of it when it is what would apply without it, as writers never
    /// write a default (ADR-008).
    pub fn choose_preset(&self, name: &str) -> Result<(), String> {
        let machine = Self::shell_of(Some(&self.machine));
        let mut person = Self::shell_of(self.person.as_ref());
        let had = person.preset.take().is_some();
        let without = Self::layout_from(&machine, &person);
        match (without.preset != name, had) {
            (true, _) => self.set("shell.preset", Some(name)),
            (false, true) => self.set("shell.preset", None),
            (false, false) => Ok(()),
        }
    }

    /// Whether windows tile: written, or taken out when the machine's file
    /// or the preset already says so.
    pub fn choose_tiling(&self, on: bool) -> Result<(), String> {
        let machine = Self::shell_of(Some(&self.machine));
        let mut person = Self::shell_of(self.person.as_ref());
        let had = person.tiling.take().is_some();
        let without = Self::layout_from(&machine, &person);
        match (without.tiling != on, had) {
            (true, _) => self.set("shell.tiling", Some(if on { "true" } else { "false" })),
            (false, true) => self.set("shell.tiling", None),
            (false, false) => Ok(()),
        }
    }

    /// Sets `key` to `value` in the person's file, or takes it out with
    /// none, as `edel system set` and `unset` change a file; the message
    /// to show when it cannot.
    pub fn set(&self, key: &str, value: Option<&str>) -> Result<(), String> {
        let path = self
            .person
            .as_ref()
            .ok_or("Settings needs HOME or XDG_CONFIG_HOME to find your settings")?;
        system::edit_file(path, |text| match value {
            Some(value) => system::set(text, key, value),
            None => system::unset(text, key),
        })
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
    }
}

/// A preset's name as people read it: `mac-like` is Mac-like.
pub fn title(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn choosing_a_preset_writes_what_edel_system_set_writes() {
        let dir = scratch("set");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(dir.join("person/edel/system.toml")),
        };
        assert_eq!(
            files.layout(),
            Layout {
                preset: "classic".into(),
                tiling: false
            }
        );
        files.set("shell.preset", Some("mac-like")).unwrap();
        let written = std::fs::read_to_string(files.person.as_ref().unwrap()).unwrap();
        let by_command = system::set("format = 1\n", "shell.preset", "mac-like").unwrap();
        assert_eq!(written, by_command);
        assert_eq!(files.layout().preset, "mac-like");
        // Hive tiles unless shell.tiling says otherwise.
        files.set("shell.preset", Some("hive")).unwrap();
        assert!(files.layout().tiling);
        files.set("shell.tiling", Some("false")).unwrap();
        assert!(!files.layout().tiling);
        files.set("shell.tiling", None).unwrap();
        assert!(files.layout().tiling);
        // The command's checks are the row's: the same message.
        let error = files.set("shell.preset", Some("tablet")).unwrap_err();
        let by_command = system::set("format = 1\n", "shell.preset", "tablet").unwrap_err();
        assert_eq!(error, format!("{by_command:#}"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_persons_file_wins_over_the_machines() {
        let dir = scratch("layers");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(dir.join("person.toml")),
        };
        std::fs::write(
            &files.machine,
            "format = 1\n[shell]\npreset = \"windows-like\"\n",
        )
        .unwrap();
        assert_eq!(files.layout().preset, "windows-like");
        std::fs::write(
            files.person.as_ref().unwrap(),
            "format = 1\n[shell]\npreset = \"hive\"\n",
        )
        .unwrap();
        assert_eq!(files.layout().preset, "hive");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writers_never_write_a_default() {
        let dir = scratch("defaults");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        // Mac-like is written; Classic, the default, takes it out again.
        files.choose_preset("mac-like").unwrap();
        assert!(
            std::fs::read_to_string(&person)
                .unwrap()
                .contains("mac-like")
        );
        files.choose_preset("classic").unwrap();
        assert!(!std::fs::read_to_string(&person).unwrap().contains("preset"));
        // With the machine on Windows-like, Classic must be written.
        std::fs::write(
            &files.machine,
            "format = 1\n[shell]\npreset = \"windows-like\"\n",
        )
        .unwrap();
        files.choose_preset("classic").unwrap();
        assert!(
            std::fs::read_to_string(&person)
                .unwrap()
                .contains("classic")
        );
        assert_eq!(files.layout().preset, "classic");
        files.choose_preset("windows-like").unwrap();
        assert!(!std::fs::read_to_string(&person).unwrap().contains("preset"));
        // Hive tiles: turning tiling on there writes nothing, off writes.
        files.choose_preset("hive").unwrap();
        files.choose_tiling(true).unwrap();
        assert!(!std::fs::read_to_string(&person).unwrap().contains("tiling"));
        files.choose_tiling(false).unwrap();
        assert!(
            std::fs::read_to_string(&person)
                .unwrap()
                .contains("tiling = false")
        );
        assert!(!files.layout().tiling);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preset_titles_read_as_people_write_them() {
        assert_eq!(title("mac-like"), "Mac-like");
        assert_eq!(title("windows-like"), "Windows-like");
        assert_eq!(title("hive"), "Hive");
    }
}
