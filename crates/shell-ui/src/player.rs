//! The player (M5.9d, M5.9i, M5.10c): what plays, as quick settings shows it
//! above or below its card and the lock screen in its corner, both calling
//! this one module at their own sizes (ADR-010's one copy of everything).
//! Two shapes ([`Shape`]): the card, a pill `HEIGHT` high with its app's
//! icon and name centred at its top, then the cover, the title over the
//! artist, and previous, play or pause and next, with a dot for each player
//! under them when two or more play (`docs/mockups/shell/player.jpg`); and
//! the lock screen's wide pill `WIDE` across (M5.10c), its cover at the
//! left, the app's name, the title over the artist and the track's bar with
//! the time played and the length, and the buttons at its right
//! (`docs/mockups/shell/lock-media.jpg`). The cover, the words, the buttons,
//! the app's row and the dots are drawn by the same functions for both.
//! Plain data and drawing, tested without a display: [`place`] and
//! [`place_wide`] lay out a card, [`hit`] finds a part, [`paint`] draws it.
//! What plays comes from `mpris.rs`.

use std::time::Duration;

use edel::tokens::{Colour, Tokens};
use tiny_skia::Pixmap;

use crate::paint::{self, Face, Text, fill, mix};
use crate::popup::{Rect, dim, focus_ring, icon_in};

/// The card's height, logical pixels. It is a pill, its corners half its
/// height, with its app's name at its top and room for the pages' dots at
/// its foot (M5.9i).
pub const HEIGHT: f32 = 120.0;
/// The wide pill's size, logical pixels (M5.10c): the lock screen's player,
/// at its bottom left.
/// The wide pill's buttons' glyphs, logical pixels (M5.10c).
const WIDE_GLYPH: f32 = 22.0;
/// Play's or Pause's glyph on the wide pill's 48 px disc.
const WIDE_PLAY_GLYPH: f32 = 18.0;
pub const WIDE: (f32, f32) = (400.0, 100.0);
/// The row the wide pill grows by under its track with two or more players,
/// so its dots lie clear of the times, `DOT_FOOT` from its foot as quick
/// settings' do.
const WIDE_PAGES_ROW: f32 = 16.0;

/// The wide pill's size with `pages` players: `WIDE`, and one row taller
/// for the dots with two or more.
pub fn wide_size(pages: usize) -> (f32, f32) {
    let row = if pages > 1 { WIDE_PAGES_ROW } else { 0.0 };
    (WIDE.0, WIDE.1 + row)
}
/// The cover's side, and the room between the player's parts.
const COVER: f32 = 64.0;
const SPACE: f32 = 10.0;
/// The room between the pill's ends and its cover or Next button (M5.9i).
const SIDE: f32 = 28.0;
/// The wide pill's cover side, and its room from the left end (M5.10c).
const WIDE_COVER: f32 = 76.0;
const WIDE_EDGE: f32 = 12.0;
/// The room between the wide pill's cover and its words (M5.10c).
const WIDE_GAP: f32 = 16.0;
/// The wide pill's words box, from the card's top (M5.10c).
const WIDE_WORDS_HIGH: f32 = 40.0;
/// The wide pill's progress bar, its height and its length (M5.10c). On a
/// Compact screen the bar stops where the words stop.
const WIDE_BAR_HIGH: f32 = 4.0;
const WIDE_BAR: f32 = 136.0;
/// The wide pill's left column, top to bottom (M5.10c4): the app's row,
/// `WIDE_ROW_GAP`, the title over the artist, `WIDE_BAR_GAP` (the mockup's
/// room over the bar), the bar, 2 px, the times; one block centred on the
/// card, and kept `WIDE_ROW_GAP` above the dots when there are dots.
const WIDE_ROW_GAP: f32 = 4.0;
const WIDE_BAR_GAP: f32 = 8.0;
const WIDE_COLUMN: f32 =
    APP_HIGH + WIDE_ROW_GAP + WIDE_WORDS_HIGH + WIDE_BAR_GAP + WIDE_BAR_HIGH + 2.0 + WIDE_TIME_HIGH;
/// The track's times, their height and their width (M5.10c): the played
/// time at the bar's left end, the length at its right.
const WIDE_TIME_HIGH: f32 = 14.0;
const WIDE_TIME_WIDE: f32 = 40.0;
/// The times' text, logical pixels (M5.10c).
const WIDE_TIME_SIZE: f32 = 11.0;
/// The wide pill's buttons: the centres of Previous, Play and Next, from
/// the card's left, and the sides of Previous and Next and of Play (M5.10c).
/// The centres are the mockup's. Play's centre is 316, not the 340 the
/// first plan had: at 340 its 48 px would reach 364 and Next could not fit
/// inside the card beside it.
const WIDE_PREVIOUS_X: f32 = 266.0;
const WIDE_PLAY_X: f32 = 316.0;
const WIDE_NEXT_X: f32 = 366.0;
const WIDE_SMALL: f32 = 28.0;
const WIDE_LARGE: f32 = 48.0;
/// The side of every button on a Compact screen, where a finger lands
/// (M5.10c): the centres stay where they are, so the gaps shrink.
const TOUCH: f32 = 44.0;
/// The app's row (its icon and name), its top and height, from the card's
/// top; the icon's side; and the middle of the cover's row (M5.9i).
const APP_TOP: f32 = 12.0;
const APP_HIGH: f32 = 16.0;
pub const APP_ICON: f32 = 14.0;
const ROW_MIDDLE: f32 = 64.0;
/// The cover's initial, in logical pixels on the card's 64 px cover; it
/// grows with a bigger cover (M5.10c).
const LETTER: f32 = 22.0;
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
    /// The time played now, when the app says (M5.10c). The wide pill draws
    /// it at the bar's left end.
    pub position: Option<Duration>,
    /// The track's length, when the app says (M5.10c). The wide pill draws
    /// it at the bar's right end, and the bar's played part needs it.
    pub length: Option<Duration>,
}

/// Which of the two shapes a player is drawn in (M5.10c).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Quick settings' card, laid out by [`place`].
    Card,
    /// The lock screen's wide pill, laid out by [`place_wide`].
    Wide,
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
    /// The shape these parts are laid out in (M5.10c).
    pub shape: Shape,
    pub cover: Rect,
    /// The title and artist's box.
    pub words: Rect,
    pub previous: Rect,
    pub play: Rect,
    pub next: Rect,
    /// The app's row: across the card, its icon and name centred in it; in
    /// the wide pill, from the words' left (M5.10c).
    pub app: Rect,
    /// The pages' dots, the shown one wider; none for one page.
    pub dots: Vec<Rect>,
    /// The track's progress bar, its whole length; Wide only (M5.10c).
    pub progress: Option<Rect>,
    /// The time played, at the bar's left end, and the length, at its right
    /// end; Wide only (M5.10c).
    pub times: Option<(Rect, Rect)>,
}

/// The colours a player is drawn in: a card's on the panel's colour
/// (quick settings), the wide pill's on the lock screen's glass over its
/// dark backdrop, in both schemes (M5.10c4, `docs/mockups/shell/
/// lock-media.jpg`: white, its muted words at 68 percent, the track at 22).
#[derive(Clone, Copy)]
struct Inks {
    text: Colour,
    muted: Colour,
    track: Colour,
    /// Play's disc and its glyph.
    disc: Colour,
    on_disc: Colour,
}

impl Inks {
    fn of(shape: Shape, tokens: &Tokens) -> Inks {
        match shape {
            Shape::Card => Inks {
                text: tokens.panel_text,
                muted: dim(tokens),
                track: mix(tokens.panel_text, tokens.panel, 0.75),
                disc: tokens.panel_text,
                on_disc: tokens.panel,
            },
            Shape::Wide => {
                let white = tokens.backdrop_text;
                Inks {
                    text: white,
                    muted: Colour { a: 0.68, ..white },
                    track: Colour { a: 0.22, ..white },
                    disc: white,
                    on_disc: tokens.lock_backdrop_deep,
                }
            }
        }
    }

    /// `alpha` of the text, for hover lights and faint dots.
    fn veil(self, alpha: f32) -> Colour {
        Colour {
            a: alpha,
            ..self.text
        }
    }
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
        shape: Shape::Card,
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
        progress: None,
        times: None,
    }
}

/// The parts of the wide pill at `card` (M5.10c), `pages` players in all
/// and `page` the one shown: the cover at the left, the app's row and the
/// words beside it, the track's bar and times under them, and Previous,
/// Play and Next at the right, all centred on the card's middle. On a
/// Compact screen the buttons are 44 px and keep their centres.
pub fn place_wide(card: Rect, compact: bool, pages: usize, page: usize) -> Parts {
    let centred = |cx: f32, side: f32| {
        Rect::new(
            card.x + cx - side / 2.0,
            card.y + card.h / 2.0 - side / 2.0,
            side,
            side,
        )
    };
    let small = if compact { TOUCH } else { WIDE_SMALL };
    let large = if compact { TOUCH } else { WIDE_LARGE };
    let previous = centred(WIDE_PREVIOUS_X, small);
    let play = centred(WIDE_PLAY_X, large);
    let next = centred(WIDE_NEXT_X, small);
    let cover = Rect::new(
        card.x + WIDE_EDGE,
        card.y + (card.h - WIDE_COVER) / 2.0,
        WIDE_COVER,
        WIDE_COVER,
    );
    let left = cover.right() + WIDE_GAP;
    let dots_top = card.y + card.h - DOT_FOOT - DOT;
    let mut top = card.y + (card.h - WIDE_COLUMN) / 2.0;
    if pages > 1 {
        top = top.min(dots_top - WIDE_ROW_GAP - WIDE_COLUMN);
    }
    let app = Rect::new(left, top, (previous.x - SPACE - left).max(0.0), APP_HIGH);
    let words = Rect::new(
        left,
        app.y + APP_HIGH + WIDE_ROW_GAP,
        app.w,
        WIDE_WORDS_HIGH,
    );
    let bar = Rect::new(
        words.x,
        words.y + WIDE_WORDS_HIGH + WIDE_BAR_GAP,
        WIDE_BAR.min(words.w),
        WIDE_BAR_HIGH,
    );
    let time_top = bar.y + WIDE_BAR_HIGH + 2.0;
    let times = (
        Rect::new(bar.x, time_top, WIDE_TIME_WIDE, WIDE_TIME_HIGH),
        Rect::new(
            bar.right() - WIDE_TIME_WIDE,
            time_top,
            WIDE_TIME_WIDE,
            WIDE_TIME_HIGH,
        ),
    );
    Parts {
        card,
        shape: Shape::Wide,
        cover,
        words,
        previous,
        play,
        next,
        app,
        // Centred across the card at its foot, in the row `wide_size`
        // adds under the track, as quick settings' are.
        dots: dot_rects(pages, page, card, dots_top),
        progress: Some(bar),
        times: Some(times),
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

/// A track's time as the player shows it (M5.10c): `m:ss`, and `h:mm:ss`
/// from an hour, so 84 s reads "1:24" and 3725 s "1:02:05".
pub fn clock_time(d: Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Where the pointer and the keyboard are, and the page shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    pub hover: Option<Part>,
    pub focus: Option<Part>,
    pub page: usize,
}

/// Draws the player's parts (not its card, which the caller draws as its
/// own cards are): its cover, the title over the artist, the wide pill's
/// bar and times, the previous, play or pause and next buttons, and the
/// app's row and the dots, with the keyboard's ring and the hover light
/// where they are. A cover is its picture in a circle, else a circle of the
/// accent going darker, with the app's first letter. Without `text`,
/// everything but the words.
pub fn paint(
    pixmap: &mut Pixmap,
    p: &PlayerView,
    parts: &Parts,
    marks: Marks,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    s: f32,
) {
    cover(pixmap, p, parts, tokens, text.as_deref_mut(), s);
    words(pixmap, p, parts, tokens, text.as_deref_mut(), s);
    track(pixmap, p, parts, tokens, text.as_deref_mut(), s);
    buttons(pixmap, p, parts, marks, tokens, s);
    app_row(pixmap, p, parts, tokens, text, s);
    dots(pixmap, parts, marks, tokens, s);
}

/// The cover, in both shapes: its picture in a circle, else a circle of the
/// accent going darker with the app's first letter (M5.10c: the letter
/// grows with the cover).
fn cover(
    pixmap: &mut Pixmap,
    p: &PlayerView,
    parts: &Parts,
    tokens: &Tokens,
    text: Option<&mut Text>,
    s: f32,
) {
    let cover = parts.cover;
    let (x, y, w, h) = cover.device(s);
    match &p.cover {
        Some(picture) => paint::circle_picture(pixmap, picture, (x, y, w, h)),
        None => {
            let bottom = mix(tokens.accent, tokens.panel_text, 0.45);
            paint::fill_vertical(pixmap, (x, y, w, h), w / 2.0, tokens.accent, bottom);
            if let Some(text) = text {
                let letter = LETTER * cover.w / COVER * s;
                let initial = p
                    .identity
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default();
                let mut line = text.fit_in(&initial, letter, cover.w * s, Face::SEMIBOLD);
                let left = x + ((w - line.width) / 2.0).round();
                let top = y + (h - letter * 1.25) / 2.0;
                text.draw(pixmap, &mut line, left, top, tokens.accent_text);
            }
        }
    }
}

/// The title over the artist, from the words' box's left, beside the
/// cover; the sizes are the card's (12.5 and 11 px) or the wide pill's (17
/// and 13 px), as the mockups set them (M5.10c).
fn words(
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
    let ink = Inks::of(parts.shape, tokens);
    let (title_px, artist_px) = match parts.shape {
        Shape::Card => (12.5, 11.0),
        Shape::Wide => (17.0, 13.0),
    };
    let room = parts.words.w * s;
    let (wx, wy, _, wh) = parts.words.device(s);
    let mut title = text.fit_in(&p.title, title_px * s, room, Face::SEMIBOLD);
    let title_h = title_px * s * 1.25;
    if p.artist.is_empty() {
        let top = wy + (wh - title_h) / 2.0;
        text.draw(pixmap, &mut title, wx, top, ink.text);
    } else {
        let mut artist = text.fit(&p.artist, artist_px * s, room);
        let block = title_h + 2.0 * s + artist_px * s * 1.25;
        let top = wy + (wh - block) / 2.0;
        text.draw(pixmap, &mut title, wx, top, ink.text);
        text.draw(pixmap, &mut artist, wx, top + title_h + 2.0 * s, ink.muted);
    }
}

/// The track's progress (the wide pill, M5.10c): the bar's track, its played
/// part up to the share of the length the position is, and the time played
/// at its left end and the length at its right. No times without a length;
/// no played part without a position and a length.
fn track(
    pixmap: &mut Pixmap,
    p: &PlayerView,
    parts: &Parts,
    tokens: &Tokens,
    text: Option<&mut Text>,
    s: f32,
) {
    let Some(bar) = parts.progress else {
        return;
    };
    let ink = Inks::of(parts.shape, tokens);
    let (x, y, w, h) = bar.device(s);
    let radius = (h / 2.0).min(w / 2.0);
    fill(pixmap, x, y, w, h, radius, ink.track);
    if let (Some(at), Some(length)) = (p.position, p.length) {
        if !length.is_zero() {
            let share = (at.as_secs_f32() / length.as_secs_f32()).clamp(0.0, 1.0);
            let played = w * share;
            if played > 0.0 {
                let radius = radius.min(played / 2.0);
                fill(pixmap, x, y, played, h, radius, ink.text);
            }
        }
    }
    let (Some((played_box, length_box)), Some(length), Some(text)) = (parts.times, p.length, text)
    else {
        return;
    };
    if let Some(at) = p.position {
        clock_label(pixmap, text, at, played_box, false, ink, s);
    }
    clock_label(pixmap, text, length, length_box, true, ink, s);
}

/// One of the track's times, drawn in its box from the left, or from the
/// right when `right` is set, and centred vertically (M5.10c).
fn clock_label(
    pixmap: &mut Pixmap,
    text: &mut Text,
    time: Duration,
    at: Rect,
    right: bool,
    ink: Inks,
    s: f32,
) {
    let size = WIDE_TIME_SIZE * s;
    let mut line = text.line_in(&clock_time(time), size, Face::REGULAR.tabular());
    let (x, y, w, h) = at.device(s);
    let left = if right { x + w - line.width } else { x };
    let top = y + (h - size * 1.25) / 2.0;
    text.draw(pixmap, &mut line, left, top, ink.muted);
}

/// The buttons, in both shapes: Previous and Next, dimmed where the player
/// cannot do them, and Play or Pause, a filled circle with its glyph in the
/// card's colour. Each has its hover light and the keyboard's ring.
fn buttons(
    pixmap: &mut Pixmap,
    p: &PlayerView,
    parts: &Parts,
    marks: Marks,
    tokens: &Tokens,
    s: f32,
) {
    // The buttons' glyphs: larger on the wide pill, as the lock screen's
    // mockup draws them (M5.10c).
    let (glyph_px, play_px) = match parts.shape {
        Shape::Card => (14.0, 14.0),
        Shape::Wide => (WIDE_GLYPH, WIDE_PLAY_GLYPH),
    };
    let ink = Inks::of(parts.shape, tokens);
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
            fill(pixmap, x, y, w, h, w / 2.0, ink.veil(0.06));
        }
        let colour = if able {
            ink.text
        } else {
            Colour {
                a: 0.5,
                ..ink.muted
            }
        };
        icon_in(pixmap, glyph, glyph_px, rect, s, colour);
        if marks.focus == Some(part) {
            focus_ring(pixmap, rect, rect.h / 2.0, s, tokens);
        }
    }
    let rect = parts.play;
    let (x, y, w, h) = rect.device(s);
    fill(pixmap, x, y, w, h, w / 2.0, ink.disc);
    if marks.hover == Some(Part::PlayPause) {
        fill(
            pixmap,
            x,
            y,
            w,
            h,
            w / 2.0,
            Colour {
                a: 0.12,
                ..ink.on_disc
            },
        );
    }
    let glyph = if p.playing {
        "media-pause"
    } else {
        "media-play"
    };
    icon_in(pixmap, glyph, play_px, rect, s, ink.on_disc);
    if marks.focus == Some(Part::PlayPause) {
        focus_ring(pixmap, rect, rect.h / 2.0, s, tokens);
    }
}

/// The pages' dots (M5.9i): the shown one in the accent and wider, the
/// others faint, each with its hover light and the keyboard's ring.
fn dots(pixmap: &mut Pixmap, parts: &Parts, marks: Marks, tokens: &Tokens, s: f32) {
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
                ..Inks::of(parts.shape, tokens).text
            }
        };
        fill(pixmap, x, y, w, h, h / 2.0, colour);
        if marks.focus == Some(Part::Dot(i)) {
            focus_ring(pixmap, *dot, dot.h / 2.0, s, tokens);
        }
    }
}

/// The app's row at the player's top (M5.9i): its icon, when there is
/// one, then its name, small and muted, centred together across the card;
/// in the wide pill, the icon and name start at the words' left (M5.10c).
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
    let lead = side + space;
    let room = match parts.shape {
        Shape::Card => rw - 2.0 * SIDE * s - lead,
        Shape::Wide => rw - lead,
    };
    let mut name = text.fit(&p.identity, size, room.max(0.0));
    let left = match parts.shape {
        Shape::Card => (rx + (rw - lead - name.width) / 2.0).round(),
        Shape::Wide => rx.round(),
    };
    let top = (ry + (rh - side) / 2.0).round();
    match icon {
        // Drawn at the side the card asks for, whatever size it was read at.
        Some(icon) => {
            let k = side / icon.width().max(1) as f32;
            pixmap.draw_pixmap(
                0,
                0,
                icon.as_ref(),
                &tiny_skia::PixmapPaint {
                    quality: tiny_skia::FilterQuality::Bilinear,
                    ..tiny_skia::PixmapPaint::default()
                },
                tiny_skia::Transform::from_row(k, 0.0, 0.0, k, left, top),
                None,
            );
        }
        // No icon: the app's initial on an accent tile, as the panel's
        // apps widget draws one.
        None => {
            fill(pixmap, left, top, side, side, side * 0.3, tokens.accent);
            let initial = p
                .identity
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default();
            let mut letter = text.fit_in(&initial, side * 0.64, side, Face::SEMIBOLD);
            let lx = left + (side - letter.width) / 2.0;
            let ly = top + (side - side * 0.64 * 1.25) / 2.0;
            text.draw(pixmap, &mut letter, lx, ly, tokens.accent_text);
        }
    }
    let top = ry + (rh - size * 1.25) / 2.0;
    text.draw(
        pixmap,
        &mut name,
        left + lead,
        top,
        Inks::of(parts.shape, tokens).muted,
    );
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
            position: None,
            length: None,
        }
    }

    /// The wide pill as the lock screen puts it, at (24, 676).
    fn wide_card() -> Rect {
        Rect::new(24.0, 676.0, WIDE.0, WIDE.1)
    }

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.y + b.h && b.y < a.y + a.h
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
        assert_eq!(at_zero.shape, Shape::Card);
        assert!(at_zero.progress.is_none() && at_zero.times.is_none());
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

    #[test]
    fn the_wide_pill_lays_its_parts_inside_its_card_and_apart() {
        let card = wide_card();
        let inside = |r: &Rect| {
            r.x >= card.x
                && r.y >= card.y
                && r.right() <= card.right()
                && r.y + r.h <= card.y + card.h
        };
        for compact in [false, true] {
            let parts = place_wide(card, compact, 1, 0);
            assert_eq!(parts.shape, Shape::Wide);
            let bar = parts.progress.expect("the wide pill has a bar");
            let (played, length) = parts.times.expect("the wide pill has times");
            for r in [
                parts.cover,
                parts.words,
                parts.previous,
                parts.play,
                parts.next,
                parts.app,
                bar,
                played,
                length,
            ] {
                assert!(inside(&r), "{r:?} leaves the card, compact {compact}");
            }
            let buttons = [parts.previous, parts.play, parts.next];
            for (i, a) in buttons.iter().enumerate() {
                for b in &buttons[i + 1..] {
                    assert!(!overlap(a, b), "{a:?} and {b:?} overlap, compact {compact}");
                }
                assert!(!overlap(a, &parts.words), "the words meet a button");
                assert!(!overlap(a, &bar), "the bar meets a button");
            }
            assert!(!overlap(&parts.cover, &parts.words));
            assert!(!overlap(&played, &length));
        }
    }

    #[test]
    fn the_wide_pill_places_its_cover_play_and_bar_as_the_mockup_does() {
        let parts = place_wide(wide_card(), false, 1, 0);
        assert_eq!(parts.cover.x, 36.0);
        assert_eq!(parts.cover.w, WIDE_COVER);
        assert_eq!(parts.play.x + parts.play.w / 2.0, 24.0 + WIDE_PLAY_X);
        assert_eq!(parts.progress.expect("a bar").w, 136.0);
        assert_eq!(parts.words.x, parts.app.x);
        assert!(parts.dots.is_empty());
    }

    #[test]
    fn the_compact_buttons_are_touch_sized_and_keep_their_centres() {
        let card = Rect::new(0.0, 0.0, 352.0, HEIGHT);
        let card_parts = place(card, true, 1, 0);
        for r in [card_parts.previous, card_parts.play, card_parts.next] {
            assert!(
                r.w >= 44.0 && r.h >= 44.0 && card.contains(r.x, r.y),
                "{r:?}"
            );
        }
        let parts = place_wide(wide_card(), true, 1, 0);
        for r in [parts.previous, parts.play, parts.next] {
            assert_eq!(r.w, TOUCH);
            assert_eq!(r.h, TOUCH);
        }
        assert_eq!(
            parts.next.x + parts.next.w / 2.0,
            24.0 + WIDE_NEXT_X,
            "the centres stay where the wide pill has them"
        );
    }

    #[test]
    fn the_wide_pills_dots_lie_in_their_own_row_clear_of_the_times() {
        assert_eq!(wide_size(1), WIDE);
        let (w, h) = wide_size(3);
        let card = Rect::new(24.0, 600.0, w, h);
        let parts = place_wide(card, false, 3, 1);
        let (played, length) = parts.times.expect("the times");
        let bar = parts.progress.expect("the bar");
        for dot in &parts.dots {
            for other in [played, length, bar, parts.play, parts.cover] {
                assert!(
                    dot.y >= other.y + other.h + 4.0,
                    "{dot:?} is close to {other:?}"
                );
            }
            assert_eq!(dot.y + dot.h, card.y + card.h - DOT_FOOT);
        }
        // The app's row, the words and the bar each lie under the one before.
        for (one, next) in [(parts.app, parts.words), (parts.words, bar), (bar, played)] {
            assert!(one.y + one.h <= next.y, "{one:?} meets {next:?}");
        }
        // The cover and the buttons stay centred on the taller card.
        let middle = card.y + card.h / 2.0;
        for part in [parts.cover, parts.previous, parts.play, parts.next] {
            assert_eq!(part.y + part.h / 2.0, middle, "{part:?}");
        }
    }

    #[test]
    fn clock_time_reads_minutes_and_seconds_then_hours() {
        let at = |secs: u64| clock_time(Duration::from_secs(secs));
        assert_eq!(at(0), "0:00");
        assert_eq!(at(59), "0:59");
        assert_eq!(at(84), "1:24");
        assert_eq!(at(3600), "1:00:00");
        assert_eq!(at(3725), "1:02:05");
    }

    #[test]
    fn hit_finds_the_wide_pills_buttons() {
        let parts = place_wide(wide_card(), false, 1, 0);
        let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (x, y) = mid(parts.play);
        assert_eq!(hit(&playing(true), &parts, x, y), Some(Part::PlayPause));
        let (x, y) = mid(parts.previous);
        assert_eq!(hit(&playing(true), &parts, x, y), Some(Part::Previous));
        assert_eq!(hit(&playing(false), &parts, x, y), None);
        let (x, y) = mid(parts.next);
        assert_eq!(hit(&playing(true), &parts, x, y), Some(Part::Next));
    }

    #[test]
    fn the_wide_pill_draws_its_played_part_lighter_than_its_track() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let card = wide_card();
        let parts = place_wide(card, false, 1, 0);
        let view = PlayerView {
            title: "Night Drive".into(),
            artist: "Lumen".into(),
            identity: "Music".into(),
            position: Some(Duration::from_secs(84)),
            length: Some(Duration::from_secs(221)),
            ..playing(true)
        };
        let mut pixmap = Pixmap::new(1280, 800).unwrap();
        fill(
            &mut pixmap,
            0.0,
            0.0,
            1280.0,
            800.0,
            0.0,
            tokens.lock_backdrop_deep,
        );
        paint(
            &mut pixmap,
            &view,
            &parts,
            Marks::default(),
            &tokens,
            Some(&mut text),
            1.0,
        );
        let bar = parts.progress.expect("a bar");
        let level = |px: f32, py: f32| {
            let c = pixmap.pixel(px as u32, py as u32).expect("on the pixmap");
            u32::from(c.red()) + u32::from(c.green()) + u32::from(c.blue())
        };
        let mid = bar.y + bar.h / 2.0;
        let played = level(bar.x + 4.0, mid);
        let far = level(bar.right() - 4.0, mid);
        assert!(played > far, "played {played}, track {far}");
        if let Ok(dir) = std::env::var("EDEL_LOCK_PNG") {
            let (x, y, w, h) = card.device(1.0);
            let mut shot = Pixmap::new(1280, 800).unwrap();
            fill(
                &mut shot,
                0.0,
                0.0,
                1280.0,
                800.0,
                0.0,
                tokens.lock_backdrop_deep,
            );
            crate::lock::glass(&mut shot, (x, y, w, h), h / 2.0, 1.0, &tokens);
            paint(
                &mut shot,
                &view,
                &parts,
                Marks::default(),
                &tokens,
                Some(&mut text),
                1.0,
            );
            shot.save_png(format!("{dir}/player-wide.png")).unwrap();
        }
    }

    /// Under `EDEL_LOCK_PNG`, the lock screen with two players on the wide
    /// pill, the first playing and shown, its page dot lit (M5.10c), for
    /// looking at the whole screen as the mockup draws it.
    #[test]
    fn the_lock_screen_with_two_players_draws() {
        let Ok(dir) = std::env::var("EDEL_LOCK_PNG") else {
            return;
        };
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let mut pixmap = Pixmap::new(1280, 800).expect("a screen-sized pixmap");
        let mut screen =
            crate::lock::View::new("14:06".into(), "Saturday 3 October".into(), "Ali".into());
        screen.dots = 6;
        screen.notices = 3;
        screen.apps = vec![
            ("Mail".into(), String::new()),
            ("Chat".into(), String::new()),
        ];
        crate::lock::paint(&mut pixmap, &screen, &tokens, &mut text, None, 1.0);
        let corner = crate::corner::View {
            layout: Some("EN".into()),
            menu: false,
            restart_waiting: false,
            hover: None,
        };
        let corner_at = crate::corner::layout(1280.0, 800.0, &corner);
        crate::corner::paint(&mut pixmap, &corner, &corner_at, &tokens, &mut text, 1.0);
        let (cw, ch) = wide_size(2);
        let card = Rect::new(24.0, 800.0 - 24.0 - ch, cw, ch);
        let (x, y, w, h) = card.device(1.0);
        crate::lock::lift(&mut pixmap, card, card.h / 2.0, 1.0, &tokens);
        crate::lock::glass(&mut pixmap, (x, y, w, h), h / 2.0, 1.0, &tokens);
        // A cover and an app icon as an app gives them, the mockup's colours.
        let picture = |side: u32, stops: &[(f32, u32)]| {
            let mut p = Pixmap::new(side, side).expect("a picture");
            let stops = stops
                .iter()
                .map(|&(at, rgb)| {
                    let [_, r, g, b] = rgb.to_be_bytes();
                    tiny_skia::GradientStop::new(at, tiny_skia::Color::from_rgba8(r, g, b, 255))
                })
                .collect();
            let shader = tiny_skia::LinearGradient::new(
                tiny_skia::Point::from_xy(0.0, 0.0),
                tiny_skia::Point::from_xy(0.0, side as f32),
                stops,
                tiny_skia::SpreadMode::Pad,
                tiny_skia::Transform::identity(),
            )
            .expect("a gradient");
            let paint = tiny_skia::Paint {
                shader,
                ..tiny_skia::Paint::default()
            };
            let all =
                tiny_skia::Rect::from_xywh(0.0, 0.0, side as f32, side as f32).expect("a square");
            p.fill_rect(all, &paint, tiny_skia::Transform::identity(), None);
            p
        };
        let cover = picture(76, &[(0.0, 0x2a2b6c), (0.55, 0x8a55b8), (1.0, 0xf48a6c)]);
        let icon = picture(14, &[(0.0, 0xff6f8f), (1.0, 0xe0335a)]);
        let view = PlayerView {
            title: "Night Drive".into(),
            artist: "Lumen".into(),
            identity: "Music".into(),
            playing: true,
            can_previous: true,
            can_next: true,
            cover: Some(cover),
            app_icon: Some(icon),
            position: Some(Duration::from_secs(84)),
            length: Some(Duration::from_secs(221)),
        };
        let parts = place_wide(card, false, 2, 0);
        let marks = Marks {
            hover: None,
            focus: None,
            page: 0,
        };
        paint(
            &mut pixmap,
            &view,
            &parts,
            marks,
            &tokens,
            Some(&mut text),
            1.0,
        );
        pixmap
            .save_png(format!("{dir}/lock-player-pages.png"))
            .expect("the PNG is written");
    }
}
