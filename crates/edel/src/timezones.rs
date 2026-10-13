//! Time zones (`region.timezone`): the names IANA's tzdata gives (`Europe/Berlin`),
//! the list a person picks from, the one the installer suggests, and how one reads
//! in a list. The zone files are tzdata's, in `places::ZONEINFO_DIR`; nothing is
//! downloaded.

use std::path::Path;

use crate::i18n::tr;
use crate::languages;
use crate::lock_screen;
use crate::places;

/// The release's zone when the file names none: the clock in UTC.
pub const DEFAULT: &str = "UTC";

/// Whether `name` has a zone name's shape: letters, digits, `/`, `_`, `-`
/// and `+`, no leading, trailing or double `/`, at most 64 characters.
pub fn is_zone_name(name: &str) -> bool {
    lock_screen::is_zone_name(name)
}

/// Whether `name` is a zone this machine has a file for in `dir`
/// (`places::ZONEINFO_DIR`, or a test's folder); with no such folder at
/// all, a name of the right shape is taken (a server image carries no
/// tzdata, and the file is applied where it does).
pub fn known_in(dir: &Path, name: &str) -> bool {
    if !dir.is_dir() {
        return is_zone_name(name);
    }
    is_zone_name(name) && !name.contains("..") && dir.join(name).is_file()
}

/// The zones a person picks from: tzdata's `zone1970.tab` in `dir` (its third
/// column), and `UTC`, each once, sorted by `label`. Empty without the file.
pub fn list_in(dir: &Path) -> Vec<String> {
    let Ok(table) = std::fs::read_to_string(dir.join("zone1970.tab")) else {
        return Vec::new();
    };
    let mut zones: Vec<String> = table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split('\t').nth(2))
        .map(str::to_string)
        .collect();
    zones.push(DEFAULT.to_string());
    zones.sort();
    zones.dedup();
    zones.sort_by_key(|zone| label(zone));
    zones
}

/// How a zone reads in a list: its city with spaces, then its region after a
/// middle dot, such as "New York · America"; "UTC" stays as it is; a deeper
/// zone such as `America/Argentina/Buenos_Aires` reads "Buenos Aires · America".
pub fn label(zone: &str) -> String {
    match zone.split('/').collect::<Vec<_>>().as_slice() {
        [region, .., city] => format!("{} · {region}", city.replace('_', " ")),
        _ => zone.to_string(),
    }
}

/// The zone the installer suggests for `language` (a locale such as
/// `uz_UZ.UTF-8`): the language table's, else `DEFAULT`. A language given
/// without `.UTF-8` is looked up with it.
pub fn suggested(language: &str) -> &'static str {
    languages::find(language)
        .or_else(|| languages::find(&format!("{language}.UTF-8")))
        .map_or(DEFAULT, |found| found.zone)
}

/// The zone answer as every way of asking takes it (the terminal and
/// Settings): a zone this machine knows, or the words saying what to give.
pub fn answer(value: &str) -> Result<String, String> {
    let value = value.trim();
    if known_in(Path::new(places::ZONEINFO_DIR), value) {
        Ok(value.to_string())
    } else {
        Err(tr("a time zone such as Europe/Berlin or Asia/Tashkent").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder for one test, named after it and the process, removed after.
    fn scratch(test: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("edel-timezones-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn without_a_folder_a_name_of_the_right_shape_is_known() {
        let missing = std::env::temp_dir().join("edel-timezones-no-such-folder");
        assert!(known_in(&missing, "Europe/Berlin"));
        assert!(!known_in(&missing, "../etc"));
    }

    #[test]
    fn with_a_folder_only_its_files_are_known() {
        let dir = scratch("known");
        std::fs::create_dir_all(dir.join("Europe")).unwrap();
        std::fs::write(dir.join("Europe/Berlin"), b"TZif").unwrap();
        assert!(known_in(&dir, "Europe/Berlin"));
        assert!(!known_in(&dir, "Europe/Nowhere"));
        assert!(!known_in(&dir, "../etc"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lists_the_zones_of_the_table_sorted_by_label_with_utc() {
        let dir = scratch("list");
        let table = "# comment line\n\
            DE,DK,NO,SE,SJ\t+5230+01322\tEurope/Berlin\tGerman\n\
            US\t+404251-0740023\tAmerica/New_York\tEastern\n\
            AR\t-3436-05827\tAmerica/Argentina/Buenos_Aires\tBuenos Aires\n";
        std::fs::write(dir.join("zone1970.tab"), table).unwrap();
        assert_eq!(
            list_in(&dir),
            vec![
                "Europe/Berlin",
                "America/Argentina/Buenos_Aires",
                "America/New_York",
                "UTC",
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(list_in(&dir).is_empty());
    }

    #[test]
    fn labels_show_the_city_then_the_region() {
        assert_eq!(label("America/New_York"), "New York · America");
        assert_eq!(label("UTC"), "UTC");
        assert_eq!(
            label("America/Argentina/Buenos_Aires"),
            "Buenos Aires · America"
        );
    }

    #[test]
    fn suggests_the_zone_of_a_language() {
        assert_eq!(suggested("uz_UZ.UTF-8"), "Asia/Tashkent");
        assert_eq!(suggested("uz_UZ"), "Asia/Tashkent");
        assert_eq!(suggested("xx"), "UTC");
    }

    #[test]
    fn every_suggested_zone_has_the_zone_shape() {
        for language in languages::LANGUAGES {
            assert!(is_zone_name(language.zone), "{}", language.zone);
        }
    }

    #[test]
    fn answers_refuse_a_zone_the_machine_lacks() {
        assert!(answer("  ").is_err());
        assert!(answer("../etc").is_err());
    }
}
