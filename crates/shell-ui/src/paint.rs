//! What the panel looks like (M5.1b), drawn with tiny-skia into a pixmap
//! the size of the panel's buffer: the panel in its token colour, the
//! menu button's icon at the start, the clock at the end and, on the Full
//! and Balanced tiers, rounded fillets where the panel meets the screen's
//! sides, so the area above it reads as one rounded shape (REVIEW-shells.md).
//! Sizes are logical pixels times the buffer's scale; colours come from
//! the design tokens.

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Transform};

use edel::tokens::{Colour, Tokens};

/// Everything the panel shows, at one scale.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// The buffer's size in its pixels: the panel and, above it, the
    /// fillets' strip.
    pub width: u32,
    pub height: u32,
    pub scale: u32,
    /// The clock's text, such as `14:05`.
    pub clock: String,
    /// Whether the fillets are drawn (not on the Lite tier).
    pub fillets: bool,
}

/// How tall the strip above the panel is, for the fillets, in logical
/// pixels: the tokens' corner radius.
pub fn fillet_height(tokens: &Tokens) -> u32 {
    tokens.radius
}

/// The fonts and glyph cache text is drawn with, loaded once.
pub struct Text {
    fonts: FontSystem,
    glyphs: SwashCache,
}

impl Text {
    pub fn load() -> Text {
        Text {
            fonts: FontSystem::new(),
            glyphs: SwashCache::new(),
        }
    }
}

/// tiny-skia's colour for a token colour.
fn colour(c: Colour) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(c.r, c.g, c.b, c.a).unwrap_or(tiny_skia::Color::BLACK)
}

fn paint_of(c: Colour) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(colour(c));
    paint.anti_alias = true;
    paint
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
/// side and the panel's top, cut by a quarter circle of radius `r`, at
/// `x` (the screen's side) and `bottom` (the panel's top). `left` says
/// which side.
fn fillet(x: f32, bottom: f32, r: f32, left: bool) -> Option<Path> {
    let k = r * 0.552_284_8;
    let mut p = PathBuilder::new();
    // From the corner at the screen's side, down to the panel, along it,
    // and back up round the curve.
    let out = if left { x + r } else { x - r };
    let bend = if left { x + r - k } else { x - r + k };
    p.move_to(x, bottom - r);
    p.line_to(x, bottom);
    p.line_to(out, bottom);
    p.cubic_to(bend, bottom, x, bottom - r + k, x, bottom - r);
    p.close();
    p.finish()
}

/// Draws `look` into `pixmap`, which is `look.width` by `look.height`.
pub fn paint(pixmap: &mut Pixmap, look: &Look, tokens: &Tokens, text: Option<&mut Text>) {
    let s = look.scale.max(1) as f32;
    let (w, h) = (look.width as f32, look.height as f32);
    let strip = fillet_height(tokens) as f32 * s;
    let panel_h = h - strip;
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let panel = paint_of(tokens.panel);
    if let Some(rect) = Rect::from_xywh(0.0, strip, w, panel_h) {
        pixmap.fill_rect(rect, &panel, Transform::identity(), None);
    }
    if look.fillets {
        for path in [
            fillet(0.0, strip, strip, true),
            fillet(w, strip, strip, false),
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
    // The menu button's icon: four rounded squares, the launcher to come
    // (M5.3), centred in a square as tall as the panel.
    let icon = paint_of(tokens.panel_text);
    let cell = (panel_h * 0.16).round();
    let gap = (cell * 0.5).round();
    let left = ((panel_h - (2.0 * cell + gap)) / 2.0).round();
    let top = strip + left;
    for (i, j) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let x = left + i * (cell + gap);
        let y = top + j * (cell + gap);
        if let Some(square) = rounded(x, y, cell, cell, cell * 0.3) {
            pixmap.fill_path(
                &square,
                &icon,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    if let Some(text) = text {
        clock(pixmap, look, tokens, text, strip, panel_h);
    }
}

/// The clock, centred in the panel's height, its end a panel's height
/// from the screen's end.
fn clock(
    pixmap: &mut Pixmap,
    look: &Look,
    tokens: &Tokens,
    text: &mut Text,
    strip: f32,
    panel_h: f32,
) {
    let s = look.scale.max(1) as f32;
    let size = tokens.panel_text_size as f32 * s;
    let mut buffer = Buffer::new(&mut text.fonts, Metrics::new(size, size * 1.25));
    buffer.set_size(Some(look.width as f32), Some(panel_h));
    let attrs = Attrs::new().family(Family::Name("Inter"));
    buffer.set_text(&look.clock, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut text.fonts, false);
    let width = buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0, f32::max);
    let x = (look.width as f32 - panel_h * 0.4 - width).round() as i32;
    let y = (strip + (panel_h - size * 1.25) / 2.0).round() as i32;
    let c = tokens.panel_text;
    let ink = Color::rgba(
        (c.r * 255.0) as u8,
        (c.g * 255.0) as u8,
        (c.b * 255.0) as u8,
        255,
    );
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let pixels = pixmap.data_mut();
    buffer.draw(
        &mut text.fonts,
        &mut text.glyphs,
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

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pixmap.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    fn look(fillets: bool) -> (Look, Tokens) {
        let tokens = Tokens::built_in();
        let strip = fillet_height(&tokens);
        let look = Look {
            width: 1280,
            height: tokens.panel_height + strip,
            scale: 1,
            clock: "14:05".into(),
            fillets,
        };
        (look, tokens)
    }

    #[test]
    fn the_panel_is_its_token_colour_with_the_strip_above_it_clear() {
        let (look, tokens) = look(true);
        let mut pixmap = Pixmap::new(look.width, look.height).unwrap();
        paint(&mut pixmap, &look, &tokens, None);
        let strip = fillet_height(&tokens);
        let panel = tokens.panel.bytes();
        // The panel's middle and its bottom row, past the icon.
        assert_eq!(pixel(&pixmap, 640, strip + 20), panel);
        assert_eq!(pixel(&pixmap, 640, look.height - 1), panel);
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
    fn lite_draws_no_fillets() {
        let (look, tokens) = look(false);
        let mut pixmap = Pixmap::new(look.width, look.height).unwrap();
        paint(&mut pixmap, &look, &tokens, None);
        let strip = fillet_height(&tokens);
        assert_eq!(pixel(&pixmap, 0, strip - 1)[3], 0);
    }

    #[test]
    fn the_menu_icon_is_drawn_in_the_text_colour() {
        let (look, tokens) = look(false);
        let mut pixmap = Pixmap::new(look.width, look.height).unwrap();
        paint(&mut pixmap, &look, &tokens, None);
        let strip = fillet_height(&tokens);
        // The first square's middle: about a third into the panel.
        let h = tokens.panel_height as f32;
        let cell = (h * 0.16).round();
        let left = ((h - (2.0 * cell + (cell * 0.5).round())) / 2.0).round();
        let mid = (left + cell / 2.0) as u32;
        assert_eq!(pixel(&pixmap, mid, strip + mid), tokens.panel_text.bytes());
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
