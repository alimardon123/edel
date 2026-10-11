//! What `appearance.wallpaper` names (M5.10c4b), for the compositor that
//! draws it and the lock screen and Settings that show it: the one place
//! that turns the key's value into a picture or a colour.
//!
//! The value is one of: absent, the default wallpaper ([`DEFAULT`]) when
//! the `wallpapers` feature carries it, else the background colour; `none`,
//! the background colour; `#rrggbb`, that colour; a name, the pair of that
//! name in [`places::WALLPAPERS_DIR`] (`NAME-light.jpg` and
//! `NAME-dark.jpg`, the one the colour scheme names, or `NAME.jpg` for
//! both); or a path starting with `/`, that picture in both schemes.
//!
//! `appearance.wallpaper_dark` (M5.10c4d) is the picture for dark mode when
//! it differs: a person's own light and dark pair is two paths, one in each
//! key. [`value_for`] picks which of the two applies in a scheme.

use std::path::{Path, PathBuf};

use crate::places;
use crate::tokens::Colour;

/// The wallpaper shown while the key is absent, when its pictures exist.
pub const DEFAULT: &str = "hills";

/// The value that asks for no picture, the background colour alone.
pub const NONE: &str = "none";

/// The pictures' file types, in the order a name is looked for.
pub const EXTENSIONS: &[&str] = &["jpg", "png"];

/// What lies behind the windows.
#[derive(Debug, Clone, PartialEq)]
pub enum Wallpaper {
    /// The tokens' background colour, as before there were wallpapers.
    Background,
    /// One colour.
    Colour(Colour),
    /// A picture, scaled to fill each screen.
    Picture(PathBuf),
}

/// The value that applies in the scheme `dark` names: `wallpaper_dark` in
/// dark mode when it is set, else `wallpaper` (M5.10c4d). Absent is absent,
/// so [`resolve`] then gives the default.
pub fn value_for<'a>(
    wallpaper: Option<&'a str>,
    wallpaper_dark: Option<&'a str>,
    dark: bool,
) -> Option<&'a str> {
    if dark {
        wallpaper_dark.or(wallpaper)
    } else {
        wallpaper
    }
}

/// What `value` shows in the scheme `dark` names, the pairs looked for in
/// `dir` ([`places::WALLPAPERS_DIR`] on a machine). A name or a path whose
/// picture is not there is the background colour: a reader is lenient
/// (ADR-008), and [`check`] says why to the one who writes it.
pub fn resolve(value: Option<&str>, dark: bool, dir: &Path) -> Wallpaper {
    let value = value.map(str::trim);
    match value {
        None => find(dir, DEFAULT, dark).map_or(Wallpaper::Background, Wallpaper::Picture),
        Some(v) if v.is_empty() || v == NONE => Wallpaper::Background,
        Some(v) if v.starts_with('#') => {
            Colour::parse(v).map_or(Wallpaper::Background, Wallpaper::Colour)
        }
        Some(v) if v.starts_with('/') => {
            let path = PathBuf::from(v);
            if path.is_file() {
                Wallpaper::Picture(path)
            } else {
                Wallpaper::Background
            }
        }
        Some(name) => find(dir, name, dark).map_or(Wallpaper::Background, Wallpaper::Picture),
    }
}

/// [`resolve`] on this machine.
pub fn on_this_machine(value: Option<&str>, dark: bool) -> Wallpaper {
    resolve(value, dark, Path::new(places::WALLPAPERS_DIR))
}

/// The picture of the wallpaper `name` in `dir` for the scheme `dark`
/// names: `NAME-dark` or `NAME-light`, else `NAME` alone, in the first of
/// [`EXTENSIONS`] that exists. A name with a `/` or a `.` in it is no
/// wallpaper's, so it never leaves `dir`.
pub fn find(dir: &Path, name: &str, dark: bool) -> Option<PathBuf> {
    if name.is_empty() || name.contains(['/', '.']) {
        return None;
    }
    let side = if dark { "dark" } else { "light" };
    [format!("{name}-{side}"), name.to_string()]
        .into_iter()
        .flat_map(|stem| {
            EXTENSIONS
                .iter()
                .map(move |ext| dir.join(format!("{stem}.{ext}")))
        })
        .find(|path| path.is_file())
}

/// The wallpapers in `dir`, by name, each once, sorted: what Settings
/// offers and `edel settings check` accepts.
pub fn names(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let file = e.file_name().into_string().ok()?;
            let (stem, ext) = file.rsplit_once('.')?;
            if !EXTENSIONS.contains(&ext) {
                return None;
            }
            let name = stem
                .strip_suffix("-dark")
                .or_else(|| stem.strip_suffix("-light"))
                .unwrap_or(stem);
            Some(name.to_string())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Whether `value` has a wallpaper's form, wherever it is checked (a
/// settings file is checked on machines that lack the pictures): `none`, a
/// colour `#rrggbb`, an absolute path with no control characters, or a
/// name of lowercase letters, digits and hyphens.
pub fn is_form(value: &str) -> bool {
    value == NONE
        || (value.starts_with('#') && Colour::parse(value).is_some())
        || (value.starts_with('/') && !value.chars().any(char::is_control))
        || (!value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
}

/// Why `value` names nothing to show, for the one who writes it: `None`
/// when it is `none`, a colour, a wallpaper in `dir` or a picture that
/// exists.
pub fn check(value: &str, dir: &Path) -> Option<String> {
    let v = value.trim();
    if v == NONE {
        return None;
    }
    if v.starts_with('#') {
        return Colour::parse(v)
            .is_none()
            .then(|| format!("{v} is not a colour; write it as #rrggbb"));
    }
    if v.starts_with('/') {
        return (!Path::new(v).is_file()).then(|| format!("there is no picture at {v}"));
    }
    if find(dir, v, false).is_some() || find(dir, v, true).is_some() {
        return None;
    }
    let known = names(dir);
    Some(if known.is_empty() {
        format!(
            "there is no wallpaper named {v}; give a picture's path, a colour as #rrggbb, or none"
        )
    } else {
        format!(
            "there is no wallpaper named {v}; the wallpapers are {}, or give a picture's path, a colour as #rrggbb, or none",
            known.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "edel-wallpapers-{}-{}",
            std::process::id(),
            files.join("+").replace('.', "_")
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        dir
    }

    #[test]
    fn a_name_takes_the_schemes_picture_and_a_single_one_serves_both() {
        let dir = dir_with(&["hills-light.jpg", "hills-dark.jpg", "earth.png"]);
        assert_eq!(
            resolve(Some("hills"), true, &dir),
            Wallpaper::Picture(dir.join("hills-dark.jpg"))
        );
        assert_eq!(
            resolve(Some("hills"), false, &dir),
            Wallpaper::Picture(dir.join("hills-light.jpg"))
        );
        assert_eq!(
            resolve(Some("earth"), true, &dir),
            Wallpaper::Picture(dir.join("earth.png"))
        );
        assert_eq!(names(&dir), vec!["earth", "hills"]);
    }

    #[test]
    fn absent_is_the_default_when_it_is_carried_else_the_background() {
        let with = dir_with(&["hills-light.jpg", "hills-dark.jpg"]);
        assert_eq!(
            resolve(None, false, &with),
            Wallpaper::Picture(with.join("hills-light.jpg"))
        );
        let without = dir_with(&[]);
        assert_eq!(resolve(None, true, &without), Wallpaper::Background);
        assert_eq!(resolve(Some("none"), true, &with), Wallpaper::Background);
    }

    #[test]
    fn colours_paths_and_unknown_names_are_read_leniently() {
        let dir = dir_with(&["hills-dark.jpg"]);
        assert_eq!(
            resolve(Some("#1f2b48"), true, &dir),
            Wallpaper::Colour(Colour::parse("#1f2b48").unwrap())
        );
        assert_eq!(resolve(Some("#zz"), true, &dir), Wallpaper::Background);
        assert_eq!(resolve(Some("sea"), true, &dir), Wallpaper::Background);
        assert_eq!(
            resolve(Some("../hills-dark"), true, &dir),
            Wallpaper::Background
        );
        let picture = dir.join("hills-dark.jpg");
        assert_eq!(
            resolve(picture.to_str(), false, &dir),
            Wallpaper::Picture(picture.clone())
        );
        assert_eq!(
            resolve(Some("/no/such.jpg"), false, &dir),
            Wallpaper::Background
        );
    }

    #[test]
    fn dark_mode_takes_the_dark_key_when_set_else_the_light_one() {
        assert_eq!(
            value_for(Some("/a/light.jpg"), Some("/a/dark.jpg"), true),
            Some("/a/dark.jpg")
        );
        assert_eq!(
            value_for(Some("/a/light.jpg"), Some("/a/dark.jpg"), false),
            Some("/a/light.jpg")
        );
        assert_eq!(value_for(Some("hills"), None, true), Some("hills"));
        assert_eq!(value_for(None, Some("/a/dark.jpg"), false), None);
        assert_eq!(
            value_for(None, Some("/a/dark.jpg"), true),
            Some("/a/dark.jpg")
        );
        assert_eq!(value_for(None, None, true), None);
    }

    #[test]
    fn the_form_is_none_a_colour_a_path_or_a_name() {
        for good in ["none", "#1f2b48", "/home/ali/sea.jpg", "hills", "night-2"] {
            assert!(is_form(good), "{good}");
        }
        for bad in ["", "#12", "Hills", "sea.jpg", "../x", "a b", "/a\nb"] {
            assert!(!is_form(bad), "{bad:?}");
        }
    }

    #[test]
    fn check_says_what_is_wrong_and_what_would_do() {
        let dir = dir_with(&["hills-light.jpg", "hills-dark.jpg"]);
        assert_eq!(check("hills", &dir), None);
        assert_eq!(check("none", &dir), None);
        assert_eq!(check("#ffffff", &dir), None);
        assert_eq!(
            check("sea", &dir).unwrap(),
            "there is no wallpaper named sea; the wallpapers are hills, or give a picture's path, a colour as #rrggbb, or none"
        );
        assert_eq!(
            check("#12", &dir).unwrap(),
            "#12 is not a colour; write it as #rrggbb"
        );
        assert_eq!(
            check("/no/such.jpg", &dir).unwrap(),
            "there is no picture at /no/such.jpg"
        );
    }
}
