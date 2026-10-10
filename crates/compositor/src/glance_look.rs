//! What the overview paints itself (M5.2j-b, `docs/mockups/shell/overview.jpg`):
//! the strip's tray with each workspace's label and the frame that adds
//! one, each spread window's name under it, and the close button over the
//! window under the pointer. Each is a tiny-skia picture at the screen's
//! scale, painted again only when what it shows changes; `glance.rs`
//! places them. Colours come from the tokens.

use tiny_skia::{
    FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, PixmapPaint, Point, Rect,
    SpreadMode, Stroke, Transform,
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

/// The tray of `plan` on a screen at `scale`: a rounded card in the
/// panel's colour, a hairline inside its edge, each frame's `labels`
/// under it, and the add frame drawn as a dashed outline with a plus and
/// "New" (`new`). Its size is the tray's in pixels.
pub fn tray(
    plan: &Plan,
    labels: &[String],
    new: &str,
    scale: f64,
    tokens: &Tokens,
    text: Option<&mut Text>,
) -> Option<Pixmap> {
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
    let card = rounded(0.5, 0.5, w - 1.0, h - 1.0, TRAY_RADIUS as f32 * s)?;
    pixmap.fill_path(
        &card,
        &paint_of(Colour {
            a: 0.82,
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
    if let Some(add) = plan.add {
        let (x, y, aw, ah) = local(add);
        if let Some(outline) = rounded(x + 0.5, y + 0.5, aw - 1.0, ah - 1.0, 6.0 * s) {
            let dashed = Stroke {
                width: (1.5 * s).max(1.0),
                dash: tiny_skia::StrokeDash::new(vec![4.0 * s, 3.0 * s], 0.0),
                ..Stroke::default()
            };
            let ink = paint_of(Colour {
                a: 0.55,
                ..tokens.panel_text
            });
            pixmap.stroke_path(&outline, &ink, &dashed, Transform::identity(), None);
            let (cx, cy, arm) = (x + aw / 2.0, y + ah / 2.0, 7.0 * s);
            let mut plus = PathBuilder::new();
            plus.move_to(cx - arm, cy);
            plus.line_to(cx + arm, cy);
            plus.move_to(cx, cy - arm);
            plus.line_to(cx, cy + arm);
            if let Some(plus) = plus.finish() {
                let thick = Stroke {
                    width: (1.6 * s).max(1.0),
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
        text.set_size(SMALL * s);
        let ink = Colour {
            a: 0.85,
            ..tokens.panel_text
        };
        let mut put = |r: Rectangle<i32, Logical>, words: &str, text: &mut Text| {
            let (x, y, fw, fh) = local(r);
            let centre_y = y + fh + LABEL as f32 * s / 2.0;
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
        for (frame, label) in plan.frames.iter().zip(labels) {
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
    let colour = |a: f32| {
        let [r, g, b, _] = tokens.panel_text.bytes();
        tiny_skia::Color::from_rgba8(r, g, b, (a * 255.0).round() as u8)
    };
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
                a: 0.85,
                ..tokens.panel_text
            }),
            &stroke,
            Transform::identity(),
            None,
        );
    }
}

/// A window's name pill: its app's `icon`, when it has one, and its
/// `title`, in the panel's colours, at most `most` logical pixels wide.
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
            a: 0.88,
            ..tokens.panel
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
    text.draw_on(&mut pixmap, &words, x.round(), baseline, tokens.panel_text);
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
