//! What the launcher and the window switcher share (M5.3b, M5.3c): a
//! surface of their own above everything, made when shown and let go when
//! hidden, so shell-ui's idle memory stays the panel's; and a card in the
//! menus' colour and corners holding rows of text, the chosen one lit,
//! and on the Full and Balanced tiers its shadow round it (M5.5e). Sizes
//! come from the tokens, so a change there reaches both. Drawing is plain
//! and tested without a display.

use smithay_client_toolkit::compositor::{CompositorState, FrameCallbackData, Region};
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::reexports::client::protocol::{wl_shm, wl_surface};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Layer, LayerSurface};
use smithay_client_toolkit::shm::slot::SlotPool;
use tiny_skia::{Pixmap, PixmapPaint, Stroke, Transform};

use edel::tokens::{Colour, Tokens};

use crate::Shell;
use crate::paint::{self, Text, fill, lit, mix, paint_of};

/// The room round a card's rows, and text's from a row's left, in logical
/// pixels.
pub const PAD: f32 = 8.0;
pub const INSET: f32 = 12.0;

/// A popup's surface, its own buffers and what it last drew, `V`.
pub struct Popup<V> {
    pub surface: LayerSurface,
    pool: SlotPool,
    /// Its card's size in logical pixels, and its screen's scale.
    size: (u32, u32),
    scale: u32,
    /// The room for the shadow on every side of the card, logical pixels:
    /// none on Lite (M5.5e). The surface is the card and this round it.
    room: u32,
    /// The cards it holds, logical pixels from its top left corner; none
    /// means one card, the whole of it, in the menus' corners.
    cards: Vec<Card>,
    /// The shadow, drawn once for a card size, scale and cards.
    shadow: Option<((u32, u32, u32), Vec<Card>, Pixmap)>,
    /// Configured, so it may draw.
    ready: bool,
    /// Drawn and not yet shown by the compositor: the next drawing waits
    /// for its frame, as the panel's does.
    waiting: bool,
    drawn: Option<(V, u32)>,
}

impl<V: Clone + PartialEq> Popup<V> {
    /// A surface named `name` for a card `size` big with `room` round it
    /// for its shadow, with buffers for cards up to `most` at `scale`; the
    /// caller places it and commits.
    pub fn new(
        shell: &Shell,
        name: &'static str,
        size: (u32, u32),
        most: (u32, u32),
        scale: u32,
        room: u32,
    ) -> Option<Popup<V>> {
        let (mw, mh) = (most.0 + 2 * room, most.1 + 2 * room);
        let bytes = (mw * mh * 4 * scale * scale) as usize;
        let pool = SlotPool::new(bytes, &shell.shm)
            .inspect_err(|e| eprintln!("edel-shell-ui: no memory for the {name}: {e}"))
            .ok()?;
        let surface = shell.compositor.create_surface(&shell.qh);
        let surface =
            shell
                .layers
                .create_layer_surface(&shell.qh, surface, Layer::Overlay, Some(name), None);
        let popup = Popup {
            surface,
            pool,
            size,
            scale,
            room,
            cards: Vec::new(),
            shadow: None,
            ready: false,
            waiting: false,
            drawn: None,
        };
        popup.set_size(&shell.compositor);
        Some(popup)
    }

    /// The surface's size, the card's and the room round it, and where it
    /// takes clicks: on the cards alone.
    fn set_size(&self, compositor: &CompositorState) {
        let (w, h) = self.size;
        let room = self.room;
        self.surface.set_size(w + 2 * room, h + 2 * room);
        if let Ok(region) = Region::new(compositor) {
            if self.cards.is_empty() {
                region.add(room as i32, room as i32, w as i32, h as i32);
            }
            for c in &self.cards {
                let (x, y, w, h) = c.rect.device(1.0);
                region.add(
                    room as i32 + x as i32,
                    room as i32 + y as i32,
                    w as i32,
                    h as i32,
                );
            }
            self.surface
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
    }

    /// A new card size, drawn once the compositor agrees.
    pub fn resize(&mut self, size: (u32, u32), compositor: &CompositorState) {
        if size != self.size {
            self.size = size;
            self.ready = false;
            self.set_size(compositor);
            self.surface.commit();
        }
    }

    /// The cards it holds apart on its surface (the notification centre's
    /// notifications and calendar), each with its own shadow and taking
    /// clicks, the room between them neither; none for one card, the whole
    /// surface. Takes effect with the next drawing.
    pub fn set_cards(&mut self, cards: Vec<Card>, compositor: &CompositorState) {
        if cards != self.cards {
            self.cards = cards;
            self.set_size(compositor);
        }
    }

    /// The card's size now, logical pixels: what [`Popup::resize`] last
    /// asked for.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The room round the card, logical pixels: where the card starts.
    pub fn room(&self) -> u32 {
        self.room
    }

    pub fn configured(&mut self) {
        self.ready = true;
    }

    /// The compositor showed the last drawing.
    pub fn framed(&mut self) {
        self.waiting = false;
    }

    pub fn set_scale(&mut self, factor: i32) {
        self.scale = factor.clamp(1, 4) as u32;
    }

    pub fn scale(&self) -> f32 {
        self.scale as f32
    }

    pub fn is(&self, surface: &wl_surface::WlSurface) -> bool {
        self.surface.wl_surface() == surface
    }

    /// A clear pixmap to draw `view` into, unless it may not draw yet or
    /// already shows it.
    pub fn canvas(&self, view: &V) -> Option<Pixmap> {
        if !self.ready
            || self.waiting
            || self
                .drawn
                .as_ref()
                .is_some_and(|(v, s)| v == view && *s == self.scale)
        {
            return None;
        }
        Pixmap::new(self.size.0 * self.scale, self.size.1 * self.scale)
    }

    /// The card in `pixmap` with its shadow round it, when it has room
    /// for one; the shadow is drawn once per card size and scale.
    fn framed_card(&mut self, pixmap: &Pixmap, tokens: &Tokens) -> Option<Pixmap> {
        if self.room == 0 {
            return None;
        }
        let s = self.scale;
        let key = (pixmap.width(), pixmap.height(), s);
        if self
            .shadow
            .as_ref()
            .is_none_or(|(k, c, _)| *k != key || *c != self.cards)
        {
            let room = self.room * s;
            let shadow = if self.cards.is_empty() {
                let r = tokens.radius_menu as f32 * s as f32;
                paint::shadow(key.0, key.1, room, r, tokens, s as f32)?
            } else {
                // Each card's own shadow, laid where the card lies.
                let mut all = Pixmap::new(key.0 + 2 * room, key.1 + 2 * room)?;
                for c in &self.cards {
                    let (x, y, w, h) = c.rect.device(s as f32);
                    let one = paint::shadow(
                        w as u32,
                        h as u32,
                        room,
                        c.radius * s as f32,
                        tokens,
                        s as f32,
                    )?;
                    let paint = PixmapPaint::default();
                    all.draw_pixmap(
                        x as i32,
                        y as i32,
                        one.as_ref(),
                        &paint,
                        Transform::identity(),
                        None,
                    );
                }
                all
            };
            self.shadow = Some((key, self.cards.clone(), shadow));
        }
        let (_, _, shadow) = self.shadow.as_ref()?;
        let mut out = shadow.clone();
        let at = (self.room * s) as i32;
        let paint = PixmapPaint::default();
        out.draw_pixmap(at, at, pixmap.as_ref(), &paint, Transform::identity(), None);
        Some(out)
    }

    /// Shows `pixmap`, the card drawing `view`, with its shadow round it;
    /// whether it is the first time.
    pub fn show(
        &mut self,
        view: V,
        pixmap: &Pixmap,
        tokens: &Tokens,
        name: &str,
        qh: &QueueHandle<Shell>,
    ) -> bool {
        let framed = self.framed_card(pixmap, tokens);
        let pixmap = framed.as_ref().unwrap_or(pixmap);
        let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
        let (buffer, canvas) = match self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        {
            Ok(made) => made,
            Err(e) => {
                eprintln!("edel-shell-ui: drawing the {name} failed: {e}");
                return false;
            }
        };
        paint::to_argb(pixmap, canvas);
        let surface = self.surface.wl_surface();
        surface.set_buffer_scale(self.scale as i32);
        surface.damage_buffer(0, 0, w, h);
        if let Err(e) = buffer.attach_to(surface) {
            eprintln!("edel-shell-ui: drawing the {name} failed: {e}");
            return false;
        }
        surface.frame(qh, FrameCallbackData(surface.clone()));
        self.surface.commit();
        self.waiting = true;
        let first = self.drawn.is_none();
        self.drawn = Some((view, self.scale));
        first
    }
}

/// A rectangle, logical pixels from a card's top left corner: what quick
/// settings, the banner and the notification centre lay their parts out in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn middle(&self) -> f32 {
        self.y + self.h / 2.0
    }

    /// At scale `s`, on whole pixels.
    pub fn device(&self, s: f32) -> (f32, f32, f32, f32) {
        let (x, y) = ((self.x * s).round(), (self.y * s).round());
        let (r, b) = (
            ((self.x + self.w) * s).round(),
            ((self.y + self.h) * s).round(),
        );
        (x, y, r - x, b - y)
    }
}

/// The panel's text dimmed towards the card: secondary text and the
/// quieter kind, as the mockups' `text-2` and `text-3`.
pub fn dim(tokens: &Tokens) -> Colour {
    mix(tokens.panel_text, tokens.panel, 0.38)
}

/// The text's colour at `alpha` over the card: what the mockups' `fill`
/// and `fill-2` are.
pub fn veil(tokens: &Tokens, alpha: f32) -> Colour {
    Colour {
        a: alpha,
        ..tokens.panel_text
    }
}

/// The slider's knob: the lighter of the window's and the text's colour,
/// white on the light scheme and near white on the dark.
pub fn knob(tokens: &Tokens) -> Colour {
    let light = |c: Colour| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if light(tokens.window) > light(tokens.panel_text) {
        tokens.window
    } else {
        tokens.panel_text
    }
}

/// An icon of the shell's own centred in `r`, `px` logical pixels across.
pub fn icon_in(pixmap: &mut Pixmap, name: &str, px: f32, r: Rect, s: f32, c: Colour) {
    let side = (px * s).round();
    let (x, y, w, h) = r.device(s);
    paint::icon(
        pixmap,
        name,
        side,
        x + ((w - side) / 2.0).round(),
        y + ((h - side) / 2.0).round(),
        c,
    );
}

/// `text` broken into at most `lines` lines that `measure` finds no wider
/// than `room`, at spaces; a word wider than a line is broken where it
/// fills one. The last line holds everything left, to be cut short with an
/// ellipsis when drawn (`Text::fit`), so a long text never grows the card
/// past its lines. No text, no lines.
pub fn wrap(
    text: &str,
    lines: usize,
    room: f32,
    mut measure: impl FnMut(&str) -> f32,
) -> Vec<String> {
    let mut words: std::collections::VecDeque<String> =
        text.split_whitespace().map(String::from).collect();
    let mut out = Vec::new();
    while out.len() + 1 < lines {
        let Some(mut line) = words.pop_front() else {
            break;
        };
        if measure(&line) > room {
            // The most characters that fit, by halving, and at least one.
            let ends: Vec<usize> = line.char_indices().map(|(i, _)| i).skip(1).collect();
            let (mut fits, mut over) = (0usize, ends.len() + 1);
            while fits + 1 < over {
                let mid = (fits + over) / 2;
                if measure(&line[..ends[mid - 1]]) <= room {
                    fits = mid;
                } else {
                    over = mid;
                }
            }
            let at = if fits == 0 {
                ends.first().copied()
            } else {
                ends.get(fits - 1).copied()
            };
            if let Some(at) = at {
                let rest = line.split_off(at);
                words.push_front(rest);
            }
            out.push(line);
            continue;
        }
        while let Some(next) = words.front() {
            let joined = format!("{line} {next}");
            if measure(&joined) > room {
                break;
            }
            line = joined;
            words.pop_front();
        }
        out.push(line);
    }
    if lines > 0 && !words.is_empty() {
        out.push(words.into_iter().collect::<Vec<_>>().join(" "));
    }
    out
}

/// A card a popup holds, logical pixels from its top left corner, with its
/// corners' radius: where it is drawn, casts its shadow and takes clicks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Card {
    pub rect: Rect,
    pub radius: f32,
}

/// Clears `pixmap` and draws `cards` in the panel's colour at scale `s`,
/// the room between them left clear.
pub fn cards(pixmap: &mut Pixmap, tokens: &Tokens, s: f32, cards: &[Card]) {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    for c in cards {
        let (x, y, w, h) = c.rect.device(s);
        fill(pixmap, x, y, w, h, c.radius * s, tokens.panel);
    }
}

/// Clears `pixmap` and draws the card, the whole of it, at scale `s`.
pub fn card(pixmap: &mut Pixmap, tokens: &Tokens, s: f32) {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
    fill(
        pixmap,
        0.0,
        0.0,
        w,
        h,
        tokens.radius_menu as f32 * s,
        tokens.panel,
    );
}

/// Where text `size` pixels high sits to be in the middle of a row from
/// `top`, `height` high, in logical pixels, at scale `s`.
pub fn middle(top: f32, height: f32, size: f32, s: f32) -> f32 {
    (top + height / 2.0) * s - size * 0.625
}

/// Draws `labels` as rows from `top` logical pixels down, the card's width
/// less its padding, `chosen` lit, in `ink`, at scale `s`; without `text`,
/// only the light.
pub fn rows<'a>(
    pixmap: &mut Pixmap,
    tokens: &Tokens,
    text: Option<&mut Text>,
    labels: impl Iterator<Item = &'a str>,
    top: f32,
    chosen: Option<usize>,
    s: f32,
) {
    let w = pixmap.width() as f32 / s;
    let row = tokens.row as f32;
    if let Some(i) = chosen {
        let y = top + i as f32 * row;
        let r = tokens.radius_control as f32 * s;
        fill(
            pixmap,
            PAD * s,
            y * s,
            (w - 2.0 * PAD) * s,
            row * s,
            r,
            lit(tokens),
        );
    }
    let Some(text) = text else {
        return;
    };
    let size = tokens.panel_text_size as f32 * s;
    let room = (w - 2.0 * (PAD + INSET)) * s;
    for (i, label) in labels.enumerate() {
        let mut line = text.fit(label, size, room);
        let y = middle(top + i as f32 * row, row, size, s);
        text.draw(pixmap, &mut line, (PAD + INSET) * s, y, tokens.panel_text);
    }
}

/// A radio button `d` logical pixels across with its top left corner at
/// `x`, `y` logical pixels, at scale `s`: a ring, and when `on` the ring
/// and a dot half its width in the accent, so exactly one row of a menu
/// shows it is the choice. Off, the ring is the panel's text dimmed
/// towards the card, the colour of a picture's tiles. The ring is as thick
/// as a picture's frame lines, 1.5 px, because the tokens have no line
/// width.
pub fn radio(pixmap: &mut Pixmap, tokens: &Tokens, x: f32, y: f32, d: f32, on: bool, s: f32) {
    let ring = (1.5 * s).max(1.0);
    let (cx, cy) = ((x + d / 2.0) * s, (y + d / 2.0) * s);
    // The stroke lies half inside and half outside its path.
    let r = d / 2.0 * s - ring / 2.0;
    let ink = if on {
        tokens.accent
    } else {
        mix(tokens.panel_text, tokens.panel, 0.45)
    };
    let Some(circle) = tiny_skia::PathBuilder::from_circle(cx, cy, r) else {
        return;
    };
    let stroke = Stroke {
        width: ring,
        ..Stroke::default()
    };
    pixmap.stroke_path(
        &circle,
        &paint_of(ink),
        &stroke,
        Transform::identity(),
        None,
    );
    if on {
        let dot = tiny_skia::PathBuilder::from_circle(cx, cy, d / 4.0 * s);
        if let Some(dot) = dot {
            pixmap.fill_path(
                &dot,
                &paint_of(ink),
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pixmap.pixel(x, y).unwrap().demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    /// Six pixels a character.
    fn six(s: &str) -> f32 {
        s.chars().count() as f32 * 6.0
    }

    #[test]
    fn text_wraps_at_spaces_and_the_last_line_keeps_the_rest() {
        // Room for 10 characters.
        let room = 60.0;
        assert_eq!(wrap("", 2, room, six), Vec::<String>::new());
        assert_eq!(wrap("hello", 2, room, six), ["hello"]);
        assert_eq!(
            wrap("hello brave new world", 3, room, six),
            ["hello", "brave new", "world"]
        );
        // Two lines: the second holds the rest, to be cut when drawn.
        assert_eq!(
            wrap("one two three four five six seven", 2, room, six),
            ["one two", "three four five six seven"]
        );
        assert_eq!(wrap("a  b\n c", 1, room, six), ["a b c"]);
        assert!(wrap("anything", 0, room, six).is_empty());
    }

    #[test]
    fn a_word_wider_than_a_line_is_broken_where_it_fills_one() {
        let room = 30.0;
        // Five characters a line.
        assert_eq!(wrap("abcdefghijkl", 3, room, six), ["abcde", "fghij", "kl"]);
        assert_eq!(
            wrap("ab abcdefghij", 3, room, six),
            ["ab", "abcde", "fghij"]
        );
        // Never an empty line, even when one character is too wide.
        assert_eq!(wrap("abc", 3, 2.0, six), ["a", "b", "c"]);
        // Multi-byte characters are cut at a character.
        assert_eq!(
            wrap("\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}", 3, room, six),
            ["\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}", "\u{e9}"]
        );
    }

    #[test]
    fn the_card_is_round_and_the_chosen_row_is_lit_at_any_scale() {
        let tokens = Tokens::built_in();
        let row = tokens.row;
        for s in [1, 2] {
            let mut pixmap = Pixmap::new(200 * s, (2 * row + 16) * s).unwrap();
            card(&mut pixmap, &tokens, s as f32);
            rows(
                &mut pixmap,
                &tokens,
                None,
                ["a", "b"].into_iter(),
                PAD,
                Some(1),
                s as f32,
            );
            assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "a round corner");
            let back = tokens.panel.bytes();
            let at = |i: u32| {
                pixel(
                    &pixmap,
                    (PAD as u32 + 4) * s,
                    (PAD as u32 + i * row + row / 2) * s,
                )
            };
            assert_eq!(at(0), back);
            assert_ne!(at(1), back);
        }
    }
}
