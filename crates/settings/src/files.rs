//! The settings files Settings reads and writes (M5.6a): the machine's,
//! which `edel settings set` writes as root, and the person's, in their
//! own config folder, which the app writes with no privilege; the desktop reads the person's over the machine's. A choice
//! that is what would apply anyway is taken out of the file rather than
//! written, as writers never write a default (ADR-008).

use std::path::PathBuf;

use edel::presets::{self, Policy};
use edel::{places, system};

/// What the Layout page shows.
#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    pub preset: String,
    pub tiling: bool,
}

pub struct Files {
    pub machine: PathBuf,
    pub person: Option<PathBuf>,
}

impl Files {
    /// This machine's file and this person's.
    pub fn here() -> Files {
        Files {
            machine: places::found(&places::machine_settings()),
            person: places::person_settings().map(|p| places::found(&p)),
        }
    }

    /// One file's `[layout]`, read leniently as the desktop reads it
    /// (ADR-008); nothing for a file that is missing or not TOML.
    fn layout_of(path: Option<&PathBuf>) -> system::Layout {
        path.and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| system::read(&text).ok())
            .map(|read| read.file.layout)
            .unwrap_or_default()
    }

    /// The layout from the machine's keys with `person`'s laid over them,
    /// the preset and its policy under both.
    fn layout_from(machine: &system::Layout, person: &system::Layout) -> Layout {
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
        let machine = Self::layout_of(Some(&self.machine));
        let person = Self::layout_of(self.person.as_ref());
        Self::layout_from(&machine, &person)
    }

    /// Chooses preset `name`: written to the person's file, or taken out
    /// of it when it is what would apply without it.
    pub fn choose_preset(&self, name: &str) -> Result<(), String> {
        let machine = Self::layout_of(Some(&self.machine));
        let mut person = Self::layout_of(self.person.as_ref());
        let had = person.preset.take().is_some();
        let without = Self::layout_from(&machine, &person);
        match (without.preset != name, had) {
            (true, _) => self.set("layout.preset", Some(name)),
            (false, true) => self.set("layout.preset", None),
            (false, false) => Ok(()),
        }
    }

    /// Whether windows tile: written, or taken out when the machine's file
    /// or the preset already says so.
    pub fn choose_tiling(&self, on: bool) -> Result<(), String> {
        let machine = Self::layout_of(Some(&self.machine));
        let mut person = Self::layout_of(self.person.as_ref());
        let had = person.tiling.take().is_some();
        let without = Self::layout_from(&machine, &person);
        match (without.tiling != on, had) {
            (true, _) => self.set("layout.tiling", Some(if on { "true" } else { "false" })),
            (false, true) => self.set("layout.tiling", None),
            (false, false) => Ok(()),
        }
    }

    /// Sets `key` to `value` in the person's file, or takes it out with
    /// none, with the functions `edel settings set` and `reset` use, so a
    /// refused value reads the same in both; the message when it cannot.
    pub fn set(&self, key: &str, value: Option<&str>) -> Result<(), String> {
        let path = self
            .person
            .as_ref()
            .ok_or("there is no home folder to keep your settings in")?;
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|_| format!("format = {}\n", system::FORMAT));
        let edited = match value {
            Some(value) => system::set(&text, key, value),
            None => system::unset(&text, key),
        }
        .map_err(|e| format!("{e:#}"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("could not make {}: {e}", dir.display()))?;
        }
        // Through a rename, so the desktop never reads half a file.
        let new = path.with_extension("toml.edel-new");
        std::fs::write(&new, edited)
            .and_then(|()| std::fs::rename(&new, path))
            .map_err(|e| format!("could not write {}: {e}", path.display()))
    }
}

/// A preset's name as people read it: `mac-like` is Mac-like.
pub fn title(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
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
    fn writers_never_write_a_default() {
        let dir = scratch("defaults");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        let read = || std::fs::read_to_string(&person).unwrap_or_default();
        // Mac-like is written; Classic, the default, takes it out again.
        files.choose_preset("mac-like").unwrap();
        assert!(
            read().contains("[layout]\npreset = \"mac-like\""),
            "{}",
            read()
        );
        files.choose_preset("classic").unwrap();
        assert!(!read().contains("preset"), "{}", read());
        // With the machine on Windows-like, Classic must be written.
        std::fs::write(
            &files.machine,
            "format = 1\n[layout]\npreset = \"windows-like\"\n",
        )
        .unwrap();
        files.choose_preset("classic").unwrap();
        assert!(read().contains("classic"));
        assert_eq!(files.layout().preset, "classic");
        files.choose_preset("windows-like").unwrap();
        assert!(!read().contains("preset"));
        // Hive tiles: turning tiling on there writes nothing, off writes.
        files.choose_preset("hive").unwrap();
        files.choose_tiling(true).unwrap();
        assert!(!read().contains("tiling"));
        files.choose_tiling(false).unwrap();
        assert!(read().contains("tiling = false"));
        assert!(!files.layout().tiling);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_is_what_edel_settings_set_writes() {
        let dir = scratch("same");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        files.choose_preset("mac-like").unwrap();
        let by_command = system::set(
            &format!("format = {}\n", system::FORMAT),
            "layout.preset",
            "mac-like",
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&person).unwrap(), by_command);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refused_value_reads_as_the_command_says_it() {
        let dir = scratch("refused");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(dir.join("person.toml")),
        };
        let row = files.set("layout.preset", Some("hiv")).unwrap_err();
        let command = system::set("format = 1\n", "layout.preset", "hiv").unwrap_err();
        assert_eq!(row, format!("{command:#}"));
        assert!(row.contains("did you mean"), "{row}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preset_titles_read_as_people_write_them() {
        assert_eq!(title("classic"), "Classic");
        assert_eq!(title("mac-like"), "Mac-like");
        assert_eq!(title("windows-like"), "Windows-like");
    }
}
