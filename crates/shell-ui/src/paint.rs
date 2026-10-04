//! What a panel looks like (M5.1b, M5.1c), drawn with tiny-skia into a
//! pixmap the size of the panel's buffer: the panel in its token colour,
//! its widgets laid out from the preset (from its start, in its centre and
//! towards its end) and, on the Full and Balanced tiers, rounded fillets
//! where the panel meets the screen's sides, so the area beside it reads
//! as one rounded shape (REVIEW-shells.md). A dock (M5.4d) is a card in
//! the panel's colour, as wide as what it holds and rounded all round,
//! with no fillets. Sizes are logical pixels times the buffer's scale;
//! colours come from the design tokens.

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};

use edel::presets::{Edge, Style};
use edel::tokens::{Colour, Tokens};

use crate::widgets::{Canvas, Live, Widget};

/// Everything a panel shows, at one scale.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// The buffer's size in its pixels: the panel and, on its inner side,
    /// the fillets' strip.
    pub width: u32,
    pub height: u32,
    pub scale: u32,
    pub edge: Edge,
    pub style: Style,
    /// Whether the fillets are drawn (not on the Lite tier).
    pub fillets: bool,
    /// What each widget shows, in the row's order.
    pub shown: Vec<String>,
}

/// A panel's widgets, from the preset, as this machine can show them.
#[derive(Default)]
pub struct Row {
    pub start: Vec<&'static Widget>,
    pub centre: Vec<&'static Widget>,
    pub end: Vec<&'static Widget>,
}

impl Row {
    pub fn all(&self) -> impl Iterator<Item = &'static Widget> + '_ {
        self.start
            .iter()
            .chain(&self.centre)
            .chain(&self.end)
            .copied()
    }

    /// What each widget shows now, for [`Look::shown`].
    pub fn shows(&self, live: &Live) -> Vec<String> {
        self.all().map(|w| (w.shows)(live)).collect()
    }

    /// The `i`th widget, start to end.
    pub fn widget(&self, i: usize) -> Option<&'static Widget> {
        self.all().nth(i)
    }
}

/// How tall the strip beside the panel is, for the fillets, in logical
/// pixels: the tokens' corner radius.
pub fn fillet_height(tokens: &Tokens) -> u32 {
    tokens.radius
}

/// How tall the strip for the fillets is on a panel of `style`: a dock
/// has none.
pub fn strip(style: Style, tokens: &Tokens) -> u32 {
    match style {
        Style::Bar => fillet_height(tokens),
        Style::Dock => 0,
    }
}

/// The panel's top row in its buffer, in logical pixels; the strip lies
/// on the side towards the screen's middle.
pub fn panel_top(edge: Edge, style: Style, tokens: &Tokens) -> u32 {
    match edge {
        Edge::Bottom => strip(style, tokens),
        Edge::Top => 0,
    }
}

/// The room inside a dock's card at each end, in logical pixels.
pub const DOCK_PAD: f32 = 8.0;
/// A dock's height and its corners' radius, in logical pixels: taller and
/// rounder than a bar, for bigger icons.
pub const DOCK_HEIGHT: u32 = 60;
const DOCK_RADIUS: f32 = 18.0;

/// A panel's own height, without the fillets' strip, in logical pixels.
pub fn height(style: Style, tokens: &Tokens) -> u32 {
    match style {
        Style::Bar => tokens.panel_height,
        Style::Dock => DOCK_HEIGHT,
    }
}

/// The fonts and glyph cache text is drawn with, loaded once.
pub struct Text {
    fonts: FontSystem,
    glyphs: SwashCache,
    /// The interface font's family, from the tokens.
    family: String,
}

/// One line of text, shaped.
pub struct Line {
    buffer: Buffer,
    pub width: f32,
}

impl Text {
    /// Text in `family`, the tokens' interface font.
    pub fn load(family: &str) -> Text {
        Text {
            fonts: FontSystem::new(),
            glyphs: SwashCache::new(),
            family: family.to_string(),
        }
    }

    /// `text` shaped in the interface font, falling back for other
    /// scripts, `size` pixels high.
    pub fn line(&mut self, text: &str, size: f32) -> Line {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, size * 1.25));
        buffer.set_size(None, None);
        let attrs = Attrs::new().family(Family::Name(&self.family));
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        let width = buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max);
        Line { buffer, width }
    }

    /// `text` as one line `size` pixels high in at most `room` pixels:
    /// whole, or cut short with an ellipsis, or nothing if not even that
    /// fits.
    pub fn fit(&mut self, text: &str, size: f32, room: f32) -> Line {
        let whole = self.line(text, size);
        if whole.width <= room {
            return whole;
        }
        // The most characters that fit before the ellipsis, by halving.
        let ends: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let (mut fits, mut over) = (0, ends.len());
        let mut best = self.line("\u{2026}", size);
        while fits + 1 < over {
            let mid = (fits + over) / 2;
            let line = self.line(&format!("{}\u{2026}", text[..ends[mid]].trim_end()), size);
            if line.width <= room {
                (fits, best) = (mid, line);
            } else {
                over = mid;
            }
        }
        if best.width > room {
            return self.line("", size);
        }
        best
    }

    /// Draws `line` with its top left at `x`, `y`, rounded to pixels.
    pub fn draw(&mut self, pixmap: &mut Pixmap, line: &mut Line, x: f32, y: f32, colour: Colour) {
        let (x, y) = (x.round() as i32, y.round() as i32);
        let ink = Color::rgba(
            (colour.r * 255.0) as u8,
            (colour.g * 255.0) as u8,
            (colour.b * 255.0) as u8,
            255,
        );
        let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
        let pixels = pixmap.data_mut();
        line.buffer.draw(
            &mut self.fonts,
            &mut self.glyphs,
            ink,
            |gx, gy, gw, gh, color| {
                let alpha = color.a() as f32 / 255.0;
                if alpha == 0.0 {
                    return;
                }
                for py in (y + gy)..(y + gy + gh as i32) {
                    for px in (x + gx)..(x + gx + gw as i32) {
                        if px < 0 || py < 0 || px >= pw || py >= ph {
                            continue;
                        }
                        let i = ((py * pw + px) * 4) as usize;
                        // Premultiplied source over premultiplied destination.
                        for (k, channel) in [color.r(), color.g(), color.b(), 255]
                            .into_iter()
                            .enumerate()
                        {
                            let src = channel as f32 * alpha;
                            let dst = pixels[i + k] as f32;
                            pixels[i + k] = (src + dst * (1.0 - alpha)).round() as u8;
                        }
                    }
                }
            },
        );
    }
}

/// tiny-skia's colour for a token colour.
fn colour(c: Colour) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(c.r, c.g, c.b, c.a).unwrap_or(tiny_skia::Color::BLACK)
}

/// `a` moved towards `b` by `t`, 0 to 1, opaque: text dimmed towards the
/// panel it lies on.
pub fn mix(a: Colour, b: Colour, t: f32) -> Colour {
    Colour {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
}

pub fn paint_of(c: Colour) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(colour(c));
    paint.anti_alias = true;
    paint
}

/// The shell's icon `name` from `design/icons/` (M5.5d), `px` pixels
/// square in `colour`, its top left corner at `x`, `y`.
pub fn icon(pixmap: &mut Pixmap, name: &str, px: f32, x: f32, y: f32, colour: Colour) {
    if let Some(icon) = edel::icons::draw(name, px.round() as u32, colour) {
        let (x, y) = (x.round() as i32, y.round() as i32);
        let paint = PixmapPaint::default();
        pixmap.draw_pixmap(x, y, icon.as_ref(), &paint, Transform::identity(), None);
    }
}

/// The accent, faint: under a chosen row or a switched-on button.
pub fn lit(tokens: &Tokens) -> Colour {
    Colour {
        a: 0.18,
        ..tokens.accent
    }
}

/// Fills the rectangle with corners of radius `r` in `c`.
pub fn fill(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Colour) {
    if let Some(path) = rounded(x, y, w, h, r) {
        pixmap.fill_path(
            &path,
            &paint_of(c),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// A rectangle with corners of radius `r`, as a path.
pub fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    // The cubic Bézier that best follows a quarter circle.
    let k = r * 0.552_284_8;
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(x + w - r, y);
    p.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    p.line_to(x + w, y + h - r);
    p.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    p.line_to(x + r, y + h);
    p.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    p.line_to(x, y + r);
    p.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    p.close();
    p.finish()
}

/// A fillet: the panel's colour filling the corner between the screen's
/// side at `x` and the panel's inner edge at `y`, cut by a quarter circle
/// of radius `r`. `left` says which side; `up` that the screen's middle is
/// above (a bottom panel).
fn fillet(x: f32, y: f32, r: f32, left: bool, up: bool) -> Option<Path> {
    let k = r * 0.552_284_8;
    let out = if left { x + r } else { x - r };
    let bend = if left { x + r - k } else { x - r + k };
    let towards = if up { -1.0 } else { 1.0 };
    let mut p = PathBuilder::new();
    // From the screen's side, along it to the panel, along the panel, and
    // back round the curve.
    p.move_to(x, y + towards * r);
    p.line_to(x, y);
    p.line_to(out, y);
    p.cubic_to(bend, y, x, y + towards * (r - k), x, y + towards * r);
    p.close();
    p.finish()
}

/// Draws `look` into `pixmap`, which is `look.width` by `look.height`,
/// with `row`'s widgets showing `look.shown`; returns where each widget
/// lies, start to end, as its left edge and width in logical pixels.
pub fn paint(
    pixmap: &mut Pixmap,
    look: &Look,
    tokens: &Tokens,
    text: Option<&mut Text>,
    icons: Option<&mut crate::icons::Icons>,
    row: &Row,
) -> Vec<(f32, f32)> {
    let s = look.scale.max(1) as f32;
    let (w, h) = (look.width as f32, look.height as f32);
    let strip = strip(look.style, tokens) as f32 * s;
    let panel_h = h - strip;
    let top = panel_top(look.edge, look.style, tokens) as f32 * s;
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let panel = paint_of(tokens.panel);
    let dock = look.style == Style::Dock;
    if dock {
        // A card rounded all round, with a hairline of the text's colour
        // inside its edge, so it reads on any background.
        let r = DOCK_RADIUS * s;
        let line = Colour {
            a: 0.1,
            ..tokens.panel_text
        };
        fill(pixmap, 0.0, top, w, panel_h, r, line);
        fill(
            pixmap,
            s,
            top + s,
            w - 2.0 * s,
            panel_h - 2.0 * s,
            r - s,
            tokens.panel,
        );
    } else if let Some(rect) = Rect::from_xywh(0.0, top, w, panel_h) {
        pixmap.fill_rect(rect, &panel, Transform::identity(), None);
    }
    if look.fillets && !dock {
        let (inner, up) = match look.edge {
            Edge::Bottom => (strip, true),
            Edge::Top => (panel_h, false),
        };
        for path in [
            fillet(0.0, inner, strip, true, up),
            fillet(w, inner, strip, false, up),
        ]
        .into_iter()
        .flatten()
        {
            pixmap.fill_path(
                &path,
                &panel,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    let mut canvas = Canvas {
        pixmap,
        tokens,
        text,
        icons,
        scale: s,
        top,
        height: panel_h,
        dock,
    };
    let mut shown = look.shown.iter().map(String::as_str);
    // Each group's widgets with what they show and their widths.
    let mut measure = |group: &[&'static Widget]| {
        group
            .iter()
            .map(|widget| {
                let showing = shown.next().unwrap_or("");
                let width = (widget.width)(&mut canvas, showing);
                (*widget, showing, width)
            })
            .collect::<Vec<_>>()
    };
    let start = measure(&row.start);
    let centre = measure(&row.centre);
    let end = measure(&row.end);
    let total = |group: &[(&Widget, &str, f32)]| group.iter().map(|g| g.2).sum::<f32>();
    let pad = if dock { (DOCK_PAD * s).round() } else { 0.0 };
    let mut places = Vec::new();
    for (group, from) in [
        (&start, pad),
        (&centre, ((w - total(&centre)) / 2.0).round()),
        (&end, w - pad - total(&end)),
    ] {
        let mut x = from;
        for (widget, showing, width) in group {
            (widget.draw)(&mut canvas, showing, x);
            places.push((x / s, width / s));
            x += width;
        }
    }
    places
}

/// How wide a dock holding `row`, showing `shown`, is in logical pixels:
/// its widgets side by side and the card's room at both ends.
pub fn natural_width(
    tokens: &Tokens,
    text: Option<&mut Text>,
    icons: Option<&mut crate::icons::Icons>,
    row: &Row,
    shown: &[String],
    scale: u32,
) -> u32 {
    let s = scale.max(1) as f32;
    let Some(mut pixmap) = Pixmap::new(1, (DOCK_HEIGHT as f32 * s) as u32) else {
        return 0;
    };
    let height = pixmap.height() as f32;
    let mut canvas = Canvas {
        pixmap: &mut pixmap,
        tokens,
        text,
        icons,
        scale: s,
        top: 0.0,
        height,
        dock: true,
    };
    let widgets: f32 = row
        .all()
        .zip(shown)
        .map(|(widget, showing)| (widget.width)(&mut canvas, showing))
        .sum();
    (widgets / s + 2.0 * DOCK_PAD).ceil() as u32
}

/// The pixmap's premultiplied RGBA as the BGRA bytes `wl_shm`'s
/// ARGB8888 wants, little-endian.
pub fn to_argb(pixmap: &Pixmap, out: &mut [u8]) {
    for (src, dst) in pixmap.data().chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{find, menu};

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pixmap.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    fn classic() -> Row {
        Row {
            start: vec![find("menu").unwrap()],
            centre: vec![],
            end: vec![find("clock").unwrap()],
        }
    }

    /// A panel along `edge`, drawn without fonts.
    fn drawn(edge: Edge, fillets: bool, row: &Row) -> (Pixmap, Tokens) {
        let tokens = Tokens::built_in();
        let look = Look {
            width: 1280,
            height: tokens.panel_height + fillet_height(&tokens),
            scale: 1,
            edge,
            style: Style::Bar,
            fillets,
            shown: row.shows(&Live::default()),
        };
        let mut pixmap = Pixmap::new(look.width, look.height).unwrap();
        paint(&mut pixmap, &look, &tokens, None, None, row);
        (pixmap, tokens)
    }

    #[test]
    fn the_panel_is_its_token_colour_with_the_strip_above_it_clear() {
        let (pixmap, tokens) = drawn(Edge::Bottom, true, &classic());
        let strip = fillet_height(&tokens);
        let panel = tokens.panel.bytes();
        // The panel's middle and its bottom row, past the icon.
        assert_eq!(pixel(&pixmap, 640, strip + 20), panel);
        assert_eq!(pixel(&pixmap, 640, pixmap.height() - 1), panel);
        // Above it, clear but for the fillets at the very ends.
        assert_eq!(pixel(&pixmap, 640, 0)[3], 0);
        assert_eq!(
            pixel(&pixmap, 0, strip - 1),
            panel,
            "the left fillet's corner"
        );
        assert_eq!(pixel(&pixmap, 1279, strip - 1), panel, "the right one's");
        assert_eq!(
            pixel(&pixmap, strip - 1, 0)[3],
            0,
            "the curve leaves the corner's far side clear"
        );
    }

    #[test]
    fn a_top_panel_has_its_strip_below() {
        let (pixmap, tokens) = drawn(Edge::Top, true, &classic());
        let panel = tokens.panel.bytes();
        let h = tokens.panel_height;
        assert_eq!(pixel(&pixmap, 640, 0), panel);
        assert_eq!(pixel(&pixmap, 640, h - 1), panel);
        assert_eq!(pixel(&pixmap, 640, h)[3], 0, "the strip is clear");
        assert_eq!(pixel(&pixmap, 0, h), panel, "the left fillet's corner");
        assert_eq!(pixel(&pixmap, 1279, h), panel, "the right one's");
        assert_eq!(
            pixel(&pixmap, tokens.radius - 1, pixmap.height() - 1)[3],
            0,
            "the curve leaves the corner's far side clear"
        );
    }

    #[test]
    fn a_dock_is_a_card_as_wide_as_what_it_holds_rounded_all_round() {
        let tokens = Tokens::built_in();
        let menu = find("menu").unwrap();
        let row = Row {
            start: vec![],
            centre: vec![menu, menu],
            end: vec![],
        };
        let shown = row.shows(&Live::default());
        let width = natural_width(&tokens, None, None, &row, &shown, 1);
        let h = DOCK_HEIGHT;
        assert_eq!(height(Style::Dock, &tokens), h);
        assert_eq!(width, 2 * h + 2 * DOCK_PAD as u32);
        let look = Look {
            width,
            height: h,
            scale: 1,
            edge: Edge::Bottom,
            style: Style::Dock,
            fillets: true,
            shown,
        };
        let mut pixmap = Pixmap::new(look.width, look.height).unwrap();
        let places = paint(&mut pixmap, &look, &tokens, None, None, &row);
        assert_eq!(
            places,
            [(DOCK_PAD, h as f32), (DOCK_PAD + h as f32, h as f32)]
        );
        assert_eq!(pixel(&pixmap, width / 2, 1), tokens.panel.bytes());
        assert_ne!(
            pixel(&pixmap, width / 2, 0),
            tokens.panel.bytes(),
            "a hairline runs along its edge"
        );
        assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "its corners are round");
        assert_eq!(pixel(&pixmap, width - 1, h - 1)[3], 0);
        // No fillets and no strip: its buffer is the card.
        assert_eq!(strip(Style::Dock, &tokens), 0);
        assert_eq!(panel_top(Edge::Bottom, Style::Dock, &tokens), 0);
    }

    #[test]
    fn lite_draws_no_fillets() {
        let (pixmap, tokens) = drawn(Edge::Bottom, false, &classic());
        assert_eq!(pixel(&pixmap, 0, fillet_height(&tokens) - 1)[3], 0);
    }

    #[test]
    fn widgets_sit_at_the_start_the_centre_and_the_end() {
        // Whether the menu icon's first square has its middle at x + mid.
        let icon_at = |pixmap: &Pixmap, tokens: &Tokens, x: u32| {
            let strip = fillet_height(tokens);
            let (cell, _, inset) = menu::icon(tokens.panel_height as f32);
            let mid = (inset + cell / 2.0) as u32;
            pixel(pixmap, x + mid, strip + mid) == tokens.panel_text.bytes()
        };
        // Classic: the menu at the panel's start.
        let (pixmap, tokens) = drawn(Edge::Bottom, false, &classic());
        assert!(icon_at(&pixmap, &tokens, 0));
        // The same widget in the centre and at the end, none at the start.
        let menu = find("menu").unwrap();
        let row = Row {
            start: vec![],
            centre: vec![menu],
            end: vec![menu],
        };
        let (pixmap, tokens) = drawn(Edge::Bottom, false, &row);
        let h = tokens.panel_height;
        assert!(!icon_at(&pixmap, &tokens, 0));
        assert!(icon_at(&pixmap, &tokens, 640 - h / 2));
        assert!(icon_at(&pixmap, &tokens, 1280 - h));
    }

    #[test]
    fn shm_bytes_are_bgra() {
        let mut pixmap = Pixmap::new(1, 1).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(0x11, 0x22, 0x33, 0xff));
        let mut out = [0u8; 4];
        to_argb(&pixmap, &mut out);
        assert_eq!(out, [0x33, 0x22, 0x11, 0xff]);
    }
}
