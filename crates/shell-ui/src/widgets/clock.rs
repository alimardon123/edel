//! The panel's clock (M5.1b, M5.29): the local time as hours and minutes
//! over a short date (`Sat 3 Oct`), from the zone `/etc/localtime` names,
//! else UTC, as the mockups set it: both in tabular figures and
//! right-aligned, the time in the panel text's size, semibold, in its
//! colour, the date in `size.panel_text_small`, dimmer; and how long until
//! the next minute starts, when the panel draws it again. Nothing ticks
//! between. The date's words are English until translations come
//! (M5.24).

use std::time::Duration;

use accesskit::Role;
use jiff::Zoned;

use super::{Canvas, Live, Widget, no_input};
use crate::paint::{Face, mix};

pub const WIDGET: Widget = Widget {
    name: "clock",
    needs: None,
    shows,
    width,
    draw,
    input: no_input,
    parts: super::no_parts,
    role: Role::Label,
    label,
};

/// What it shows: the time, a line break and the date.
fn shows(_: &Live) -> String {
    text(&Zoned::now())
}

/// What a screen reader says: "14:05, Sat 3 Oct".
fn label(shown: &str) -> String {
    shown.replace('\n', ", ")
}

/// The room before the text and after it, in logical pixels.
const ROOM_START: f32 = 6.0;
const ROOM_END: f32 = 8.0;
/// How far apart the two lines' baselines are, in times the time's size
/// (the mockups' line height).
const LEADING: f32 = 1.2;

/// The time's and the date's size in the pixmap's pixels.
fn sizes(canvas: &Canvas) -> (f32, f32) {
    let t = canvas.tokens;
    (
        t.panel_text_size as f32 * canvas.scale,
        t.panel_text_small_size as f32 * canvas.scale,
    )
}

/// The faces of the time and the date.
const TIME: Face = Face::SEMIBOLD.tabular();
const DATE: Face = Face::REGULAR.tabular();

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    let (big, small) = sizes(canvas);
    let (time, date) = shown.split_once('\n').unwrap_or((shown, ""));
    let room = (ROOM_START + ROOM_END) * canvas.scale;
    let Some(text) = canvas.text.as_deref_mut() else {
        return room;
    };
    let wide = text
        .line_in(time, big, TIME)
        .width
        .max(text.line_in(date, small, DATE).width);
    wide + room
}

/// Both lines right-aligned, together centred in the panel's height.
fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let (big, small) = sizes(canvas);
    let s = canvas.scale;
    let (time, date) = shown.split_once('\n').unwrap_or((shown, ""));
    let tokens = canvas.tokens;
    // The lines' boxes are 1.25 times their size; they are set 1.2 times
    // the time's size apart, as a web page's line height would.
    let block = big * LEADING + small * LEADING;
    let top = canvas.top + ((canvas.height - block) / 2.0).round();
    let dim = mix(tokens.panel_text, tokens.panel, 0.3);
    let ink = tokens.panel_text;
    let Some(text) = canvas.text.as_deref_mut() else {
        return;
    };
    let mut first = text.line_in(time, big, TIME);
    let mut second = text.line_in(date, small, DATE);
    let right = x + ROOM_START * s + first.width.max(second.width);
    let (x1, x2) = (right - first.width, right - second.width);
    text.draw(canvas.pixmap, &mut first, x1, top, ink);
    let at = top + (big * LEADING).round();
    text.draw(canvas.pixmap, &mut second, x2, at, dim);
}

/// `14:05`, a line break and `Sat 3 Oct`.
pub fn text(now: &Zoned) -> String {
    now.strftime("%H:%M\n%a %-d %b").to_string()
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
    fn the_clock_shows_the_time_over_a_short_date() {
        assert_eq!(
            text(&at("2026-10-03T14:05:59+00:00[UTC]")),
            "14:05\nSat 3 Oct"
        );
        assert_eq!(
            text(&at("2026-10-03T09:00:00+00:00[UTC]")),
            "09:00\nSat 3 Oct"
        );
        assert_eq!(
            text(&at("2026-12-25T23:59:00+00:00[UTC]")),
            "23:59\nFri 25 Dec"
        );
        assert_eq!(
            text(&at("2027-01-01T00:00:00+00:00[UTC]")),
            "00:00\nFri 1 Jan"
        );
        assert_eq!(label("14:05\nSat 3 Oct"), "14:05, Sat 3 Oct");
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
