//! Themes (M5.12b6a, ADR-008, ADR-010): what a theme is and how it applies.
//!
//! A theme is a topic: its pictures, as references (a wallpaper's name or a
//! picture's path, as `appearance.wallpaper` takes them), and one named set
//! of appearance values. `appearance.theme` picks it, under any value the
//! person set on top. Built-in themes are the files `themes/NAME.toml` at
//! the repository's root (their owner, ADR-010), read here through
//! `include_str!` as presets are; the person's own are `[themes.NAME]`
//! tables of the settings file, with the same keys.
//!
//! [`themed`] is the one function that applies a theme. Every reader of
//! appearance values calls it on each file it reads, before it reads; the
//! writers (`set`, `write`, `export`, `diff`) never do, so they write the
//! file as it is. [`changed`] says what the person set on top of the theme,
//! and [`save_text`], [`delete_text`] and [`reset_text`] are the command's
//! writers, each on a file's text.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use toml::{Table, Value};

use crate::i18n::{tr, trf};
use crate::settings::{self, Appearance, OwnTheme, SettingsFile};
use crate::wallpapers;

/// The built-in themes, in the order people see them; a new one is a file
/// under `themes/` and a line here. The release's own theme comes first.
pub const BUILT_IN: &[(&str, &str)] = &[
    ("edel", include_str!("../../../themes/edel.toml")),
    ("aurora", include_str!("../../../themes/aurora.toml")),
    ("earth", include_str!("../../../themes/earth.toml")),
];

/// The theme in use while `appearance.theme` is absent: the release's own,
/// whose values are the release's defaults, so nothing changes on screen.
pub const DEFAULT: &str = "edel";

/// The keys a theme never holds. The style (`mode`), the wallpaper the
/// person picks and the workspace switcher's three are the person's own
/// choices, outside every theme, so picking another theme keeps them.
pub const NOT_IN_A_THEME: &[&str] = &[
    "theme",
    "mode",
    "wallpaper",
    "wallpaper_dark",
    "switcher_look",
    "switcher_shown",
    "switcher_ends",
];

/// The most pictures a theme holds.
const MOST_PICTURES: usize = 32;

/// The most characters a theme's title may have.
const MOST_TITLE: usize = 40;

/// Whether `name` is a theme's name: letters, digits and hyphens, not
/// starting with a hyphen, up to 32 characters, the same rule as a look's.
pub fn is_name(name: &str) -> bool {
    crate::lock_screen::is_look_name(name)
}

/// Whether `name` is one of [`BUILT_IN`]'s, which never changes.
pub fn is_built_in(name: &str) -> bool {
    BUILT_IN.iter().any(|(built_in, _)| *built_in == name)
}

/// The settings file text that a built-in theme's file reads as: its
/// values under `[themes.NAME]`, the same table a person's theme is, so
/// [`settings::read`] and [`settings::check`] judge both alike.
fn as_person_file(name: &str, text: &str) -> Option<String> {
    let mut theme: Table = toml::from_str(text).ok()?;
    if let Some(Value::Table(values)) = theme.remove("appearance") {
        for (key, value) in values {
            theme.insert(key, value);
        }
    }
    let mut themes = Table::new();
    themes.insert(name.to_string(), Value::Table(theme));
    let mut root = Table::new();
    root.insert("format".into(), Value::Integer(settings::FORMAT));
    root.insert("themes".into(), Value::Table(themes));
    toml::to_string(&root).ok()
}

/// A built-in theme as the person's own tables read it, or none when its
/// file is wrong (a test holds every file right).
fn built_in(name: &str) -> Option<OwnTheme> {
    let (_, text) = BUILT_IN.iter().find(|(built_in, _)| *built_in == name)?;
    let read = settings::read(&as_person_file(name, text)?).ok()?;
    read.file.themes.get(name).cloned()
}

/// The theme `name` names: a built-in one, else the person's own in `own`.
pub fn lookup(name: &str, own: &BTreeMap<String, OwnTheme>) -> Option<OwnTheme> {
    if is_built_in(name) {
        built_in(name)
    } else {
        own.get(name).cloned()
    }
}

/// What a theme is called on screen: its title, else its name made
/// readable (`my-aurora` reads My aurora).
pub fn title(name: &str, own: &BTreeMap<String, OwnTheme>) -> String {
    lookup(name, own)
        .and_then(|theme| theme.title)
        .unwrap_or_else(|| readable(name))
}

/// A name with its first letter capitalised and its hyphens as spaces.
fn readable(name: &str) -> String {
    let words = name.replace('-', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => words,
    }
}

/// `own`'s values with `theme`'s laid under them, key by key, colour
/// tokens by name: the person's wins. A key of [`NOT_IN_A_THEME`] is never
/// taken from the theme. None when a value cannot be shown as a table.
fn merged_table(own: &Appearance, theme: &Appearance) -> Option<Table> {
    let Value::Table(mut merged) = Value::try_from(own).ok()? else {
        return None;
    };
    let Value::Table(mut values) = Value::try_from(theme).ok()? else {
        return None;
    };
    for key in NOT_IN_A_THEME {
        values.remove(*key);
    }
    merge(&mut merged, values);
    Some(merged)
}

/// Lays `theme` under `own`, table by table: a key `own` has stays.
fn merge(own: &mut Table, theme: Table) {
    for (key, value) in theme {
        match own.get_mut(&key) {
            Some(Value::Table(inner)) => {
                if let Value::Table(theirs) = value {
                    merge(inner, theirs);
                }
            }
            Some(_) => {}
            None => {
                own.insert(key, value);
            }
        }
    }
}

/// `own` with `theme`'s values under it ([`merged_table`]), as appearance
/// values; `own` as it is when that cannot be done.
fn filled(own: &Appearance, theme: &Appearance) -> Appearance {
    merged_table(own, theme)
        .and_then(|table| Value::Table(table).try_into().ok())
        .unwrap_or_else(|| own.clone())
}

/// The file with the theme in use applied: every appearance value the file
/// leaves unset is the theme's, colour tokens merged by name, and the
/// wallpaper, when unset, is the theme's first picture. A file with no
/// `appearance.theme`, or naming no theme, comes back unchanged. Pure, and
/// the one function every reader calls (ADR-010).
pub fn themed(file: &SettingsFile) -> SettingsFile {
    let Some(name) = file.appearance.theme.as_deref() else {
        return file.clone();
    };
    let Some(theme) = lookup(name, &file.themes) else {
        return file.clone();
    };
    let mut out = file.clone();
    out.appearance = filled(&file.appearance, &theme.values);
    if out.appearance.wallpaper.is_none() {
        out.appearance.wallpaper = theme.pictures.first().cloned();
    }
    out
}

/// Each value of a table as a dotted path (`colours.panel`), in order.
fn flatten(prefix: &str, value: &Value, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Table(table) => {
            for (key, inner) in table {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&path, inner, out);
            }
        }
        other => {
            out.insert(prefix.to_string(), other.clone());
        }
    }
}

/// The first part of a dotted path.
fn top(path: &str) -> &str {
    path.split('.').next().unwrap_or(path)
}

/// The appearance keys the file sets on top of its theme with a value that
/// differs from the theme's, as `appearance.accent` or
/// `appearance.colours.panel`; the page's "Changed: WHAT" says these. The
/// theme in use is `edel` while no `appearance.theme` names one.
pub fn changed(file: &SettingsFile) -> Vec<String> {
    let name = file.appearance.theme.as_deref().unwrap_or(DEFAULT);
    let theme = lookup(name, &file.themes)
        .map(|theme| theme.values)
        .unwrap_or_default();
    let (Ok(own), Ok(theme)) = (Value::try_from(&file.appearance), Value::try_from(&theme)) else {
        return Vec::new();
    };
    let (mut own_values, mut theme_values) = (BTreeMap::new(), BTreeMap::new());
    flatten("", &own, &mut own_values);
    flatten("", &theme, &mut theme_values);
    own_values
        .iter()
        .filter(|(path, _)| !NOT_IN_A_THEME.contains(&top(path)))
        .filter(|(path, value)| theme_values.get(*path) != Some(*value))
        .map(|(path, _)| format!("appearance.{path}"))
        .collect()
}

/// The appearance keys the file sets outside its wallpapers and style: what
/// `save` moves into a theme and `reset` takes out.
fn own_keys(file: &SettingsFile) -> Vec<String> {
    let Ok(Value::Table(table)) = Value::try_from(&file.appearance) else {
        return Vec::new();
    };
    table
        .keys()
        .filter(|key| !NOT_IN_A_THEME.contains(&key.as_str()))
        .map(|key| format!("appearance.{key}"))
        .collect()
}

/// The check of one theme's table (`[themes.NAME]`, without its values):
/// its title, its pictures, and no key a theme never holds. The values are
/// checked by [`settings`]'s normaliser, as every key is.
pub fn check_table(table: &Table) -> Result<(), String> {
    for key in NOT_IN_A_THEME {
        if table.contains_key(*key) {
            return Err(trf(
                "{key} is not kept in a theme; set it under [appearance] instead",
                &[("key", *key)],
            ));
        }
    }
    if let Some(title) = table.get("title") {
        match title.as_str() {
            Some(text) if !text.trim().is_empty() && text.chars().count() <= MOST_TITLE => {}
            _ => {
                return Err(trf(
                    "a title in quotes, up to {most} characters, such as \"My aurora\"",
                    &[("most", &MOST_TITLE.to_string())],
                ));
            }
        }
    }
    if let Some(pictures) = table.get("pictures") {
        let Some(items) = pictures.as_array() else {
            return Err(tr("pictures as a list in quotes, such as [\"aurora\"]").to_string());
        };
        if items.len() > MOST_PICTURES {
            return Err(trf(
                "a theme holds up to {most} pictures; move some to another theme",
                &[("most", &MOST_PICTURES.to_string())],
            ));
        }
        for item in items {
            match item.as_str() {
                Some(value) if wallpapers::is_form(value) => {}
                _ => {
                    return Err(trf(
                        "{value} is not a picture; give a wallpaper's name such as \"aurora\", a picture's full path, or a colour as \"#rrggbb\"",
                        &[("value", &item.to_string())],
                    ));
                }
            }
        }
    }
    Ok(())
}

/// What the file's themes and the theme in use say wrong, one line each,
/// as `edel settings check` gives them: a theme's name, a built-in name
/// taken by a person's own theme, and an `appearance.theme` that names no
/// theme.
pub fn problems(file: &SettingsFile) -> Vec<String> {
    let mut lines = Vec::new();
    for name in file.themes.keys() {
        let shown = format!("themes.{name}");
        if !is_name(name) {
            let message = trf(
                "{name} is not a theme's name; use letters, digits and hyphens, such as \"my-aurora\"",
                &[("name", &format!("{name:?}"))],
            );
            lines.push(format!("{shown}: {message}"));
        } else if is_built_in(name) {
            let message = trf(
                "{name} is a built-in theme; name yours another way, such as \"my-theme\"",
                &[("name", &format!("{name:?}"))],
            );
            lines.push(format!("{shown}: {message}"));
        }
    }
    if let Some(name) = file.appearance.theme.as_deref() {
        if !is_built_in(name) && !file.themes.contains_key(name) {
            let builtins: Vec<&str> = BUILT_IN.iter().map(|(built_in, _)| *built_in).collect();
            let own: Vec<&str> = file.themes.keys().map(String::as_str).collect();
            let message = if own.is_empty() {
                trf(
                    "{name} is not a theme; choose {builtins}",
                    &[
                        ("name", &format!("{name:?}")),
                        ("builtins", &settings::or_list(&builtins)),
                    ],
                )
            } else {
                trf(
                    "{name} is not a theme; choose {builtins}, or one of yours ({own})",
                    &[
                        ("name", &format!("{name:?}")),
                        ("builtins", &settings::or_list(&builtins)),
                        ("own", &settings::or_list(&own)),
                    ],
                )
            };
            lines.push(format!("appearance.theme: {message}"));
        }
    }
    lines
}

/// One theme as `edel theme list` shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub title: String,
    pub built_in: bool,
    pub in_use: bool,
}

/// The themes in the order `edel theme list` shows them: the built-in ones,
/// then the person's own by name, the one in use marked.
pub fn rows(file: &SettingsFile) -> Vec<Row> {
    let in_use = file.appearance.theme.as_deref().unwrap_or(DEFAULT);
    let built_in = BUILT_IN.iter().map(|(name, _)| *name);
    let own = file
        .themes
        .keys()
        .map(String::as_str)
        .filter(|name| !is_built_in(name));
    built_in
        .chain(own)
        .map(|name| Row {
            name: name.to_string(),
            title: title(name, &file.themes),
            built_in: is_built_in(name),
            in_use: name == in_use,
        })
        .collect()
}

/// The value a theme gives `key` (an `appearance.*` key) in the files, the
/// person's over the machine's, when a file names a theme; None when no file
/// does, so the caller reads each file's own value as before. Colour tokens
/// are looked up by their path, as the tokens take them.
pub fn chosen(key: &str, machine: Option<&str>, person: Option<&str>) -> Option<String> {
    key.strip_prefix("appearance.")?;
    // Most files name no theme, and then nothing is parsed: this is asked at
    // each drawing of a menu, so the common case costs a search of the text.
    let names_some_theme = |text: Option<&str>| text.is_some_and(|text| text.contains("theme"));
    if !names_some_theme(machine) && !names_some_theme(person) {
        return None;
    }
    let read = |text: Option<&str>| text.and_then(|text| settings::read(text).ok());
    let (machine, person) = (read(machine), read(person));
    let names_a_theme = [&machine, &person]
        .into_iter()
        .flatten()
        .any(|read| read.file.appearance.theme.is_some());
    if !names_a_theme {
        return None;
    }
    let person = person.map(|read| themed(&read.file));
    let machine = machine.map(|read| themed(&read.file));
    let value = person
        .as_ref()
        .and_then(|file| value_of(file, key))
        .or_else(|| machine.as_ref().and_then(|file| value_of(file, key)))?;
    Some(match value {
        Value::String(text) => text,
        other => other.to_string(),
    })
}

/// The value of an `appearance.*` key in `file`, by its path.
fn value_of(file: &SettingsFile, key: &str) -> Option<Value> {
    let key = key.strip_prefix("appearance.")?;
    let mut value = Value::try_from(&file.appearance).ok()?;
    for part in key.split('.') {
        value = value.as_table()?.get(part)?.clone();
    }
    Some(value)
}

/// `edel theme save NAME`, on a file's text: makes `[themes.NAME]` from the
/// theme in use with the person's changes on top, its pictures the theme's
/// (the person's wallpaper first, when it is not one of them), then sets
/// `appearance.theme` to it and takes the moved keys out of `[appearance]`,
/// so the screen is the same. A theme of the person's own that is in use
/// is updated; another name taken is refused, and so is a built-in's.
pub fn save_text(text: &str, name: &str) -> Result<String> {
    let file = settings::read(text)?.file;
    if !is_name(name) {
        bail!(
            "{}",
            trf(
                "{name} is not a theme's name; use letters, digits and hyphens, such as \"my-aurora\"",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    if is_built_in(name) {
        bail!(
            "{}",
            trf(
                "{name} is a built-in theme and stays as it is; name yours another way, such as \"my-theme\"",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    let in_use = file.appearance.theme.clone();
    let in_use_name = in_use.as_deref().unwrap_or(DEFAULT);
    if file.themes.contains_key(name) && in_use_name != name {
        bail!(
            "{}",
            trf(
                "a theme of yours is already called {name}; pick another name, or make it the theme in use first",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    let base = lookup(in_use_name, &file.themes).unwrap_or_default();
    let mut pictures = base.pictures.clone();
    if let Some(wallpaper) = file.appearance.wallpaper.as_deref() {
        if !pictures.iter().any(|picture| picture == wallpaper) {
            pictures.insert(0, wallpaper.to_string());
        }
    }
    let mut table = merged_table(&file.appearance, &base.values).unwrap_or_default();
    for key in NOT_IN_A_THEME {
        table.remove(*key);
    }
    let values: Appearance = Value::Table(table).try_into()?;
    let theme = OwnTheme {
        title: None,
        pictures,
        values,
    };
    let Value::Table(theme) = Value::try_from(&theme)? else {
        bail!("{}", tr("a theme could not be written as a table"));
    };
    let mut text = settings::set(
        text,
        &format!("themes.{name}"),
        &Value::Table(theme).to_string(),
    )?;
    text = settings::set(&text, "appearance.theme", &format!("{name:?}"))?;
    let moved = own_keys(&file);
    if !moved.is_empty() {
        let moved: Vec<&str> = moved.iter().map(String::as_str).collect();
        text = settings::unset_all(&text, &moved)?;
    }
    Ok(text)
}

/// `edel theme delete NAME`, on a file's text: removes `[themes.NAME]`. A
/// built-in theme, a name that is no theme of the person's, and the theme in
/// use are refused, the last with the way to pick another first.
pub fn delete_text(text: &str, name: &str) -> Result<String> {
    let file = settings::read(text)?.file;
    if is_built_in(name) {
        bail!(
            "{}",
            trf(
                "{name} is a built-in theme, which stays as it is",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    if !file.themes.contains_key(name) {
        bail!(
            "{}",
            trf(
                "there is no theme of yours called {name}; edel theme list shows them",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    if file.appearance.theme.as_deref().unwrap_or(DEFAULT) == name {
        bail!(
            "{}",
            trf(
                "{name} is the theme in use; pick another theme first: edel settings set appearance.theme=edel",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    settings::unset(text, &format!("themes.{name}"))
}

/// `edel theme reset`, on a file's text: takes out the appearance values the
/// person set on top of the theme (the wallpaper and the style stay), so the
/// theme shows as made. None when there is nothing to take out.
pub fn reset_text(text: &str) -> Result<Option<String>> {
    let file = settings::read(text)?.file;
    let keys = own_keys(&file);
    if keys.is_empty() {
        return Ok(None);
    }
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    Ok(Some(settings::unset_all(text, &keys)?))
}

/// A name for a new theme of the person's, from `base`: its letters in
/// lower case with hyphens between the rest, then `-2`, `-3` ... for the
/// first one no built-in theme and none of `taken` has (`my-theme`,
/// `my-theme-2`). Settings' Blank tile names its theme with it.
pub fn free_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let stem = slug(base);
    let free = |name: &str| is_name(name) && !is_built_in(name) && !taken(name);
    std::iter::once(stem.clone())
        .chain((2..).map(|n| format!("{stem}-{n}")))
        .find(|name| free(name))
        .unwrap_or_else(|| format!("theme-{}", stem.len()))
}

/// A copy's name, from a theme's `title`: the title's slug with `-2`, `-3`
/// ... counted on, so Aurora's copy is `aurora-2` (Duplicate, Keep as new).
pub fn copy_name(title: &str, taken: impl Fn(&str) -> bool) -> String {
    let stem = slug(title);
    let free = |name: &str| is_name(name) && !is_built_in(name) && !taken(name);
    (2..)
        .map(|n| format!("{stem}-{n}"))
        .find(|name| free(name))
        .unwrap_or_else(|| format!("theme-{}", stem.len()))
}

/// `text` as a name: letters and digits in lower case, each run of other
/// characters one hyphen, at most 24 characters, `theme` when none is left.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let short: String = out.trim_end_matches('-').chars().take(24).collect();
    let short = short.trim_end_matches('-');
    if short.is_empty() {
        "theme".to_string()
    } else {
        short.to_string()
    }
}

/// A new own theme with no values and the release's default picture, and
/// `appearance.theme` set to it, on a file's text (Settings' Blank tile).
pub fn blank_text(text: &str, name: &str) -> Result<String> {
    let file = settings::read(text)?.file;
    check_new_name(&file, name)?;
    let theme = OwnTheme {
        title: None,
        pictures: vec![wallpapers::DEFAULT.to_string()],
        values: Appearance::default(),
    };
    let Value::Table(theme) = Value::try_from(&theme)? else {
        bail!("{}", tr("a theme could not be written as a table"));
    };
    let text = settings::set(
        text,
        &format!("themes.{name}"),
        &Value::Table(theme).to_string(),
    )?;
    settings::set(&text, "appearance.theme", &format!("{name:?}"))
}

/// Renames one of the person's own themes on a file's text, its values and
/// pictures kept, and the theme in use follows the new name. A built-in
/// theme, a name that is no theme of the person's and a name taken are
/// refused.
pub fn rename_text(text: &str, old: &str, new: &str) -> Result<String> {
    let file = settings::read(text)?.file;
    if is_built_in(old) {
        bail!(
            "{}",
            trf(
                "{name} is a built-in theme, which stays as it is",
                &[("name", &format!("{old:?}"))]
            )
        );
    }
    let Some(theme) = file.themes.get(old).cloned() else {
        bail!(
            "{}",
            trf(
                "there is no theme of yours called {name}; edel theme list shows them",
                &[("name", &format!("{old:?}"))]
            )
        );
    };
    if new == old {
        return Ok(text.to_string());
    }
    check_new_name(&file, new)?;
    let Value::Table(table) = Value::try_from(&theme)? else {
        bail!("{}", tr("a theme could not be written as a table"));
    };
    // The new theme first, then the theme in use moves to it, and only then
    // the old one goes: each step leaves a file that checks.
    let text = settings::set(
        text,
        &format!("themes.{new}"),
        &Value::Table(table).to_string(),
    )?;
    let text = if file.appearance.theme.as_deref() == Some(old) {
        settings::set(&text, "appearance.theme", &format!("{new:?}"))?
    } else {
        text
    };
    settings::unset(&text, &format!("themes.{old}"))
}

/// The checks a new name of one of the person's own themes meets: a valid
/// name, not a built-in one and not one of theirs already.
fn check_new_name(file: &SettingsFile, name: &str) -> Result<()> {
    if !is_name(name) {
        bail!(
            "{}",
            trf(
                "{name} is not a theme's name; use letters, digits and hyphens, such as \"my-aurora\"",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    if is_built_in(name) {
        bail!(
            "{}",
            trf(
                "{name} is a built-in theme and stays as it is; name yours another way, such as \"my-theme\"",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    if file.themes.contains_key(name) {
        bail!(
            "{}",
            trf(
                "a theme of yours is already called {name}; pick another name, or make it the theme in use first",
                &[("name", &format!("{name:?}"))]
            )
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEATURE_PICTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../features/wallpapers/usr/share/edel/wallpapers"
    );

    fn file(text: &str) -> SettingsFile {
        settings::read(text).unwrap().file
    }

    fn accent_of(file: &SettingsFile) -> Option<String> {
        themed(file).appearance.accent
    }

    #[test]
    fn themed_gives_the_file_back_with_no_theme_key() {
        let plain = file("format = 1\n[appearance]\nradius = 9\n");
        assert_eq!(themed(&plain), plain);
    }

    #[test]
    fn aurora_fills_accent_and_wallpaper_when_the_file_leaves_them_unset() {
        let aurora = file("format = 1\n[appearance]\ntheme = \"aurora\"\n");
        let applied = themed(&aurora);
        assert_eq!(applied.appearance.accent.as_deref(), Some("wallpaper"));
        assert_eq!(applied.appearance.wallpaper.as_deref(), Some("aurora"));
    }

    #[test]
    fn the_persons_own_accent_wins_over_the_themes() {
        let mine = file("format = 1\n[appearance]\ntheme = \"aurora\"\naccent = \"#d0363f\"\n");
        assert_eq!(accent_of(&mine).as_deref(), Some("#d0363f"));
    }

    #[test]
    fn an_own_theme_applies_its_pictures_and_values() {
        let text = "format = 1\n[appearance]\ntheme = \"mine\"\n[themes.mine]\npictures = [\"earth\"]\nradius = 4\n";
        let mine = file(text);
        let applied = themed(&mine);
        assert_eq!(applied.appearance.wallpaper.as_deref(), Some("earth"));
        assert_eq!(applied.appearance.radius, Some(4));
    }

    #[test]
    fn colours_merge_per_token_with_the_persons_winning() {
        let text = "format = 1\n[appearance]\ntheme = \"mine\"\n[appearance.colours]\npanel = \"#112233\"\n[themes.mine]\n[themes.mine.colours]\npanel = \"#000001\"\nwindow = \"#445566\"\n";
        let applied = themed(&file(text));
        let colours = applied.appearance.colours.expect("colours");
        assert_eq!(
            colours.both.get("panel").map(String::as_str),
            Some("#112233")
        );
        assert_eq!(
            colours.both.get("window").map(String::as_str),
            Some("#445566")
        );
    }

    #[test]
    fn the_style_and_the_wallpaper_are_never_taken_from_a_theme() {
        let text = "format = 1\n[appearance]\ntheme = \"aurora\"\nmode = \"dark\"\n[themes.aurora]\nmode = \"dark\"\n";
        let applied = themed(&file(text));
        assert_eq!(applied.appearance.mode.as_deref(), Some("dark"));
        assert_eq!(applied.appearance.wallpaper.as_deref(), Some("aurora"));
    }

    #[test]
    fn changed_lists_an_accent_set_on_aurora() {
        let text = "format = 1\n[appearance]\ntheme = \"aurora\"\naccent = \"#d0363f\"\n";
        assert_eq!(changed(&file(text)), ["appearance.accent"]);
    }

    #[test]
    fn changed_lists_nothing_for_a_value_the_theme_already_gives() {
        let text = "format = 1\n[appearance]\ntheme = \"aurora\"\naccent = \"wallpaper\"\n";
        assert!(changed(&file(text)).is_empty());
    }

    #[test]
    fn changed_names_each_colour_token_the_person_sets() {
        let text = "format = 1\n[appearance]\ntheme = \"edel\"\n[appearance.colours]\npanel = \"#112233\"\n";
        assert_eq!(changed(&file(text)), ["appearance.colours.panel"]);
    }

    #[test]
    fn save_writes_the_theme_and_keeps_the_screen_the_same() {
        let text = "format = 1\n[appearance]\ntheme = \"aurora\"\naccent = \"#d0363f\"\n";
        let before = accent_of(&file(text));
        let saved = save_text(text, "mine").unwrap();
        let after = file(&saved);
        assert_eq!(after.themes["mine"].pictures, ["aurora"]);
        assert_eq!(
            after.themes["mine"].values.accent.as_deref(),
            Some("#d0363f")
        );
        assert_eq!(after.appearance.theme.as_deref(), Some("mine"));
        assert_eq!(after.appearance.accent, None);
        assert_eq!(accent_of(&after), before);
        assert!(changed(&after).is_empty());
    }

    #[test]
    fn save_puts_the_persons_wallpaper_first_when_it_is_not_a_picture_of_the_theme() {
        let text = "format = 1\n[appearance]\ntheme = \"aurora\"\nwallpaper = \"/home/a/x.jpg\"\n";
        let saved = file(&save_text(text, "mine").unwrap());
        assert_eq!(saved.themes["mine"].pictures, ["/home/a/x.jpg", "aurora"]);
        assert_eq!(saved.appearance.wallpaper.as_deref(), Some("/home/a/x.jpg"));
    }

    #[test]
    fn save_refuses_a_built_in_name_and_another_own_name() {
        let text = "format = 1\n[appearance]\ntheme = \"aurora\"\n";
        assert!(
            save_text(text, "aurora")
                .unwrap_err()
                .to_string()
                .contains("built-in")
        );
        let two = "format = 1\n[appearance]\ntheme = \"aurora\"\n[themes.mine]\naccent = \"red\"\n";
        assert!(
            save_text(two, "mine")
                .unwrap_err()
                .to_string()
                .contains("already called")
        );
        assert!(save_text(text, "Bad Name").is_err());
    }

    #[test]
    fn save_updates_the_own_theme_in_use() {
        let text = "format = 1\n[appearance]\ntheme = \"mine\"\naccent = \"red\"\n[themes.mine]\npictures = [\"earth\"]\n";
        let saved = file(&save_text(text, "mine").unwrap());
        assert_eq!(saved.themes["mine"].pictures, ["earth"]);
        assert_eq!(saved.themes["mine"].values.accent.as_deref(), Some("red"));
        assert_eq!(saved.appearance.accent, None);
    }

    #[test]
    fn delete_refuses_the_theme_in_use_and_built_ins() {
        let text = "format = 1\n[appearance]\ntheme = \"mine\"\n[themes.mine]\naccent = \"red\"\n";
        let refused = delete_text(text, "mine").unwrap_err().to_string();
        assert!(refused.contains("theme in use"), "{refused}");
        assert!(refused.contains("edel settings set appearance.theme=edel"));
        assert!(delete_text(text, "aurora").is_err());
        let other =
            "format = 1\n[appearance]\ntheme = \"aurora\"\n[themes.mine]\naccent = \"red\"\n";
        let gone = file(&delete_text(other, "mine").unwrap());
        assert!(gone.themes.is_empty());
    }

    #[test]
    fn reset_takes_the_values_on_top_out_and_keeps_the_wallpaper() {
        let text = "format = 1\n[appearance]\ntheme = \"aurora\"\naccent = \"#d0363f\"\nwallpaper = \"earth\"\nmode = \"dark\"\n";
        let reset = file(&reset_text(text).unwrap().unwrap());
        assert_eq!(reset.appearance.accent, None);
        assert_eq!(reset.appearance.wallpaper.as_deref(), Some("earth"));
        assert_eq!(reset.appearance.mode.as_deref(), Some("dark"));
        assert_eq!(
            reset_text("format = 1\n[appearance]\ntheme = \"aurora\"\n").unwrap(),
            None
        );
    }

    #[test]
    fn set_refuses_a_theme_that_is_not_there() {
        let error = settings::set("format = 1\n", "appearance.theme", "\"nope\"")
            .unwrap_err()
            .to_string();
        assert!(error.contains("\"nope\" is not a theme"), "{error}");
        assert!(error.contains("choose edel, aurora or earth"), "{error}");
        assert!(settings::set("format = 1\n", "appearance.theme", "\"aurora\"").is_ok());
    }

    #[test]
    fn check_refuses_a_theme_with_a_wrong_value_or_a_mode() {
        let lines = settings::check(
            "format = 1\n[themes.mine]\nmode = \"dark\"\n[themes.other]\nradius = 40\n",
        )
        .unwrap();
        assert!(lines.iter().any(|line| line.contains("mode")), "{lines:?}");
        assert!(
            lines.iter().any(|line| line.contains("radius")),
            "{lines:?}"
        );
    }

    #[test]
    fn every_built_in_theme_file_passes_the_checker() {
        for (name, text) in BUILT_IN {
            // Checked under a name of its own: a built-in's name is refused
            // for a person's theme, as `problems` says.
            let person = as_person_file("sample", text).expect("a theme file reads as TOML");
            let lines = settings::check(&person).unwrap();
            assert!(lines.is_empty(), "{name}: {lines:?}");
            let theme = built_in(name).expect("the file reads as a theme");
            for picture in &theme.pictures {
                assert_eq!(
                    wallpapers::check(picture, std::path::Path::new(FEATURE_PICTURES)),
                    None,
                    "{name} names {picture}"
                );
            }
        }
    }

    #[test]
    fn the_built_in_list_is_the_files_under_themes() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../themes");
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .map(|name| name.trim_end_matches(".toml").to_string())
            .collect();
        found.sort();
        let mut listed: Vec<String> = BUILT_IN.iter().map(|(name, _)| name.to_string()).collect();
        listed.sort();
        assert_eq!(found, listed);
    }

    #[test]
    fn rows_list_the_built_in_themes_then_the_own_with_the_one_in_use() {
        let text = "format = 1\n[appearance]\ntheme = \"mine\"\n[themes.mine]\ntitle = \"Mine\"\n[themes.zed]\n";
        let rows = rows(&file(text));
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["edel", "aurora", "earth", "mine", "zed"]);
        assert!(rows[3].in_use);
        assert_eq!(rows[3].title, "Mine");
        assert_eq!(rows[4].title, "Zed");
        assert!(rows[0].built_in && !rows[3].built_in);
    }

    #[test]
    fn the_tokens_take_the_themes_values_and_the_persons_over_them() {
        let theme = "format = 1\n[appearance]\ntheme = \"mine\"\n[themes.mine]\nradius = 4\n";
        let (tokens, notes) =
            crate::tokens::load_from_files(crate::tokens::Scheme::Light, None, Some(theme), None);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(tokens.radius_window, 4);
        let over =
            "format = 1\n[appearance]\ntheme = \"mine\"\nradius = 9\n[themes.mine]\nradius = 4\n";
        let (tokens, _) =
            crate::tokens::load_from_files(crate::tokens::Scheme::Light, None, Some(over), None);
        assert_eq!(tokens.radius_window, 9);
    }

    #[test]
    fn chosen_gives_the_theme_value_and_none_without_a_theme() {
        let person = "format = 1\n[appearance]\ntheme = \"aurora\"\n";
        assert_eq!(
            chosen("appearance.accent", None, Some(person)).as_deref(),
            Some("wallpaper")
        );
        assert_eq!(
            chosen("appearance.accent", None, Some("format = 1\n")),
            None
        );
    }

    #[test]
    fn a_blank_theme_has_no_values_and_the_default_picture_and_is_chosen() {
        let text = blank_text("format = 1\n", "my-blank").unwrap();
        let file = file(&text);
        let theme = file.themes.get("my-blank").unwrap();
        assert_eq!(theme.pictures, vec![wallpapers::DEFAULT.to_string()]);
        assert_eq!(theme.values, Appearance::default());
        assert_eq!(file.appearance.theme.as_deref(), Some("my-blank"));
        assert!(changed(&file).is_empty());
    }

    #[test]
    fn a_blank_theme_refuses_a_built_in_name_and_one_of_theirs() {
        assert!(blank_text("format = 1\n", "aurora").is_err());
        let own = blank_text("format = 1\n", "mine").unwrap();
        assert!(blank_text(&own, "mine").is_err());
    }

    #[test]
    fn a_copy_is_named_from_the_title_and_counts_on_past_taken_names() {
        assert_eq!(copy_name("Aurora", |_| false), "aurora-2");
        assert_eq!(copy_name("My aurora", |_| false), "my-aurora-2");
        assert_eq!(copy_name("Aurora", |name| name == "aurora-2"), "aurora-3");
        assert_eq!(free_name("My theme", |_| false), "my-theme");
        assert_eq!(
            free_name("My theme", |name| name == "my-theme"),
            "my-theme-2"
        );
        assert_eq!(free_name("Edel", |_| false), "edel-2");
    }

    #[test]
    fn rename_keeps_the_values_and_follows_the_theme_in_use() {
        let text = "format = 1\n[appearance]\ntheme = \"mine\"\n[themes.mine]\npictures = [\"hills\"]\nradius = 9\n";
        let renamed = rename_text(text, "mine", "ours").unwrap();
        let file = file(&renamed);
        assert!(!file.themes.contains_key("mine"));
        assert_eq!(file.themes["ours"].values.radius, Some(9));
        assert_eq!(file.appearance.theme.as_deref(), Some("ours"));
        assert!(rename_text(text, "aurora", "ours").is_err());
        assert!(rename_text(text, "mine", "earth").is_err());
    }
}
