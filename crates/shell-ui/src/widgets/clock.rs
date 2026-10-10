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

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{Face, fill, mix};
use edel::i18n::n_;

pub const WIDGET: Widget = Widget {
    name: "clock",
    title: n_("Clock"),
    needs: None,
    shows,
    width,
    draw,
    input,
    parts: super::no_parts,
    role: Role::Label,
    label,
};

/// What it shows: the time, a line break and the date, and a `*` before
/// them while the notification centre is open, which the clock then lights
/// as the status area lights while quick settings are open (M5.9b).
fn shows(live: &Live) -> String {
    let now = text(&Zoned::now());
    if live.centre {
        format!("{OPEN}{now}")
    } else {
        now
    }
}

/// The mark in front of what it shows while the notification centre is
/// open.
const OPEN: char = '*';

/// What it shows without the mark, and whether the mark was there.
fn unmarked(shown: &str) -> (&str, bool) {
    match shown.strip_prefix(OPEN) {
        Some(rest) => (rest, true),
        None => (shown, false),
    }
}

/// What a screen reader says: "14:05, Sat 3 Oct".
fn label(shown: &str) -> String {
    unmarked(shown).0.replace('\n', ", ")
}

/// A click opens the notification centre, or closes it.
fn input(_: &mut Canvas, _: &str, input: Input) -> Option<Action> {
    matches!(input, Input::Click(..)).then_some(Action::Centre)
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
    let (shown, _) = unmarked(shown);
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
    let (shown, open) = unmarked(shown);
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
    let wide = first.width.max(second.width);
    if open {
        // Lit as the status area's pill is while its card is open.
        let (pw, ph) = (
            wide + (ROOM_START + ROOM_END) * s,
            tokens.panel_control as f32 * s,
        );
        let py = canvas.top + ((canvas.height - ph) / 2.0).round();
        let lit = edel::tokens::Colour {
            a: 0.085,
            ..tokens.panel_text
        };
        let r = tokens.radius_control as f32 * s;
        fill(canvas.pixmap, x + 2.0 * s, py, pw - 4.0 * s, ph, r, lit);
    }
    let right = x + ROOM_START * s + wide;
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
    #[test]
    fn a_click_opens_the_notification_centre_and_the_clock_is_lit_while_it_is_open() {
        let mut live = Live::default();
        assert!(!shows(&live).starts_with(OPEN));
        live.centre = true;
        let open = shows(&live);
        assert!(open.starts_with(OPEN), "{open:?}");
        // The mark is no part of what a screen reader says.
        assert_eq!(label("*14:05\nSat 3 Oct"), "14:05, Sat 3 Oct");
        assert_eq!(unmarked("*14:05\nSat 3 Oct"), ("14:05\nSat 3 Oct", true));
        assert_eq!(unmarked("14:05\nSat 3 Oct"), ("14:05\nSat 3 Oct", false));
        let tokens = edel::tokens::Tokens::built_in();
        let mut pixmap = tiny_skia::Pixmap::new(200, 60).unwrap();
        let mut canvas = Canvas {
            pixmap: &mut pixmap,
            tokens: &tokens,
            text: None,
            icons: None,
            scale: 1.0,
            top: 0.0,
            height: 60.0,
            dock: false,
            along_top: false,
        };
        assert_eq!(
            input(&mut canvas, "14:05\nSat 3 Oct", Input::Click(10.0, 60.0)),
            Some(Action::Centre)
        );
        assert_eq!(input(&mut canvas, "x", Input::Menu(10.0, 60.0)), None);
        assert_eq!(input(&mut canvas, "x", Input::Scroll(1)), None);
    }

    #[test]
    fn the_open_clock_has_a_lit_button_behind_it() {
        let tokens = edel::tokens::Tokens::built_in();
        let mut text = crate::paint::Text::load(&tokens.font);
        if text.line("A", 13.0).width == 0.0 {
            return; // no fonts on this machine
        }
        let mut cell = |shown: &str| {
            let mut pixmap = tiny_skia::Pixmap::new(120, 60).unwrap();
            pixmap.fill(tiny_skia::Color::from_rgba8(
                (tokens.panel.r * 255.0) as u8,
                (tokens.panel.g * 255.0) as u8,
                (tokens.panel.b * 255.0) as u8,
                255,
            ));
            let mut canvas = Canvas {
                pixmap: &mut pixmap,
                tokens: &tokens,
                text: Some(&mut text),
                icons: None,
                scale: 1.0,
                top: 0.0,
                height: 60.0,
                dock: false,
                along_top: false,
            };
            draw(&mut canvas, shown, 0.0);
            // Inside the button's corner, clear of the letters.
            let p = pixmap.pixel(6, 30).unwrap().demultiply();
            [p.red(), p.green(), p.blue()]
        };
        let closed = cell("14:05\nSat 3 Oct");
        let open = cell("*14:05\nSat 3 Oct");
        assert_eq!(closed, tokens.panel.bytes()[..3]);
        assert_ne!(open, closed, "lit while the centre is open");
    }
}
