//! The settings files Settings reads and writes (M5.6a): the machine's,
//! which `edel settings set` writes as root, and the person's, in their
//! own config folder, which the app writes with no privilege; the desktop
//! reads the person's over the machine's. A choice
//! that is what would apply anyway is taken out of the file rather than
//! written, as writers never write a default (ADR-008).

use std::collections::BTreeMap;
use std::path::PathBuf;

use edel::i18n::{tr, trf};
use edel::presets::{self, Panel, Policy};
use edel::{panel_edit, places, settings};

/// What the Layout, Panels, Workspaces and Appearance pages show: each
/// key's value as the desktop applies it, from the person's file, the
/// machine's, the preset or the release.
#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    /// The built-in preset the desktop lays out from: the preset in use, or
    /// the one an own preset starts from (M5.17).
    pub preset: String,
    /// The preset's name as the files say it, the person's over the
    /// machine's: a built-in preset's, or one of the files' own (M5.17).
    /// The Layout page marks its card by this one.
    pub named: String,
    pub tiling: bool,
    pub title_bars: String,
    pub window_buttons: String,
    /// How tiling lays windows out (M5.16a).
    pub tiling_style: String,
    /// Each title bar button shown (M5.18a).
    pub close_button: bool,
    pub minimize_button: bool,
    pub maximize_button: bool,
    /// How many workspaces (`workspaces.count`).
    pub workspaces: u32,
    /// Whether an empty workspace always waits at the end (`workspaces.dynamic`).
    pub dynamic_workspaces: bool,
    /// Whether each screen shows its own workspace (`workspaces.per_screen`).
    pub workspaces_per_screen: bool,
    /// The switcher's look, `numbers` or `button` (`appearance.switcher_look`).
    pub workspaces_look: String,
    /// How many workspaces the switcher shows at once (`appearance.switcher_shown`).
    pub workspaces_shown: u32,
    /// What the switcher shows where more workspaces lie (`appearance.switcher_ends`).
    pub workspaces_ends: String,
    /// The workspaces' names, the first workspace's first (`workspaces.names`).
    pub workspace_names: Vec<String>,
    /// The wallpaper of each workspace, the first workspace's first, an
    /// empty text Appearance's (`workspaces.wallpapers`, M5.12b5).
    pub workspace_wallpapers: Vec<String>,
    /// Apps and the workspace each always opens on (`workspaces.apps`).
    pub app_workspaces: BTreeMap<String, i64>,
    /// The side the overview's strip of workspaces lies on
    /// (`workspaces.overview_strip`).
    pub overview_strip: String,
    /// Light, dark or automatic (`appearance.mode`, M5.2q).
    pub mode: String,
}

/// A list of texts as the TOML array `edel settings set` takes, such as
/// `["Mail", ""]`.
pub fn list_text(names: &[String]) -> String {
    toml::Value::Array(
        names
            .iter()
            .map(|name| toml::Value::String(name.clone()))
            .collect(),
    )
    .to_string()
}

/// Apps and their workspace numbers as the TOML inline table `edel settings
/// set` takes, such as `{ "org.mozilla.firefox" = 2 }`.
pub fn apps_text(apps: &BTreeMap<String, i64>) -> String {
    let table = apps
        .iter()
        .map(|(app, number)| (app.clone(), toml::Value::Integer(*number)))
        .collect();
    toml::Value::Table(table).to_string()
}

impl Layout {
    /// `key`'s value as `edel settings set` writes it.
    pub fn value(&self, key: &str) -> Option<String> {
        match key {
            "layout.preset" => Some(self.named.clone()),
            "layout.tiling" => Some(self.tiling.to_string()),
            "layout.title_bars" => Some(self.title_bars.clone()),
            "layout.window_buttons" => Some(self.window_buttons.clone()),
            "layout.tiling_style" => Some(self.tiling_style.clone()),
            "layout.close_button" => Some(self.close_button.to_string()),
            "layout.minimize_button" => Some(self.minimize_button.to_string()),
            "layout.maximize_button" => Some(self.maximize_button.to_string()),
            "workspaces.count" => Some(self.workspaces.to_string()),
            "workspaces.dynamic" => Some(self.dynamic_workspaces.to_string()),
            "workspaces.per_screen" => Some(self.workspaces_per_screen.to_string()),
            "appearance.switcher_look" => Some(self.workspaces_look.clone()),
            "appearance.switcher_shown" => Some(self.workspaces_shown.to_string()),
            "appearance.switcher_ends" => Some(self.workspaces_ends.clone()),
            "workspaces.names" => Some(list_text(&self.workspace_names)),
            "workspaces.wallpapers" => Some(list_text(&self.workspace_wallpapers)),
            "workspaces.apps" => Some(apps_text(&self.app_workspaces)),
            "workspaces.overview_strip" => Some(self.overview_strip.clone()),
            "appearance.mode" => Some(self.mode.clone()),
            _ => None,
        }
    }
}

/// `key`'s value in one file's `[layout]`, as `edel settings set` writes
/// it; none when the file has none.
fn own(file: &settings::SettingsFile, key: &str) -> Option<String> {
    let flag = |v: Option<bool>| v.map(|b| b.to_string());
    let (layout, spaces, look) = (&file.layout, &file.workspaces, &file.appearance);
    match key {
        "layout.preset" => layout.preset.clone(),
        "layout.tiling" => flag(layout.tiling),
        "layout.title_bars" => layout.title_bars.clone(),
        "layout.window_buttons" => layout.window_buttons.clone(),
        "layout.tiling_style" => layout.tiling_style.clone(),
        "layout.close_button" => flag(layout.close_button),
        "layout.minimize_button" => flag(layout.minimize_button),
        "layout.maximize_button" => flag(layout.maximize_button),
        "workspaces.count" => spaces.count.map(|n| n.to_string()),
        "workspaces.dynamic" => flag(spaces.dynamic),
        "workspaces.per_screen" => flag(spaces.per_screen),
        "workspaces.names" => spaces.names.as_deref().map(list_text),
        "workspaces.wallpapers" => spaces.wallpapers.as_deref().map(list_text),
        "workspaces.apps" => spaces.apps.as_ref().map(apps_text),
        "workspaces.overview_strip" => spaces.overview_strip.clone(),
        "appearance.wallpaper" => look.wallpaper.clone(),
        "appearance.mode" => look.mode.clone(),
        "appearance.switcher_look" => look.switcher_look.clone(),
        "appearance.switcher_shown" => look.switcher_shown.map(|n| n.to_string()),
        "appearance.switcher_ends" => look.switcher_ends.clone(),
        _ => None,
    }
}

/// The preset's name as the files say it, the person's over the machine's:
/// a built-in preset's, or one of the files' own `[presets]`; the default
/// when neither names one (M5.17). The files are read as they are, not with
/// the preset in use applied, so the name is the one written.
fn named_preset(machine: &settings::SettingsFile, person: &settings::SettingsFile) -> String {
    let said = person
        .layout
        .preset
        .as_ref()
        .or(machine.layout.preset.as_ref());
    match said {
        Some(name)
            if presets::is_built_in(name)
                || person.presets.contains_key(name)
                || machine.presets.contains_key(name) =>
        {
            name.clone()
        }
        _ => presets::DEFAULT.to_string(),
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

    /// One file read leniently as the desktop reads it (ADR-008), with the
    /// theme in use applied (M5.12b6a); nothing for a file that is missing or
    /// not TOML. Writers read through `file_of` too, so this is only for
    /// what is shown: a writer never saves a theme's values.
    fn read(path: Option<&PathBuf>) -> Option<settings::SettingsFile> {
        path.and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| settings::read(&text).ok())
            .map(|read| edel::settings::in_use(&read.file))
    }

    /// One file as the desktop reads it, empty when there is no file.
    fn file_of(path: Option<&PathBuf>) -> settings::SettingsFile {
        Self::read(path).unwrap_or_default()
    }

    /// One file as it is written, read leniently, empty when there is none
    /// (M5.17): the preset's name and its own presets are read this way.
    fn raw_file_of(path: Option<&PathBuf>) -> settings::SettingsFile {
        path.and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| settings::read(&text).ok())
            .map(|read| read.file)
            .unwrap_or_default()
    }

    /// The layout the desktop shows, from `merged`: the person's file laid
    /// over the machine's ([`Files::layers`]), the preset and the release
    /// under both.
    fn layout_from(merged: &settings::SettingsFile) -> Layout {
        let (layout, spaces, appearance) = (&merged.layout, &merged.workspaces, &merged.appearance);
        let chosen = layout.preset.clone();
        let (preset, _) = presets::named(chosen.as_deref());
        let name = chosen
            .filter(|n| presets::NAMES.contains(&n.as_str()))
            .unwrap_or_else(|| presets::DEFAULT.to_string());
        let known = |key: &str, value: Option<&String>| {
            value
                .filter(|v| crate::rows::values(key).contains(&v.as_str()))
                .cloned()
        };
        let most = presets::MOST_WORKSPACES as u32;
        let in_range = |n: u32| (1..=most).contains(&n);
        Layout {
            preset: name,
            // Filled in from the files as they say it (`named_preset`).
            named: String::new(),
            tiling: layout
                .tiling
                .unwrap_or(preset.windows.policy == Policy::Tiling),
            // Absent, the preset's title bars, else every window has its bar
            // (docs/settings.md, M5.18b).
            title_bars: known("layout.title_bars", layout.title_bars.as_ref())
                .or_else(|| preset.windows.title_bars.map(|t| t.name().to_string()))
                .unwrap_or_else(|| "always".to_string()),
            window_buttons: known("layout.window_buttons", layout.window_buttons.as_ref())
                .unwrap_or_else(|| preset.windows.buttons.name().to_string()),
            // Absent, the preset's style, else tiling stacks (docs/settings.md,
            // M5.18b).
            tiling_style: known("layout.tiling_style", layout.tiling_style.as_ref())
                .or_else(|| preset.windows.tiling_style.clone())
                .unwrap_or_else(|| "stack".to_string()),
            // Absent, every bar shows each button (docs/settings.md).
            close_button: layout.close_button.unwrap_or(true),
            minimize_button: layout.minimize_button.unwrap_or(true),
            maximize_button: layout.maximize_button.unwrap_or(true),
            // Absent, the preset's count of workspaces (M5.2n); a number
            // out of range is the same as none.
            workspaces: spaces
                .count
                .filter(|n| in_range(*n))
                .unwrap_or(preset.workspaces.count as u32),
            dynamic_workspaces: spaces.dynamic.unwrap_or(false),
            workspaces_per_screen: spaces.per_screen.unwrap_or(false),
            workspaces_look: known(
                "appearance.switcher_look",
                appearance.switcher_look.as_ref(),
            )
            .unwrap_or_else(|| settings::SWITCHER_LOOK_DEFAULT.to_string()),
            workspaces_shown: appearance
                .switcher_shown
                .filter(|n| in_range(*n))
                .unwrap_or(settings::SWITCHER_SHOWN_DEFAULT),
            workspaces_ends: known(
                "appearance.switcher_ends",
                appearance.switcher_ends.as_ref(),
            )
            .unwrap_or_else(|| settings::SWITCHER_ENDS_DEFAULT.to_string()),
            // Absent, the strip lies on the left (M5.2j-b).
            overview_strip: known("workspaces.overview_strip", spaces.overview_strip.as_ref())
                .unwrap_or_else(|| settings::OVERVIEW_STRIP_DEFAULT.to_string()),
            // The person's list or table is the whole one, not merged with
            // the machine's (M5.2n).
            workspace_names: spaces.names.clone().unwrap_or_default(),
            // The same whole list rule, for each workspace's wallpaper
            // (M5.12b5); absent is Appearance's for every workspace.
            workspace_wallpapers: spaces.wallpapers.clone().unwrap_or_default(),
            app_workspaces: spaces.apps.clone().unwrap_or_default(),
            // Absent, the release's choice (docs/settings.md).
            mode: known("appearance.mode", appearance.mode.as_ref())
                .unwrap_or_else(|| release_value("appearance.mode")),
        }
    }

    /// The layout the desktop shows: the person's keys over the
    /// machine's, then the preset, Classic when none is chosen.
    pub fn layout(&self) -> Layout {
        let mut layout = Self::layout_from(self.layers().merged());
        layout.named = named_preset(
            &Self::raw_file_of(Some(&self.machine)),
            &Self::raw_file_of(self.person.as_ref()),
        );
        layout
    }

    /// Both files as the desktop applies them, each read once: what every
    /// reader of "the person's file over the machine's" asks for
    /// (`edel::settings::Layers`).
    pub fn layers(&self) -> edel::settings::Layers {
        let text = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        edel::settings::Layers::read(
            text(Some(&self.machine)).as_deref(),
            text(self.person.as_ref()).as_deref(),
        )
    }

    /// The own presets the Layout page offers: the machine's and the
    /// person's, the person's of a name over the machine's (M5.17), as the
    /// files write them.
    pub fn own_presets(&self) -> BTreeMap<String, settings::OwnPreset> {
        let mut all = Self::raw_file_of(Some(&self.machine)).presets;
        all.extend(Self::raw_file_of(self.person.as_ref()).presets);
        all
    }

    /// The layout keys the person's own file sets besides its preset, as
    /// `edel::presets::changed` names them (M5.17): Save as preset is offered
    /// while there are any.
    pub fn changed_layout(&self) -> Vec<String> {
        presets::changed(&Self::raw_file_of(self.person.as_ref()))
    }

    /// Saves the layout in use as the person's own preset `name`
    /// (`presets::save_text`, Save as preset, M5.17), one write.
    pub fn save_preset(&self, name: &str) -> Result<(), String> {
        let machine = std::fs::read_to_string(&self.machine).ok();
        self.edit_person(|text| presets::save_text(text, machine.as_deref(), name))
    }

    /// Deletes the person's own preset `name` (M5.17), one write; refused
    /// for the preset in use, as the library says.
    pub fn delete_preset(&self, name: &str) -> Result<(), String> {
        self.edit_person(|text| presets::delete_text(text, name))
    }

    /// The lock screen as the desktop reads it (M5.10c5c): each key the
    /// person's file sets over the machine's, and the looks of both, the
    /// person's of a name over the machine's.
    pub fn lock(&self) -> settings::LockScreen {
        let (machine, person) = self.texts();
        edel::lock_screen::lock_over(machine.as_deref(), Some(&person))
    }

    /// The machine's settings text and the person's, as the look writers of
    /// the library read them (M5.10c7h); the person's is empty with no file.
    pub fn texts(&self) -> (Option<String>, String) {
        (
            std::fs::read_to_string(&self.machine).ok(),
            self.person
                .as_ref()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .unwrap_or_default(),
        )
    }

    /// Writes the person's whole settings text (M5.10c7h), through the same
    /// rename `set` makes for one key.
    pub fn write_person(&self, text: &str) -> Result<(), String> {
        let path = self
            .person
            .as_ref()
            .ok_or("there is no home folder to keep your settings in")?;
        settings::write_text(path, text).map_err(|e| format!("{e:#}"))
    }

    /// This person's own file as the desktop reads it, empty when there is
    /// none (the lock screen's own looks are written from it).
    pub fn person_file(&self) -> settings::SettingsFile {
        Self::file_of(self.person.as_ref())
    }

    /// The person's file's text as it is, empty when there is none: what the
    /// theme edits read, which must see the theme's own values unmixed.
    pub fn person_text(&self) -> String {
        self.person
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap_or_default()
    }

    /// The machine's file's text as it is, empty when there is none.
    pub fn machine_text(&self) -> String {
        std::fs::read_to_string(&self.machine).unwrap_or_default()
    }

    /// Writes the person's whole file as `edit` makes it from its text
    /// (ADR-008: the library's writers do the editing, a file never half
    /// written, the file's lock held); a file with no text starts with the
    /// format line, and one that cannot be read is refused.
    pub fn edit_person<E: std::fmt::Display>(
        &self,
        edit: impl FnOnce(&str) -> Result<String, E>,
    ) -> Result<(), String> {
        let path = self
            .person
            .as_ref()
            .ok_or("there is no home folder to keep your settings in")?;
        // The library's edit takes its own errors; this one's are kept here
        // and said as they are.
        let mut refused = None;
        let written = settings::edit(path, |text| match edit(text) {
            Ok(edited) => Ok(edited),
            Err(e) => {
                refused = Some(format!("{e:#}"));
                Ok(text.to_string())
            }
        });
        match (refused, written) {
            (Some(why), _) => Err(why),
            (None, written) => written.map_err(|e| format!("{e:#}")),
        }
    }

    /// The panels that apply, as shell-ui draws them (M5.31d): the person's
    /// `panels.list` over the machine's, else the preset's.
    pub fn panels(&self) -> Vec<Panel> {
        let read = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        panel_edit::applying(
            read(Some(&self.machine)).as_deref(),
            read(self.person.as_ref()).as_deref(),
        )
    }

    /// The apps the apps widget pins that apply (M5.31d): the person's
    /// `panels.pinned` over the machine's, else the preset's.
    pub fn pins(&self) -> Vec<String> {
        let read = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        panel_edit::pins_applying(
            read(Some(&self.machine)).as_deref(),
            read(self.person.as_ref()).as_deref(),
        )
    }

    /// The apps `startup.apps` names that apply (M6.10): the person's list
    /// over the machine's, which it replaces whole, else none.
    pub fn startup(&self) -> Vec<String> {
        self.layers()
            .merged()
            .startup
            .apps
            .clone()
            .unwrap_or_default()
    }

    /// Writes the person's `startup.apps` as `list` (M6.10), the whole list
    /// at once. A list the machine's already gives is taken out instead, as
    /// writers never write what would apply anyway (ADR-008), so an empty
    /// list goes only when the machine has none.
    pub fn choose_startup(&self, list: &[String]) -> Result<(), String> {
        let machine = self
            .layers()
            .machine()
            .startup
            .apps
            .clone()
            .unwrap_or_default();
        if list == machine.as_slice() {
            self.set("startup.apps", None)
        } else {
            self.set("startup.apps", Some(&list_text(list)))
        }
    }

    /// Chooses `value` for the key `key` of the Layout, Workspaces or
    /// Appearance page: written to the person's
    /// file, or taken out of it when it is what would apply without it,
    /// as writers never write a default (ADR-008).
    pub fn choose(&self, key: &str, value: &str) -> Result<(), String> {
        if key == "layout.preset" {
            return self.choose_preset(value);
        }
        let machine = Self::raw_file_of(Some(&self.machine));
        // Whether the person has the line is read from the file as written,
        // and what applies without it from that file with its own preset and
        // theme applied, so a value an own preset holds counts as applying
        // anyway (the whole-system review of 2026-10-25).
        let mut person = Self::raw_file_of(self.person.as_ref());
        let had = match key {
            "layout.tiling" => person.layout.tiling.take().is_some(),
            "layout.title_bars" => person.layout.title_bars.take().is_some(),
            "layout.window_buttons" => person.layout.window_buttons.take().is_some(),
            "layout.tiling_style" => person.layout.tiling_style.take().is_some(),
            "layout.close_button" => person.layout.close_button.take().is_some(),
            "layout.minimize_button" => person.layout.minimize_button.take().is_some(),
            "layout.maximize_button" => person.layout.maximize_button.take().is_some(),
            "workspaces.count" => person.workspaces.count.take().is_some(),
            "workspaces.dynamic" => person.workspaces.dynamic.take().is_some(),
            "workspaces.per_screen" => person.workspaces.per_screen.take().is_some(),
            "appearance.switcher_look" => person.appearance.switcher_look.take().is_some(),
            "appearance.switcher_shown" => person.appearance.switcher_shown.take().is_some(),
            "appearance.switcher_ends" => person.appearance.switcher_ends.take().is_some(),
            "workspaces.names" => person.workspaces.names.take().is_some(),
            "workspaces.wallpapers" => person.workspaces.wallpapers.take().is_some(),
            "workspaces.apps" => person.workspaces.apps.take().is_some(),
            "workspaces.overview_strip" => person.workspaces.overview_strip.take().is_some(),
            "appearance.mode" => person.appearance.mode.take().is_some(),
            _ => return Err(format!("{key} is not a key these pages choose")),
        };
        let without =
            Self::layout_from(edel::settings::Layers::of(Some(&machine), Some(&person)).merged());
        match (without.value(key).as_deref() != Some(value), had) {
            (true, _) => self.set(key, Some(value)),
            (false, true) => self.set(key, None),
            (false, false) => Ok(()),
        }
    }

    /// Chooses the preset `value` names, a built-in one or the person's own
    /// (M5.17): written to the person's file, or taken out of it when the
    /// preset the files name without it is the same.
    fn choose_preset(&self, value: &str) -> Result<(), String> {
        let machine = Self::raw_file_of(Some(&self.machine));
        let mut person = Self::raw_file_of(self.person.as_ref());
        let had = person.layout.preset.take().is_some();
        match (named_preset(&machine, &person) != value, had) {
            (true, _) => self.set("layout.preset", Some(value)),
            (false, true) => self.set("layout.preset", None),
            (false, false) => Ok(()),
        }
    }

    /// Each screen's `[displays.NAME]` as the desktop follows it: the
    /// person's keys over the machine's, key by key.
    pub fn displays(&self) -> BTreeMap<String, settings::Display> {
        self.layers().merged().displays.clone()
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
        let layers = self.layers();
        let (resolved, _) = edel::shortcuts::resolve(&layers.merged().shortcuts);
        resolved
            .into_iter()
            .find(|(a, _)| a.name == action)
            .and_then(|(_, keys)| keys)
            .map(|keys| keys.to_string())
    }

    /// The whole `[shortcuts]` as the desktop follows it: the machine's
    /// lines, the person's over them (M5.13b, the page's conflicts and
    /// the chords it writes).
    pub fn shortcut_table(&self) -> BTreeMap<String, String> {
        self.layers().merged().shortcuts.clone()
    }

    /// The value to write for `action`'s line once `keys` are chosen: the
    /// keys, or `None` when they are what applies without the person's
    /// line (the machine's, else the release's default), so the line is
    /// taken out and never written (ADR-008).
    pub fn shortcut_line(&self, action: &str, keys: &str) -> Option<String> {
        let key = format!("shortcuts.{action}");
        let read = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        let without = settings::chosen(&key, read(Some(&self.machine)).as_deref(), None)
            .or_else(|| edel::shortcuts::find(action).map(|a| a.default.to_string()));
        match without {
            Some(without) if same_value(&without, keys) => None,
            _ => Some(keys.to_string()),
        }
    }

    /// Writes `[shortcuts]` lines of the person's file in one write, in the
    /// order given (a chord's old holder is cleared before the chord moves
    /// to it); `None` takes a line out.
    pub fn write_shortcuts(&self, edits: &[(String, Option<String>)]) -> Result<(), String> {
        let path = self
            .person
            .as_ref()
            .ok_or("there is no home folder to keep your settings in")?;
        let pairs: Vec<(&str, Option<&str>)> = edits
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_deref()))
            .collect();
        settings::write_all(path, &pairs).map_err(|e| format!("{e:#}"))
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

    /// Whether sound plays with little delay (M7.6b), as the compositor
    /// reads it: the person's file over the machine's, each with its
    /// profiles laid under it (`edel::settings::low_latency`).
    pub fn low_latency(&self) -> bool {
        settings::low_latency(
            Self::read(Some(&self.machine)).as_ref(),
            Self::read(self.person.as_ref()).as_ref(),
        )
    }

    /// [`Files::low_latency`] without the person's file: what applies once
    /// their line is taken out.
    pub fn low_latency_without_mine(&self) -> bool {
        settings::low_latency(Self::read(Some(&self.machine)).as_ref(), None)
    }

    /// `key`'s value as the desktop reads it, the person's file over the
    /// machine's; none when neither sets it (`edel::settings::chosen`).
    pub fn chosen(&self, key: &str) -> Option<String> {
        let read = |path: Option<&PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        settings::chosen(
            key,
            read(Some(&self.machine)).as_deref(),
            read(self.person.as_ref()).as_deref(),
        )
    }

    /// The colour scheme the files give now and the sun's next change, the
    /// one rule `edel::sun` owns (M7.14).
    pub fn scheme(&self) -> (edel::tokens::Scheme, Option<edel::sun::Change>) {
        let read = |path: &std::path::Path| std::fs::read_to_string(path).ok();
        let machine = read(&self.machine);
        let person = self.person.as_deref().and_then(read);
        edel::sun::scheme_of(machine.as_deref(), person.as_deref())
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

    /// The person's own value of each key of the page for `section`, as
    /// `edel settings set` writes it, none where their file has none: what
    /// Undo puts back (M5.6a).
    pub fn own_keys(&self, section: &str) -> Vec<(&'static str, Option<String>)> {
        // The file as written: an own preset's or a theme's values are not
        // the person's lines, so Undo never writes them as such.
        let person = Self::raw_file_of(self.person.as_ref());
        crate::rows::on_page(section)
            .map(|row| (row.key, own(&person, row.key)))
            .collect()
    }

    /// Puts each of a page's keys back as `before` held them
    /// (Undo), with the functions `edel settings set` and `reset` use, so
    /// the person's other keys, a change made elsewhere to them and the
    /// file's comments all stay.
    pub fn restore(&self, before: &[(&'static str, Option<String>)]) -> Result<(), String> {
        let sections: Vec<&str> = before
            .iter()
            .filter_map(|(key, _)| key.split_once('.').map(|(s, _)| s))
            .collect();
        let now: Vec<(&'static str, Option<String>)> = sections
            .iter()
            .flat_map(|section| self.own_keys(section))
            .collect();
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
        let path = self.person.as_ref().ok_or_else(|| {
            tr("This account has no home folder to keep your settings in.").to_string()
        })?;
        settings::write(path, key, value).map_err(|e| {
            let denied = e.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
            });
            refused(path, denied, format!("{e:#}"))
        })
    }
}

/// Why a change to the person's file at `path` was not saved: a sentence
/// that says what to do when the account may not write the file
/// (`denied`), else the writer's own `words` (`docs/MESSAGES.md`).
fn refused(path: &std::path::Path, denied: bool, words: String) -> String {
    if denied {
        trf(
            "Could not save this change in your settings file ({path}): this account may not write it. Log out and in again, or ask an administrator.",
            &[("path", &path.display().to_string())],
        )
    } else {
        words
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

/// The release's value of `key`: the key table's default (`edel::key_help`,
/// its owner) as text, a TOML string's quotes taken off; empty for a key the
/// table does not hold.
pub fn release_value(key: &str) -> String {
    edel::key_help::find(key)
        .map_or("", |help| help.default)
        .trim_matches('"')
        .to_string()
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
    fn a_file_the_account_may_not_write_says_what_to_do() {
        let path = places::settings_in(std::path::Path::new("/home/ali/.config/edel"));
        let path = path.as_path();
        let said = refused(path, true, "could not write it".into());
        assert!(
            said.starts_with("Could not save this change in your settings file (/home/ali/"),
            "{said}"
        );
        assert!(said.ends_with("ask an administrator."), "{said}");
        assert_eq!(refused(path, false, "x: y".into()), "x: y");
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
    fn a_choice_against_a_profiles_value_is_written() {
        // The gaming profile gives game mode "always": picking the release's
        // "plugged-in" must write it, or the profile's value stays (the
        // architecture review of 2026-10-31).
        let dir = scratch("profile-choice");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        let machine = format!(
            "format = {}\n[system]\nprofiles = [\"gaming\"]\n",
            settings::FORMAT
        );
        std::fs::write(&files.machine, machine).unwrap();
        assert!(matches!(
            files.source("power.game_mode"),
            settings::Source::Profile(_)
        ));
        files
            .choose_over("power.game_mode", "plugged-in", Some("plugged-in"))
            .unwrap();
        let read = std::fs::read_to_string(&person).unwrap();
        assert!(read.contains("game_mode = \"plugged-in\""), "{read}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_row_set_back_against_an_own_preset_writes_its_value() {
        // Classic with Tile windows on, saved as mine: turning the row off
        // must write tiling = false, as mine tiles without the line, and
        // Undo must not copy mine's lines into [layout] (the whole-system
        // review of 2026-10-25).
        let dir = scratch("own-preset");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        let mine = format!(
            "format = {}\n[layout]\npreset = \"mine\"\n\n[presets.mine]\npreset = \"classic\"\ntiling = true\n",
            settings::FORMAT
        );
        std::fs::write(&person, &mine).unwrap();
        assert_eq!(
            files
                .own_keys("layout")
                .iter()
                .find(|(k, _)| *k == "layout.tiling")
                .unwrap()
                .1,
            None
        );
        files.choose("layout.tiling", "false").unwrap();
        let read = || std::fs::read_to_string(&person).unwrap();
        assert!(read().contains("tiling = false"), "{}", read());
        files.choose("layout.tiling", "true").unwrap();
        assert_eq!(read(), mine);
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
        let before = files.own_keys("layout");
        assert!(before.iter().all(|(_, v)| v.is_none()));
        files.choose("layout.preset", "hive").unwrap();
        files.choose("layout.minimize_button", "false").unwrap();
        assert_ne!(files.own_keys("layout"), before);
        files.restore(&before).unwrap();
        assert_eq!(files.own_keys("layout"), before);
        assert_eq!(files.layout().preset, "classic");
        // A choice made before is brought back, and a key another page or
        // `edel settings set` changed meanwhile stays as it is now.
        files.choose("layout.preset", "mac-like").unwrap();
        let before = files.own_keys("layout");
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
    fn the_banners_place_is_automatic_until_chosen_and_never_written_automatic() {
        let dir = scratch("position");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        let read = || std::fs::read_to_string(&person).unwrap_or_default();
        let key = settings::NOTIFICATION_POSITION;
        assert_eq!(files.chosen(key), None);
        files.choose_over(key, "auto", Some("auto")).unwrap();
        assert!(!read().contains("position"), "{}", read());
        files.choose_over(key, "top-right", Some("auto")).unwrap();
        assert!(read().contains("position = \"top-right\""), "{}", read());
        assert_eq!(files.chosen(key).as_deref(), Some("top-right"));
        // Back to automatic takes the line out again.
        files.choose_over(key, "auto", Some("auto")).unwrap();
        assert!(!read().contains("position"), "{}", read());
        // A value the key does not take is refused as the command refuses it.
        let e = files.set(key, Some("middle")).unwrap_err();
        assert!(e.contains("notifications.position"), "{e}");
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
    fn low_latency_follows_the_profile_and_off_over_it_is_written() {
        let dir = scratch("latency");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(dir.join("person.toml")),
        };
        let read = || std::fs::read_to_string(dir.join("person.toml")).unwrap_or_default();
        let key = settings::LOW_LATENCY;
        // Off at first, and no line is written for off.
        assert!(!files.low_latency());
        // On, the person's line is written, over what applies without it.
        files
            .choose_over(
                key,
                "true",
                Some(&files.low_latency_without_mine().to_string()),
            )
            .unwrap();
        assert!(read().contains("[sound]\nlow_latency = true"), "{}", read());
        files.set(key, None).unwrap();
        // The creative profile on the machine turns it on, for the person too.
        std::fs::write(
            &files.machine,
            "format = 1\n[system]\nprofiles = [\"creative\"]\n",
        )
        .unwrap();
        assert!(files.low_latency());
        assert!(files.low_latency_without_mine());
        // Off over the profile is written, and then it is off.
        files
            .choose_over(
                key,
                "false",
                Some(&files.low_latency_without_mine().to_string()),
            )
            .unwrap();
        assert!(read().contains("low_latency = false"), "{}", read());
        assert!(!files.low_latency());
        // On again, which the profile gives, takes the line out.
        files
            .choose_over(
                key,
                "true",
                Some(&files.low_latency_without_mine().to_string()),
            )
            .unwrap();
        assert!(!read().contains("low_latency"), "{}", read());
        assert!(files.low_latency());
        // A word that is no flag is refused with what to write.
        assert_eq!(
            files.set(key, Some("fast")).unwrap_err(),
            "sound.low_latency: expected true or false, not \"fast\""
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_workspace_count_is_written_only_when_it_differs_from_the_preset() {
        let dir = scratch("workspaces");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        let read = || std::fs::read_to_string(&person).unwrap_or_default();
        // Classic has four workspaces: four writes nothing.
        files.choose("workspaces.count", "4").unwrap();
        assert!(!person.exists(), "{}", read());
        assert_eq!(files.layout().workspaces, 4);
        // Six is written exactly as `edel settings set` writes it.
        files.choose("workspaces.count", "6").unwrap();
        let by_command = settings::set(
            &format!("format = {}\n", settings::FORMAT),
            "workspaces.count",
            "6",
        )
        .unwrap();
        assert_eq!(read(), by_command);
        assert!(read().contains("[workspaces]\ncount = 6"), "{}", read());
        assert_eq!(files.layout().workspaces, 6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_names_are_written_as_edel_settings_set_writes_them() {
        let dir = scratch("names");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        files
            .choose("workspaces.names", &list_text(&["Mail".into()]))
            .unwrap();
        let by_command = settings::set(
            &format!("format = {}\n", settings::FORMAT),
            "workspaces.names",
            r#"["Mail"]"#,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&person).unwrap(), by_command);
        assert_eq!(files.layout().workspace_names, ["Mail"]);
        assert_eq!(
            list_text(&["Mail".into(), String::new()]),
            r#"["Mail", ""]"#
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_apps_are_written_as_a_table_that_edel_settings_takes() {
        let dir = scratch("apps");
        let person = dir.join("person.toml");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: Some(person.clone()),
        };
        let mut apps = BTreeMap::new();
        apps.insert("org.mozilla.firefox".to_string(), 2);
        let text = apps_text(&apps);
        assert_eq!(text, r#"{ "org.mozilla.firefox" = 2 }"#);
        assert_eq!(apps_text(&BTreeMap::new()), "{}");
        assert!(settings::set("format = 1\n", "workspaces.apps", &text).is_ok());
        files.choose("workspaces.apps", &text).unwrap();
        assert_eq!(files.layout().app_workspaces, apps);
        // An empty table or list is what a person writes over a machine's
        // own, so `edel settings set` must take them too.
        assert!(settings::set("format = 1\n", "workspaces.apps", "{}").is_ok());
        assert!(settings::set("format = 1\n", "workspaces.names", "[]").is_ok());
        assert!(read_back(&person).contains("org.mozilla.firefox"));
        // Nothing in the table is what applies without it: it is taken out.
        files.choose("workspaces.apps", "{}").unwrap();
        assert!(!read_back(&person).contains("firefox"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The text of a file, or nothing when it is not there.
    fn read_back(path: &PathBuf) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn the_switcher_shows_three_numbers_until_a_file_says_otherwise() {
        let dir = scratch("shown");
        let files = Files {
            machine: dir.join("machine.toml"),
            person: None,
        };
        assert_eq!(
            files.layout().value("appearance.switcher_shown").as_deref(),
            Some("3")
        );
        assert_eq!(
            files.layout().value("appearance.switcher_look").as_deref(),
            Some("numbers")
        );
        assert_eq!(
            files.layout().value("appearance.switcher_ends").as_deref(),
            Some("arrows")
        );
        // The overview's strip lies on the left until a file says otherwise.
        assert_eq!(
            files.layout().value("workspaces.overview_strip").as_deref(),
            Some("left")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preset_titles_read_as_people_write_them() {
        assert_eq!(title("classic"), "Classic");
        assert_eq!(title("mac-like"), "Mac-like");
        assert_eq!(title("windows-like"), "Windows-like");
    }

    #[test]
    fn the_radius_slider_starts_from_the_theme_the_person_picks() {
        // Earth gives 12 px corners (themes/earth.toml); the release gives 10.
        let dir = scratch("radius-theme");
        let machine = dir.join("machine.toml");
        let person = dir.join("person.toml");
        std::fs::write(&machine, "format = 1\n").unwrap();
        std::fs::write(&person, "format = 1\n[appearance]\ntheme = \"earth\"\n").unwrap();
        let files = Files {
            machine,
            person: Some(person),
        };
        assert_eq!(crate::shape::now(&files)[crate::shape::RADIUS], 12);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_own_themes_colour_shows_in_the_colours_the_rows_read() {
        let dir = scratch("own-theme-colour");
        let machine = dir.join("machine.toml");
        let person = dir.join("person.toml");
        std::fs::write(&machine, "format = 1\n").unwrap();
        std::fs::write(
            &person,
            "format = 1\n[appearance]\ntheme = \"mine\"\n[themes.mine.colours]\npanel = \"#102030\"\n",
        )
        .unwrap();
        let files = Files {
            machine,
            person: Some(person),
        };
        let colours = files.layers().merged().appearance.colours.clone();
        let shown = crate::colours::listed(colours.as_ref());
        assert!(
            shown
                .iter()
                .any(|item| item.name == "panel" && item.value == "#102030")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_screens_keys_from_both_files_are_shown_together() {
        // The machine places the screen, the person scales it: both show,
        // each key from the file that sets it.
        let dir = scratch("display-keys");
        let machine = dir.join("machine.toml");
        let person = dir.join("person.toml");
        std::fs::write(&machine, "format = 1\n[displays.main]\nposition = [0, 0]\n").unwrap();
        std::fs::write(&person, "format = 1\n[displays.main]\nscale = 1.5\n").unwrap();
        let files = Files {
            machine,
            person: Some(person),
        };
        let shown = files.displays();
        let main = shown.get("main").expect("the screen is shown");
        assert_eq!(main.position, Some([0, 0]));
        assert_eq!(main.scale, Some(1.5));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
