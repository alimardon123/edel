//! What the overview paints itself (M5.2j-b, `docs/mockups/shell/overview.jpg`):
//! the strip's tray with each workspace's label and the frame that adds
//! one, each spread window's name under it, and the close button over the
//! window under the pointer. Each is a tiny-skia picture at the screen's
//! scale, painted again only when what it shows changes; `glance.rs`
//! places them. Colours come from the tokens.

use tiny_skia::{
    FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, PixmapPaint, Point,
    RadialGradient, Rect, SpreadMode, Stroke, Transform,
};

use edel::tokens::{Colour, Tokens};
use edel_compositor::overview::{LABEL, Plan, TRAY_PAD, TRAY_RADIUS};
use smithay::utils::{Logical, Rectangle};

use edel_compositor::frame::Text;

/// The labels' and names' text size, logical pixels.
pub const SMALL: f32 = 11.0;
pub const NAME: f32 = 12.0;
/// A name pill's height and its room at each side; the icon's side.
pub const PILL: i32 = 22;
const PILL_PAD: f32 = 9.0;
pub const NAME_ICON: u32 = 14;
/// The close button's side.
pub const CLOSE: i32 = 20;

fn paint_of(colour: Colour) -> Paint<'static> {
    let mut paint = Paint::default();
    let [r, g, b, _] = colour.bytes();
    paint.set_color_rgba8(r, g, b, (colour.a * 255.0).round() as u8);
    paint.anti_alias = true;
    paint
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(x + w - r, y);
    p.quad_to(x + w, y, x + w, y + r);
    p.line_to(x + w, y + h - r);
    p.quad_to(x + w, y + h, x + w - r, y + h);
    p.line_to(x + r, y + h);
    p.quad_to(x, y + h, x, y + h - r);
    p.line_to(x, y + r);
    p.quad_to(x, y, x + r, y);
    p.close();
    p.finish()
}

/// `text` centred on `middle`, its line's middle at `centre_y`, cut to
/// `room` pixels.
fn centred(
    pixmap: &mut Pixmap,
    text: &mut Text,
    words: &str,
    middle: f32,
    centre_y: f32,
    room: f32,
    colour: Colour,
) {
    let words = text.fit(words, room.max(0.0));
    let w = text.width(&words);
    let baseline = (centre_y + (text.ascent() + text.descent()) / 2.0).round();
    text.draw_on(pixmap, &words, (middle - w / 2.0).round(), baseline, colour);
}

/// The backdrop of a screen `w` by `h` pixels where there is no
/// wallpaper: `backdrop` at the top to `backdrop_deep` at the bottom,
/// lit softly from the upper left, as the mockup draws it.
pub fn backdrop(w: u32, h: u32, tokens: &Tokens) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    let (fw, fh) = (w as f32, h as f32);
    let rect = Rect::from_xywh(0.0, 0.0, fw, fh)?;
    if let Some(fall) = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(0.0, fh),
        vec![
            GradientStop::new(0.0, colour_of(tokens.backdrop, 1.0)),
            GradientStop::new(1.0, colour_of(tokens.backdrop_deep, 1.0)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        pixmap.fill_rect(rect, &shaded(fall), Transform::identity(), None);
    }
    // Darker toward the edges, so the overview reads as a place of its own.
    let middle = Point::from_xy(fw / 2.0, fh * 0.45);
    if let Some(vignette) = RadialGradient::new(
        middle,
        0.0,
        middle,
        (fw * fw + fh * fh).sqrt() * 0.6,
        vec![
            GradientStop::new(0.55, colour_of(tokens.backdrop_deep, 0.0)),
            GradientStop::new(1.0, colour_of(tokens.backdrop_deep, 0.55)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        pixmap.fill_rect(rect, &shaded(vignette), Transform::identity(), None);
    }
    let light = Point::from_xy(fw * 0.3, fh * 0.15);
    if let Some(glow) = RadialGradient::new(
        light,
        0.0,
        light,
        fw.max(fh) * 0.6,
        vec![
            GradientStop::new(0.0, colour_of(tokens.backdrop_text, 0.16)),
            GradientStop::new(1.0, colour_of(tokens.backdrop_text, 0.0)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        pixmap.fill_rect(rect, &shaded(glow), Transform::identity(), None);
    }
    Some(pixmap)
}

fn colour_of(colour: Colour, a: f32) -> tiny_skia::Color {
    let [r, g, b, _] = colour.bytes();
    tiny_skia::Color::from_rgba8(r, g, b, (colour.a * a * 255.0).round() as u8)
}

fn shaded(shader: tiny_skia::Shader<'static>) -> Paint<'static> {
    Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    }
}

/// Whether a colour is light: the tray is a light frost on a light
/// scheme and a dark one on a dark scheme.
fn light(colour: Colour) -> bool {
    0.2126 * colour.r + 0.7152 * colour.g + 0.0722 * colour.b > 0.5
}

/// How a workspace's frame is marked in the strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mark {
    Plain,
    /// The shown workspace, or where a held one would land: an accent
    /// ring round it.
    Lit,
    /// The place of the workspace held by the pointer: an empty outline.
    Away,
}

/// A frame's corners and its ring's distance and width, logical pixels.
const FRAME_RADIUS: f32 = 7.0;
const RING_GAP: f32 = 3.0;
const RING: f32 = 2.5;

/// One workspace's frame painted at `x`, `y`, `w` by `h` pixels: a small
/// screen with the backdrop and a panel along its bottom, `bar` pixels
/// high, ringed in the accent when lit.
fn card(
    pixmap: &mut Pixmap,
    at: (f32, f32, f32, f32),
    bar: f32,
    mark: Mark,
    s: f32,
    tokens: &Tokens,
) {
    let (x, y, w, h) = at;
    let r = FRAME_RADIUS * s;
    if mark == Mark::Away {
        if let Some(outline) = rounded(x + 0.5, y + 0.5, w - 1.0, h - 1.0, r) {
            let dashed = Stroke {
                width: (1.2 * s).max(1.0),
                dash: tiny_skia::StrokeDash::new(vec![4.0 * s, 3.0 * s], 0.0),
                ..Stroke::default()
            };
            let ink = paint_of(Colour {
                a: 0.45,
                ..tokens.backdrop_text
            });
            pixmap.stroke_path(&outline, &ink, &dashed, Transform::identity(), None);
        }
        return;
    }
    if mark == Mark::Lit {
        let g = RING_GAP * s;
        if let Some(ring) = rounded(x - g, y - g, w + 2.0 * g, h + 2.0 * g, r + g) {
            let stroke = Stroke {
                width: RING * s,
                ..Stroke::default()
            };
            pixmap.stroke_path(
                &ring,
                &paint_of(tokens.accent),
                &stroke,
                Transform::identity(),
                None,
            );
        }
    }
    let Some(face) = rounded(x, y, w, h, r) else {
        return;
    };
    if let Some(fall) = LinearGradient::new(
        Point::from_xy(x, y),
        Point::from_xy(x + w * 0.4, y + h),
        vec![
            GradientStop::new(0.0, colour_of(tokens.backdrop, 1.0)),
            GradientStop::new(1.0, colour_of(tokens.backdrop_deep, 1.0)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        pixmap.fill_path(
            &face,
            &shaded(fall),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    // The panel along the bottom, inside the round corners.
    let mut clip = tiny_skia::Mask::new(pixmap.width(), pixmap.height());
    if let Some(clip) = clip.as_mut() {
        clip.fill_path(&face, FillRule::Winding, true, Transform::identity());
        if let Some(panel) = Rect::from_xywh(x, y + h - bar, w, bar) {
            pixmap.fill_rect(
                panel,
                &paint_of(Colour {
                    a: 0.9,
                    ..tokens.panel
                }),
                Transform::identity(),
                Some(clip),
            );
        }
    }
    let hairline = Stroke {
        width: s.max(1.0),
        ..Stroke::default()
    };
    pixmap.stroke_path(
        &face,
        &paint_of(Colour {
            a: 0.18,
            ..tokens.backdrop_text
        }),
        &hairline,
        Transform::identity(),
        None,
    );
}

/// One workspace's frame alone, `w` by `h` logical pixels at `scale`,
/// with room for its ring: the frame a held workspace is drawn with.
/// The frame lies `margin` pixels in from the picture's corner.
pub fn frame(w: i32, h: i32, bar: f32, scale: f64, tokens: &Tokens) -> Option<(Pixmap, i32)> {
    let s = scale as f32;
    let margin = ((RING_GAP + RING) * s).ceil();
    let (pw, ph) = (w as f32 * s, h as f32 * s);
    let mut pixmap = Pixmap::new(
        (pw + 2.0 * margin).ceil() as u32,
        (ph + 2.0 * margin).ceil() as u32,
    )?;
    card(
        &mut pixmap,
        (margin, margin, pw, ph),
        bar * s,
        Mark::Lit,
        s,
        tokens,
    );
    Some((pixmap, margin as i32))
}

/// The tray of `plan` on a screen at `scale`: a rounded frost over the
/// backdrop with a hairline inside its edge, each workspace in view a
/// small screen (`card`) with its label under it, as `frames` gives each, the add frame drawn as a dashed outline with a plus and
/// "New" (`new`), and an arrow at each end where more lie. `bar` is the
/// frames' panel's height, logical pixels. Its size is the tray's in
/// pixels.
pub fn tray(
    plan: &Plan,
    frames: &[(String, Mark)],
    bar: f32,
    words: (&str, &str),
    scale: f64,
    tokens: &Tokens,
    text: Option<&mut Text>,
) -> Option<Pixmap> {
    let (new, caption) = words;
    let s = scale as f32;
    let t = plan.tray;
    let (w, h) = ((t.size.w as f32 * s).ceil(), (t.size.h as f32 * s).ceil());
    let mut pixmap = Pixmap::new(w as u32, h as u32)?;
    let local = |r: Rectangle<i32, Logical>| {
        (
            (r.loc.x - t.loc.x) as f32 * s,
            (r.loc.y - t.loc.y) as f32 * s,
            r.size.w as f32 * s,
            r.size.h as f32 * s,
        )
    };
    let tray = rounded(0.5, 0.5, w - 1.0, h - 1.0, TRAY_RADIUS as f32 * s)?;
    let (frost, rim) = if light(tokens.panel) {
        (0.24, 0.40)
    } else {
        (0.55, 0.10)
    };
    pixmap.fill_path(
        &tray,
        &paint_of(Colour {
            a: frost,
            ..tokens.panel
        }),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let hairline = Stroke {
        width: s.max(1.0),
        ..Stroke::default()
    };
    pixmap.stroke_path(
        &tray,
        &paint_of(Colour {
            a: rim,
            ..tokens.backdrop_text
        }),
        &hairline,
        Transform::identity(),
        None,
    );
    for (i, frame) in plan.frames.iter().enumerate() {
        if !frame.is_empty() {
            let mark = frames.get(i).map_or(Mark::Plain, |(_, mark)| *mark);
            card(&mut pixmap, local(*frame), bar * s, mark, s, tokens);
        }
    }
    if let Some(add) = plan.add {
        let (x, y, aw, ah) = local(add);
        // A smaller tile than a frame, centred in its place, as drawn.
        let side = ah.min(aw) * 0.78;
        let (x, y) = (x + (aw - side) / 2.0, y + (ah - side) / 2.0);
        if let Some(outline) = rounded(x + 0.5, y + 0.5, side - 1.0, side - 1.0, 8.0 * s) {
            let dashed = Stroke {
                width: (1.4 * s).max(1.0),
                dash: tiny_skia::StrokeDash::new(vec![4.0 * s, 3.0 * s], 0.0),
                ..Stroke::default()
            };
            let ink = paint_of(Colour {
                a: 0.8,
                ..tokens.backdrop_text
            });
            pixmap.stroke_path(&outline, &ink, &dashed, Transform::identity(), None);
            let (cx, cy, arm) = (x + side / 2.0, y + side / 2.0, side * 0.16);
            let mut plus = PathBuilder::new();
            plus.move_to(cx - arm, cy);
            plus.line_to(cx + arm, cy);
            plus.move_to(cx, cy - arm);
            plus.line_to(cx, cy + arm);
            if let Some(plus) = plus.finish() {
                let thick = Stroke {
                    width: (1.8 * s).max(1.0),
                    line_cap: tiny_skia::LineCap::Round,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&plus, &ink, &thick, Transform::identity(), None);
            }
        }
    }
    for (arrow, start) in [(plan.before, true), (plan.after, false)] {
        if let Some(arrow) = arrow {
            let (x, y, aw, ah) = local(arrow);
            ends(&mut pixmap, (x, y, aw, ah), start, s, tokens);
        }
    }
    if let Some(text) = text {
        // The caption, "Workspaces", quiet at the tray's head.
        text.set_size(SMALL * s);
        let (cx, cy, cw, ch) = local(plan.caption);
        let words = text.fit(caption, cw);
        let baseline = (cy + ch / 2.0 + (text.ascent() + text.descent()) / 2.0).round();
        let quiet = Colour {
            a: 0.7,
            ..tokens.backdrop_text
        };
        text.draw_on(&mut pixmap, &words, (cx + 2.0 * s).round(), baseline, quiet);
        let ink = Colour {
            a: 0.92,
            ..tokens.backdrop_text
        };
        let mut put = |r: Rectangle<i32, Logical>, words: &str, text: &mut Text| {
            let (x, y, fw, fh) = local(r);
            let centre_y = y + fh + LABEL as f32 * s / 2.0 + 1.0 * s;
            centred(
                &mut pixmap,
                text,
                words,
                x + fw / 2.0,
                centre_y,
                fw + 8.0 * s,
                ink,
            );
        };
        for (frame, (label, _)) in plan.frames.iter().zip(frames) {
            if !frame.is_empty() {
                put(*frame, label, text);
            }
        }
        if let Some(add) = plan.add {
            put(add, new, text);
        }
    }
    Some(pixmap)
}

/// The accent ring round the window under the pointer, `w` by `h` logical
/// pixels at `scale`, a small gap out from its edge with round corners,
/// as the mockup draws it: a picture that lies `margin` pixels out from
/// the window's corner on every side.
pub fn ring(w: i32, h: i32, scale: f64, tokens: &Tokens) -> Option<(Pixmap, i32)> {
    let s = scale as f32;
    let margin = ((RING_GAP + RING + 1.0) * s).ceil();
    let (pw, ph) = (w as f32 * s, h as f32 * s);
    let mut pixmap = Pixmap::new(
        (pw + 2.0 * margin).ceil() as u32,
        (ph + 2.0 * margin).ceil() as u32,
    )?;
    let g = RING_GAP * s + RING * s / 2.0;
    let path = rounded(
        margin - g,
        margin - g,
        pw + 2.0 * g,
        ph + 2.0 * g,
        6.0 * s + g,
    )?;
    let stroke = Stroke {
        width: (RING + 0.5) * s,
        ..Stroke::default()
    };
    pixmap.stroke_path(
        &path,
        &paint_of(tokens.accent),
        &stroke,
        Transform::identity(),
        None,
    );
    Some((pixmap, margin as i32))
}

/// The soft shadow under a spread window `w` by `h` logical pixels at
/// `scale`, cast by the one light above (`shadow`, `shadow_blur`,
/// `shadow_offset`): a picture that lies `margin` pixels out from the
/// window's corner on every side.
pub fn shadow(w: i32, h: i32, scale: f64, tokens: &Tokens) -> Option<(Pixmap, i32)> {
    let s = scale as f32;
    // A light lift, as the mockup has it: three quarters of the menus'
    // blur, half their drop, about half their depth.
    let blur = (tokens.shadow_blur.max(8) as f32 * 0.75 * s).ceil();
    let drop = tokens.shadow_offset as f32 * 0.5 * s;
    let margin = (blur + drop).ceil();
    let (pw, ph) = (w as f32 * s, h as f32 * s);
    let mut pixmap = Pixmap::new(
        (pw + 2.0 * margin).ceil() as u32,
        (ph + 2.0 * margin).ceil() as u32,
    )?;
    // Rings growing out from the window, each fainter, add up to a blur.
    let steps = 12;
    for k in 0..steps {
        let grow = blur * (k as f32 + 0.5) / steps as f32;
        let a = tokens.shadow.a * 0.55 / steps as f32;
        if let Some(path) = rounded(
            margin - grow,
            margin - grow + drop,
            pw + 2.0 * grow,
            ph + 2.0 * grow,
            6.0 * s + grow,
        ) {
            pixmap.fill_path(
                &path,
                &paint_of(Colour { a, ..tokens.shadow }),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    Some((pixmap, margin as i32))
}

/// An end of a strip that scrolls, in the room `at` (x, y, width, height
/// in pixels) at its start or not: the next frame's edge fading out
/// toward the tray's end, as the panel's switcher draws its ends, under
/// a small arrow pointing to where more lie.
fn ends(pixmap: &mut Pixmap, at: (f32, f32, f32, f32), start: bool, s: f32, tokens: &Tokens) {
    let (x, y, w, h) = at;
    let across = w < h;
    let pad = TRAY_PAD as f32 * s;
    // The sliver of the next frame, inside the tray's padding.
    let sliver = if across {
        Rect::from_xywh(x + pad / 2.0, y + pad, w - pad, h - 2.0 * pad)
    } else {
        Rect::from_xywh(x + pad, y + pad / 2.0, w - 2.0 * pad, h - pad)
    };
    let (near, far) = match (across, start) {
        (true, true) => (Point::from_xy(x + w, y), Point::from_xy(x, y)),
        (true, false) => (Point::from_xy(x, y), Point::from_xy(x + w, y)),
        (false, true) => (Point::from_xy(x, y + h), Point::from_xy(x, y)),
        (false, false) => (Point::from_xy(x, y), Point::from_xy(x, y + h)),
    };
    let colour = |a: f32| colour_of(tokens.backdrop_text, a);
    if let (Some(sliver), Some(fade)) = (
        sliver,
        LinearGradient::new(
            near,
            far,
            vec![
                GradientStop::new(0.0, colour(0.16)),
                GradientStop::new(1.0, colour(0.0)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        ),
    ) {
        let paint = Paint {
            shader: fade,
            anti_alias: true,
            ..Paint::default()
        };
        pixmap.fill_rect(sliver, &paint, Transform::identity(), None);
    }
    // The arrow, a chevron pointing out along the strip.
    let (cx, cy, arm) = (x + w / 2.0, y + h / 2.0, 4.5 * s);
    let mut chevron = PathBuilder::new();
    match (across, start) {
        (true, true) => {
            chevron.move_to(cx + arm / 2.0, cy - arm);
            chevron.line_to(cx - arm / 2.0, cy);
            chevron.line_to(cx + arm / 2.0, cy + arm);
        }
        (true, false) => {
            chevron.move_to(cx - arm / 2.0, cy - arm);
            chevron.line_to(cx + arm / 2.0, cy);
            chevron.line_to(cx - arm / 2.0, cy + arm);
        }
        (false, true) => {
            chevron.move_to(cx - arm, cy + arm / 2.0);
            chevron.line_to(cx, cy - arm / 2.0);
            chevron.line_to(cx + arm, cy + arm / 2.0);
        }
        (false, false) => {
            chevron.move_to(cx - arm, cy - arm / 2.0);
            chevron.line_to(cx, cy + arm / 2.0);
            chevron.line_to(cx + arm, cy - arm / 2.0);
        }
    }
    if let Some(chevron) = chevron.finish() {
        let stroke = Stroke {
            width: (1.8 * s).max(1.0),
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(
            &chevron,
            &paint_of(Colour {
                a: 0.9,
                ..tokens.backdrop_text
            }),
            &stroke,
            Transform::identity(),
            None,
        );
    }
}

/// A window's title bar as the overview draws it, `w` by `h` pixels: the
/// same for every window, focused or not, in the focused bar's colours
/// with the title centred (`title_size` pixels per em), or no title when
/// it is too small to read.
pub fn bar(
    w: u32,
    h: u32,
    title: &str,
    title_size: f32,
    tokens: &Tokens,
    text: Option<&mut Text>,
) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    pixmap.fill(colour_of(tokens.title_bar_focused, 1.0));
    if let Some(text) = text.filter(|_| title_size >= 7.0) {
        text.set_size(title_size);
        let (fw, fh) = (w as f32, h as f32);
        centred(
            &mut pixmap,
            text,
            title,
            fw / 2.0,
            fh / 2.0,
            fw * 0.8,
            tokens.title_text,
        );
    }
    Some(pixmap)
}

/// A window's name pill: its app's `icon`, when it has one, and its
/// `title`, light on a dark pill over the backdrop, at most `most`
/// logical pixels wide.
pub fn name(
    title: &str,
    icon: Option<&Pixmap>,
    most: i32,
    scale: f64,
    tokens: &Tokens,
    text: &mut Text,
) -> Option<Pixmap> {
    let s = scale as f32;
    text.set_size(NAME * s);
    let icon_w = icon.map_or(0.0, |_| NAME_ICON as f32 * s + 6.0 * s);
    let room = (most as f32 * s - 2.0 * PILL_PAD * s - icon_w).max(0.0);
    let words = text.fit(title, room);
    let w = (text.width(&words) + icon_w + 2.0 * PILL_PAD * s).ceil();
    let h = (PILL as f32 * s).ceil();
    let mut pixmap = Pixmap::new(w.max(1.0) as u32, h as u32)?;
    let pill = rounded(0.0, 0.0, w, h, h / 2.0)?;
    pixmap.fill_path(
        &pill,
        &paint_of(Colour {
            r: tokens.backdrop_deep.r * 0.55,
            g: tokens.backdrop_deep.g * 0.55,
            b: tokens.backdrop_deep.b * 0.55,
            a: 0.86,
        }),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let mut x = PILL_PAD * s;
    if let Some(icon) = icon {
        let y = ((h - icon.height() as f32) / 2.0).round() as i32;
        pixmap.draw_pixmap(
            x.round() as i32,
            y,
            icon.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        x += icon_w;
    }
    let baseline = (h / 2.0 + (text.ascent() + text.descent()) / 2.0).round();
    text.draw_on(
        &mut pixmap,
        &words,
        x.round(),
        baseline,
        tokens.backdrop_text,
    );
    Some(pixmap)
}

/// The close button over a window's corner: a circle in the panel text's
/// colour with the shell's close icon in the panel's.
pub fn close(scale: f64, tokens: &Tokens) -> Option<Pixmap> {
    let s = scale as f32;
    let side = (CLOSE as f32 * s).ceil();
    let mut pixmap = Pixmap::new(side as u32, side as u32)?;
    let disc = tiny_skia::PathBuilder::from_circle(side / 2.0, side / 2.0, side / 2.0)?;
    pixmap.fill_path(
        &disc,
        &paint_of(tokens.panel_text),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let px = (side / 2.0).round() as u32;
    if let Some(mask) = edel::icons::mask("close", px) {
        let at = ((side - px as f32) / 2.0).round() as i64;
        let [r, g, b, _] = tokens.panel.bytes();
        let width = pixmap.width() as i64;
        let data = pixmap.data_mut();
        for (i, p) in mask.data().chunks_exact(4).enumerate() {
            let a = f32::from(p[3]) / 255.0;
            if a <= 0.0 {
                continue;
            }
            let (x, y) = (at + (i as u32 % px) as i64, at + (i as u32 / px) as i64);
            let j = ((y * width + x) * 4) as usize;
            for (k, c) in [r, g, b].into_iter().enumerate() {
                let under = f32::from(data[j + k]);
                data[j + k] = (f32::from(c) * a + under * (1.0 - a)).round() as u8;
            }
        }
    }
    Some(pixmap)
}

/// Paints `mask`, an icon from `edel::icons`, in `colour` at `x`, `y`.
fn tinted(pixmap: &mut Pixmap, mask: &Pixmap, x: i32, y: i32, colour: Colour) {
    let [r, g, b, _] = colour.bytes();
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    let side = mask.width() as i32;
    let data = pixmap.data_mut();
    for (i, p) in mask.data().chunks_exact(4).enumerate() {
        let a = f32::from(p[3]) / 255.0 * colour.a;
        if a <= 0.0 {
            continue;
        }
        let (px, py) = (x + i as i32 % side, y + i as i32 / side);
        if px < 0 || py < 0 || px >= w || py >= h {
            continue;
        }
        let j = ((py * w + px) * 4) as usize;
        for (k, c) in [r, g, b].into_iter().enumerate() {
            let under = f32::from(data[j + k]);
            data[j + k] = (f32::from(c) * a + under * (1.0 - a)).round() as u8;
        }
        let under = f32::from(data[j + 3]);
        data[j + 3] = (255.0 * a + under * (1.0 - a)).round() as u8;
    }
}

/// The overview's search field (M5.2j-b3), `w` by `h` logical pixels at
/// `scale`: a pill in the panel's colours with the search icon, and what
/// was typed with a caret after it, or `placeholder` dimmed when nothing
/// was.
pub fn search(
    w: i32,
    h: i32,
    query: &str,
    placeholder: &str,
    scale: f64,
    tokens: &Tokens,
    text: &mut Text,
) -> Option<Pixmap> {
    let s = scale as f32;
    let (pw, ph) = ((w as f32 * s).ceil(), (h as f32 * s).ceil());
    let mut pixmap = Pixmap::new(pw as u32, ph as u32)?;
    let pill = rounded(0.5, 0.5, pw - 1.0, ph - 1.0, ph / 2.0)?;
    pixmap.fill_path(
        &pill,
        &paint_of(Colour {
            a: 0.94,
            ..tokens.panel
        }),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let hairline = Stroke {
        width: s.max(1.0),
        ..Stroke::default()
    };
    pixmap.stroke_path(
        &pill,
        &paint_of(Colour {
            a: 0.10,
            ..tokens.panel_text
        }),
        &hairline,
        Transform::identity(),
        None,
    );
    let icon = (16.0 * s).round() as u32;
    let pad = 14.0 * s;
    if let Some(mask) = edel::icons::mask("search", icon) {
        let y = ((ph - icon as f32) / 2.0).round() as i32;
        tinted(
            &mut pixmap,
            &mask,
            pad.round() as i32,
            y,
            tokens.title_text_unfocused,
        );
    }
    text.set_size(NAME * s);
    let x = (pad + icon as f32 + 8.0 * s).round();
    let room = (pw - x - pad).max(0.0);
    let baseline = (ph / 2.0 + (text.ascent() + text.descent()) / 2.0).round();
    if query.is_empty() {
        let words = text.fit(placeholder, room);
        text.draw_on(
            &mut pixmap,
            &words,
            x,
            baseline,
            tokens.title_text_unfocused,
        );
    } else {
        // The end of a long query shows, as it is where the caret is.
        let mut shown: String = query.to_string();
        while text.width(&shown) > room - 4.0 * s && !shown.is_empty() {
            shown.remove(0);
        }
        text.draw_on(&mut pixmap, &shown, x, baseline, tokens.panel_text);
        let caret_x = x + text.width(&shown) + 2.0 * s;
        if let Some(caret) = Rect::from_xywh(caret_x, ph * 0.28, (1.5 * s).max(1.0), ph * 0.44) {
            pixmap.fill_rect(caret, &paint_of(tokens.accent), Transform::identity(), None);
        }
    }
    Some(pixmap)
}

/// One row of the search's card: its icon, its name and what it is
/// ("Window" or "App").
pub struct Row<'a> {
    pub icon: Option<&'a Pixmap>,
    pub name: &'a str,
    pub kind: &'a str,
}

/// The card of results under the search field, `w` logical pixels wide at
/// `scale`: a rounded card in the panel's colours, `CARD_PAD` above and
/// below, each row `ROW` high with its icon, name and kind, the chosen one
/// in the accent.
pub fn results(
    rows: &[Row],
    chosen: usize,
    w: i32,
    scale: f64,
    tokens: &Tokens,
    text: &mut Text,
) -> Option<Pixmap> {
    let (row, pad) = (crate::glance_search::ROW, crate::glance_search::CARD_PAD);
    let s = scale as f32;
    let pw = (w as f32 * s).ceil();
    let ph = ((rows.len() as i32 * row + 2 * pad) as f32 * s).ceil();
    let mut pixmap = Pixmap::new(pw as u32, ph as u32)?;
    let card = rounded(0.5, 0.5, pw - 1.0, ph - 1.0, 14.0 * s)?;
    pixmap.fill_path(
        &card,
        &paint_of(Colour {
            a: 0.97,
            ..tokens.panel
        }),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let hairline = Stroke {
        width: s.max(1.0),
        ..Stroke::default()
    };
    pixmap.stroke_path(
        &card,
        &paint_of(Colour {
            a: 0.10,
            ..tokens.panel_text
        }),
        &hairline,
        Transform::identity(),
        None,
    );
    let (rh, inset) = (row as f32 * s, 6.0 * s);
    for (i, r) in rows.iter().enumerate() {
        let y = (pad as f32 * s) + i as f32 * rh;
        let lit = i == chosen;
        if lit {
            if let Some(back) = rounded(inset, y + 2.0 * s, pw - 2.0 * inset, rh - 4.0 * s, 9.0 * s)
            {
                pixmap.fill_path(
                    &back,
                    &paint_of(tokens.accent),
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
        let (ink, dim) = if lit {
            (
                tokens.accent_text,
                Colour {
                    a: 0.8,
                    ..tokens.accent_text
                },
            )
        } else {
            (tokens.panel_text, tokens.title_text_unfocused)
        };
        let mut x = inset + 10.0 * s;
        let side = (20.0 * s).round() as u32;
        if let Some(icon) = r.icon {
            let iy = (y + (rh - icon.height() as f32) / 2.0).round() as i32;
            pixmap.draw_pixmap(
                x.round() as i32,
                iy,
                icon.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        } else if let Some(mask) = edel::icons::mask("app-generic", side) {
            // No icon of its own: the shell's generic one, in the row's ink.
            let iy = (y + (rh - side as f32) / 2.0).round() as i32;
            tinted(&mut pixmap, &mask, x.round() as i32, iy, dim);
        }
        x += 20.0 * s + 10.0 * s;
        text.set_size(SMALL * s);
        let kind_w = text.width(r.kind);
        let kind_x = pw - inset - 12.0 * s - kind_w;
        let baseline = (y + rh / 2.0 + (text.ascent() + text.descent()) / 2.0).round();
        text.draw_on(&mut pixmap, r.kind, kind_x.round(), baseline, dim);
        text.set_size(NAME * s);
        let words = text.fit(r.name, (kind_x - x - 10.0 * s).max(0.0));
        let baseline = (y + rh / 2.0 + (text.ascent() + text.descent()) / 2.0).round();
        text.draw_on(&mut pixmap, &words, x.round(), baseline, ink);
    }
    Some(pixmap)
}
