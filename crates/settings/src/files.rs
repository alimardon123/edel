//! The settings files Settings reads and writes (M5.6a): the machine's,
//! which `edel settings set` writes as root, and the person's, in their
//! own config folder, which the app writes with no privilege; the desktop
//! reads the person's over the machine's. A choice
//! that is what would apply anyway is taken out of the file rather than
//! written, as writers never write a default (ADR-008).

use std::collections::BTreeMap;
use std::path::PathBuf;

use edel::presets::{self, Panel, Policy};
use edel::{panel_edit, places, settings};

/// What the Layout page shows: each key's value as the desktop applies
/// it, from the person's file, the machine's, the preset or the release.
#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    pub preset: String,
    pub tiling: bool,
    pub title_bars: String,
    pub window_buttons: String,
    /// How tiling lays windows out (M5.16a).
    pub tiling_style: String,
    /// Each title bar button shown (M5.18a).
    pub close_button: bool,
    pub minimize_button: bool,
    pub maximize_button: bool,
}

impl Layout {
    /// `key`'s value as `edel settings set` writes it.
    pub fn value(&self, key: &str) -> Option<String> {
        match key {
            "layout.preset" => Some(self.preset.clone()),
            "layout.tiling" => Some(self.tiling.to_string()),
            "layout.title_bars" => Some(self.title_bars.clone()),
            "layout.window_buttons" => Some(self.window_buttons.clone()),
            "layout.tiling_style" => Some(self.tiling_style.clone()),
            "layout.close_button" => Some(self.close_button.to_string()),
            "layout.minimize_button" => Some(self.minimize_button.to_string()),
            "layout.maximize_button" => Some(self.maximize_button.to_string()),
            _ => None,
        }
    }
}

/// `key`'s value in one file's `[layout]`, as `edel settings set` writes
/// it; none when the file has none.
fn own(layout: &settings::Layout, key: &str) -> Option<String> {
    let flag = |v: Option<bool>| v.map(|b| b.to_string());
    match key {
        "layout.preset" => layout.preset.clone(),
        "layout.tiling" => flag(layout.tiling),
        "layout.title_bars" => layout.title_bars.clone(),
        "layout.window_buttons" => layout.window_buttons.clone(),
        "layout.tiling_style" => layout.tiling_style.clone(),
        "layout.close_button" => flag(layout.close_button),
        "layout.minimize_button" => flag(layout.minimize_button),
        "layout.maximize_button" => flag(layout.maximize_button),
        _ => None,
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
    fn read(path: Option<&PathBuf>) -> Option<settings::SettingsFile> {
        path.and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| settings::read(&text).ok())
            .map(|read| read.file)
    }

    /// One file's `[layout]`, empty when there is no file.
    fn layout_of(path: Option<&PathBuf>) -> settings::Layout {
        Self::read(path).map(|f| f.layout).unwrap_or_default()
    }

    /// The layout from the machine's keys with `person`'s laid over them,
    /// the preset and the release under both.
    fn layout_from(machine: &settings::Layout, person: &settings::Layout) -> Layout {
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
        let style = person
            .tiling_style
            .as_ref()
            .or(machine.tiling_style.as_ref());
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
            // Absent, tiling stacks (docs/settings.md).
            tiling_style: known("layout.tiling_style", style)
                .unwrap_or_else(|| "stack".to_string()),
            // Absent, every bar shows each button (docs/settings.md).
            close_button: person.close_button.or(machine.close_button).unwrap_or(true),
            minimize_button: person
                .minimize_button
                .or(machine.minimize_button)
                .unwrap_or(true),
            maximize_button: person
                .maximize_button
                .or(machine.maximize_button)
                .unwrap_or(true),
        }
    }

    /// The layout the desktop shows: the person's keys over the
    /// machine's, then the preset, Classic when none is chosen.
    pub fn layout(&self) -> Layout {
        let machine = Self::layout_of(Some(&self.machine));
        let person = Self::layout_of(self.person.as_ref());
        Self::layout_from(&machine, &person)
    }

    /// The panels that apply, as shell-ui draws them (M5.31d): the person's
    /// `layout.panels` over the machine's, else the preset's.
    pub fn panels(&self) -> Vec<Panel> {
        let read = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        panel_edit::applying(
            read(Some(&self.machine)).as_deref(),
            read(self.person.as_ref()).as_deref(),
        )
    }

    /// The apps the apps widget pins that apply (M5.31d): the person's
    /// `apps.pinned` over the machine's, else the preset's.
    pub fn pins(&self) -> Vec<String> {
        let read = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        panel_edit::pins_applying(
            read(Some(&self.machine)).as_deref(),
            read(self.person.as_ref()).as_deref(),
        )
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
            "layout.tiling_style" => person.tiling_style.take().is_some(),
            "layout.close_button" => person.close_button.take().is_some(),
            "layout.minimize_button" => person.minimize_button.take().is_some(),
            "layout.maximize_button" => person.maximize_button.take().is_some(),
            _ => return Err(format!("{key} is not on the Layout page")),
        };
        let without = Self::layout_from(&machine, &person);
        match (without.value(key).as_deref() != Some(value), had) {
            (true, _) => self.set(key, Some(value)),
            (false, true) => self.set(key, None),
            (false, false) => Ok(()),
        }
    }

    /// Each screen's `[displays.NAME]` as the desktop follows it: the
    /// person's keys over the machine's, key by key.
    pub fn displays(&self) -> BTreeMap<String, settings::Display> {
        let mut all = Self::read(Some(&self.machine))
            .map(|f| f.displays)
            .unwrap_or_default();
        let person = Self::read(self.person.as_ref())
            .map(|f| f.displays)
            .unwrap_or_default();
        for (name, own) in person {
            let d = all.entry(name).or_default();
            d.position = own.position.or(d.position);
            d.scale = own.scale.or(d.scale);
            d.resolution = own.resolution.or(d.resolution.take());
            d.refresh_rate = own.refresh_rate.or(d.refresh_rate);
            d.enabled = own.enabled.or(d.enabled);
            d.rotation = own.rotation.or(d.rotation);
        }
        all
    }

    /// Chooses `value` for `key`: written to the person's file, or taken
    /// out of it when it is what would apply without it, the machine's
    /// value or else `release`, the one the release gives (ADR-008:
    /// writers never write a default). Numbers are compared as numbers,
    /// so `2` and `2.0` are one value.
    pub fn choose_over(&self, key: &str, value: &str, release: Option<&str>) -> Result<(), String> {
        let read = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        let machine = settings::chosen(key, read(Some(&self.machine)).as_deref(), None);
        let without = machine.or_else(|| release.map(String::from));
        let had = matches!(self.source(key), settings::Source::Person(_));
        match (without.is_some_and(|w| same_value(&w, value)), had) {
            (false, _) => self.set(key, Some(value)),
            (true, true) => self.set(key, None),
            (true, false) => Ok(()),
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

    /// Whether banners are kept away (M5.9b): the person's file over the
    /// machine's, off when neither says.
    pub fn do_not_disturb(&self) -> bool {
        let read = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        settings::flag(
            settings::DO_NOT_DISTURB,
            read(Some(&self.machine)).as_deref(),
            read(self.person.as_ref()).as_deref(),
        )
        .unwrap_or(false)
    }

    /// Where `key`'s value comes from: the person's file, the machine's,
    /// or neither (M5.6b).
    pub fn source(&self, key: &str) -> settings::Source {
        let read = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        settings::source(
            key,
            read(Some(&self.machine)).as_deref(),
            read(self.person.as_ref()).as_deref(),
        )
    }

    /// The person's own value of each of the Layout page's keys, as
    /// `edel settings set` writes it, none where their file has none: what
    /// Undo puts back (M5.6a).
    pub fn own_layout(&self) -> Vec<(&'static str, Option<String>)> {
        let person = Self::layout_of(self.person.as_ref());
        crate::rows::on_page("layout")
            .map(|row| (row.key, own(&person, row.key)))
            .collect()
    }

    /// Puts each of the Layout page's keys back as `before` held them
    /// (Undo), with the functions `edel settings set` and `reset` use, so
    /// the person's other keys, a change made elsewhere to them and the
    /// file's comments all stay.
    pub fn restore(&self, before: &[(&'static str, Option<String>)]) -> Result<(), String> {
        let now = self.own_layout();
        for (key, was) in before {
            let is = now
                .iter()
                .find(|(k, _)| k == key)
                .and_then(|(_, v)| v.clone());
            if *was != is {
                self.set(key, was.as_deref())?;
            }
        }
        Ok(())
    }

    /// Sets `key` to `value` in the person's file, or takes it out with
    /// none, with the functions `edel settings set` and `reset` use, so a
    /// refused value reads the same in both; the message when it cannot.
    pub fn set(&self, key: &str, value: Option<&str>) -> Result<(), String> {
        let path = self
            .person
            .as_ref()
            .ok_or("there is no home folder to keep your settings in")?;
        settings::write(path, key, value).map_err(|e| format!("{e:#}"))
    }
}

/// Whether two values are one: equal, or equal as numbers (`2` and `2.0`).
fn same_value(a: &str, b: &str) -> bool {
    a == b
        || a.parse::<f64>()
            .ok()
            .zip(b.parse::<f64>().ok())
            .is_some_and(|(a, b)| (a - b).abs() < 1e-9)
}

/// A value's name as people read it, with a capital: `floating only` is
/// Floating only. A preset's is `edel::presets::title`, which translates.
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
        // A button hidden is written; shown again, it is taken out.
        files.choose("layout.minimize_button", "false").unwrap();
        assert!(read().contains("minimize_button = false"), "{}", read());
        assert!(!files.layout().minimize_button);
        files.choose("layout.minimize_button", "true").unwrap();
        assert!(!read().contains("minimize_button"), "{}", read());
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
        let by_command = settings::set(
            &format!("format = {}\n", settings::FORMAT),
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
        let command = settings::set("format = 1\n", "layout.preset", "hiv").unwrap_err();
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
            settings::Source::Person(_)
        ));
        files.set("layout.preset", None).unwrap();
        assert!(!std::fs::read_to_string(&person).unwrap().contains("preset"));
        assert_eq!(files.source("layout.preset"), settings::Source::Release);
        assert_eq!(files.layout().preset, "classic");
        std::fs::write(&files.machine, "format = 1\n[layout]\npreset = \"hive\"\n").unwrap();
        assert!(matches!(
            files.source("layout.preset"),
            settings::Source::Machine(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_puts_the_pages_keys_back_and_leaves_the_rest() {
        let dir = scratch("undo");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        // Nothing chosen at first: Undo takes a choice out again.
        let before = files.own_layout();
        assert!(before.iter().all(|(_, v)| v.is_none()));
        files.choose("layout.preset", "hive").unwrap();
        files.choose("layout.minimize_button", "false").unwrap();
        assert_ne!(files.own_layout(), before);
        files.restore(&before).unwrap();
        assert_eq!(files.own_layout(), before);
        assert_eq!(files.layout().preset, "classic");
        // A choice made before is brought back, and a key another page or
        // `edel settings set` changed meanwhile stays as it is now.
        files.choose("layout.preset", "mac-like").unwrap();
        let before = files.own_layout();
        files.choose("layout.preset", "hive").unwrap();
        files.set("appearance.mode", Some("dark")).unwrap();
        files.restore(&before).unwrap();
        assert_eq!(files.layout().preset, "mac-like");
        let text = std::fs::read_to_string(&person).unwrap();
        assert!(text.contains("mode = \"dark\""), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_screens_default_is_never_written() {
        let dir = scratch("displays");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        let read = || std::fs::read_to_string(&person).unwrap_or_default();
        let key = "displays.Virtual-1.scale";
        // The compositor worked out 1.5 for this screen: choosing it
        // writes nothing, another scale is written, and 1.5 takes it out.
        files.choose_over(key, "1.5", Some("1.5")).unwrap();
        assert!(!read().contains("scale"), "{}", read());
        files.choose_over(key, "2", Some("1.5")).unwrap();
        assert!(
            read().contains("[displays.Virtual-1]\nscale = 2"),
            "{}",
            read()
        );
        assert_eq!(files.displays()["Virtual-1"].scale, Some(2.0));
        files.choose_over(key, "1.50", Some("1.5")).unwrap();
        assert!(!read().contains("scale"), "{}", read());
        // With the machine on 2, 1.5 must be written to override it, and
        // 2 is the machine's own, so it is taken out again.
        std::fs::write(
            &files.machine,
            "format = 1\n[displays.Virtual-1]\nscale = 2.0\nposition = [5, 6]\n",
        )
        .unwrap();
        files.choose_over(key, "1.5", Some("1.5")).unwrap();
        assert!(read().contains("scale = 1.5"), "{}", read());
        files
            .set("displays.Virtual-1.enabled", Some("false"))
            .unwrap();
        let merged = &files.displays()["Virtual-1"];
        assert_eq!(merged.scale, Some(1.5), "the person's over the machine's");
        assert_eq!(merged.position, Some([5, 6]), "the machine's, key by key");
        assert_eq!(merged.enabled, Some(false));
        files.choose_over(key, "2", Some("1.5")).unwrap();
        assert!(!read().contains("scale"), "{}", read());
        // A flag: on is the default and is never written.
        files
            .choose_over("displays.Virtual-1.enabled", "true", Some("true"))
            .unwrap();
        assert!(!read().contains("enabled"), "{}", read());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn do_not_disturb_is_off_until_a_file_says_so_and_is_never_written_off() {
        let dir = scratch("dnd");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        let read = || std::fs::read_to_string(&person).unwrap_or_default();
        let key = settings::DO_NOT_DISTURB;
        assert!(!files.do_not_disturb());
        // Off is the default: choosing it writes nothing.
        files.choose_over(key, "false", Some("false")).unwrap();
        assert!(!read().contains("do_not_disturb"), "{}", read());
        files.choose_over(key, "true", Some("false")).unwrap();
        assert!(read().contains("do_not_disturb = true"), "{}", read());
        assert!(files.do_not_disturb());
        files.choose_over(key, "false", Some("false")).unwrap();
        assert!(!read().contains("do_not_disturb"), "{}", read());
        // With the machine keeping banners away, off must be written.
        std::fs::write(
            &files.machine,
            "format = 1\n[notifications]\ndo_not_disturb = true\n",
        )
        .unwrap();
        assert!(files.do_not_disturb());
        files.choose_over(key, "false", Some("false")).unwrap();
        assert!(read().contains("do_not_disturb = false"), "{}", read());
        assert!(!files.do_not_disturb());
        // The row's file is what `edel settings set` writes, and a refusal reads the same.
        let by_command = settings::set("format = 1\n", key, "true").unwrap();
        files.set(key, None).unwrap();
        std::fs::remove_file(&files.machine).unwrap();
        files.choose_over(key, "true", Some("false")).unwrap();
        assert_eq!(read(), by_command);
        let refused = files.set(key, Some("maybe")).unwrap_err();
        assert_eq!(
            refused,
            "notifications.do_not_disturb: expected true or false, not \"maybe\""
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preset_titles_read_as_people_write_them() {
        assert_eq!(title("classic"), "Classic");
        assert_eq!(title("mac-like"), "Mac-like");
        assert_eq!(title("windows-like"), "Windows-like");
    }
}
