//! Sunrise and sunset (M7.14): when the desktop turns dark and light again
//! under `appearance.mode = "auto"` at the place `location.place` names
//! (M6.14c), worked out here alone from the place's coordinates (tzdata's,
//! `timezones::coordinates`) and the clock, with NOAA's solar equations: no
//! network and no location service. The compositor and shell-ui read the
//! scheme through [`scheme_of`], and the compositor wakes at [`next_of`]'s
//! time; night light reads the same times later (Seam, ADR-010).

use crate::i18n::trf;
use crate::settings;
use crate::timezones;
use crate::tokens::Scheme;

/// `appearance.mode`'s key, the one name every part reads it by.
pub const MODE: &str = "appearance.mode";

/// `location.place`'s key: auto follows the sun only where it names a place.
pub const PLACE: &str = "location.place";

/// How far below the horizon the sun's middle is at sunrise and sunset, in
/// degrees: its radius and the air's refraction, as almanacs count it.
const HORIZON: f64 = -0.833;

/// Seconds in a day.
const DAY: i64 = 86_400;

/// How many days ahead [`next_change`] looks: past a polar night.
const LOOK_AHEAD: i64 = 370;

/// What the sun does on one day at a place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Day {
    /// It rises and sets, at these times in seconds since 1970 (UTC).
    Times { rise: i64, set: i64 },
    /// It stays up all day (a polar day).
    Up,
    /// It stays down all day (a polar night).
    Down,
}

/// The sun at `latitude` and `longitude` (degrees, north and east) on the
/// day `day` (days since 1970, UTC): its rise before and its set after the
/// solar noon nearest that day's noon at that longitude.
pub fn day(latitude: f64, longitude: f64, day: i64) -> Day {
    // The solar noon first, then each event worked out again at its own
    // time, as the sun moves a little between them.
    let noon = (day * DAY) as f64 + 43_200.0 - longitude * 240.0;
    let at = |guess: f64, sign: f64| -> Option<f64> {
        let mut t = guess;
        for _ in 0..2 {
            let (eq_time, declination) = sun_at(t);
            let hour = hour_angle(latitude, declination)?;
            let solar_noon = (day * DAY) as f64 + 43_200.0 - longitude * 240.0 - eq_time * 60.0;
            t = solar_noon + sign * hour * 240.0;
        }
        Some(t)
    };
    let (_, declination) = sun_at(noon);
    match hour_angle(latitude, declination) {
        None if latitude * declination > 0.0 => Day::Up,
        None => Day::Down,
        Some(_) => match (at(noon, -1.0), at(noon, 1.0)) {
            (Some(rise), Some(set)) => Day::Times {
                rise: rise.round() as i64,
                set: set.round() as i64,
            },
            _ if latitude * declination > 0.0 => Day::Up,
            _ => Day::Down,
        },
    }
}

/// The equation of time in minutes and the sun's declination in degrees at
/// `t` seconds since 1970, by NOAA's solar calculator.
fn sun_at(t: f64) -> (f64, f64) {
    let julian = t / DAY as f64 + 2440587.5;
    let c = (julian - 2451545.0) / 36525.0;
    let mean_long = (280.46646 + c * (36000.76983 + c * 0.0003032)).rem_euclid(360.0);
    let anomaly = 357.52911 + c * (35999.05029 - 0.0001537 * c);
    let e = 0.016708634 - c * (0.000042037 + 0.0000001267 * c);
    let m = anomaly.to_radians();
    let centre = m.sin() * (1.914602 - c * (0.004817 + 0.000014 * c))
        + (2.0 * m).sin() * (0.019993 - 0.000101 * c)
        + (3.0 * m).sin() * 0.000289;
    let omega = (125.04 - 1934.136 * c).to_radians();
    let apparent = (mean_long + centre - 0.00569 - 0.00478 * omega.sin()).to_radians();
    let mean_obliquity =
        23.0 + (26.0 + (21.448 - c * (46.815 + c * (0.00059 - c * 0.001813))) / 60.0) / 60.0;
    let obliquity = (mean_obliquity + 0.00256 * omega.cos()).to_radians();
    let declination = (obliquity.sin() * apparent.sin()).asin();
    let y = (obliquity / 2.0).tan().powi(2);
    let l0 = mean_long.to_radians();
    let eq_time = 4.0
        * (y * (2.0 * l0).sin() - 2.0 * e * m.sin() + 4.0 * e * y * m.sin() * (2.0 * l0).cos()
            - 0.5 * y * y * (4.0 * l0).sin()
            - 1.25 * e * e * (2.0 * m).sin())
        .to_degrees();
    (eq_time, declination.to_degrees())
}

/// The hour angle of sunrise in degrees, none when the sun does not cross
/// the horizon that day.
fn hour_angle(latitude: f64, declination: f64) -> Option<f64> {
    let (phi, delta) = (latitude.to_radians(), declination.to_radians());
    let cos = (HORIZON.to_radians().sin() - phi.sin() * delta.sin()) / (phi.cos() * delta.cos());
    (-1.0..=1.0).contains(&cos).then(|| cos.acos().to_degrees())
}

/// A change of scheme at a time, in seconds since 1970.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub at: i64,
    pub to: Scheme,
}

/// The sun's events around `from_day`, in time order: each a time and the
/// scheme it brings (sunrise light, sunset dark).
fn events(latitude: f64, longitude: f64, days: std::ops::RangeInclusive<i64>) -> Vec<Change> {
    let mut all = Vec::new();
    for d in days {
        if let Day::Times { rise, set } = day(latitude, longitude, d) {
            all.push(Change {
                at: rise,
                to: Scheme::Light,
            });
            all.push(Change {
                at: set,
                to: Scheme::Dark,
            });
        }
    }
    all.sort_by_key(|c| c.at);
    all
}

/// The scheme the sun gives at `now` (seconds since 1970) at the place:
/// light from sunrise, dark from sunset, and through a polar day or night
/// what that day is.
pub fn scheme_at(latitude: f64, longitude: f64, now: i64) -> Scheme {
    let today = now.div_euclid(DAY);
    if let Some(last) = events(latitude, longitude, today - 1..=today + 1)
        .into_iter()
        .rev()
        .find(|c| c.at <= now)
    {
        return last.to;
    }
    match day(latitude, longitude, today) {
        Day::Down => Scheme::Dark,
        _ => Scheme::Light,
    }
}

/// The next time the sun changes the scheme after `now` at the place, none
/// within [`LOOK_AHEAD`] days.
pub fn next_change(latitude: f64, longitude: f64, now: i64) -> Option<Change> {
    let shown = scheme_at(latitude, longitude, now);
    let today = now.div_euclid(DAY);
    // A day at a time, so a polar night costs a few hundred small sums,
    // once, never a list of a year's events.
    (today - 1..=today + LOOK_AHEAD).find_map(|d| {
        events(latitude, longitude, d..=d)
            .into_iter()
            .find(|c| c.at > now && c.to != shown)
    })
}

/// The coordinates auto follows: those of the place `location.place` names
/// (a city by its time zone), none without a place or for `UTC`.
pub fn place_coordinates(place: Option<&str>) -> Option<(f64, f64)> {
    timezones::coordinates(place?)
}

/// The scheme `mode` gives at `now`: `light` and `dark` as they are; `auto`
/// (and no value, its default) the sun's at the place's coordinates, else
/// the release's choice, light; anything else light too. The change that
/// comes next comes with it, only for auto with a place.
pub fn scheme_for(
    mode: Option<&str>,
    place: Option<(f64, f64)>,
    now: i64,
) -> (Scheme, Option<Change>) {
    match (mode.unwrap_or("auto"), place) {
        ("auto", Some((lat, lon))) => (scheme_at(lat, lon, now), next_change(lat, lon, now)),
        (value, _) => (Scheme::parse(value).unwrap_or_default(), None),
    }
}

/// The scheme the settings files give now: `appearance.mode` and
/// `location.place`, each the person's over the machine's (themes and
/// presets applied, `settings::chosen`), and the change that comes next.
/// Every part that decides light or dark calls this.
pub fn scheme_of(machine: Option<&str>, person: Option<&str>) -> (Scheme, Option<Change>) {
    let mode = settings::chosen(MODE, machine, person);
    let place = settings::chosen(PLACE, machine, person);
    scheme_for(mode.as_deref(), place_coordinates(place.as_deref()), now())
}

/// Seconds since 1970 by the system's clock.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The words for a change, as Settings shows them under Automatic and
/// `edel settings get appearance.mode` prints them: "Dark at 18:42" in this
/// computer's time zone, with the day when it is not within a day.
pub fn change_words(change: Change, now: i64) -> String {
    let Ok(stamp) = jiff::Timestamp::from_second(change.at) else {
        return String::new();
    };
    let local = stamp.to_zoned(crate::timezones::system());
    let time = local.strftime("%H:%M").to_string();
    let soon = change.at - now < DAY;
    match (change.to, soon) {
        (Scheme::Dark, true) => trf("Dark at {time}", &[("time", &time)]),
        (Scheme::Light, true) => trf("Light at {time}", &[("time", &time)]),
        (to, false) => {
            let date = local.strftime("%Y-%m-%d").to_string();
            match to {
                Scheme::Dark => trf(
                    "Dark on {date} at {time}",
                    &[("date", &date), ("time", &time)],
                ),
                Scheme::Light => trf(
                    "Light on {date} at {time}",
                    &[("date", &date), ("time", &time)],
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seconds since 1970 of a UTC date and time.
    fn utc(y: i32, mo: i8, d: i8, h: i8, mi: i8) -> i64 {
        jiff::civil::date(y.try_into().unwrap(), mo, d)
            .at(h, mi, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap()
            .timestamp()
            .as_second()
    }

    /// The events within the UTC day of `at`, from the days around it.
    fn within(lat: f64, lon: f64, at: i64) -> Vec<Change> {
        let d = at.div_euclid(DAY);
        events(lat, lon, d - 1..=d + 1)
            .into_iter()
            .filter(|c| c.at.div_euclid(DAY) == d)
            .collect()
    }

    // The US Naval Observatory's table (aa.usno.navy.mil, Rise and Set in
    // UTC, rounded to the minute), read 2026-10-17. Each of ours within a
    // minute of it.
    #[test]
    fn the_sun_times_match_the_naval_observatory_to_the_minute() {
        let cases = [
            // Tashkent, the place CI sets.
            (
                41.3333,
                69.3,
                (2026, 10, 17),
                [(1, 36, true), (12, 39, false)],
            ),
            // London at midsummer.
            (
                51.5083,
                -0.125,
                (2026, 6, 21),
                [(3, 43, true), (20, 21, false)],
            ),
            // Reykjavik at the equinox.
            (
                64.15,
                -21.85,
                (2026, 3, 20),
                [(7, 28, true), (19, 43, false)],
            ),
            // Sydney at midsummer: its set and the next morning's rise fall
            // on the same UTC day.
            (
                -33.8667,
                151.2167,
                (2026, 12, 21),
                [(9, 5, false), (18, 41, true)],
            ),
        ];
        for (lat, lon, (y, m, d), want) in cases {
            let got = within(lat, lon, utc(y, m, d, 12, 0));
            assert_eq!(got.len(), 2, "{lat},{lon} on {y}-{m}-{d}: {got:?}");
            for (change, (h, mi, rise)) in got.iter().zip(want) {
                let table = utc(y, m, d, h, mi);
                assert!(
                    (change.at - table).abs() <= 60,
                    "{lat},{lon} on {y}-{m}-{d}: {} s from the table's {h}:{mi}",
                    change.at - table
                );
                let to = if rise { Scheme::Light } else { Scheme::Dark };
                assert_eq!(change.to, to);
            }
        }
    }

    #[test]
    fn auto_is_light_by_day_and_dark_by_night_with_the_next_change() {
        let (lat, lon) = (41.3333, 69.3);
        let morning = utc(2026, 10, 17, 8, 0);
        assert_eq!(scheme_at(lat, lon, morning), Scheme::Light);
        let next = next_change(lat, lon, morning).unwrap();
        assert_eq!(next.to, Scheme::Dark);
        assert!((next.at - utc(2026, 10, 17, 12, 39)).abs() <= 60);
        let night = utc(2026, 10, 17, 18, 0);
        assert_eq!(scheme_at(lat, lon, night), Scheme::Dark);
        let next = next_change(lat, lon, night).unwrap();
        assert_eq!(next.to, Scheme::Light);
        assert!(next.at > night && next.at - night < DAY);
        // Exactly at sunset it is dark, so a timer that fires then finds
        // the change made.
        let set = next_change(lat, lon, morning).unwrap().at;
        assert_eq!(scheme_at(lat, lon, set), Scheme::Dark);
        assert_eq!(scheme_at(lat, lon, set - 1), Scheme::Light);
    }

    #[test]
    fn a_polar_night_is_dark_until_the_sun_comes_back() {
        // Longyearbyen, 78 degrees north, in December and in June.
        let (lat, lon) = (78.2167, 15.6333);
        let december = utc(2026, 12, 21, 12, 0);
        assert_eq!(day(lat, lon, december.div_euclid(DAY)), Day::Down);
        assert_eq!(scheme_at(lat, lon, december), Scheme::Dark);
        let back = next_change(lat, lon, december).unwrap();
        assert_eq!(back.to, Scheme::Light);
        assert!(back.at > utc(2027, 2, 1, 0, 0) && back.at < utc(2027, 3, 1, 0, 0));
        let june = utc(2026, 6, 21, 0, 0);
        assert_eq!(day(lat, lon, june.div_euclid(DAY)), Day::Up);
        assert_eq!(scheme_at(lat, lon, june), Scheme::Light);
    }

    #[test]
    fn only_auto_with_a_place_follows_the_sun() {
        let night = utc(2026, 10, 17, 18, 0);
        let tashkent = Some((41.3333, 69.3));
        assert_eq!(scheme_for(Some("auto"), tashkent, night).0, Scheme::Dark);
        assert_eq!(scheme_for(None, tashkent, night).0, Scheme::Dark);
        assert!(scheme_for(None, tashkent, night).1.is_some());
        assert_eq!(
            scheme_for(Some("light"), tashkent, night),
            (Scheme::Light, None)
        );
        assert_eq!(scheme_for(Some("dark"), None, night), (Scheme::Dark, None));
        assert_eq!(scheme_for(Some("auto"), None, night), (Scheme::Light, None));
        assert_eq!(scheme_for(None, None, night), (Scheme::Light, None));
        assert_eq!(place_coordinates(None), None);
        assert_eq!(place_coordinates(Some("UTC")), None);
    }

    #[test]
    fn the_words_say_when_the_change_comes() {
        let now = utc(2026, 10, 17, 8, 0);
        let soon = Change {
            at: utc(2026, 10, 17, 12, 39),
            to: Scheme::Dark,
        };
        // The time is this computer's, so only its shape is held here.
        let words = change_words(soon, now);
        assert!(words.starts_with("Dark at "), "{words}");
        assert_eq!(words.len(), "Dark at 12:39".len());
        let later = Change {
            at: utc(2027, 2, 14, 9, 30),
            to: Scheme::Light,
        };
        assert!(change_words(later, now).starts_with("Light on 2027-02-1"));
    }
}
