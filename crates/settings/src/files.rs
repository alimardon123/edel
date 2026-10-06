//! The settings files Settings reads and writes (M5.6a): the machine's,
//! which `edel settings set` writes as root, and the person's, in their
//! own config folder, which the app writes with no privilege; the desktop
//! reads the person's over the machine's. A choice
//! that is what would apply anyway is taken out of the file rather than
//! written, as writers never write a default (ADR-008).

use std::path::PathBuf;

use edel::presets::{self, Policy};
use edel::{places, system};

/// What the Layout page shows: each key's value as the desktop applies
/// it, from the person's file, the machine's, the preset or the release.
#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    pub preset: String,
    pub tiling: bool,
    pub title_bars: String,
    pub window_buttons: String,
}

impl Layout {
    /// `key`'s value as `edel settings set` writes it.
    pub fn value(&self, key: &str) -> Option<String> {
        match key {
            "layout.preset" => Some(self.preset.clone()),
            "layout.tiling" => Some(self.tiling.to_string()),
            "layout.title_bars" => Some(self.title_bars.clone()),
            "layout.window_buttons" => Some(self.window_buttons.clone()),
            _ => None,
        }
    }
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

    /// One file read leniently as the desktop reads it (ADR-008); nothing
    /// for a file that is missing or not TOML.
    fn read(path: Option<&PathBuf>) -> Option<system::SystemFile> {
        path.and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| system::read(&text).ok())
            .map(|read| read.file)
    }

    /// One file's `[layout]`, empty when there is no file.
    fn layout_of(path: Option<&PathBuf>) -> system::Layout {
        Self::read(path).map(|f| f.layout).unwrap_or_default()
    }

    /// The layout from the machine's keys with `person`'s laid over them,
    /// the preset and the release under both.
    fn layout_from(machine: &system::Layout, person: &system::Layout) -> Layout {
        let chosen = person.preset.clone().or(machine.preset.clone());
        let (preset, _) = presets::named(chosen.as_deref());
        let name = chosen
            .filter(|n| presets::NAMES.contains(&n.as_str()))
            .unwrap_or_else(|| presets::DEFAULT.to_string());
        let known = |key: &str, value: Option<&String>| {
            value
                .filter(|v| crate::rows::values(key).contains(&v.as_str()))
                .cloned()
        };
        let title_bars = person.title_bars.as_ref().or(machine.title_bars.as_ref());
        let buttons = person
            .window_buttons
            .as_ref()
            .or(machine.window_buttons.as_ref());
        Layout {
            preset: name,
            tiling: person
                .tiling
                .or(machine.tiling)
                .unwrap_or(preset.windows.policy == Policy::Tiling),
            // Absent, every window has its bar (docs/settings.md).
            title_bars: known("layout.title_bars", title_bars)
                .unwrap_or_else(|| "always".to_string()),
            window_buttons: known("layout.window_buttons", buttons)
                .unwrap_or_else(|| preset.windows.buttons.name().to_string()),
        }
    }

    /// The layout the desktop shows: the person's keys over the
    /// machine's, then the preset, Classic when none is chosen.
    pub fn layout(&self) -> Layout {
        let machine = Self::layout_of(Some(&self.machine));
        let person = Self::layout_of(self.person.as_ref());
        Self::layout_from(&machine, &person)
    }

    /// Chooses `value` for the layout key `key`: written to the person's
    /// file, or taken out of it when it is what would apply without it,
    /// as writers never write a default (ADR-008).
    pub fn choose(&self, key: &str, value: &str) -> Result<(), String> {
        let machine = Self::layout_of(Some(&self.machine));
        let mut person = Self::layout_of(self.person.as_ref());
        let had = match key {
            "layout.preset" => person.preset.take().is_some(),
            "layout.tiling" => person.tiling.take().is_some(),
            "layout.title_bars" => person.title_bars.take().is_some(),
            "layout.window_buttons" => person.window_buttons.take().is_some(),
            _ => return Err(format!("{key} is not on the Layout page")),
        };
        let without = Self::layout_from(&machine, &person);
        match (without.value(key).as_deref() != Some(value), had) {
            (true, _) => self.set(key, Some(value)),
            (false, true) => self.set(key, None),
            (false, false) => Ok(()),
        }
    }

    /// The keys of `action` as the desktop follows them: the person's
    /// `[shortcuts]` over the machine's over the release's.
    pub fn shortcut(&self, action: &str) -> Option<String> {
        let shortcuts = |path| Self::read(path).map(|f| f.shortcuts).unwrap_or_default();
        let mut table = shortcuts(Some(&self.machine));
        table.extend(shortcuts(self.person.as_ref()));
        let (resolved, _) = edel::shortcuts::resolve(&table);
        resolved
            .into_iter()
            .find(|(a, _)| a.name == action)
            .and_then(|(_, keys)| keys)
            .map(|keys| keys.to_string())
    }

    /// Where `key`'s value comes from: the person's file, the machine's,
    /// or neither (M5.6b).
    pub fn source(&self, key: &str) -> system::Source {
        let read = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        system::source(
            key,
            read(Some(&self.machine)).as_deref(),
            read(self.person.as_ref()).as_deref(),
        )
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
        files.choose("layout.preset", "mac-like").unwrap();
        assert!(
            read().contains("[layout]\npreset = \"mac-like\""),
            "{}",
            read()
        );
        files.choose("layout.preset", "classic").unwrap();
        assert!(!read().contains("preset"), "{}", read());
        // With the machine on Windows-like, Classic must be written.
        std::fs::write(
            &files.machine,
            "format = 1\n[layout]\npreset = \"windows-like\"\n",
        )
        .unwrap();
        files.choose("layout.preset", "classic").unwrap();
        assert!(read().contains("classic"));
        assert_eq!(files.layout().preset, "classic");
        files.choose("layout.preset", "windows-like").unwrap();
        assert!(!read().contains("preset"));
        // Hive tiles: turning tiling on there writes nothing, off writes.
        files.choose("layout.preset", "hive").unwrap();
        files.choose("layout.tiling", "true").unwrap();
        assert!(!read().contains("tiling"));
        files.choose("layout.tiling", "false").unwrap();
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
        files.choose("layout.preset", "mac-like").unwrap();
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
    fn reset_after_mac_like_takes_the_key_out() {
        let dir = scratch("reset");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        files.choose("layout.preset", "mac-like").unwrap();
        assert!(matches!(
            files.source("layout.preset"),
            system::Source::Person(_)
        ));
        files.set("layout.preset", None).unwrap();
        assert!(!std::fs::read_to_string(&person).unwrap().contains("preset"));
        assert_eq!(files.source("layout.preset"), system::Source::Release);
        assert_eq!(files.layout().preset, "classic");
        std::fs::write(&files.machine, "format = 1\n[layout]\npreset = \"hive\"\n").unwrap();
        assert!(matches!(
            files.source("layout.preset"),
            system::Source::Machine(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preset_titles_read_as_people_write_them() {
        assert_eq!(title("classic"), "Classic");
        assert_eq!(title("mac-like"), "Mac-like");
        assert_eq!(title("windows-like"), "Windows-like");
    }
}
