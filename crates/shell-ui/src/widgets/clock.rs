//! The panel's clock (M5.1b): the local time as hours and minutes, from
//! the zone `/etc/localtime` names, else UTC, in the panel text's colour
//! and size, with room on both sides; and how long until the next minute
//! starts, when the panel draws it again. Nothing ticks between.

use std::time::Duration;

use jiff::Zoned;

use super::{Canvas, Widget};

pub const WIDGET: Widget = Widget {
    name: "clock",
    needs: None,
    shows,
    width,
    draw,
};

fn shows() -> String {
    text(&Zoned::now())
}

/// The text's size and the room on each side of it, in the pixmap's
/// pixels.
fn sizes(canvas: &Canvas) -> (f32, f32) {
    (
        canvas.tokens.panel_text_size as f32 * canvas.scale,
        canvas.height * 0.4,
    )
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    let (size, room) = sizes(canvas);
    let text = canvas.text.as_deref_mut();
    text.map_or(0.0, |t| t.line(shown, size).width) + 2.0 * room
}

/// Centred in the panel's height.
fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let (size, room) = sizes(canvas);
    let y = canvas.top + (canvas.height - size * 1.25) / 2.0;
    let ink = canvas.tokens.panel_text;
    if let Some(text) = canvas.text.as_deref_mut() {
        let mut line = text.line(shown, size);
        text.draw(canvas.pixmap, &mut line, x + room, y, ink);
    }
}

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
