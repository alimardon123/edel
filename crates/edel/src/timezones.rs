//! Time zones (`region.timezone`): the names IANA's tzdata gives (`Europe/Berlin`),
//! the list a person picks from, the one the installer suggests, and how one reads
//! in a list. The zone files are tzdata's, in `places::ZONEINFO_DIR`; nothing is
//! downloaded.

use std::path::Path;

use crate::i18n::{tr, trf};
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

/// The text of tzdata's `zone1970.tab` in `dir`, the one place its name is
/// written; empty when it cannot be read.
pub fn zone_table(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("zone1970.tab")).unwrap_or_default()
}

/// The zones a person picks from: tzdata's `zone1970.tab` in `dir` (its third
/// column), and `UTC`, each once, sorted by `label`. Empty without the file.
pub fn list_in(dir: &Path) -> Vec<String> {
    let table = zone_table(dir);
    if table.is_empty() {
        return Vec::new();
    }
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

/// The latitude and longitude, in degrees, of `zone`'s principal city, from
/// the text of tzdata's `zone1970.tab` (`table`). The second column is ISO
/// 6709, `+DDMM+DDDMM` or `+DDMMSS+DDDMMSS`, `+` north and east, `-` south
/// and west. None for `UTC`, a zone the table lacks, or a malformed field.
pub fn coordinates_in(table: &str, zone: &str) -> Option<(f64, f64)> {
    if zone == DEFAULT {
        return None;
    }
    let iso = table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let mut columns = line.split('\t');
            let iso = columns.nth(1)?;
            (columns.next()? == zone).then_some(iso)
        })?;
    // The second sign starts the longitude, which has three degree digits
    // where the latitude has two.
    let cut = iso
        .char_indices()
        .skip(1)
        .find(|(_, c)| *c == '+' || *c == '-')?
        .0;
    Some((degrees(&iso[..cut], 2)?, degrees(&iso[cut..], 3)?))
}

/// One ISO 6709 coordinate, `+DDMM` or `+DDMMSS` with `width` degree
/// digits, in degrees with its sign.
fn degrees(part: &str, width: usize) -> Option<f64> {
    let sign = match part.chars().next()? {
        '+' => 1.0,
        '-' => -1.0,
        _ => return None,
    };
    let digits = &part[1..];
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let len = digits.len();
    if len != width + 2 && len != width + 4 {
        return None;
    }
    let number = |from: usize, to: usize| digits.get(from..to)?.parse::<f64>().ok();
    let whole = number(0, width)?;
    let minutes = number(width, width + 2)?;
    let seconds = if len == width + 4 {
        number(width + 2, width + 4)?
    } else {
        0.0
    };
    Some(sign * (whole + minutes / 60.0 + seconds / 3600.0))
}

/// The coordinates of `zone` from the tzdata files in `places::ZONEINFO_DIR`,
/// as [`coordinates_in`] reads them; None without the files.
pub fn coordinates(zone: &str) -> Option<(f64, f64)> {
    coordinates_in(&zone_table(Path::new(places::ZONEINFO_DIR)), zone)
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
    } else if value.is_empty() {
        Err(tr("choose a time zone, such as Europe/Berlin or Asia/Tashkent").to_string())
    } else {
        Err(trf(
            "{value} is not a time zone this computer knows; choose one such as Europe/Berlin or Asia/Tashkent",
            &[("value", &format!("{value:?}"))],
        ))
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

    fn close(value: Option<(f64, f64)>, lat: f64, lon: f64) -> bool {
        value.is_some_and(|(a, b)| (a - lat).abs() < 1e-6 && (b - lon).abs() < 1e-6)
    }

    #[test]
    fn coordinates_read_the_iso_6709_fields_of_the_table() {
        let table = "# comment\n\
            UZ\t+4120+06918\tAsia/Tashkent\tTashkent\n\
            AU\t-3352+15113\tAustralia/Sydney\tNSW\n\
            XX\t+651234-1502345\tExample/Seconds\tsecond form\n\
            YY\t+41x0+06918\tExample/Broken\tbroken\n";
        assert!(close(
            coordinates_in(table, "Asia/Tashkent"),
            41.0 + 20.0 / 60.0,
            69.3
        ));
        assert!(close(
            coordinates_in(table, "Australia/Sydney"),
            -(33.0 + 52.0 / 60.0),
            151.0 + 13.0 / 60.0
        ));
        assert!(close(
            coordinates_in(table, "Example/Seconds"),
            65.0 + 12.0 / 60.0 + 34.0 / 3600.0,
            -(150.0 + 23.0 / 60.0 + 45.0 / 3600.0)
        ));
        assert_eq!(coordinates_in(table, "Example/Broken"), None);
        assert_eq!(coordinates_in(table, "Nowhere/Land"), None);
        assert_eq!(coordinates_in(table, DEFAULT), None);
    }

    #[test]
    fn a_zone_without_the_files_has_no_coordinates() {
        let dir = scratch("coordinates");
        assert!(zone_table(&dir).is_empty());
        std::fs::write(dir.join("zone1970.tab"), "UZ\t+4120+06918\tAsia/Tashkent\n").unwrap();
        assert!(zone_table(&dir).contains("Asia/Tashkent"));
        std::fs::remove_dir_all(&dir).unwrap();
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
