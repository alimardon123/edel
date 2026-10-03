//! The panel's clock (M5.1b): the local time as hours and minutes, from
//! the zone `/etc/localtime` names, else UTC, and how long until the next
//! minute starts, when the panel draws it again. Nothing ticks between.

use std::time::Duration;

use jiff::Zoned;

/// `14:05`.
pub fn text(now: &Zoned) -> String {
    now.strftime("%H:%M").to_string()
}

/// From `now` to the start of the next minute, and a little more, so the
/// new minute is always the one read.
pub fn until_next_minute(now: &Zoned) -> Duration {
    let into =
        u64::from(now.second().unsigned_abs()) * 1000 + u64::from(now.millisecond().unsigned_abs());
    Duration::from_millis(60_000 - into.min(59_999) + 20)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Zoned {
        text.parse().unwrap()
    }

    #[test]
    fn the_clock_shows_hours_and_minutes() {
        assert_eq!(text(&at("2026-10-03T14:05:59+00:00[UTC]")), "14:05");
        assert_eq!(text(&at("2026-10-03T09:00:00+00:00[UTC]")), "09:00");
    }

    #[test]
    fn it_wakes_just_after_the_next_minute() {
        assert_eq!(
            until_next_minute(&at("2026-10-03T14:05:00+00:00[UTC]")),
            Duration::from_millis(60_020)
        );
        assert_eq!(
            until_next_minute(&at("2026-10-03T14:05:59.5+00:00[UTC]")),
            Duration::from_millis(520)
        );
    }
}
