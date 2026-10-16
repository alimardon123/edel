//! The place of this computer (M6.14c): what the location portal gives apps
//! when they ask. The place stays on the computer. GeoClue, the system's
//! location service, reads `/etc/geolocation` as its static source, which
//! `edel settings apply` writes from `location.place`: a city, named by its
//! time zone, whose coordinates come from tzdata. With `location.online`,
//! GeoClue also finds the computer from the Wi-Fi networks around it through
//! BeaconDB, by a drop-in that `apply` writes. Pure functions on the settings
//! file and on text; `machine.rs` writes the files.

use crate::settings::SettingsFile;
use crate::timezones;

/// The accuracy the place is given with, in metres: a city, not a street,
/// as the place is a city.
pub const ACCURACY_METRES: u32 = 10_000;

/// The zone the place is of: `location.place`, else the time zone of
/// `region.timezone`; None when that is `UTC` (or no zone at all), so no
/// place is given.
pub fn zone_of(file: &SettingsFile) -> Option<&str> {
    let zone = file
        .location
        .place
        .as_deref()
        .or(file.region.timezone.as_deref())?;
    (zone != timezones::DEFAULT).then_some(zone)
}

/// The text of GeoClue's static source for `zone` at the coordinates given:
/// as GeoClue reads it, one bare number a line after any comment, the
/// latitude and longitude in degrees, the altitude (0) and the accuracy in
/// metres (`ACCURACY_METRES`).
pub fn geolocation_text(zone: &str, latitude: f64, longitude: f64) -> String {
    format!(
        "# Written by edel settings apply from location.place ({zone}); edit that, not this file.\n\
         {latitude:.6}\n\
         {longitude:.6}\n\
         0\n\
         {ACCURACY_METRES}\n"
    )
}

/// The drop-in that turns GeoClue's lookup of nearby Wi-Fi networks on, with
/// BeaconDB as the service that answers it. Its submit-data is off, so the
/// computer sends the networks it sees and never gives its own data back.
pub const ONLINE_CONF: &str = "# Written by edel settings apply from location.online: find this computer\n\
# from the Wi-Fi networks around it, through the community service BeaconDB.\n\
[wifi]\n\
enable=true\n\
url=https://api.beacondb.net/v1/geolocate\n\
submit-data=false\n";

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> SettingsFile {
        crate::settings::read(text).expect("a readable file").file
    }

    #[test]
    fn the_place_wins_over_the_time_zone_and_utc_gives_none() {
        let placed = file(
            "format = 1\n[region]\ntimezone = \"Europe/Berlin\"\n[location]\nplace = \"Asia/Tashkent\"\n",
        );
        assert_eq!(zone_of(&placed), Some("Asia/Tashkent"));
        let zoned = file("format = 1\n[region]\ntimezone = \"Europe/Berlin\"\n");
        assert_eq!(zone_of(&zoned), Some("Europe/Berlin"));
        assert_eq!(zone_of(&file("format = 1\n")), None);
        let utc = file("format = 1\n[region]\ntimezone = \"UTC\"\n");
        assert_eq!(zone_of(&utc), None);
        let utc_place = file(
            "format = 1\n[region]\ntimezone = \"Europe/Berlin\"\n[location]\nplace = \"UTC\"\n",
        );
        assert_eq!(zone_of(&utc_place), None);
    }

    #[test]
    fn the_static_source_holds_the_place_as_geoclue_reads_it() {
        assert_eq!(
            geolocation_text("Asia/Tashkent", 41.0 + 20.0 / 60.0, 69.3),
            "# Written by edel settings apply from location.place (Asia/Tashkent); edit that, not this file.\n\
             41.333333\n\
             69.300000\n\
             0\n\
             10000\n"
        );
    }

    #[test]
    fn the_online_drop_in_turns_on_the_wifi_lookup_only() {
        assert!(ONLINE_CONF.contains("[wifi]\nenable=true\n"));
        assert!(ONLINE_CONF.contains("url=https://api.beacondb.net/v1/geolocate\n"));
        assert!(ONLINE_CONF.contains("submit-data=false\n"));
        assert!(!ONLINE_CONF.contains("[ip]"));
    }
}
