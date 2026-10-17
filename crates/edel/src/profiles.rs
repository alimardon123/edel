//! Profiles (roadmap M7.6): what a kind of work needs, each one file under
//! `profiles/`, built into the library. A profile is a few lines of the
//! settings file (the add-ons, the apps, a value or two); the machine's file
//! names the profiles it wants, `system.profiles`, and never their lines,
//! so a profile a release renews reaches every machine that names it
//! (ADR-008). Readers lay the profiles under the file through
//! [`crate::settings::in_use`] and the system's own readers through
//! [`applied`]; the writers never do. (The kernel's power profile is
//! [`crate::profile`], another thing.)
//!
//! Seam (ADR-011): roles for old devices (M7.10m) are profiles too; a
//! profile's `[services]` lines start and stop what a role needs.

use toml::{Table, Value};

use crate::i18n::{n_, tr};
use crate::settings::{self, SettingsFile};

/// The profiles this release has, by name, in the order the installer and
/// Settings offer them. A new one is a file under `profiles/`, a line here,
/// its name in [`NAMES`] and its words in [`title`] and [`about`].
pub const BUILT_IN: &[(&str, &str)] = &[
    ("office", include_str!("../../../profiles/office.toml")),
    (
        "developer",
        include_str!("../../../profiles/developer.toml"),
    ),
    ("creative", include_str!("../../../profiles/creative.toml")),
    ("gaming", include_str!("../../../profiles/gaming.toml")),
];

/// The names in [`BUILT_IN`]: what `system.profiles` may list on this
/// release, so `edel settings check` and `set` refuse any other.
pub const NAMES: &[&str] = &["office", "developer", "creative", "gaming"];

/// The sections a profile may not set: who logs in, the machine's own
/// kind and profiles, what an install does, and a person's own presets
/// and themes.
const NOT_IN_A_PROFILE: &[&str] = &["system", "users", "install", "presets", "themes"];

/// A profile's name as people read it, in their language.
pub fn title(name: &str) -> String {
    match name {
        "office" => tr(n_("Office")).to_string(),
        "developer" => tr(n_("Developer")).to_string(),
        "creative" => tr(n_("Creative")).to_string(),
        "gaming" => tr(n_("Gaming")).to_string(),
        _ => name.to_string(),
    }
}

/// What a profile brings, in a few words, as the installer and Settings
/// say it.
pub fn about(name: &str) -> String {
    match name {
        "office" => tr(n_("LibreOffice and printing")).to_string(),
        "developer" => tr(n_("Podman containers and the VSCodium editor")).to_string(),
        "creative" => tr(n_("Krita for drawing and GIMP for photos")).to_string(),
        "gaming" => tr(n_("Steam, and game mode for every fullscreen game")).to_string(),
        _ => String::new(),
    }
}

/// Whether `name` is a profile this release has.
pub fn is_built_in(name: &str) -> bool {
    NAMES.contains(&name)
}

/// Problems with the file's `system.profiles`, one line each, as `edel
/// settings check` and `set` give them: a name this release has no
/// profile for.
pub fn problems(file: &SettingsFile) -> Vec<String> {
    file.system
        .profiles
        .iter()
        .flatten()
        .filter(|name| !is_built_in(name))
        .map(|name| {
            format!(
                "system.profiles: {}",
                crate::i18n::trf(
                    "there is no profile {name}; the profiles are {all}",
                    &[("name", &format!("{name:?}")), ("all", &NAMES.join(", "))]
                )
            )
        })
        .collect()
}

/// The lines of profile `name` as a table, without `format`; none for a
/// name this release lacks.
fn lines(name: &str) -> Option<Table> {
    let (_, text) = BUILT_IN.iter().find(|(n, _)| *n == name)?;
    // Every built-in profile is checked by the tests.
    let mut table: Table = toml::from_str(text).expect("the profiles are checked by the tests");
    table.remove("format");
    Some(table)
}

/// The file with the profiles it names laid under it: a value the file
/// sets stays, a list the file has keeps its items and gains the
/// profile's after them, none twice, and a key only a profile sets takes
/// its value. Profiles are read in the order the file names them, after
/// the file and before the preset (ADR-008's order), so of two profiles
/// that set one value, the first one's wins. A name this release lacks is
/// passed over ([`problems`] names it). Pure.
pub fn applied(file: &SettingsFile) -> SettingsFile {
    let names = file.system.profiles.as_deref().unwrap_or_default();
    if names.is_empty() {
        return file.clone();
    }
    let Ok(Value::Table(mut out)) = Value::try_from(file) else {
        return file.clone();
    };
    for table in names.iter().filter_map(|n| lines(n)) {
        lay_under(&mut out, table);
    }
    Value::Table(out)
        .try_into()
        .unwrap_or_else(|_| file.clone())
}

/// Lays `profile` under `own`, table by table: a value `own` has stays, a
/// list gains the items it lacks.
fn lay_under(own: &mut Table, profile: Table) {
    for (key, value) in profile {
        match (own.get_mut(&key), value) {
            (Some(Value::Table(inner)), Value::Table(theirs)) => lay_under(inner, theirs),
            (Some(Value::Array(items)), Value::Array(theirs)) => {
                for item in theirs {
                    if !items.contains(&item) {
                        items.push(item);
                    }
                }
            }
            (Some(_), _) => {}
            (None, value) => {
                own.insert(key, value);
            }
        }
    }
}

/// Reads a profile strictly, as the tests read every built-in one: a
/// settings file with no problems, no key this release skips and none of
/// the sections a profile may not set.
pub fn check(text: &str) -> anyhow::Result<()> {
    let read = settings::read(text)?;
    if let Some(problem) = read.problems.first() {
        anyhow::bail!("{problem}");
    }
    if let Some(key) = read.later.first() {
        anyhow::bail!("{key} is not supported yet");
    }
    let table: Table = toml::from_str(text)?;
    if let Some(section) = NOT_IN_A_PROFILE.iter().find(|s| table.contains_key(**s)) {
        anyhow::bail!("a profile may not set [{section}]");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> SettingsFile {
        settings::read(text).unwrap().file
    }

    #[test]
    fn every_profile_reads_strictly_and_is_named_once() {
        assert_eq!(BUILT_IN.len(), NAMES.len());
        for ((name, text), listed) in BUILT_IN.iter().zip(NAMES) {
            assert_eq!(name, listed);
            check(text).unwrap_or_else(|e| panic!("profiles/{name}.toml: {e}"));
            assert_ne!(title(name), *name, "{name} has no title");
            assert!(!about(name).is_empty(), "{name} has no words");
        }
    }

    #[test]
    fn a_profile_may_not_name_people_or_profiles() {
        assert!(check("format = 1\n[system]\nprofiles = [\"gaming\"]\n").is_err());
        assert!(check("format = 1\n[users.ann]\nadmin = true\n").is_err());
    }

    #[test]
    fn every_add_on_a_profile_names_is_one() {
        let features = concat!(env!("CARGO_MANIFEST_DIR"), "/../../features");
        for name in NAMES {
            let named = applied(&file(&format!(
                "format = 1\n[system]\nprofiles = [\"{name}\"]\n"
            )));
            for addon in named.addons.installed.iter().flatten() {
                let text = std::fs::read_to_string(format!("{features}/{addon}.toml"))
                    .unwrap_or_else(|_| panic!("{name}: no feature {addon}"));
                assert!(
                    text.contains("addon = true"),
                    "{name}: {addon} is no add-on"
                );
            }
        }
    }

    #[test]
    fn a_file_without_profiles_is_unchanged() {
        let own = file("format = 1\n[power]\ngame_mode = \"off\"\n");
        assert_eq!(applied(&own), own);
    }

    #[test]
    fn the_file_wins_and_its_lists_gain_the_profiles_items() {
        let own = applied(&file(
            "format = 1\n[system]\nprofiles = [\"gaming\"]\n\
             [power]\ngame_mode = \"off\"\n\
             [apps]\ninstalled = [\"io.mpv.Mpv\"]\n",
        ));
        assert_eq!(own.power.game_mode.as_deref(), Some("off"));
        assert_eq!(
            own.apps.installed.unwrap(),
            ["io.mpv.Mpv", "com.valvesoftware.Steam"]
        );
        // What the file leaves out the profile gives.
        let bare = applied(&file("format = 1\n[system]\nprofiles = [\"gaming\"]\n"));
        assert_eq!(bare.power.game_mode.as_deref(), Some("always"));
    }

    #[test]
    fn profiles_merge_in_the_order_named_and_the_same_every_time() {
        let both = "format = 1\n[system]\nprofiles = [\"office\", \"developer\"]\n";
        let once = applied(&file(both));
        assert_eq!(once, applied(&file(both)));
        assert_eq!(
            once.addons.installed.unwrap(),
            ["printing", "containers"],
            "the profiles' items in the order they are named"
        );
        let apps = once.apps.installed.unwrap();
        assert!(apps.contains(&"org.libreoffice.LibreOffice".to_string()));
        assert!(apps.contains(&"com.vscodium.codium".to_string()));
        // The file still names only the profiles.
        assert_eq!(once.system.profiles.unwrap(), ["office", "developer"]);
    }

    #[test]
    fn a_name_this_release_lacks_is_passed_over_and_named() {
        let own = file("format = 1\n[system]\nprofiles = [\"chess\", \"office\"]\n");
        assert!(applied(&own).apps.installed.is_some());
        let lines = problems(&own);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"chess\""), "{}", lines[0]);
    }
}
