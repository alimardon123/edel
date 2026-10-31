//! The look of every edit mode (M5.10c6c3), plain data and drawing, tested
//! without a display: the lock screen's edit mode now, a panel's drawer and
//! any later editor (the greeter's, say) with no copy (ADR-010). An editor
//! picks its `Style` and gets the same parts: the lock screen over its
//! frosted glass, the desktop over the panel's colours. Every colour comes
//! from the tokens and every size from the consts here; nothing is a bare
//! literal in drawing code.
//!
//! Coordinates: the functions take rectangles in the drawing surface's
//! logical pixels and the scale `s` (device pixels per logical pixel); a
//! caller that draws a screen-sized picture at a smaller size maps its
//! rectangles first. Shadows are the caller's where the surface has room
//! for them (`lock::lift`), because a sheet drawn into a layer of its own
//! size has none: `sheet` draws only what lies inside its rectangle.

use edel::lock_screen::Guide;
use edel::tokens::{Colour, Tokens};
use tiny_skia::{FillRule, PathBuilder, Pixmap, Stroke, StrokeDash, Transform};

use crate::lock;
use crate::paint::{self, Face, Text, fill, icon, mix, paint_of};
use crate::popup::{self, Rect};

/// How big an edit mode's corners and marks are: the glass or the desktop,
/// and a scale for its corners (1 everywhere so far).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub glass: bool,
    pub scale: f32,
}

impl Style {
    /// Over the lock screen's frosted glass on a dark picture.
    pub const GLASS: Style = Style {
        glass: true,
        scale: 1.0,
    };
    /// Over the desktop, in the panel's colours.
    pub const DESKTOP: Style = Style {
        glass: false,
        scale: 1.0,
    };

    /// A corner of `r` logical pixels in this style, in device pixels.
    fn corner(self, r: f32, s: f32) -> f32 {
        r * self.scale * s
    }
}

/// How a part is drawn: at rest, under the pointer, with the keyboard's
/// focus, or lifted while it is dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Rest,
    Hover,
    Focus,
    Lifted,
}

/// A button's kind: the accent filled, a quiet line with no fill, or (M5.10c7f)
/// the glass text filled with the stage's dark words, the lock editor's Done
/// and its preview's Back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Accent,
    Quiet,
    Light,
}

/// The margin round a placed part's outline, logical pixels.
pub const PART_PAD: f32 = 8.0;
/// A tile's icon, logical pixels across; it sits in the tile's top 60 percent.
pub const ICON: f32 = 22.0;
/// A button's height on a desktop and on a touch screen (Compact).
pub const BUTTON: f32 = 36.0;
pub const BUTTON_TOUCH: f32 = 44.0;
/// A chip's padding, logical pixels across and high.
const CHIP_PAD_X: f32 = 6.0;
const CHIP_PAD_Y: f32 = 3.0;
/// A chip's room above the part's outline, logical pixels.
const CHIP_GAP: f32 = 6.0;
/// The dashes of a resting part's outline and of the guides, logical pixels.
const REST_DASH: (f32, f32) = (5.0, 4.0);
const GUIDE_DASH: (f32, f32) = (4.0, 4.0);
/// The line of a resting or hovered part, and the accent line of a lifted or
/// focused part (logical pixels).
const LINE: f32 = 1.0;
const LIFTED_LINE: f32 = 2.0;
/// The guides' width, logical pixels.
const GUIDE_LINE: f32 = 1.0;
/// The share of the tile above which its title sits, and the share of the
/// picture's strength a chosen tile keeps.
const ICON_SHARE: f32 = 0.6;
const CHOSEN: f32 = 0.5;
/// The sheet's glass base, and the desktop's sheet opacity.
const GLASS_BASE: f32 = 0.7;
const DESKTOP_SHEET: f32 = 0.96;
/// The glass stage (M5.10c7f, the mockup's edit screen): one glow of
/// `lock_edit_glow` centred 35 percent down the screen, reaching 75 percent of
/// its width, at full strength so its centre reads as the token.
const GLASS_GLOW_DOWN: f32 = 0.35;
const GLASS_GLOW_REACH: f32 = 0.75;
const GLASS_GLOW: f32 = 1.0;
/// Where the stage's lower shade starts (a share of the screen's height) and
/// how strong it is at the foot (the shadow colour's alpha there).
const SHADE_FROM: f32 = 0.45;
const SHADE: f32 = 0.5;
/// A side panel's fill and hairline, white at these shares (M5.10c7f).
const SIDE_FILL: f32 = 0.05;
const SIDE_EDGE: f32 = 0.09;
/// The glass picture's hairline, white at this share (M5.10c7f).
const STAGE_EDGE: f32 = 0.12;
/// The chosen part's ring on the glass (M5.10c7f): 1.5 px of white at 90
/// percent on the part's own corners, with a 4 px halo of the accent at 45
/// percent just outside it.
const MARK_RING: f32 = 1.5;
const MARK_HALO: f32 = 4.0;
const MARK_RING_SHARE: f32 = 0.9;
/// The glass slider's track (`slider`, M5.10c7g): 4 px high, its rest white at
/// 18 percent, and its knob 16 px across.
pub const TRACK: f32 = 4.0;
pub const TRACK_REST: f32 = 0.18;
pub const KNOB: f32 = 16.0;
const MARK_HALO_SHARE: f32 = 0.45;

/// The colours an editor draws its parts in, for its style: the text, the
/// raised surface, the light of a hovered row, the outline, the accent and
/// the text on it, and the dimmed text (the hint).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inks {
    pub text: Colour,
    pub raised: Colour,
    pub lit: Colour,
    pub line: Colour,
    pub accent: Colour,
    pub accent_text: Colour,
    pub dim: Colour,
    pub fill: Colour,
}

/// The inks of `style`. On the glass the text is the tokens' backdrop text
/// (near white) at the same strengths the mockups use; on the desktop it is
/// the panel's text, and the raised surfaces are the menus' own.
pub fn inks(style: Style, tokens: &Tokens) -> Inks {
    if style.glass {
        let text = tokens.backdrop_text;
        Inks {
            text,
            raised: Colour { a: 0.08, ..text },
            lit: Colour { a: 0.14, ..text },
            line: Colour { a: 0.40, ..text },
            accent: tokens.accent,
            accent_text: tokens.accent_text,
            dim: Colour { a: 0.70, ..text },
            fill: tokens.lock_glass,
        }
    } else {
        let text = tokens.panel_text;
        Inks {
            text,
            raised: popup::raised(tokens),
            lit: paint::lit(tokens),
            line: Colour { a: 0.40, ..text },
            accent: tokens.accent,
            accent_text: tokens.accent_text,
            dim: Colour { a: 0.70, ..text },
            fill: tokens.card,
        }
    }
}

/// A rectangle grown by `by` on every side.
fn grown(r: Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + 2.0 * by, r.h + 2.0 * by)
}

/// The area a part's outline covers: the part with `PART_PAD` round it.
pub fn part_area(r: Rect) -> Rect {
    grown(r, PART_PAD)
}

/// Dims the place a part has been dragged away from (M5.10c6c3): its area
/// filled with the deep backdrop at 94 percent, so the lock's picture
/// there stops showing the part; the part's resting outline is drawn over it.
pub fn vacate(pixmap: &mut Pixmap, r: Rect, tokens: &Tokens, s: f32) {
    let (x, y, w, h) = part_area(r).device(s);
    let base = Colour {
        a: 0.94,
        ..tokens.lock_backdrop_deep
    };
    fill(pixmap, x, y, w, h, tokens.radius_menu as f32 * s, base);
}

/// Strokes the rounded rectangle `rect` (device pixels) inside its edge,
/// `width` device pixels wide, dashed when `dash` (device pixels) is given.
pub(crate) fn stroke(
    pixmap: &mut Pixmap,
    rect: (f32, f32, f32, f32),
    radius: f32,
    width: f32,
    c: Colour,
    dash: Option<(f32, f32)>,
) {
    let (x, y, w, h) = rect;
    let half = width / 2.0;
    let Some(path) = paint::rounded(
        x + half,
        y + half,
        w - width,
        h - width,
        (radius - half).max(0.0),
    ) else {
        return;
    };
    let dash = dash.and_then(|(on, off)| StrokeDash::new(vec![on, off], 0.0));
    let stroke = Stroke {
        width,
        dash,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint_of(c), &stroke, Transform::identity(), None);
}

/// The stage round the lock screen's picture in the edit mode (M5.10c6c3):
/// the screen filled with the deep backdrop, a soft glow centred on the picture
/// (the backdrop's own glow helper), and the picture's shadow, cast from
/// `preview` (logical pixels on the screen). On the glass (M5.10c7f) the fill is
/// `lock_edit_deep` and the glow `lock_edit_glow`, 35 percent down the screen,
/// reaching 75 percent of its width. Desktop: the panel's colour, no glow. The
/// picture itself is drawn by `stage_frame`, over this.
pub fn stage(
    pixmap: &mut Pixmap,
    screen: Rect,
    preview: Rect,
    tokens: &Tokens,
    style: Style,
    s: f32,
) {
    let ph = pixmap.height() as f32;
    let base = if style.glass {
        tokens.lock_edit_deep
    } else {
        tokens.panel
    };
    fill(pixmap, 0.0, 0.0, screen.w * s, screen.h * s, 0.0, base);
    if style.glass {
        let centre = (screen.w / 2.0 * s, screen.h * GLASS_GLOW_DOWN * s);
        let reach = (screen.w * GLASS_GLOW_REACH * s).max(1.0);
        lock::glow(pixmap, centre, reach, tokens.lock_edit_glow, GLASS_GLOW, ph);
        shade_below(pixmap, screen, tokens, s);
    }
    lock::lift(pixmap, preview, picture_corner(style, tokens), s, tokens);
}

/// Darkens the stage's lower part, as the mockup's backdrop settles there
/// (Alimardon, 2026-10-11): from `SHADE_FROM` of the screen's height down,
/// the tokens' shadow colour grows to `SHADE` of its strength at the foot.
/// The stage only: the picture is drawn over it afterwards.
fn shade_below(pixmap: &mut Pixmap, screen: Rect, tokens: &Tokens, s: f32) {
    let (w, h) = (screen.w * s, screen.h * s);
    let ink = tokens.shadow;
    let at = |a: f32| tiny_skia::Color::from_rgba(ink.r, ink.g, ink.b, a.clamp(0.0, 1.0));
    let (Some(clear), Some(dark)) = (at(0.0), at(SHADE)) else {
        return;
    };
    let stops = vec![
        tiny_skia::GradientStop::new(0.0, clear),
        tiny_skia::GradientStop::new(1.0, dark),
    ];
    if let (Some(shader), Some(rect)) = (
        tiny_skia::LinearGradient::new(
            tiny_skia::Point::from_xy(0.0, h * SHADE_FROM),
            tiny_skia::Point::from_xy(0.0, h),
            stops,
            tiny_skia::SpreadMode::Pad,
            Transform::identity(),
        ),
        tiny_skia::Rect::from_xywh(0.0, h * SHADE_FROM, w, h * (1.0 - SHADE_FROM)),
    ) {
        let paint = tiny_skia::Paint {
            shader,
            ..tiny_skia::Paint::default()
        };
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }
}

/// The corners of the picture in `style` (logical pixels): `radius_card` on
/// the glass, `radius_dock` on the desktop.
fn picture_corner(style: Style, tokens: &Tokens) -> f32 {
    if style.glass {
        tokens.radius_card as f32 * style.scale
    } else {
        tokens.radius_dock as f32 * style.scale
    }
}

/// The picture round its corners, over the stage (M5.10c6c3): `picture`, a
/// whole screen at device size, is scaled into the `preview` rectangle
/// (logical pixels) and its corners are cut to `PREVIEW_RADIUS`; then a
/// hairline in the style's edge colour round it.
pub fn stage_frame(
    pixmap: &mut Pixmap,
    picture: &Pixmap,
    preview: Rect,
    tokens: &Tokens,
    style: Style,
    s: f32,
) {
    let (x, y, w, h) = preview.device(s);
    let (pw, ph) = (w.round().max(1.0) as u32, h.round().max(1.0) as u32);
    let Some(mut layer) = Pixmap::new(pw, ph) else {
        return;
    };
    let scale = Transform::from_scale(
        pw as f32 / picture.width().max(1) as f32,
        ph as f32 / picture.height().max(1) as f32,
    );
    let smooth = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..tiny_skia::PixmapPaint::default()
    };
    layer.draw_pixmap(0, 0, picture.as_ref(), &smooth, scale, None);
    let radius = picture_corner(style, tokens) * s;
    if let Some(mut mask) = tiny_skia::Mask::new(pw, ph) {
        if let Some(path) = paint::rounded(0.0, 0.0, pw as f32, ph as f32, radius) {
            mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
            layer.apply_mask(&mask);
        }
    }
    pixmap.draw_pixmap(
        x.round() as i32,
        y.round() as i32,
        layer.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    let edge = if style.glass {
        Colour {
            a: STAGE_EDGE,
            ..tokens.backdrop_text
        }
    } else {
        tokens.edge
    };
    stroke(pixmap, (x, y, w, h), radius, LINE * s, edge, None);
}

/// A side panel of the glass edit mode (M5.10c7f, the mockup's wallpaper and
/// clock panels): `SIDE_RADIUS` corners, white at `SIDE_FILL`, a 1 px hairline
/// of white at `SIDE_EDGE`, and no shadow. Draws nothing outside `r`.
pub fn side_panel(pixmap: &mut Pixmap, r: Rect, tokens: &Tokens, s: f32) {
    let (x, y, w, h) = r.device(s);
    let radius = tokens.radius_dock as f32 * s;
    let fill_colour = Colour {
        a: SIDE_FILL,
        ..tokens.backdrop_text
    };
    fill(pixmap, x, y, w, h, radius, fill_colour);
    let edge = Colour {
        a: SIDE_EDGE,
        ..tokens.backdrop_text
    };
    stroke(pixmap, (x, y, w, h), radius, LINE * s, edge, None);
}

/// The track of a glass slider (M5.10c7g, the Dim of the wallpaper panel):
/// `TRACK` logical pixels high with round ends, its rest white at
/// `TRACK_REST`, filled in the accent from its left end to `share` (0 to 1)
/// of the knob's travel, and a white `KNOB` at that point. `track` is the
/// whole slider; the knob's centre travels from half a knob in from its left
/// to half a knob in from its right, so the knob never leaves it.
pub fn slider(pixmap: &mut Pixmap, track: Rect, share: f32, tokens: &Tokens, s: f32) {
    let ink = inks(Style::GLASS, tokens);
    let share = share.clamp(0.0, 1.0);
    let bar_y = track.y + (track.h - TRACK) / 2.0;
    let centre_y = track.y + track.h / 2.0;
    let travel = track.w - KNOB;
    let knob_x = track.x + KNOB / 2.0 + travel * share;
    let (x, y, w, h) = Rect::new(track.x, bar_y, track.w, TRACK).device(s);
    let rest = Colour {
        a: TRACK_REST,
        ..ink.text
    };
    fill(pixmap, x, y, w, h, TRACK / 2.0 * s, rest);
    let filled = (knob_x - track.x).max(0.0);
    if filled > 0.0 {
        fill(pixmap, x, y, filled * s, h, TRACK / 2.0 * s, ink.accent);
    }
    if let Some(path) = PathBuilder::from_circle(knob_x * s, centre_y * s, KNOB / 2.0 * s) {
        pixmap.fill_path(
            &path,
            &paint_of(ink.text),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// A colour swatch (M8.21 part 21k2): its circle, logical pixels across, the
/// ring round the chosen one, and the share of its strength a swatch keeps
/// when it does not read (it cannot be chosen).
const SWATCH: f32 = 20.0;
const SWATCH_CHOSEN: f32 = 28.0;
const SWATCH_RING: f32 = 2.0;
const SWATCH_FADE: f32 = 0.35;

/// One swatch of a card's colour grid (M8.21 part 21k2), in Settings' look
/// (`colours.rs`'s `paint`): a circle of `SWATCH` logical pixels centred in
/// `cell`, filled with `fill`, or with no fill a ring of the text's ink. The
/// chosen swatch has a ring of `SWATCH_CHOSEN` round it. A swatch that does not
/// read is drawn at `SWATCH_FADE` of its strength.
pub fn swatch(
    pixmap: &mut Pixmap,
    cell: Rect,
    fill_colour: Option<Colour>,
    chosen: bool,
    reads: bool,
    ink: &Inks,
    s: f32,
) {
    let strength = if reads { 1.0 } else { SWATCH_FADE };
    let centred = |d: f32| {
        Rect::new(
            cell.x + (cell.w - d) / 2.0,
            cell.y + (cell.h - d) / 2.0,
            d,
            d,
        )
    };
    let (x, y, w, h) = centred(SWATCH).device(s);
    match fill_colour {
        Some(colour) => fill(
            pixmap,
            x,
            y,
            w,
            h,
            SWATCH / 2.0 * s,
            Colour {
                a: colour.a * strength,
                ..colour
            },
        ),
        None => stroke(
            pixmap,
            (x, y, w, h),
            SWATCH / 2.0 * s,
            SWATCH_RING * s,
            Colour {
                a: ink.text.a * strength,
                ..ink.text
            },
            None,
        ),
    }
    if chosen {
        let (x, y, w, h) = centred(SWATCH_CHOSEN).device(s);
        let ring = Colour {
            a: ink.text.a * strength,
            ..ink.text
        };
        stroke(
            pixmap,
            (x, y, w, h),
            SWATCH_CHOSEN / 2.0 * s,
            SWATCH_RING * s,
            ring,
            None,
        );
    }
}

/// A part's outline (M5.10c6c3): `PART_PAD` outside the part `r` (logical
/// pixels, in the same surface as the rest), its corners `PART_RADIUS`.
/// `Rest` is a dashed line of the outline colour; `Hover` a fill of the
/// glass (or the card) and a solid line; `Focus` the same plus the focus
/// ring; `Lifted` a 2 px accent line, the fill and the lift's shadow under it.
pub fn outline_part(
    pixmap: &mut Pixmap,
    r: Rect,
    state: State,
    tokens: &Tokens,
    style: Style,
    s: f32,
) {
    let ink = inks(style, tokens);
    let area = part_area(r);
    let radius = tokens.radius_menu as f32 * style.scale;
    let device = area.device(s);
    let (x, y, w, h) = device;
    match state {
        State::Rest => stroke(
            pixmap,
            device,
            radius * s,
            LINE * s,
            Colour {
                a: 0.22,
                ..ink.text
            },
            Some((REST_DASH.0 * s, REST_DASH.1 * s)),
        ),
        State::Hover | State::Focus => {
            fill(pixmap, x, y, w, h, radius * s, ink.fill);
            stroke(pixmap, device, radius * s, LINE * s, ink.line, None);
            if state == State::Focus {
                popup::focus_ring(pixmap, area, radius, s, tokens);
            }
        }
        State::Lifted => {
            lock::lift(pixmap, area, radius, s, tokens);
            fill(pixmap, x, y, w, h, radius * s, ink.fill);
            stroke(
                pixmap,
                device,
                radius * s,
                LIFTED_LINE * s,
                ink.accent,
                None,
            );
        }
    }
}

/// The name pill above a part (M5.10c6c3): `text` in the accent's text on
/// the accent, `size.panel_text_small` semibold, fully round, `CHIP_PAD_X`
/// by `CHIP_PAD_Y` around it, centred on `at`'s x and `CHIP_GAP` above the
/// top of the part's outline `outline_of` (below the outline when there is
/// no room above it). Without `engine` the pill is drawn at a fixed width.
pub fn chip(
    pixmap: &mut Pixmap,
    text: &str,
    outline_of: Rect,
    tokens: &Tokens,
    engine: Option<&mut Text>,
    style: Style,
    s: f32,
) {
    let ink = inks(style, tokens);
    let size = tokens.panel_text_small_size as f32;
    let (line_w, ascent_room) = match engine {
        Some(engine) => {
            let line = engine.line_in(text, size * s, Face::SEMIBOLD);
            (line.width / s, Some(engine))
        }
        None => (text.chars().count() as f32 * size * 0.55, None),
    };
    let h = size + 2.0 * CHIP_PAD_Y;
    let w = line_w + 2.0 * CHIP_PAD_X;
    let centre = outline_of.x + outline_of.w / 2.0;
    let above = outline_of.y - PART_PAD - CHIP_GAP - h;
    let top = if above >= 0.0 {
        above
    } else {
        outline_of.y + outline_of.h + PART_PAD + CHIP_GAP
    };
    let left = (centre - w / 2.0).max(0.0);
    fill(
        pixmap,
        left * s,
        top * s,
        w * s,
        h * s,
        h * s / 2.0,
        ink.accent,
    );
    if let Some(engine) = ascent_room {
        let mut line = engine.line_in(text, size * s, Face::SEMIBOLD);
        let x = (left + CHIP_PAD_X) * s;
        let y = popup::middle(top, h, size * s, s);
        engine.draw(pixmap, &mut line, x, y, ink.accent_text);
    }
}

/// The snap guides (M5.10c6c3): each line across the `clip` rectangle only
/// (the picture's), dashed and 1 px wide in the accent at 70 percent. Each
/// guide is in the surface's logical pixels, already mapped.
pub fn guides(pixmap: &mut Pixmap, lines: &[Guide], clip: Rect, tokens: &Tokens, s: f32) {
    let ink = Colour {
        a: 0.7,
        ..tokens.accent
    };
    for guide in lines {
        let mut path = PathBuilder::new();
        if guide.vertical {
            path.move_to(guide.at * s, clip.y * s);
            path.line_to(guide.at * s, (clip.y + clip.h) * s);
        } else {
            path.move_to(clip.x * s, guide.at * s);
            path.line_to((clip.x + clip.w) * s, guide.at * s);
        }
        let Some(path) = path.finish() else {
            continue;
        };
        let dash = StrokeDash::new(vec![GUIDE_DASH.0 * s, GUIDE_DASH.1 * s], 0.0);
        let stroke = Stroke {
            width: GUIDE_LINE * s,
            dash,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint_of(ink), &stroke, Transform::identity(), None);
    }
}

/// The drawer's sheet (M5.10c6c3): over the glass, the deep backdrop at
/// `GLASS_BASE` with the glass over it and a 1 px hairline of `lock_glass_edge`;
/// over the desktop, the panel's colour at `DESKTOP_SHEET` with a hairline of
/// the edge colour. Both have `SHEET_RADIUS` corners. Draws nothing outside `r`.
pub fn sheet(pixmap: &mut Pixmap, r: Rect, tokens: &Tokens, style: Style, s: f32) {
    let (x, y, w, h) = r.device(s);
    let radius = style.corner(tokens.radius_sheet as f32, s);
    if style.glass {
        let base = Colour {
            a: GLASS_BASE,
            ..tokens.lock_backdrop_deep
        };
        fill(pixmap, x, y, w, h, radius, base);
        fill(pixmap, x, y, w, h, radius, tokens.lock_glass);
        stroke(
            pixmap,
            (x, y, w, h),
            radius,
            LINE * s,
            tokens.lock_glass_edge,
            None,
        );
    } else {
        let base = Colour {
            a: DESKTOP_SHEET,
            ..tokens.panel
        };
        fill(pixmap, x, y, w, h, radius, base);
        stroke(pixmap, (x, y, w, h), radius, LINE * s, tokens.edge, None);
    }
}

/// A drawer tile (M5.10c6c3): `TILE_RADIUS` corners, the raised fill (white at
/// 8 percent on the glass, `popup::raised` on the desktop, 14 percent on the
/// glass under the pointer), the icon `icon` (empty for none) centred in the
/// top 60 percent, the `title` under it in `size.panel_text` medium, centred
/// and cut with an ellipsis. `hover` lightens it; `focus` adds the ring; a
/// `chosen` tile (one a panel already holds) is drawn at half strength.
#[allow(clippy::too_many_arguments)]
pub fn tile(
    pixmap: &mut Pixmap,
    r: Rect,
    icon_name: &str,
    title: &str,
    state: State,
    chosen: bool,
    tokens: &Tokens,
    text: Option<&mut Text>,
    style: Style,
    s: f32,
) {
    let ink = inks(style, tokens);
    let strength = if chosen { CHOSEN } else { 1.0 };
    let alpha = |c: Colour| Colour {
        a: c.a * strength,
        ..c
    };
    let (x, y, w, h) = r.device(s);
    let radius = style.corner(tokens.radius_tile as f32, s);
    fill(pixmap, x, y, w, h, radius, alpha(ink.raised));
    if state == State::Hover || state == State::Focus {
        let over = if style.glass {
            Colour {
                a: 0.14,
                ..tokens.backdrop_text
            }
        } else {
            paint::mix(ink.raised, ink.text, 0.06)
        };
        fill(pixmap, x, y, w, h, radius, alpha(over));
    }
    if !icon_name.is_empty() {
        let px = ICON * s;
        let ix = x + ((w - px) / 2.0).round();
        let iy = y + ((h * ICON_SHARE - px) / 2.0).round();
        icon(pixmap, icon_name, px, ix, iy, alpha(ink.text));
    }
    if let Some(text) = text {
        let size = tokens.panel_text_size as f32 * s;
        let room = w - 2.0 * popup::PAD * s;
        let mut line = text.fit_in(title, size, room, Face::MEDIUM);
        // The title always lies in the tile's lower part, under the icon or
        // the picture, which sit in the top share.
        let baseline = popup::middle(r.h * ICON_SHARE, r.h * (1.0 - ICON_SHARE), size, s);
        let at = x + ((w - line.width) / 2.0).round();
        text.draw(pixmap, &mut line, at, baseline, alpha(ink.text));
    }
    if state == State::Focus {
        popup::focus_ring(pixmap, r, tokens.radius_tile as f32, s, tokens);
    }
}

/// A pill (M5.10c6c3): fully round, `r.h` high (`BUTTON` or `BUTTON_TOUCH`),
/// the label in `size.panel_text` semibold and centred. `Accent` is filled
/// with the accent and its text on it; `Quiet` a 1 px line of the text's
/// colour at 30 percent with no fill. Hover lightens the accent by 8 percent
/// (`paint::mix`) and gives a quiet button a faint fill. Draws nothing outside `r`.
#[allow(clippy::too_many_arguments)]
pub fn button(
    pixmap: &mut Pixmap,
    r: Rect,
    label: &str,
    kind: ButtonKind,
    state: State,
    tokens: &Tokens,
    text: Option<&mut Text>,
    style: Style,
    s: f32,
) {
    let ink = inks(style, tokens);
    let (x, y, w, h) = r.device(s);
    let radius = h / 2.0;
    let words = match kind {
        ButtonKind::Accent => {
            let fill_colour = if state == State::Hover || state == State::Focus {
                mix(ink.accent, ink.text, 0.08)
            } else {
                ink.accent
            };
            fill(pixmap, x, y, w, h, radius, fill_colour);
            ink.accent_text
        }
        ButtonKind::Quiet => {
            if state == State::Hover || state == State::Focus {
                fill(
                    pixmap,
                    x,
                    y,
                    w,
                    h,
                    radius,
                    Colour {
                        a: 0.08,
                        ..ink.text
                    },
                );
            }
            stroke(
                pixmap,
                (x, y, w, h),
                radius,
                LINE * s,
                Colour {
                    a: 0.30,
                    ..ink.text
                },
                None,
            );
            ink.text
        }
        ButtonKind::Light => {
            fill(pixmap, x, y, w, h, radius, ink.text);
            if style.glass {
                tokens.lock_edit_deep
            } else {
                tokens.panel
            }
        }
    };
    if let Some(text) = text {
        let size = tokens.panel_text_size as f32 * s;
        let mut line = text.line_in(label, size, Face::SEMIBOLD);
        let at = x + ((w - line.width) / 2.0).round();
        let baseline = popup::middle(r.y, r.h, size, s);
        text.draw(pixmap, &mut line, at, baseline, words);
    }
    if state == State::Focus {
        popup::focus_ring(pixmap, r, r.h / 2.0, s, tokens);
    }
}

/// A part's mark in the lock editor (M5.10c7f), the canvas as the mockup draws
/// it: at rest nothing; under the pointer (`Hover`) a faint 1 px solid outline,
/// the glass text at 25 percent, `PART_PAD` out; with the keyboard's focus or a
/// card open on it (`Focus`) a 1.5 px white ring at 90 percent on the part's own
/// corners, with a 4 px halo of the accent at 45 percent just outside it. The
/// corners scale with `style.scale`, the picture's scale. `Lifted` is
/// `outline_part`'s. The panel editor keeps `outline_part`.
pub fn mark_part(
    pixmap: &mut Pixmap,
    r: Rect,
    state: State,
    tokens: &Tokens,
    style: Style,
    s: f32,
) {
    let ink = inks(style, tokens);
    let radius = tokens.radius_card as f32 * style.scale;
    match state {
        State::Rest => {}
        State::Hover => {
            let (x, y, w, h) = part_area(r).device(s);
            let faint = Colour {
                a: 0.25,
                ..ink.text
            };
            let corners = tokens.radius_menu as f32 * style.scale * s;
            stroke(pixmap, (x, y, w, h), corners, LINE * s, faint, None);
        }
        State::Focus => {
            // Each stroke's rectangle is its outer edge: the halo's outer edge
            // lies `MARK_HALO` past the ring's, so its inner edge meets the
            // ring's outer edge; the ring is centred on the part's edge.
            let ring_out = MARK_RING / 2.0;
            let halo_out = ring_out + MARK_HALO;
            let (x, y, w, h) = grown(r, halo_out).device(s);
            let halo = Colour {
                a: MARK_HALO_SHARE,
                ..tokens.accent
            };
            stroke(
                pixmap,
                (x, y, w, h),
                (radius + halo_out) * s,
                MARK_HALO * s,
                halo,
                None,
            );
            let (x, y, w, h) = grown(r, ring_out).device(s);
            let ring = Colour {
                a: MARK_RING_SHARE,
                ..tokens.backdrop_text
            };
            stroke(
                pixmap,
                (x, y, w, h),
                (radius + ring_out) * s,
                MARK_RING * s,
                ring,
                None,
            );
        }
        State::Lifted => outline_part(pixmap, r, State::Lifted, tokens, style, s),
    }
}

/// The drawer's hint (M5.10c6c3): one line of `size.panel_text_small` at 70
/// percent of the text colour, fitted to `r`. Without `text`, nothing.
pub fn hint(
    pixmap: &mut Pixmap,
    words: &str,
    r: Rect,
    tokens: &Tokens,
    text: Option<&mut Text>,
    style: Style,
    s: f32,
) {
    let Some(text) = text else {
        return;
    };
    let ink = inks(style, tokens);
    let size = tokens.panel_text_small_size as f32 * s;
    let mut line = text.fit(words, size, r.w * s);
    let baseline = popup::middle(r.y, r.h, tokens.panel_text_small_size as f32 * s, s);
    text.draw(pixmap, &mut line, r.x * s, baseline, ink.dim);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Tokens {
        Tokens::built_in()
    }

    /// A surface of `w` by `h` device pixels, transparent.
    fn surface(w: u32, h: u32) -> Pixmap {
        Pixmap::new(w, h).unwrap()
    }

    /// The bounding box of the pixels with any ink, device pixels, as
    /// `(x0, y0, x1, y1)` with `x1`, `y1` exclusive; None when nothing is drawn.
    fn ink_box(pixmap: &Pixmap) -> Option<(u32, u32, u32, u32)> {
        let w = pixmap.width();
        let mut found: Option<(u32, u32, u32, u32)> = None;
        for y in 0..pixmap.height() {
            for x in 0..w {
                let a = pixmap.pixel(x, y).map_or(0, |p| p.alpha());
                if a > 0 {
                    found = Some(match found {
                        None => (x, y, x + 1, y + 1),
                        Some((x0, y0, x1, y1)) => {
                            (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1))
                        }
                    });
                }
            }
        }
        found
    }

    /// Asserts the ink lies inside `r` (device pixels, one pixel of slack for
    /// anti-aliasing) and that something was drawn.
    fn inside(pixmap: &Pixmap, r: Rect, s: f32) {
        let (x0, y0, x1, y1) = ink_box(pixmap).expect("something is drawn");
        let (x, y, w, h) = r.device(s);
        assert!(x0 as f32 >= x - 1.0, "ink at {x0}, rect at {x}");
        assert!(y0 as f32 >= y - 1.0, "ink at {y0}, rect at {y}");
        assert!(
            x1 as f32 <= x + w + 1.0,
            "ink ends at {x1}, rect at {}",
            x + w
        );
        assert!(
            y1 as f32 <= y + h + 1.0,
            "ink ends at {y1}, rect at {}",
            y + h
        );
    }

    /// The alpha of the pixel at `x`, `y`.
    fn alpha(pixmap: &Pixmap, x: u32, y: u32) -> u8 {
        pixmap.pixel(x, y).map_or(0, |p| p.alpha())
    }

    #[test]
    fn the_inks_of_the_glass_are_the_backdrop_text_and_the_desktop_the_panels() {
        let t = tokens();
        let glass = inks(Style::GLASS, &t);
        assert_eq!(glass.text, t.backdrop_text);
        assert_eq!(glass.fill, t.lock_glass);
        let desk = inks(Style::DESKTOP, &t);
        assert_eq!(desk.text, t.panel_text);
        assert_ne!(glass.raised, desk.raised);
    }

    #[test]
    fn the_stage_fills_the_screen_and_glows_round_the_picture() {
        let t = tokens();
        let mut pixmap = surface(320, 200);
        let screen = Rect::new(0.0, 0.0, 320.0, 200.0);
        let preview = Rect::new(80.0, 40.0, 160.0, 100.0);
        stage(&mut pixmap, screen, preview, &t, Style::GLASS, 1.0);
        let corner = pixmap.pixel(2, 2).unwrap().demultiply();
        assert_eq!(corner.alpha(), 255, "the stage fills the screen");
        let centre = pixmap.pixel(160, 90).unwrap().demultiply();
        assert_ne!(
            (centre.red(), centre.green(), centre.blue()),
            (corner.red(), corner.green(), corner.blue()),
            "the glow lies over the middle"
        );
    }

    /// The glass stage is the mockup's deep navy at the corner, lit by
    /// `lock_edit_glow` at 35 percent down the screen (M5.10c7f).
    #[test]
    fn the_glass_stage_is_the_deep_navy_at_the_corner_and_glows_in_the_middle() {
        let t = tokens();
        let mut pixmap = surface(1280, 800);
        let screen = Rect::new(0.0, 0.0, 1280.0, 800.0);
        let preview = Rect::new(256.0, 108.0, 768.0, 480.0);
        stage(&mut pixmap, screen, preview, &t, Style::GLASS, 1.0);
        // Each channel within `within` of the token, in 8 bits.
        let close = |got: Colour, want: Colour, within: f32| {
            [(got.r, want.r), (got.g, want.g), (got.b, want.b)]
                .iter()
                .all(|(g, w)| ((g * 255.0).round() - (w * 255.0).round()).abs() <= within)
        };
        let corner = pixmap.pixel(0, 0).unwrap().demultiply();
        let corner = Colour {
            r: f32::from(corner.red()) / 255.0,
            g: f32::from(corner.green()) / 255.0,
            b: f32::from(corner.blue()) / 255.0,
            a: 1.0,
        };
        // The glow's falloff (`lock::glow`) reaches the corner faintly: at
        // 1280 by 800 the corner is 6, 5 and 9 of 255 up in red, green and blue.
        assert!(close(corner, t.lock_edit_deep, 10.0), "{corner:?}");
        let (cx, cy) = (1280 / 2, (800.0 * GLASS_GLOW_DOWN) as u32);
        let mid = pixmap.pixel(cx, cy).unwrap().demultiply();
        let mid = Colour {
            r: f32::from(mid.red()) / 255.0,
            g: f32::from(mid.green()) / 255.0,
            b: f32::from(mid.blue()) / 255.0,
            a: 1.0,
        };
        assert!(close(mid, t.lock_edit_glow, 6.0), "{mid:?}");
    }

    #[test]
    fn the_stage_frame_cuts_the_picture_to_the_preview_and_edges_it() {
        let t = tokens();
        let mut pixmap = surface(320, 200);
        let mut picture = Pixmap::new(320, 200).unwrap();
        picture.fill(tiny_skia::Color::from_rgba8(200, 30, 30, 255));
        let preview = Rect::new(80.0, 40.0, 160.0, 100.0);
        stage_frame(&mut pixmap, &picture, preview, &t, Style::GLASS, 1.0);
        assert!(alpha(&pixmap, 160, 90) > 200, "the picture is inside");
        assert_eq!(alpha(&pixmap, 60, 90), 0, "nothing lies outside it");
        assert_eq!(alpha(&pixmap, 81, 41), 0, "the corner is cut");
        inside(&pixmap, Rect::new(80.0, 40.0, 160.0, 100.0), 1.0);
    }

    #[test]
    fn each_outline_state_draws_inside_its_area_and_differs_from_the_others() {
        let t = tokens();
        let part = Rect::new(100.0, 60.0, 120.0, 60.0);
        let mut drawn = Vec::new();
        for state in [State::Rest, State::Hover, State::Focus, State::Lifted] {
            let mut pixmap = surface(400, 300);
            outline_part(&mut pixmap, part, state, &t, Style::GLASS, 1.0);
            let area = part_area(part);
            if state != State::Lifted {
                // The focus ring lies 5 px outside the area.
                inside(&pixmap, grown(area, 6.0), 1.0);
            }
            drawn.push(pixmap);
        }
        for i in 0..drawn.len() {
            for j in i + 1..drawn.len() {
                assert_ne!(
                    drawn[i].data(),
                    drawn[j].data(),
                    "states {i} and {j} look the same"
                );
            }
        }
        // The resting outline is dashed: its own pixel at the grown edge is
        // ink, but the pixel just past the edge is not (no shadow at rest).
        let mut rest = surface(400, 300);
        outline_part(&mut rest, part, State::Rest, &t, Style::GLASS, 1.0);
        assert_eq!(alpha(&rest, 150, 60 - PART_PAD as u32 - 3), 0);
    }

    #[test]
    fn the_part_outline_sits_pad_outside_the_part_with_its_corners() {
        let t = tokens();
        let part = Rect::new(100.0, 60.0, 120.0, 60.0);
        let mut hover = surface(400, 300);
        outline_part(&mut hover, part, State::Hover, &t, Style::GLASS, 1.0);
        // The fill reaches the grown rectangle, not the part's own edge.
        assert!(alpha(&hover, 100 - PART_PAD as u32 + 2, 90) > 0);
        assert_eq!(alpha(&hover, 100 - PART_PAD as u32 - 2, 90), 0);
    }

    #[test]
    fn a_chip_is_an_accent_pill_above_the_outline_and_below_it_with_no_room() {
        let t = tokens();
        let outline_at = Rect::new(100.0, 100.0, 120.0, 60.0);
        let mut above = surface(400, 300);
        chip(&mut above, "Clock", outline_at, &t, None, Style::GLASS, 1.0);
        let (x0, y0, x1, y1) = ink_box(&above).expect("the chip is drawn");
        assert!(
            y1 as f32 <= outline_at.y - PART_PAD - CHIP_GAP + 1.0,
            "above the outline: {y1}"
        );
        assert!(x0 < x1 && y0 < y1);
        let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);
        let p = above.pixel(cx, cy).unwrap().demultiply();
        assert_eq!(
            (p.red(), p.green(), p.blue()),
            (
                (t.accent.r * 255.0).round() as u8,
                (t.accent.g * 255.0).round() as u8,
                (t.accent.b * 255.0).round() as u8,
            )
        );
        // With no room above (a part at the top), it goes below.
        let top = Rect::new(100.0, 4.0, 120.0, 60.0);
        let mut below = surface(400, 300);
        chip(&mut below, "Clock", top, &t, None, Style::GLASS, 1.0);
        let (_, by0, _, _) = ink_box(&below).unwrap();
        assert!(by0 as f32 >= top.y + top.h, "below the part: {by0}");
    }

    #[test]
    fn the_guides_are_dashed_lines_across_the_clip_only() {
        let t = tokens();
        let clip = Rect::new(50.0, 50.0, 200.0, 100.0);
        let mut pixmap = surface(300, 200);
        let lines = [
            Guide {
                vertical: true,
                at: 120.0,
            },
            Guide {
                vertical: false,
                at: 80.0,
            },
        ];
        guides(&mut pixmap, &lines, clip, &t, 1.0);
        inside(&pixmap, Rect::new(50.0, 50.0, 200.0, 100.0), 1.0);
        assert!(
            alpha(&pixmap, 120, 52) > 0,
            "the vertical guide is drawn in the clip"
        );
        assert_eq!(alpha(&pixmap, 120, 40), 0, "and not outside it");
        assert!(
            alpha(&pixmap, 60, 80) > 0 || alpha(&pixmap, 61, 80) > 0 || alpha(&pixmap, 62, 80) > 0
        );
    }

    #[test]
    fn a_sheet_draws_inside_its_rect_with_its_corners_on_the_glass_and_the_desktop() {
        let t = tokens();
        for style in [Style::GLASS, Style::DESKTOP] {
            let mut pixmap = surface(300, 200);
            let r = Rect::new(20.0, 20.0, 260.0, 160.0);
            sheet(&mut pixmap, r, &t, style, 1.0);
            inside(&pixmap, r, 1.0);
            assert_eq!(alpha(&pixmap, 21, 21), 0, "the corner is rounded");
            assert!(alpha(&pixmap, 150, 100) > 150, "the middle is filled");
        }
    }

    #[test]
    fn a_tile_draws_inside_its_rect_and_a_chosen_one_is_fainter() {
        let t = tokens();
        let r = Rect::new(10.0, 10.0, 104.0, 84.0);
        let mut plain = surface(140, 110);
        tile(
            &mut plain,
            r,
            "clock",
            "Clock",
            State::Rest,
            false,
            &t,
            None,
            Style::GLASS,
            1.0,
        );
        inside(&plain, r, 1.0);
        let mut chosen = surface(140, 110);
        tile(
            &mut chosen,
            r,
            "clock",
            "Clock",
            State::Rest,
            true,
            &t,
            None,
            Style::GLASS,
            1.0,
        );
        let (a, b) = (alpha(&plain, 60, 80), alpha(&chosen, 60, 80));
        assert!(b < a, "a chosen tile is fainter: {b} against {a}");
    }

    #[test]
    fn a_tile_with_its_icon_and_title_is_drawn_inside_it() {
        let t = tokens();
        let mut text = Text::load(&t.font);
        let r = Rect::new(0.0, 0.0, 104.0, 84.0);
        let mut pixmap = surface(104, 84);
        tile(
            &mut pixmap,
            r,
            "clock",
            "Clock",
            State::Focus,
            false,
            &t,
            Some(&mut text),
            Style::DESKTOP,
            1.0,
        );
        inside(&pixmap, r, 1.0);
    }

    #[test]
    fn a_button_is_a_pill_filled_for_accent_and_a_line_for_quiet() {
        let t = tokens();
        let r = Rect::new(20.0, 20.0, 88.0, BUTTON);
        let mut accent = surface(130, 80);
        button(
            &mut accent,
            r,
            "Done",
            ButtonKind::Accent,
            State::Rest,
            &t,
            None,
            Style::GLASS,
            1.0,
        );
        inside(&accent, r, 1.0);
        let c = accent.pixel(64, 38).unwrap().demultiply();
        assert_eq!(c.alpha(), 255);
        assert_eq!(alpha(&accent, 21, 21), 0, "fully round ends");
        let mut quiet = surface(130, 80);
        button(
            &mut quiet,
            r,
            "Undo",
            ButtonKind::Quiet,
            State::Rest,
            &t,
            None,
            Style::GLASS,
            1.0,
        );
        inside(&quiet, r, 1.0);
        assert_eq!(alpha(&quiet, 64, 38), 0, "a quiet button has no fill");
        assert_ne!(accent.data(), quiet.data());
    }

    #[test]
    fn a_hint_is_drawn_inside_its_line_and_nothing_without_text() {
        let t = tokens();
        let mut text = Text::load(&t.font);
        let r = Rect::new(16.0, 40.0, 300.0, 15.0);
        let mut pixmap = surface(340, 80);
        hint(
            &mut pixmap,
            "Drag a part anywhere",
            r,
            &t,
            Some(&mut text),
            Style::GLASS,
            1.0,
        );
        inside(&pixmap, Rect::new(16.0, 40.0, 300.0, 15.0), 1.0);
        let mut none = surface(340, 80);
        hint(
            &mut none,
            "Drag a part anywhere",
            r,
            &t,
            None,
            Style::GLASS,
            1.0,
        );
        assert!(ink_box(&none).is_none());
    }
}
