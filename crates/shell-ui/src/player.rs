//! The player (M5.9d, M5.9i; its own module since M5.10c): what plays, as
//! quick settings shows it above or below its card and the lock screen in
//! its corner, both calling this one module at their own sizes (ADR-010's
//! one copy of everything). A pill `HEIGHT` high, its corners half its
//! height, the app's icon and name centred at its top, then the cover, the
//! title over the artist, and previous, play or pause and next, with a dot
//! for each player under them when two or more play
//! (`docs/mockups/shell/player.jpg`). Plain data and drawing, tested
//! without a display: [`place`] lays out a card, [`hit`] finds a part,
//! [`paint`] draws it. What plays comes from `mpris.rs`.

use edel::tokens::{Colour, Tokens};
use tiny_skia::Pixmap;

use crate::paint::{self, Face, Text, fill, mix};
use crate::popup::{Rect, dim, focus_ring, icon_in, veil};

/// The card's height, logical pixels. It is a pill, its corners half its
/// height, with its app's name at its top and room for the pages' dots at
/// its foot (M5.9i).
pub const HEIGHT: f32 = 120.0;
/// The cover's side, and the room between the player's parts.
const COVER: f32 = 64.0;
const SPACE: f32 = 10.0;
/// The room between the pill's ends and its cover or Next button (M5.9i).
const SIDE: f32 = 28.0;
/// The app's row (its icon and name), its top and height, from the card's
/// top; the icon's side; and the middle of the cover's row (M5.9i).
const APP_TOP: f32 = 12.0;
const APP_HIGH: f32 = 16.0;
pub const APP_ICON: f32 = 14.0;
const ROW_MIDDLE: f32 = 64.0;
/// The pages' dots: a dot's side, the shown one's width, the room between
/// them and from the card's foot to theirs (M5.9i).
const DOT: f32 = 5.0;
const DOT_ON: f32 = 12.0;
const DOT_GAP: f32 = 5.0;
const DOT_FOOT: f32 = 9.0;
/// How far round a dot a click still finds it.
const DOT_REACH: f32 = 6.0;
/// The title and artist's box, its height.
const WORDS: f32 = 36.0;

/// What plays (M5.9d): its words, which of its buttons work and its cover,
/// decoded to a square picture when it has one.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerView {
    pub title: String,
    pub artist: String,
    /// The app's name, whose first letter is the cover when there is none.
    pub identity: String,
    pub playing: bool,
    pub can_previous: bool,
    pub can_next: bool,
    pub cover: Option<Pixmap>,
    /// Its app's icon, `APP_ICON` logical pixels across at the card's scale,
    /// when the icon themes have one (M5.9i).
    pub app_icon: Option<Pixmap>,
}

/// A part of the player a pointer or the keyboard can be on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Previous,
    PlayPause,
    Next,
    /// The dot of page `i`, with several players (M5.9i)
    Dot(usize),
}

/// Where the parts of one player's card lie, logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Parts {
    pub card: Rect,
    pub cover: Rect,
    /// The title and artist's box.
    pub words: Rect,
    pub previous: Rect,
    pub play: Rect,
    pub next: Rect,
    /// The app's row across the card, its icon and name centred in it.
    pub app: Rect,
    /// The pages' dots, the shown one wider; none for one page.
    pub dots: Vec<Rect>,
}

/// The parts of a player's card at `card`, `pages` players in all and
/// `page` the one shown. The three buttons are 44 px on a Compact screen,
/// where a finger lands.
pub fn place(card: Rect, compact: bool, pages: usize, page: usize) -> Parts {
    let middle = |side: f32| card.y + ROW_MIDDLE - side / 2.0;
    let small = if compact { 44.0 } else { 28.0 };
    let large = if compact { 44.0 } else { 32.0 };
    let cover = Rect::new(card.x + SIDE, middle(COVER), COVER, COVER);
    let next = Rect::new(card.right() - SIDE - small, middle(small), small, small);
    let play = Rect::new(next.x - SPACE - large, middle(large), large, large);
    let previous = Rect::new(play.x - SPACE - small, middle(small), small, small);
    let left = cover.right() + SPACE;
    Parts {
        card,
        cover,
        words: Rect::new(
            left,
            middle(WORDS),
            (previous.x - SPACE - left).max(0.0),
            WORDS,
        ),
        previous,
        play,
        next,
        app: Rect::new(card.x, card.y + APP_TOP, card.w, APP_HIGH),
        dots: dot_rects(pages, page, card, card.y + HEIGHT - DOT_FOOT - DOT),
    }
}

/// The pages' dots in a row centred across `card`, their tops at `top`:
/// none for fewer than two pages; the shown one `DOT_ON` wide (M5.9i).
fn dot_rects(pages: usize, page: usize, card: Rect, top: f32) -> Vec<Rect> {
    if pages < 2 {
        return Vec::new();
    }
    let widths: Vec<f32> = (0..pages)
        .map(|i| if i == page { DOT_ON } else { DOT })
        .collect();
    let all = widths.iter().sum::<f32>() + (pages - 1) as f32 * DOT_GAP;
    let mut x = card.x + (card.w - all) / 2.0;
    widths
        .into_iter()
        .map(|dw| {
            let r = Rect::new(x, top, dw, DOT);
            x += dw + DOT_GAP;
            r
        })
        .collect()
}

/// The part at `x`, `y`: Previous and Next only where the player can do
/// them; a dot, or the room round it, finds its page, and between two
/// dots the nearer.
pub fn hit(p: &PlayerView, parts: &Parts, x: f32, y: f32) -> Option<Part> {
    if p.can_previous && parts.previous.contains(x, y) {
        return Some(Part::Previous);
    }
    if parts.play.contains(x, y) {
        return Some(Part::PlayPause);
    }
    if p.can_next && parts.next.contains(x, y) {
        return Some(Part::Next);
    }
    parts
        .dots
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            y >= d.y - DOT_REACH
                && y <= d.y + d.h + DOT_REACH
                && x >= d.x - DOT_REACH
                && x <= d.right() + DOT_REACH
        })
        .min_by(|a, b| {
            let off = |d: &Rect| (x - (d.x + d.w / 2.0)).abs();
            off(a.1).total_cmp(&off(b.1))
        })
        .map(|(i, _)| Part::Dot(i))
}

/// Where the pointer and the keyboard are, and the page shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    pub hover: Option<Part>,
    pub focus: Option<Part>,
    pub page: usize,
}

/// Draws the player's parts (not its card, which the caller draws as its
/// own cards are): its cover, the title over the artist, and the previous,
/// play or pause and next buttons, with the keyboard's ring and the hover
/// light where they are. A cover is its picture in a circle, else a circle
/// of the accent going darker, with the app's first letter. Without
/// `text`, everything but the words.
pub fn paint(
    pixmap: &mut Pixmap,
    p: &PlayerView,
    parts: &Parts,
    marks: Marks,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    s: f32,
) {
    let cover = parts.cover;
    let (x, y, w, h) = cover.device(s);
    match &p.cover {
        Some(picture) => paint::circle_picture(pixmap, picture, (x, y, w, h)),
        None => {
            let bottom = mix(tokens.accent, tokens.panel_text, 0.45);
            paint::fill_vertical(pixmap, (x, y, w, h), w / 2.0, tokens.accent, bottom);
            if let Some(text) = text.as_deref_mut() {
                let initial = p
                    .identity
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default();
                let mut letter = text.fit_in(&initial, 22.0 * s, cover.w * s, Face::SEMIBOLD);
                let left = x + ((w - letter.width) / 2.0).round();
                let top = y + (h - 22.0 * s * 1.25) / 2.0;
                text.draw(pixmap, &mut letter, left, top, tokens.accent_text);
            }
        }
    }
    // The words: the title over the artist, from the box's left, beside
    // the cover, as the mockups set them.
    if let Some(text) = text.as_deref_mut() {
        let room = parts.words.w * s;
        let (wx, wy, _, wh) = parts.words.device(s);
        let mut title = text.fit_in(&p.title, 12.5 * s, room, Face::SEMIBOLD);
        let title_h = 12.5 * s * 1.25;
        if p.artist.is_empty() {
            let top = wy + (wh - title_h) / 2.0;
            text.draw(pixmap, &mut title, wx, top, tokens.panel_text);
        } else {
            let mut artist = text.fit(&p.artist, 11.0 * s, room);
            let block = title_h + 2.0 * s + 11.0 * s * 1.25;
            let top = wy + (wh - block) / 2.0;
            text.draw(pixmap, &mut title, wx, top, tokens.panel_text);
            text.draw(
                pixmap,
                &mut artist,
                wx,
                top + title_h + 2.0 * s,
                dim(tokens),
            );
        }
    }
    // The buttons: Previous and Next are dimmed where the player cannot
    // do them; Play or Pause is a filled circle with the glyph in the card.
    let sides = [
        (
            Part::Previous,
            "media-previous",
            parts.previous,
            p.can_previous,
        ),
        (Part::Next, "media-next", parts.next, p.can_next),
    ];
    for (part, glyph, rect, able) in sides {
        if marks.hover == Some(part) && able {
            let (x, y, w, h) = rect.device(s);
            fill(pixmap, x, y, w, h, w / 2.0, veil(tokens, 0.06));
        }
        let ink = if able {
            tokens.panel_text
        } else {
            Colour {
                a: 0.5,
                ..dim(tokens)
            }
        };
        icon_in(pixmap, glyph, 14.0, rect, s, ink);
        if marks.focus == Some(part) {
            focus_ring(pixmap, rect, rect.h / 2.0, s, tokens);
        }
    }
    let rect = parts.play;
    let (x, y, w, h) = rect.device(s);
    fill(pixmap, x, y, w, h, w / 2.0, tokens.panel_text);
    if marks.hover == Some(Part::PlayPause) {
        fill(pixmap, x, y, w, h, w / 2.0, veil(tokens, 0.12));
    }
    let glyph = if p.playing {
        "media-pause"
    } else {
        "media-play"
    };
    icon_in(pixmap, glyph, 14.0, rect, s, tokens.panel);
    if marks.focus == Some(Part::PlayPause) {
        focus_ring(pixmap, rect, rect.h / 2.0, s, tokens);
    }
    app_row(pixmap, p, parts, tokens, text, s);
    for (i, dot) in parts.dots.iter().enumerate() {
        let (x, y, w, h) = dot.device(s);
        let colour = if i == marks.page {
            tokens.accent
        } else {
            Colour {
                a: if marks.hover == Some(Part::Dot(i)) {
                    0.45
                } else {
                    0.25
                },
                ..tokens.panel_text
            }
        };
        fill(pixmap, x, y, w, h, h / 2.0, colour);
        if marks.focus == Some(Part::Dot(i)) {
            focus_ring(pixmap, *dot, dot.h / 2.0, s, tokens);
        }
    }
}

/// The app's row at the player's top (M5.9i): its icon, when there is
/// one, then its name, small and muted, centred together across the card.
fn app_row(
    pixmap: &mut Pixmap,
    p: &PlayerView,
    parts: &Parts,
    tokens: &Tokens,
    text: Option<&mut Text>,
    s: f32,
) {
    let Some(text) = text else {
        return;
    };
    let (rx, ry, rw, rh) = parts.app.device(s);
    let size = 11.0 * s;
    let icon = p.app_icon.as_ref();
    let side = (APP_ICON * s).round();
    let space = 5.0 * s;
    let lead = icon.map_or(0.0, |_| side + space);
    let mut name = text.fit(&p.identity, size, (rw - 2.0 * SIDE * s - lead).max(0.0));
    let left = (rx + (rw - lead - name.width) / 2.0).round();
    if let Some(icon) = icon {
        let top = (ry + (rh - side) / 2.0).round();
        pixmap.draw_pixmap(
            left as i32,
            top as i32,
            icon.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
    }
    let top = ry + (rh - size * 1.25) / 2.0;
    text.draw(pixmap, &mut name, left + lead, top, dim(tokens));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(can: bool) -> PlayerView {
        PlayerView {
            title: "Song".into(),
            artist: "Band".into(),
            identity: "Music".into(),
            playing: true,
            can_previous: can,
            can_next: can,
            cover: None,
            app_icon: None,
        }
    }

    #[test]
    fn a_card_anywhere_lays_its_parts_out_from_its_own_corner() {
        let at_zero = place(Rect::new(0.0, 0.0, 352.0, HEIGHT), false, 1, 0);
        let moved = place(Rect::new(40.0, 500.0, 352.0, HEIGHT), false, 1, 0);
        let shift = |r: Rect| Rect::new(r.x + 40.0, r.y + 500.0, r.w, r.h);
        assert_eq!(moved.cover, shift(at_zero.cover));
        assert_eq!(moved.words, shift(at_zero.words));
        assert_eq!(moved.next, shift(at_zero.next));
        assert_eq!(moved.app, shift(at_zero.app));
        assert_eq!(at_zero.next.right(), 352.0 - SIDE);
        assert!(at_zero.dots.is_empty());
    }

    #[test]
    fn dots_are_centred_on_the_card_wherever_it_lies() {
        let card = Rect::new(100.0, 0.0, 300.0, HEIGHT);
        let parts = place(card, false, 3, 1);
        assert_eq!(parts.dots.len(), 3);
        assert_eq!(parts.dots[1].w, DOT_ON);
        let left = parts.dots[0].x - card.x;
        let right = card.right() - parts.dots[2].right();
        assert!((left - right).abs() < 0.01, "{left} {right}");
    }

    #[test]
    fn previous_and_next_are_found_only_where_the_player_can_do_them() {
        let parts = place(Rect::new(0.0, 0.0, 352.0, HEIGHT), false, 2, 0);
        let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (px, py) = mid(parts.previous);
        assert_eq!(hit(&playing(true), &parts, px, py), Some(Part::Previous));
        assert_eq!(hit(&playing(false), &parts, px, py), None);
        let (x, y) = mid(parts.play);
        assert_eq!(hit(&playing(false), &parts, x, y), Some(Part::PlayPause));
        let (x, y) = mid(parts.dots[1]);
        assert_eq!(hit(&playing(true), &parts, x, y), Some(Part::Dot(1)));
    }
}
