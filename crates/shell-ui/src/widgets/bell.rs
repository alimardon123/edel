//! The notifications button (M5.9k): the shell's own bell, `design/icons/
//! bell.svg`, in a square tile `size.panel_control` high, which any panel
//! can hold (Edit panels or `panels.list`); no built-in preset holds it,
//! as the clock opens the notifications already. While notifications are
//! unread it shows what `notifications.badge` says: a small dot in the
//! accent (the default), how many, or nothing. A click opens the
//! notifications without the calendar, or closes them; while they are
//! open the tile is lit as the menu button's is.

use accesskit::Role;
use edel::i18n::{n_, tr, trf};
use edel::tokens::{Colour, Tokens};

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{self, Face, fill};

pub const WIDGET: Widget = Widget {
    name: "notifications",
    title: n_("Notifications"),
    needs: None,
    shows,
    width,
    draw,
    input: |_, _, what| input(what),
    parts: super::no_parts,
    role: Role::Button,
    label,
};

/// What it shows: `OPEN\tUNREAD\tBADGE`, `OPEN` being `1` while the
/// notifications it opened are open.
fn shows(live: &Live) -> String {
    format!(
        "{}\t{}\t{}",
        u8::from(live.bell_open),
        live.unread,
        live.badge
    )
}

/// What `shows` said, read back.
#[derive(Debug, PartialEq)]
struct Shown<'a> {
    open: bool,
    unread: usize,
    badge: &'a str,
}

fn read(shown: &str) -> Shown<'_> {
    let mut fields = shown.split('\t');
    Shown {
        open: fields.next() == Some("1"),
        unread: fields.next().and_then(|n| n.parse().ok()).unwrap_or(0),
        badge: fields.next().filter(|b| !b.is_empty()).unwrap_or("dot"),
    }
}

/// What the count badge says: the number, at most `99+`.
pub fn count_text(unread: usize) -> String {
    if unread > 99 {
        "99+".into()
    } else {
        unread.to_string()
    }
}

/// The room at each side of the tile, in logical pixels.
const ROOM: f32 = 2.0;
/// The dot's side, and how far it lies out from the bell's top right.
const DOT: f32 = 7.0;
const DOT_OUT: f32 = 2.0;
/// The count's pill: its height, the room at each end of its number and
/// the number's size, in logical pixels.
const PILL: f32 = 14.0;
const PILL_PAD: f32 = 4.0;
const PILL_TEXT: f32 = 10.0;

/// The tile: as wide as a control is high.
fn tile(tokens: &Tokens) -> f32 {
    tokens.panel_control as f32
}

/// Its width in logical pixels, whatever it shows.
pub fn logical_width(tokens: &Tokens) -> f32 {
    ROOM + tile(tokens) + ROOM
}

fn width(canvas: &mut Canvas, _: &str) -> f32 {
    logical_width(canvas.tokens) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let shown = read(shown);
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let side = (tile(tokens) * s).round();
    let tx = (x + ROOM * s).round();
    let ty = canvas.top + ((canvas.height - side) / 2.0).round();
    let ink = if shown.open {
        let lit = Colour {
            a: 0.085,
            ..tokens.panel_text
        };
        let r = tokens.radius_control as f32 * s;
        fill(canvas.pixmap, tx, ty, side, side, r, lit);
        tokens.accent
    } else {
        tokens.panel_text
    };
    let px = (tokens.panel_glyph as f32 * s).round();
    let (ix, iy) = (
        tx + ((side - px) / 2.0).round(),
        ty + ((side - px) / 2.0).round(),
    );
    paint::icon(canvas.pixmap, "bell", px, ix, iy, ink);
    if shown.unread == 0 {
        return;
    }
    match shown.badge {
        "count" => {
            let words = count_text(shown.unread);
            let (h, size) = ((PILL * s).round(), PILL_TEXT * s);
            let Some(text) = canvas.text.as_deref_mut() else {
                return;
            };
            let mut line = text.line_in(&words, size, Face::SEMIBOLD.tabular());
            let w = (line.width + 2.0 * PILL_PAD * s).max(h).round();
            let (px, py) = (tx + side - w + (DOT_OUT * s).round(), ty);
            fill(canvas.pixmap, px, py, w, h, h / 2.0, tokens.accent);
            let (lx, ly) = (
                px + ((w - line.width) / 2.0).round(),
                py + ((h - size * 1.25) / 2.0).round(),
            );
            text.draw(canvas.pixmap, &mut line, lx, ly, tokens.accent_text);
        }
        "none" => {}
        _ => {
            // A ring of the panel's colour sets the dot off the bell.
            let d = (DOT * s).round();
            let ring = (1.5 * s).round();
            let (dx, dy) = (
                ix + px + (DOT_OUT * s).round() - d,
                iy - (DOT_OUT * s).round(),
            );
            fill(
                canvas.pixmap,
                dx - ring,
                dy - ring,
                d + 2.0 * ring,
                d + 2.0 * ring,
                (d + 2.0 * ring) / 2.0,
                tokens.panel,
            );
            fill(canvas.pixmap, dx, dy, d, d, d / 2.0, tokens.accent);
        }
    }
}

/// A click opens the notifications without the calendar, or closes them.
fn input(input: Input) -> Option<Action> {
    matches!(input, Input::Click(..)).then_some(Action::Notifications)
}

/// `Notifications, 3 unread`, or `Notifications`.
fn label(shown: &str) -> String {
    let shown = read(shown);
    if shown.unread == 0 {
        return tr("Notifications").into();
    }
    trf(
        "Notifications, {count} unread",
        &[("count", &shown.unread.to_string())],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_shows_whether_it_is_open_how_many_are_unread_and_the_badge() {
        let mut live = Live::default();
        assert_eq!(
            read(&shows(&live)),
            Shown {
                open: false,
                unread: 0,
                badge: "dot"
            }
        );
        live.bell_open = true;
        live.unread = 3;
        live.badge = "count".into();
        assert_eq!(shows(&live), "1\t3\tcount");
        assert_eq!(
            read(&shows(&live)),
            Shown {
                open: true,
                unread: 3,
                badge: "count"
            }
        );
    }

    #[test]
    fn a_screen_reader_hears_how_many_are_unread() {
        assert_eq!(label("0\t0\tdot"), "Notifications");
        assert_eq!(label("0\t3\tdot"), "Notifications, 3 unread");
        assert_eq!(label("0\t1\tnone"), "Notifications, 1 unread");
    }

    #[test]
    fn the_count_stops_at_ninety_nine_and_a_click_opens_the_list() {
        assert_eq!(count_text(7), "7");
        assert_eq!(count_text(99), "99");
        assert_eq!(count_text(140), "99+");
        assert_eq!(input(Input::Click(10.0, 34.0)), Some(Action::Notifications));
        assert_eq!(input(Input::Scroll(1)), None);
    }

    #[test]
    fn it_is_a_square_tile_with_room_at_each_side() {
        let tokens = Tokens::built_in();
        assert_eq!(logical_width(&tokens), 34.0);
    }

    #[test]
    fn the_dot_is_the_accent_at_the_bells_top_right_and_nothing_with_none() {
        let tokens = Tokens::built_in();
        let mut pixmap = tiny_skia::Pixmap::new(40, 40).unwrap();
        let mut canvas = Canvas {
            pixmap: &mut pixmap,
            tokens: &tokens,
            text: None,
            icons: None,
            scale: 1.0,
            top: 0.0,
            height: 40.0,
            dock: false,
            along_top: false,
        };
        draw(&mut canvas, "0\t2\tdot", 0.0);
        // The tile is 30 px at 2,5; the bell 18 px at 8,11; the dot 7 px
        // with its top right corner 2 px out from the bell's.
        let at = pixmap.pixel(26, 12).unwrap();
        let accent = tokens.accent;
        assert_eq!(
            (at.red(), at.green(), at.blue()),
            (
                (accent.r * 255.0).round() as u8,
                (accent.g * 255.0).round() as u8,
                (accent.b * 255.0).round() as u8
            ),
            "{at:?}"
        );
        let mut pixmap = tiny_skia::Pixmap::new(40, 40).unwrap();
        let mut canvas = Canvas {
            pixmap: &mut pixmap,
            tokens: &tokens,
            text: None,
            icons: None,
            scale: 1.0,
            top: 0.0,
            height: 40.0,
            dock: false,
            along_top: false,
        };
        draw(&mut canvas, "0\t2\tnone", 0.0);
        assert_eq!(pixmap.pixel(26, 12).unwrap().alpha(), 0);
    }
}
