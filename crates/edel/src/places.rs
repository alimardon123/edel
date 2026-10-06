//! Where Edel OS keeps its files (ADR-010): every part names Edel OS's
//! places through this module, so a path or a file's name is written once
//! and a change to it is one line here. M5.25a starts it with the settings
//! file, the one file that describes a machine (ADR-006, ADR-008's same
//! names decision); M5.27 brings Edel OS's other places here.
//!
//! CI's shell scripts read the same names from `ci/names.sh`, which a test
//! holds equal to this module, and a test fails when the settings file's
//! name is written anywhere else in the code, CI or the features.

use std::path::{Path, PathBuf};

/// The settings file's name. A rename, such as to `edel.toml`, changes
/// this line and adds the old name to [`FORMER_SETTINGS`].
pub const SETTINGS: &str = "settings.toml";

/// Names the settings file had before, newest first. Readers still read a
/// file by one of them when none has the new name, and `apply` renames it
/// on disk, so a machine set up before a rename keeps its settings.
pub const FORMER_SETTINGS: &[&str] = &["system.toml"];

/// Edel OS's directory on the data partition, shared by both slots.
pub const DATA_DIR: &str = "/data/edel";

/// Where a slot keeps Edel OS's own files.
pub const SHARE_DIR: &str = "/usr/share/edel";

/// Where admins look first: `/etc/edel/` holds a link to the machine's
/// settings file.
pub const ETC_DIR: &str = "/etc/edel";

/// The machine's settings file, on the data partition.
pub fn machine_settings() -> PathBuf {
    Path::new(DATA_DIR).join(SETTINGS)
}

/// The slot's own settings file, which seeds a first boot when no stick
/// or EFI system partition holds one.
pub fn slot_settings() -> PathBuf {
    Path::new(SHARE_DIR).join(SETTINGS)
}

/// The link to the machine's settings file in `/etc/edel/`.
pub fn etc_settings() -> PathBuf {
    Path::new(ETC_DIR).join(SETTINGS)
}

/// A person's own directory for Edel OS: `$XDG_CONFIG_HOME/edel`, else
/// `~/.config/edel`; none without either variable.
pub fn person_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(config.join("edel"))
}

/// A person's own settings file, with the same keys as the machine's; for
/// the keys the desktop reads, its values win.
pub fn person_settings() -> Option<PathBuf> {
    person_dir().map(|dir| dir.join(SETTINGS))
}

/// The settings file named `name` inside `dir`, such as on a seed stick or
/// the EFI system partition.
pub fn settings_in(dir: &Path) -> PathBuf {
    dir.join(SETTINGS)
}

/// Whether `name` is the settings file's name, or one it had before; a
/// watcher follows a file by either.
pub fn is_settings_name(name: &str) -> bool {
    name == SETTINGS || FORMER_SETTINGS.contains(&name)
}

/// `path`, a settings file, as it is found: itself when it exists, else
/// the first file beside it by a former name that does, else `path`.
pub fn found(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    FORMER_SETTINGS
        .iter()
        .map(|old| path.with_file_name(old))
        .find(|old| old.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

/// The names to look for in a directory, the settings file's own first.
pub fn settings_names() -> impl Iterator<Item = &'static str> {
    std::iter::once(SETTINGS).chain(FORMER_SETTINGS.iter().copied())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_former_name_is_found_when_the_new_one_is_missing() {
        let dir = std::env::temp_dir().join(format!("edel-places-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = settings_in(&dir);
        assert_eq!(found(&path), path);
        fs::write(dir.join(FORMER_SETTINGS[0]), "format = 1\n").unwrap();
        assert_eq!(found(&path), dir.join(FORMER_SETTINGS[0]));
        fs::write(&path, "format = 1\n").unwrap();
        assert_eq!(found(&path), path);
        fs::remove_dir_all(&dir).unwrap();
        assert!(is_settings_name(SETTINGS) && is_settings_name(FORMER_SETTINGS[0]));
        assert!(!is_settings_name("state.toml"));
    }

    /// `ci/names.sh` says what this module says, for CI's shell scripts.
    #[test]
    fn ci_names_are_these() {
        let names = include_str!("../../../ci/names.sh");
        let line = format!("settings_name={SETTINGS}\n");
        assert!(names.contains(&line), "ci/names.sh lacks {line:?}");
    }

    /// The settings file's name, and every name it had, is written here
    /// and in `ci/names.sh` only: code, CI and features take it from
    /// them, and docs say "the settings file" (ADR-010).
    #[test]
    fn the_settings_file_is_named_in_one_place() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let allowed = [
            root.join("crates/edel/src/places.rs"),
            root.join("ci/names.sh"),
        ];
        let mut found = Vec::new();
        for top in ["crates", "ci", "features", "images", "presets"] {
            walk(&root.join(top), &mut |path| {
                if allowed.iter().any(|a| same(a, path))
                    || path.extension().is_some_and(|e| e == "md")
                    || path.components().any(|c| c.as_os_str() == "target")
                {
                    return;
                }
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let text = fs::read_to_string(path).unwrap_or_default();
                for named in settings_names() {
                    if name == named || text.contains(named) {
                        found.push(format!("{} names {named}", path.display()));
                    }
                }
            });
        }
        assert!(
            found.is_empty(),
            "the settings file's name belongs in edel::places and ci/names.sh only:\n{}",
            found.join("\n")
        );
    }

    fn same(a: &Path, b: &Path) -> bool {
        fs::canonicalize(a).ok() == fs::canonicalize(b).ok()
    }

    fn walk(dir: &Path, visit: &mut impl FnMut(&Path)) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, visit);
            } else {
                visit(&path);
            }
        }
    }
}
