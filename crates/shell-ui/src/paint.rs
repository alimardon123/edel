//! What a panel looks like (M5.1b, M5.1c), drawn with tiny-skia into a
//! pixmap the size of the panel's buffer: the panel in its token colour,
//! its widgets laid out from the preset (from its start, in its centre and
//! towards its end) and, on the Full and Balanced tiers, rounded fillets
//! where the panel meets the screen's sides, so the area beside it reads
//! as one rounded shape (REVIEW-shells.md). A dock (M5.4d) is a card in
//! the panel's colour, as wide as what it holds and rounded all round,
//! with no fillets. Sizes are logical pixels times the buffer's scale;
//! colours come from the design tokens.

use cosmic_text::{
    Attrs, Buffer, Color, Family, FeatureTag, FontFeatures, FontSystem, Metrics, Shaping,
    SwashCache, Weight,
};
use tiny_skia::{
    FillRule, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint,
    Point, Rect, SpreadMode, Stroke, StrokeDash, Transform,
};

use edel::i18n::tr;
use edel::presets::{Edge, Size, Style};
use edel::tokens::{Colour, Tokens};

use crate::popup;
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
    /// How tall the panel is (M5.31c), and whether a bar floats (M5.31c):
    /// a floating bar is drawn as a dock's card, with no fillets.
    pub size: Size,
    pub floating: bool,
    /// Whether the fillets are drawn (not on the Lite tier).
    pub fillets: bool,
    /// What each widget shows, in the row's order.
    pub shown: Vec<String>,
    /// Whether the panel is being edited (M5.31b): each widget gets a tile
    /// as wide as its title needs, and each empty group a place to drop
    /// into. Part 2b adds what is dragged and where it would land.
    pub editing: bool,
    /// The widget dragged off this panel (M5.31b), drawn at 35 percent
    /// strength with its tile; `None` when nothing is dragged.
    pub lifted: Option<&'static str>,
    /// Where a dragged widget would land here, logical pixels from the
    /// panel's left: a caret. `None` when none would.
    pub caret: Option<f32>,
}

/// While a panel is edited, the least room a widget's tile has beyond its
/// title, logical pixels on each side (M5.31b).
pub const EDIT_PAD: f32 = 8.0;
/// The width of an empty group's place while a panel is edited, logical
/// pixels (M5.31b).
pub const EMPTY_WIDTH: f32 = 40.0;
/// The room between two tiles while a panel is edited, logical pixels: half
/// of it at each tile's side (M5.31b).
const TILE_ROOM: f32 = 4.0;
/// A widget narrower than this, logical pixels, shows its title in edit
/// mode instead of its drawing, as an empty widget does: the separator's
/// line (13 px, its room included) and an empty tray (M5.31b). A line
/// that narrow is no picture to show.
pub const TITLE_BELOW: f32 = 16.0;
/// How strong a dragged widget is drawn, as a share of full (M5.31b).
pub const LIFTED: f32 = 0.35;

/// Where a panel's widgets lie and, while it is edited, where its empty
/// groups do: each widget's left edge and width in logical pixels, in the
/// row's order, and for each group (start, centre, end) its place when it
/// holds no widget, `None` otherwise (M5.31b).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Places {
    pub widgets: Vec<(f32, f32)>,
    pub empty: [Option<(f32, f32)>; 3],
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
    tokens.radius_menu
}

/// How tall the strip for the fillets is on a panel of `style`: a dock
/// has none, nor a floating bar, which is a card of its own.
pub fn strip(style: Style, floating: bool, tokens: &Tokens) -> u32 {
    match (style, floating) {
        (Style::Bar, false) => fillet_height(tokens),
        (Style::Bar, true) | (Style::Dock, _) => 0,
    }
}

/// The panel's top row in its buffer, in logical pixels; the strip lies
/// on the side towards the screen's middle.
pub fn panel_top(edge: Edge, style: Style, floating: bool, tokens: &Tokens) -> u32 {
    match edge {
        Edge::Bottom => strip(style, floating, tokens),
        Edge::Top => 0,
    }
}

/// The room inside a dock's card, or a floating bar's, at each end, in
/// logical pixels.
pub const DOCK_PAD: f32 = 8.0;
/// How much taller a dock is than the bar of its size, for bigger icons
/// (M5.31c): a medium dock is 60 px high, as it was before sizes.
pub const DOCK_EXTRA: u32 = 20;
/// The corners' radius of a dock or a floating bar, in logical pixels:
/// rounder than a bar's square edge, for bigger icons.
const DOCK_RADIUS: f32 = 18.0;

/// A panel's own height, without the fillets' strip, in logical pixels:
/// the bar's height for its size, a dock's with its extra room (M5.31c).
pub fn height(style: Style, size: Size, tokens: &Tokens) -> u32 {
    let bar = match size {
        Size::Small => tokens.panel_small_height,
        Size::Medium => tokens.panel_height,
        Size::Large => tokens.panel_large_height,
    };
    match style {
        Style::Bar => bar,
        Style::Dock => bar + DOCK_EXTRA,
    }
}

/// The fonts and glyph cache text is drawn with, loaded once.
pub struct Text {
    fonts: FontSystem,
    glyphs: SwashCache,
    /// The interface font's family, from the tokens.
    family: String,
}

/// How heavy a line of text is and whether its figures all have one
/// width (Inter's `tnum`), as the mockups set the clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub weight: u16,
    pub tabular: bool,
}

impl Face {
    pub const REGULAR: Face = Face {
        weight: 400,
        tabular: false,
    };
    /// Window buttons' titles.
    pub const MEDIUM: Face = Face {
        weight: 500,
        tabular: false,
    };
    /// Numbers that stand out: the clock's time, the workspaces'.
    pub const SEMIBOLD: Face = Face {
        weight: 600,
        tabular: false,
    };

    /// The same weight with tabular figures.
    pub const fn tabular(self) -> Face {
        Face {
            tabular: true,
            ..self
        }
    }
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
        self.line_in(text, size, Face::REGULAR)
    }

    /// [`Text::line`] in `face`.
    pub fn line_in(&mut self, text: &str, size: f32, face: Face) -> Line {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, size * 1.25));
        buffer.set_size(None, None);
        let mut attrs = Attrs::new()
            .family(Family::Name(&self.family))
            .weight(Weight(face.weight));
        if face.tabular {
            let mut features = FontFeatures::new();
            features.enable(FeatureTag::new(b"tnum"));
            attrs = attrs.font_features(features);
        }
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
        self.fit_in(text, size, room, Face::REGULAR)
    }

    /// [`Text::fit`] in `face`.
    pub fn fit_in(&mut self, text: &str, size: f32, room: f32, face: Face) -> Line {
        let whole = self.line_in(text, size, face);
        if whole.width <= room {
            return whole;
        }
        // The most characters that fit before the ellipsis, by halving.
        let ends: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let (mut fits, mut over) = (0, ends.len());
        let mut best = self.line_in("\u{2026}", size, face);
        while fits + 1 < over {
            let mid = (fits + over) / 2;
            let cut = format!("{}\u{2026}", text[..ends[mid]].trim_end());
            let line = self.line_in(&cut, size, face);
            if line.width <= room {
                (fits, best) = (mid, line);
            } else {
                over = mid;
            }
        }
        if best.width > room {
            return self.line_in("", size, face);
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

/// A battery `px` pixels across, its top left corner at `at` (M5.9a):
/// the outline (`battery.svg`) with its charge, `percent` of the bar inside
/// it (`battery-level.svg`, cut at that share of its own width, so the
/// file alone decides where the bar lies), and when `charging` the bolt
/// (`battery-bolt.svg`) over it with a clear edge round it, so it reads
/// on a full battery too (`charge` is the percent and whether it charges).
/// All in `colour`; `halo` is that edge's width in pixels.
pub fn battery(
    pixmap: &mut Pixmap,
    px: f32,
    at: (f32, f32),
    charge: (u32, bool),
    colour: Colour,
    halo: i32,
) {
    let (percent, charging) = charge;
    let side = px.round() as u32;
    let Some(mut layer) = Pixmap::new(side, side) else {
        return;
    };
    let plain = PixmapPaint::default();
    icon(&mut layer, "battery", px, 0.0, 0.0, colour);
    if let Some(mut level) = edel::icons::draw("battery-level", side, colour) {
        keep_share(&mut level, percent.min(100) as f32 / 100.0);
        layer.draw_pixmap(0, 0, level.as_ref(), &plain, Transform::identity(), None);
    }
    if charging {
        if let Some(bolt) = edel::icons::mask("battery-bolt", side) {
            let clear = PixmapPaint {
                blend_mode: tiny_skia::BlendMode::DestinationOut,
                ..PixmapPaint::default()
            };
            for dy in -halo..=halo {
                for dx in -halo..=halo {
                    layer.draw_pixmap(dx, dy, bolt.as_ref(), &clear, Transform::identity(), None);
                }
            }
        }
        icon(&mut layer, "battery-bolt", px, 0.0, 0.0, colour);
    }
    let (x, y) = (at.0.round() as i32, at.1.round() as i32);
    pixmap.draw_pixmap(x, y, layer.as_ref(), &plain, Transform::identity(), None);
}

/// Clears everything of `pixmap`'s shape to the right of `share` (0 to 1)
/// of its width, the shape's own, from its leftmost covered column to its
/// rightmost; some of a shape that has any is always kept, so a nearly
/// empty battery still shows a sliver.
fn keep_share(pixmap: &mut Pixmap, share: f32) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let covered = |data: &[u8], x: usize| (0..h).any(|y| data[(y * w + x) * 4 + 3] > 0);
    let (first, last) = {
        let data = pixmap.data();
        let Some(first) = (0..w).find(|&x| covered(data, x)) else {
            return;
        };
        let last = (0..w).rev().find(|&x| covered(data, x)).unwrap_or(first);
        (first, last)
    };
    let share = share.clamp(0.0, 1.0);
    let mut keep = ((last - first + 1) as f32 * share).round() as usize;
    if share > 0.0 {
        keep = keep.max(1);
    }
    let data = pixmap.data_mut();
    for y in 0..h {
        for x in (first + keep).min(w)..w {
            data[(y * w + x) * 4..][..4].fill(0);
        }
    }
}

/// Strokes the rounded rectangle's edge, inside it, `width` pixels wide:
/// the hairline round a tile or a card.
pub fn outline(pixmap: &mut Pixmap, rect: (f32, f32, f32, f32), r: f32, width: f32, c: Colour) {
    let (x, y, w, h) = rect;
    let half = width / 2.0;
    let Some(path) = rounded(
        x + half,
        y + half,
        w - width,
        h - width,
        (r - half).max(0.0),
    ) else {
        return;
    };
    let stroke = tiny_skia::Stroke {
        width,
        ..tiny_skia::Stroke::default()
    };
    pixmap.stroke_path(&path, &paint_of(c), &stroke, Transform::identity(), None);
}

/// The accent, faint: under a chosen row or a switched-on button.
pub fn lit(tokens: &Tokens) -> Colour {
    Colour {
        a: 0.18,
        ..tokens.accent
    }
}

/// Fills the rectangle with corners of radius `r` with a colour that goes
/// from `top` at its top edge to `bottom` at its bottom one (M5.9d: a
/// cover without a picture).
pub fn fill_vertical(
    pixmap: &mut Pixmap,
    (x, y, w, h): (f32, f32, f32, f32),
    r: f32,
    top: Colour,
    bottom: Colour,
) {
    let Some(path) = rounded(x, y, w, h, r) else {
        return;
    };
    let stops = vec![
        GradientStop::new(0.0, colour(top)),
        GradientStop::new(1.0, colour(bottom)),
    ];
    let start = Point::from_xy(x, y);
    let end = Point::from_xy(x, y + h);
    let Some(shader) =
        LinearGradient::new(start, end, stops, SpreadMode::Pad, Transform::identity())
    else {
        return;
    };
    let paint = Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    };
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// Draws `picture` (a square) into the rectangle `(x, y, w, h)` of device
/// pixels, cut to a circle that fills it (M5.9d: a cover). The picture is
/// scaled on its own layer, which is masked to the circle and then drawn.
pub fn circle_picture(pixmap: &mut Pixmap, picture: &Pixmap, (x, y, w, h): (f32, f32, f32, f32)) {
    let (side_w, side_h) = (w.round() as u32, h.round() as u32);
    let Some(mut layer) = Pixmap::new(side_w, side_h) else {
        return;
    };
    let scale = Transform::from_scale(
        side_w as f32 / picture.width() as f32,
        side_h as f32 / picture.height() as f32,
    );
    let smooth = PixmapPaint {
        quality: tiny_skia::FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    layer.draw_pixmap(0, 0, picture.as_ref(), &smooth, scale, None);
    let Some(circle) = rounded(0.0, 0.0, w, h, w.min(h) / 2.0) else {
        return;
    };
    let Some(mut mask) = Mask::new(side_w, side_h) else {
        return;
    };
    mask.fill_path(&circle, FillRule::Winding, true, Transform::identity());
    layer.apply_mask(&mask);
    pixmap.draw_pixmap(
        x.round() as i32,
        y.round() as i32,
        layer.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
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

/// How far a menu's shadow reaches past its card on every side, logical
/// pixels: nothing on the Lite tier, which keeps surfaces flat (ADR-002,
/// M5.5e).
pub fn shadow_room(tokens: &Tokens, flat: bool) -> u32 {
    if flat {
        0
    } else {
        tokens.shadow_blur + tokens.shadow_offset
    }
}

/// The shadow of a card `w` by `h` pixels with corners `r`, at `room`
/// pixels in from each side of a pixmap that much bigger, at scale `s`
/// (M5.5e): two layers cast by one light above, a close sharp one and a
/// far soft one, in the tokens' shadow colour; drawn once per size.
pub fn shadow(w: u32, h: u32, room: u32, r: f32, tokens: &Tokens, s: f32) -> Option<Pixmap> {
    let (pw, ph) = (w + 2 * room, h + 2 * room);
    let mut cover = vec![0.0f32; (pw * ph) as usize];
    let (blur, fall) = (
        tokens.shadow_blur as f32 * s,
        tokens.shadow_offset as f32 * s,
    );
    // (how far down, how soft, how strong)
    for (down, soft, strength) in [(fall / 3.0, blur / 4.0, 0.55), (fall, blur, 0.45)] {
        let mut mask = Pixmap::new(pw, ph)?;
        fill(
            &mut mask,
            room as f32,
            room as f32 + down,
            w as f32,
            h as f32,
            r,
            Colour {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        );
        let mut layer: Vec<f32> = mask
            .data()
            .chunks_exact(4)
            .map(|p| f32::from(p[3]) / 255.0)
            .collect();
        // Three box blurs, each a third as wide, come close to a Gaussian.
        let radius = (soft / 3.0).round() as usize;
        for _ in 0..3 {
            box_blur(&mut layer, pw as usize, ph as usize, radius);
        }
        for (c, l) in cover.iter_mut().zip(layer) {
            *c = (*c + l * strength).min(1.0);
        }
    }
    let [cr, cg, cb, ca] = tokens.shadow.bytes();
    let mut out = Pixmap::new(pw, ph)?;
    for (pixel, c) in out.data_mut().chunks_exact_mut(4).zip(cover) {
        let a = c * f32::from(ca) / 255.0;
        let mul = |v: u8| (f32::from(v) * a).round() as u8;
        pixel.copy_from_slice(&[mul(cr), mul(cg), mul(cb), (a * 255.0).round() as u8]);
    }
    Some(out)
}

/// Blurs `values`, a `w` by `h` grid, by averaging each with its
/// neighbours up to `radius` away, across and then down.
fn box_blur(values: &mut [f32], w: usize, h: usize, radius: usize) {
    if radius == 0 {
        return;
    }
    let mut line = Vec::new();
    let mut pass =
        |values: &mut [f32], len: usize, count: usize, at: &dyn Fn(usize, usize) -> usize| {
            let span = (2 * radius + 1) as f32;
            for i in 0..count {
                line.clear();
                line.extend((0..len).map(|j| values[at(i, j)]));
                let mut sum: f32 = line.iter().take(radius + 1).sum();
                for j in 0..len {
                    values[at(i, j)] = sum / span;
                    if j + radius + 1 < len {
                        sum += line[j + radius + 1];
                    }
                    if j >= radius {
                        sum -= line[j - radius];
                    }
                }
            }
        };
    pass(values, w, h, &|row, col| row * w + col);
    pass(values, h, w, &|col, row| row * w + col);
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

/// The width a widget takes while its panel is edited, device pixels: its
/// natural width, or its title with `EDIT_PAD` at each side where that is
/// wider (M5.31b). Drawing and `natural_width` both measure with it.
pub fn edit_width(canvas: &mut Canvas, widget: &Widget, natural: f32) -> f32 {
    let s = canvas.scale;
    let size = canvas.tokens.panel_text_small_size as f32 * s;
    let title = canvas
        .text
        .as_mut()
        .map_or(0.0, |text| text.line(tr(widget.title), size).width);
    natural.max(title + 2.0 * EDIT_PAD * s)
}

/// One widget's place while its panel is edited: its left edge, its width
/// and its natural width, in device pixels, and the bar's top and height.
#[derive(Clone, Copy)]
struct EditPlace {
    x: f32,
    width: f32,
    natural: f32,
    bar: (f32, f32),
}

/// Draws a widget in edit mode: its tile, then its drawing centred in it,
/// or its title, dim and centred, where it is narrower than `TITLE_BELOW`.
fn edit_widget(canvas: &mut Canvas, widget: &Widget, showing: &str, place: EditPlace) {
    let (tokens, s) = (canvas.tokens, canvas.scale);
    edit_tile(
        canvas.pixmap,
        tokens,
        (place.x, place.width),
        place.bar,
        s,
        false,
    );
    if place.natural < TITLE_BELOW * s {
        let size = tokens.panel_text_small_size as f32 * s;
        if let Some(text) = canvas.text.as_deref_mut() {
            let mut line = text.line(tr(widget.title), size);
            let at = place.x + ((place.width - line.width) / 2.0).round();
            let y = popup::middle(place.bar.0 / s, place.bar.1 / s, size, s);
            text.draw(canvas.pixmap, &mut line, at, y, popup::dim(tokens));
        }
    } else {
        let at = place.x + ((place.width - place.natural) / 2.0).round();
        (widget.draw)(canvas, showing, at);
    }
}

/// `edit_widget` for the widget dragged off its panel: drawn on a layer of
/// its own, which lies over the bar at `LIFTED` strength.
fn lifted_widget(canvas: &mut Canvas, widget: &Widget, showing: &str, place: EditPlace) {
    let (w, h) = (canvas.pixmap.width(), canvas.pixmap.height());
    let Some(mut layer) = Pixmap::new(w, h) else {
        return;
    };
    {
        let mut inner = Canvas {
            pixmap: &mut layer,
            tokens: canvas.tokens,
            text: canvas.text.as_deref_mut(),
            icons: canvas.icons.as_deref_mut(),
            scale: canvas.scale,
            top: canvas.top,
            height: canvas.height,
            dock: canvas.dock,
            along_top: canvas.along_top,
        };
        edit_widget(&mut inner, widget, showing, place);
    }
    let paint = PixmapPaint {
        opacity: LIFTED,
        ..PixmapPaint::default()
    };
    canvas
        .pixmap
        .draw_pixmap(0, 0, layer.as_ref(), &paint, Transform::identity(), None);
}

/// The caret that shows where a dragged widget would land (M5.31b): a 2 px
/// line in the accent, a button high, centred in the bar, its ends round.
/// `at` is logical pixels along the panel; `bar` is the bar's top and
/// height in device pixels.
fn caret_line(pixmap: &mut Pixmap, tokens: &Tokens, at: f32, bar: (f32, f32), s: f32) {
    let ph = tokens.panel_control as f32 * s;
    let y = bar.0 + ((bar.1 - ph) / 2.0).round();
    let w = 2.0 * s;
    fill(
        pixmap,
        (at * s - s).round(),
        y,
        w,
        ph,
        w / 2.0,
        tokens.accent,
    );
}

/// The tile of a widget or an empty group while its panel is edited, `x`
/// and `w` device pixels across the bar, `bar` its top and height in device
/// pixels: a rounded tile `panel_control` high, centred in the bar, filled
/// with the accent at 8 percent and outlined with it at 45 percent, or for
/// an empty group dashed and unfilled, where a widget can be dropped.
fn edit_tile(
    pixmap: &mut Pixmap,
    tokens: &Tokens,
    (x, w): (f32, f32),
    (top, height): (f32, f32),
    s: f32,
    empty: bool,
) {
    let ph = tokens.panel_control as f32 * s;
    let y = top + ((height - ph) / 2.0).round();
    let (x, w) = (x + TILE_ROOM / 2.0 * s, w - TILE_ROOM * s);
    let r = tokens.radius_control as f32 * s;
    let ink = Colour {
        a: 0.45,
        ..tokens.accent
    };
    if empty {
        let Some(dash) = StrokeDash::new(vec![3.0 * s, 3.0 * s], 0.0) else {
            return;
        };
        let stroke = Stroke {
            width: 1.0,
            dash: Some(dash),
            ..Stroke::default()
        };
        if let Some(path) = rounded(x, y, w, ph, r) {
            pixmap.stroke_path(&path, &paint_of(ink), &stroke, Transform::identity(), None);
        }
    } else {
        let fill_colour = Colour {
            a: 0.08,
            ..tokens.accent
        };
        fill(pixmap, x, y, w, ph, r, fill_colour);
        outline(pixmap, (x, y, w, ph), r, 1.0, ink);
    }
}

/// Draws `look` into `pixmap`, which is `look.width` by `look.height`,
/// with `row`'s widgets showing `look.shown`; returns where each widget
/// lies, start to end, and where the empty groups are while editing. A
/// dock or a floating bar is drawn as a card (M5.31c).
pub fn paint(
    pixmap: &mut Pixmap,
    look: &Look,
    tokens: &Tokens,
    text: Option<&mut Text>,
    icons: Option<&mut edel::app_icons::Icons>,
    row: &Row,
) -> Places {
    let s = look.scale.max(1) as f32;
    let (w, h) = (look.width as f32, look.height as f32);
    let strip = strip(look.style, look.floating, tokens) as f32 * s;
    let panel_h = h - strip;
    let top = panel_top(look.edge, look.style, look.floating, tokens) as f32 * s;
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let panel = paint_of(tokens.panel);
    // A dock and a floating bar are cards; only a dock is `dock` for its
    // widgets, which may draw bigger there.
    let dock = look.style == Style::Dock;
    let card = dock || look.floating;
    if card {
        // A card rounded all round, with the tokens' hairline inside its
        // edge, so it reads on any background.
        let r = DOCK_RADIUS * s;
        fill(pixmap, 0.0, top, w, panel_h, r, tokens.edge);
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
        // A hairline along the edge that faces the windows, as the
        // mockups draw it, one screen pixel at any scale.
        let at = match look.edge {
            Edge::Bottom => top,
            Edge::Top => top + panel_h - 1.0,
        };
        if let Some(rect) = Rect::from_xywh(0.0, at, w, 1.0) {
            pixmap.fill_rect(rect, &paint_of(tokens.edge), Transform::identity(), None);
        }
    }
    if look.fillets && !card {
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
        along_top: look.edge == Edge::Top,
    };
    let editing = look.editing;
    let bar = (canvas.top, canvas.height);
    let mut shown = look.shown.iter().map(String::as_str);
    // Each group's widgets with what they show, their widths and their
    // natural widths; while editing a widget is at least as wide as its
    // title, with `EDIT_PAD` at each side.
    let mut measure = |group: &[&'static Widget]| {
        group
            .iter()
            .map(|widget| {
                let showing = shown.next().unwrap_or("");
                let natural = (widget.width)(&mut canvas, showing);
                let width = match editing {
                    true => edit_width(&mut canvas, widget, natural),
                    false => natural,
                };
                (*widget, showing, width, natural)
            })
            .collect::<Vec<_>>()
    };
    let start = measure(&row.start);
    let centre = measure(&row.centre);
    let end = measure(&row.end);
    let total = |group: &[(&Widget, &str, f32, f32)]| group.iter().map(|g| g.2).sum::<f32>();
    // An empty group takes its place while editing, and nothing otherwise.
    let slot = |group: &[(&Widget, &str, f32, f32)]| match group.is_empty() && editing {
        true => EMPTY_WIDTH * s,
        false => total(group),
    };
    let pad = if card { (DOCK_PAD * s).round() } else { 0.0 };
    let mut places = Places::default();
    for (k, (group, from)) in [
        (&start, pad),
        (&centre, ((w - slot(&centre)) / 2.0).round()),
        (&end, w - pad - slot(&end)),
    ]
    .into_iter()
    .enumerate()
    {
        if group.is_empty() {
            if editing {
                edit_tile(canvas.pixmap, tokens, (from, EMPTY_WIDTH * s), bar, s, true);
                places.empty[k] = Some((from / s, EMPTY_WIDTH));
            }
            continue;
        }
        let mut x = from;
        for &(widget, showing, width, natural) in group {
            let place = EditPlace {
                x,
                width,
                natural,
                bar,
            };
            if !editing {
                // A widget as wide as its title is drawn centred in it.
                let at = x + ((width - natural) / 2.0).round();
                (widget.draw)(&mut canvas, showing, at);
            } else if look.lifted == Some(widget.name) {
                lifted_widget(&mut canvas, widget, showing, place);
            } else {
                edit_widget(&mut canvas, widget, showing, place);
            }
            places.widgets.push((x / s, width / s));
            x += width;
        }
    }
    if let Some(at) = look.caret {
        caret_line(canvas.pixmap, tokens, at, bar, s);
    }
    places
}

/// How wide a dock holding `row`, showing `shown`, is in logical pixels:
/// its widgets side by side and the card's room at both ends. `size` and
/// `scale` are the dock's size (M5.31c) and the buffer's scale, given
/// together to keep the argument count down. While the panel is edited
/// (`editing`) each widget counts as its edit width and each empty group
/// as its place, so the dock holds the tiles (M5.31b).
pub fn natural_width(
    tokens: &Tokens,
    text: Option<&mut Text>,
    icons: Option<&mut edel::app_icons::Icons>,
    row: &Row,
    shown: &[String],
    (size, scale): (Size, u32),
    editing: bool,
) -> u32 {
    let s = scale.max(1) as f32;
    let dock_height = height(Style::Dock, size, tokens);
    let Some(mut pixmap) = Pixmap::new(1, (dock_height as f32 * s) as u32) else {
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
        along_top: false,
    };
    let mut widgets: f32 = row
        .all()
        .zip(shown)
        .map(|(widget, showing)| {
            let natural = (widget.width)(&mut canvas, showing);
            match editing {
                true => edit_width(&mut canvas, widget, natural),
                false => natural,
            }
        })
        .sum();
    if editing {
        let empty = [&row.start, &row.centre, &row.end]
            .iter()
            .filter(|group| group.is_empty())
            .count();
        widgets += empty as f32 * EMPTY_WIDTH * s;
    }
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
    use crate::widgets::{self, find, menu};

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
            size: Size::Medium,
            floating: false,
            fillets,
            shown: row.shows(&Live::default()),
            editing: false,
            lifted: None,
            caret: None,
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
        assert_eq!(pixel(&pixmap, 640, h - 2), panel);
        assert_ne!(
            pixel(&pixmap, 640, h - 1),
            panel,
            "the hairline facing the windows"
        );
        assert_eq!(pixel(&pixmap, 640, h)[3], 0, "the strip is clear");
        assert_eq!(pixel(&pixmap, 0, h), panel, "the left fillet's corner");
        assert_eq!(pixel(&pixmap, 1279, h), panel, "the right one's");
        assert_eq!(
            pixel(&pixmap, tokens.radius_menu - 1, pixmap.height() - 1)[3],
            0,
            "the curve leaves the corner's far side clear"
        );
    }

    #[test]
    fn a_floating_bar_is_a_card_with_no_strip_and_each_size_has_its_height() {
        let tokens = Tokens::built_in();
        assert_eq!(strip(Style::Bar, true, &tokens), 0);
        assert_eq!(panel_top(Edge::Bottom, Style::Bar, true, &tokens), 0);
        assert_eq!(strip(Style::Bar, false, &tokens), fillet_height(&tokens));
        assert_eq!(
            height(Style::Bar, Size::Small, &tokens),
            tokens.panel_small_height
        );
        assert_eq!(
            height(Style::Bar, Size::Large, &tokens),
            tokens.panel_large_height
        );
        assert_eq!(
            height(Style::Dock, Size::Small, &tokens),
            tokens.panel_small_height + DOCK_EXTRA
        );
        let row = classic();
        let look = Look {
            width: 640,
            height: height(Style::Bar, Size::Medium, &tokens),
            scale: 1,
            edge: Edge::Bottom,
            style: Style::Bar,
            size: Size::Medium,
            floating: true,
            fillets: true,
            shown: row.shows(&Live::default()),
            editing: false,
            lifted: None,
            caret: None,
        };
        let mut pixmap = Pixmap::new(look.width, look.height).unwrap();
        paint(&mut pixmap, &look, &tokens, None, None, &row);
        assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "its corners are round");
        assert_eq!(pixel(&pixmap, 320, 1), tokens.panel.bytes());
        assert_ne!(
            pixel(&pixmap, 320, 0),
            tokens.panel.bytes(),
            "the hairline round its edge"
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
        let width = natural_width(&tokens, None, None, &row, &shown, (Size::Medium, 1), false);
        let h = height(Style::Dock, Size::Medium, &tokens);
        let each = menu::logical_width(&tokens);
        assert_eq!(width, (2.0 * each + 2.0 * DOCK_PAD) as u32);
        let look = Look {
            width,
            height: h,
            scale: 1,
            edge: Edge::Bottom,
            style: Style::Dock,
            size: Size::Medium,
            floating: false,
            fillets: true,
            shown,
            editing: false,
            lifted: None,
            caret: None,
        };
        let mut pixmap = Pixmap::new(look.width, look.height).unwrap();
        let places = paint(&mut pixmap, &look, &tokens, None, None, &row).widgets;
        assert_eq!(places, [(DOCK_PAD, each), (DOCK_PAD + each, each)]);
        assert_eq!(pixel(&pixmap, width / 2, 1), tokens.panel.bytes());
        assert_ne!(
            pixel(&pixmap, width / 2, 0),
            tokens.panel.bytes(),
            "a hairline runs along its edge"
        );
        assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "its corners are round");
        assert_eq!(pixel(&pixmap, width - 1, h - 1)[3], 0);
        // No fillets and no strip: its buffer is the card.
        assert_eq!(strip(Style::Dock, false, &tokens), 0);
        assert_eq!(panel_top(Edge::Bottom, Style::Dock, false, &tokens), 0);
    }

    #[test]
    fn menus_cast_a_shadow_from_above_except_on_lite() {
        let tokens = Tokens::built_in();
        assert_eq!(shadow_room(&tokens, true), 0, "Lite keeps menus flat");
        let room = shadow_room(&tokens, false);
        assert_eq!(room, tokens.shadow_blur + tokens.shadow_offset);
        for s in [1u32, 2] {
            let (w, h, room) = (200 * s, 100 * s, room * s);
            let r = tokens.radius_menu as f32 * s as f32;
            let pixmap = shadow(w, h, room, r, &tokens, s as f32).unwrap();
            assert_eq!(pixmap.width(), w + 2 * room);
            let alpha = |x: u32, y: u32| pixel(&pixmap, x, y)[3];
            let mid = room + w / 2;
            // Darker just below the card than just above it, and gone
            // at the pixmap's edges.
            let below = alpha(mid, room + h + 2 * s);
            let above = alpha(mid, room - 2 * s);
            assert!(below > above, "below {below}, above {above}");
            assert!(below > 0);
            assert_eq!(alpha(0, 0), 0);
            assert_eq!(alpha(mid, pixmap.height() - 1), 0);
            // In the shadow's colour, never deeper than it.
            assert!(below <= tokens.shadow.bytes()[3]);
        }
    }

    #[test]
    fn lite_draws_no_fillets() {
        let (pixmap, tokens) = drawn(Edge::Bottom, false, &classic());
        assert_eq!(pixel(&pixmap, 0, fillet_height(&tokens) - 1)[3], 0);
    }

    #[test]
    fn widgets_sit_at_the_start_the_centre_and_the_end() {
        // Whether the menu icon's first square has its middle at the
        // widget's left edge x plus the room before its tile, the
        // icon's inset in the tile and half a square.
        let icon_at = |pixmap: &Pixmap, tokens: &Tokens, x: u32| {
            let strip = fillet_height(tokens);
            let (tile_w, tile_h) = (
                (tokens.panel_control as f32 * 1.2).round(),
                tokens.panel_control as f32,
            );
            let glyph = tokens.panel_glyph as f32;
            let across = 6.0 + ((tile_w - glyph) / 2.0).round() + glyph * 0.2;
            let down = ((tokens.panel_height as f32 - tile_h) / 2.0).round()
                + ((tile_h - glyph) / 2.0).round()
                + glyph * 0.2;
            pixel(pixmap, x + across as u32, strip + down as u32) == tokens.panel_text.bytes()
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
        let w = widgets::menu::logical_width(&tokens) as u32;
        assert!(!icon_at(&pixmap, &tokens, 0));
        assert!(icon_at(&pixmap, &tokens, 640 - w / 2));
        assert!(icon_at(&pixmap, &tokens, 1280 - w));
    }

    // The panel as the mockups draw it (M5.29), drawn with fonts and a
    // few app icons from a theme of its own, for the tests below and to
    // look at: `EDEL_PANEL_PNG=DIR cargo test -p edel-shell-ui panel_png`
    // writes each panel as a PNG in DIR, light and dark.

    use crate::widgets::{Pin, Task};
    use edel::tokens::Scheme;

    const FOLDER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><path d="M5 12a4 4 0 0 1 4-4h10l5 5h15a4 4 0 0 1 4 4v21a4 4 0 0 1-4 4H9a4 4 0 0 1-4-4z" fill="#5b95ee"/><path d="M5 18h38v18a4 4 0 0 1-4 4H9a4 4 0 0 1-4-4z" fill="#3f7be0"/></svg>"##;
    const TERMINAL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><rect x="4" y="6" width="40" height="36" rx="8" fill="#23262d"/><path d="M13 18l8 6-8 6" fill="none" stroke="#eceef1" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/><path d="M25 32h10" stroke="#eceef1" stroke-width="3" stroke-linecap="round"/></svg>"##;
    const GLOBE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><circle cx="24" cy="24" r="19" fill="#4a9be8"/><path d="M5 24h38M24 5c-9 9-9 29 0 38M24 5c9 9 9 29 0 38" fill="none" stroke="#fff" stroke-width="2"/></svg>"##;

    /// A theme with the three icons, in a directory of its own.
    fn theme() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-panel-icons-{}", std::process::id()));
        let apps = dir.join("icons/hicolor/scalable/apps");
        std::fs::create_dir_all(&apps).unwrap();
        for (name, svg) in [("folder", FOLDER), ("terminal", TERMINAL), ("web", GLOBE)] {
            std::fs::write(apps.join(format!("{name}.svg")), svg).unwrap();
        }
        dir
    }

    fn task(title: &str, app: &str, focused: bool, minimized: bool) -> Task {
        Task {
            title: title.into(),
            app_id: app.into(),
            focused,
            minimized,
        }
    }

    /// The windows and apps of the mockups: Pictures focused, Terminal and
    /// Web running.
    fn live(workspaces: usize, tiling: bool) -> Live {
        let pin = |id: &str, name: &str, icon: &str| Pin {
            id: id.into(),
            name: name.into(),
            icon: icon.into(),
        };
        let installed = vec![
            pin("files", "Pictures", "folder"),
            pin("term", "Terminal", "terminal"),
            pin("web", "Web", "web"),
        ];
        Live {
            workspaces: (1..=workspaces).map(|n| (n.to_string(), n == 1)).collect(),
            windows: vec![
                task("Pictures", "files", true, false),
                task("Terminal", "term", false, false),
                task("Web", "web", false, false),
            ],
            policy: if tiling { "tiling" } else { "floating" }.into(),
            pinned: installed.clone(),
            installed,
            ..Live::default()
        }
    }

    /// Where a panel's widgets lie, by name: left edge and width.
    type Places = Vec<(&'static str, (f32, f32))>;

    /// Panel number `index` of preset `name` in `scheme`, `width` logical
    /// pixels wide at `scale`, with `live` showing and the clock at
    /// 14:05 on Sat 3 Oct, on a wallpaper-coloured screen; also where its
    /// widgets lie, in logical pixels, by name.
    fn preview(
        name: &str,
        index: usize,
        scheme: Scheme,
        width: u32,
        scale: u32,
        live: &Live,
    ) -> (Pixmap, Places, Tokens) {
        let tokens = Tokens::built_in_scheme(scheme);
        let (preset, _) = edel::presets::named(Some(name));
        let spec = &preset.panels[index];
        let pick = |names: &[String]| {
            names
                .iter()
                .filter_map(|n| find(n))
                .collect::<Vec<&'static Widget>>()
        };
        let row = Row {
            start: pick(&spec.start),
            centre: pick(&spec.centre),
            end: pick(&spec.end),
        };
        let shown: Vec<String> = row
            .all()
            .map(|w| {
                if w.name == "clock" {
                    "14:05\nSat 3 Oct".to_string()
                } else {
                    (w.shows)(live)
                }
            })
            .collect();
        let dock = spec.style == Style::Dock;
        let mut text = Text::load(&tokens.font);
        let mut icons = edel::app_icons::Icons::new(vec![theme()]);
        let natural = if dock {
            natural_width(
                &tokens,
                Some(&mut text),
                Some(&mut icons),
                &row,
                &shown,
                (spec.size, scale),
                false,
            )
        } else {
            width
        };
        let strip = strip(spec.style, spec.floating, &tokens);
        let look = Look {
            width: natural * scale,
            height: (height(spec.style, spec.size, &tokens) + strip) * scale,
            scale,
            edge: spec.edge,
            style: spec.style,
            size: spec.size,
            floating: spec.floating,
            fillets: true,
            shown,
            editing: false,
            lifted: None,
            caret: None,
        };
        let mut panel = Pixmap::new(look.width, look.height).unwrap();
        let places = paint(
            &mut panel,
            &look,
            &tokens,
            Some(&mut text),
            Some(&mut icons),
            &row,
        );
        // Over a screen's colour, as it would lie.
        let mut screen = Pixmap::new(look.width, look.height).unwrap();
        screen.fill(colour(mix(tokens.background, tokens.panel_text, 0.0)));
        screen.draw_pixmap(
            0,
            0,
            panel.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        let named = row.all().map(|w| w.name).zip(places.widgets).collect();
        (screen, named, tokens)
    }

    /// Classic's bottom panel while it is edited, the clock lifted off its
    /// end and the caret where it would land at the start (M5.31b): the
    /// caret is the accent where it stands, the lifted clock is drawn
    /// faded, and the separator's title shows in its tile, its line being
    /// under `TITLE_BELOW`. With `EDEL_PANEL_PNG` set, writes it.
    #[test]
    fn an_edited_panel_shows_the_lifted_widget_and_the_caret() {
        let tokens = Tokens::built_in();
        let (preset, _) = edel::presets::named(Some("classic"));
        let spec = &preset.panels[0];
        let pick = |names: &[String]| {
            names
                .iter()
                .filter_map(|n| find(n))
                .collect::<Vec<&'static Widget>>()
        };
        let row = Row {
            start: pick(&spec.start),
            centre: pick(&spec.centre),
            end: pick(&spec.end),
        };
        let live = Live::default();
        let shown: Vec<String> = row
            .all()
            .map(|w| {
                if w.name == "clock" {
                    "14:05\nSat 3 Oct".to_string()
                } else {
                    (w.shows)(&live)
                }
            })
            .collect();
        let mut text = Text::load(&tokens.font);
        let mut icons = edel::app_icons::Icons::new(vec![theme()]);
        let look = Look {
            width: 1280,
            height: height(spec.style, spec.size, &tokens)
                + strip(spec.style, spec.floating, &tokens),
            scale: 1,
            edge: spec.edge,
            style: spec.style,
            size: spec.size,
            floating: spec.floating,
            fillets: false,
            shown,
            editing: true,
            lifted: Some("clock"),
            caret: Some(10.0),
        };
        let mut panel = Pixmap::new(look.width, look.height).unwrap();
        let places = paint(
            &mut panel,
            &look,
            &tokens,
            Some(&mut text),
            Some(&mut icons),
            &row,
        );
        let bar_mid = (panel_top(spec.edge, spec.style, spec.floating, &tokens)
            + height(spec.style, spec.size, &tokens) / 2) as i32;
        let (x, y) = (10, bar_mid as u32);
        assert_eq!(pixel(&panel, x, y), tokens.accent.bytes(), "the caret");
        assert_eq!(places.widgets.len(), row.all().count(), "every widget lies");
        // Over the screen's colour, as it would lie, for the picture.
        let mut screen = Pixmap::new(look.width, look.height).unwrap();
        screen.fill(colour(tokens.background));
        screen.draw_pixmap(
            0,
            0,
            panel.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        write_png("panel-editing", &screen);
    }

    /// With `EDEL_PANEL_PNG` set, writes `pixmap` as `name`.png there.
    fn write_png(name: &str, pixmap: &Pixmap) {
        if let Some(dir) = std::env::var_os("EDEL_PANEL_PNG") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            pixmap.save_png(dir.join(format!("{name}.png"))).unwrap();
        }
    }

    /// Whether a pixel differs from `base` by more than a little.
    fn inked(p: [u8; 4], base: [u8; 4]) -> bool {
        (0..3)
            .map(|k| (i32::from(p[k]) - i32::from(base[k])).abs())
            .sum::<i32>()
            > 60
    }

    #[test]
    fn panel_png() {
        let mut crowded = live(4, false);
        crowded
            .windows
            .push(task("Mystery", "nothing-has-this", false, true));
        for (scheme, mode) in [(Scheme::Light, "light"), (Scheme::Dark, "dark")] {
            for (name, index, width, tiling) in [
                ("classic", 0, 1280, false),
                ("classic", 0, 360, true),
                ("mac-like", 0, 1280, false),
                ("mac-like", 1, 1280, false),
                ("windows-like", 0, 1280, false),
                ("hive", 0, 1280, true),
            ] {
                let (pixmap, _, _) = preview(name, index, scheme, width, 1, &live(4, tiling));
                write_png(&format!("{name}-{index}-{width}-{mode}"), &pixmap);
            }
            let (pixmap, _, _) = preview("classic", 0, scheme, 1280, 1, &crowded);
            write_png(&format!("classic-crowded-{mode}"), &pixmap);
            let (pixmap, _, _) = preview("classic", 0, scheme, 1280, 2, &live(4, false));
            write_png(&format!("classic-x2-{mode}"), &pixmap);
        }
    }

    #[test]
    fn the_clock_draws_the_time_over_a_smaller_dimmer_date() {
        let mut text = Text::load(&Tokens::built_in().font);
        if text.line("A", 13.0).width == 0.0 {
            return; // no fonts on this machine
        }
        let (pixmap, places, tokens) =
            preview("classic", 0, Scheme::Light, 1280, 1, &live(4, false));
        let (x, w) = places.iter().find(|(n, _)| *n == "clock").unwrap().1;
        let strip = fillet_height(&tokens);
        let base = tokens.panel.bytes();
        // Each row of the panel inside the clock, under its edge's hairline: how dark its darkest
        // pixel is, or none.
        let rows: Vec<Option<u32>> = (strip + 1..strip + tokens.panel_height - 1)
            .map(|y| {
                (x as u32..(x + w) as u32)
                    .map(|px| pixel(&pixmap, px, y))
                    .filter(|p| inked(*p, base))
                    .map(|p| 255 - u32::from(p[0]))
                    .max()
            })
            .collect();
        // Two bands of ink with clear rows between.
        let mut bands: Vec<(usize, usize, u32)> = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            match (row, bands.last_mut()) {
                (Some(d), Some(b)) if b.1 + 1 == y => (b.1, b.2) = (y, b.2.max(*d)),
                (Some(d), _) => bands.push((y, y, *d)),
                _ => {}
            }
        }
        assert_eq!(bands.len(), 2, "the time and the date: {rows:?}");
        let (time, date) = (bands[0], bands[1]);
        assert!(time.1 < date.0, "the date lies below the time");
        assert!(
            date.2 < time.2,
            "the date is dimmer: {} against {}",
            date.2,
            time.2
        );
        assert!(
            date.1 - date.0 <= time.1 - time.0 + 1,
            "and no taller than the time"
        );
    }

    #[test]
    fn window_buttons_hug_their_titles_and_an_app_without_an_icon_gets_the_generic_one() {
        let mut text = Text::load(&Tokens::built_in().font);
        if text.line("A", 13.0).width == 0.0 {
            return; // no fonts on this machine
        }
        let (_, places, _) = preview("classic", 0, Scheme::Light, 1280, 1, &live(4, false));
        let (_, list) = places.iter().find(|(n, _)| *n == "windows").unwrap().1;
        // Three short titles take far less than the old 180 px each.
        assert!(list > 150.0 && list < 3.0 * 130.0, "the list is {list} px");
        // A window of an app with no icon: the generic one is drawn in its
        // button's icon place.
        let mut mystery = live(4, false);
        mystery.windows = vec![task("Mystery", "nothing-has-this", false, false)];
        let (without, places, tokens) = preview("classic", 0, Scheme::Light, 1280, 1, &mystery);
        let (left, _) = places.iter().find(|(n, _)| *n == "windows").unwrap().1;
        let strip = fillet_height(&tokens);
        let base = tokens.panel.bytes();
        let (icon, size) = (tokens.panel_icon, tokens.panel_icon);
        let first = left as u32 + 2 + 7;
        let top = strip + (tokens.panel_height - icon) / 2;
        let ink = (top..top + size)
            .flat_map(|y| (first..first + icon).map(move |x| (x, y)))
            .filter(|&(x, y)| inked(pixel(&without, x, y), base))
            .count();
        assert!(ink > 40, "the generic icon is drawn: {ink} px of ink");
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
