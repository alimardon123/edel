//! What the compositor and shell-ui both draw with (M6.17, ADR-010): a
//! token colour as tiny-skia's, rounded rectangles, colour mixing and the
//! soft shadow a card or a window casts, written once so a change to how
//! a corner curves or a shadow falls reaches every surface. Each part
//! keeps only what is its own: the sizes and strengths it asks for.

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

use crate::pictures::box_blur;
use crate::tokens::Colour;

/// tiny-skia's colour for a token colour.
pub fn colour(c: Colour) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(c.r, c.g, c.b, c.a).unwrap_or(tiny_skia::Color::BLACK)
}

/// A paint of one colour, its edges smoothed.
pub fn paint_of(c: Colour) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(colour(c));
    paint.anti_alias = true;
    paint
}

/// `a` moved towards `b` by `t`, 0 to 1, opaque: text dimmed towards the
/// surface it lies on.
pub fn mix(a: Colour, b: Colour, t: f32) -> Colour {
    Colour {
        a: 1.0,
        ..a.towards(b, t)
    }
}

/// A rectangle with corners of radius `r`, as a path; the radius is at
/// most half the shorter side, and each corner the cubic that best
/// follows a quarter circle.
pub fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
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

/// The shadow of a shape `w` by `h` pixels with corners `r`, lying `room`
/// pixels in from each side of a pixmap that much bigger: two layers cast
/// by one light above, a close sharp one and a far soft one, `blur` pixels
/// soft and falling `fall` pixels at most, in `c` (its alpha the darkest
/// the shadow gets). Drawn once per size by its callers, and again on each
/// frame a window is resized, so only the corners and the edges are
/// blurred: a shape that wide is blurred at the smallest size whose middle
/// no corner reaches, and its middle row and column are stretched to the
/// size asked for, which gives the same pixels at a fraction of the work.
pub fn shadow(
    w: u32,
    h: u32,
    room: u32,
    r: f32,
    blur: f32,
    fall: f32,
    c: Colour,
) -> Option<Pixmap> {
    // How far in from a side a corner's curve and the blur still change
    // the shadow; past that every row (or column) is the same.
    let near =
        r.max(0.0).ceil() as u32 + blur.max(0.0).ceil() as u32 + fall.max(0.0).ceil() as u32 + 1;
    let core = 2 * near + 1;
    let (sw, sh) = (w.min(core), h.min(core));
    let small = blurred(sw, sh, room, r, blur, fall, c)?;
    if (sw, sh) == (w, h) {
        return Some(small);
    }
    let (pw, ph) = (w + 2 * room, h + 2 * room);
    let (spw, sph) = ((sw + 2 * room) as usize, sh + 2 * room);
    // A row of the small shadow stretched: its left part, its middle
    // pixel repeated, its right part.
    let (left, right) = ((spw - 1) / 2, spw - (spw - 1) / 2 - 1);
    let stretch = |from: &[u8], to: &mut [u8]| {
        let middle = &from[left * 4..left * 4 + 4];
        to[..left * 4].copy_from_slice(&from[..left * 4]);
        let tail = to.len() - right * 4;
        for pixel in to[left * 4..tail].chunks_exact_mut(4) {
            pixel.copy_from_slice(middle);
        }
        to[tail..].copy_from_slice(&from[from.len() - right * 4..]);
    };
    // The same for the rows: the top ones, the middle one repeated, the
    // bottom ones.
    let (top, bottom) = ((sph - 1) / 2, sph - (sph - 1) / 2 - 1);
    let mut out = Pixmap::new(pw, ph)?;
    let from = small.data();
    let to = out.data_mut();
    let line = pw as usize * 4;
    for y in 0..ph {
        let sy = if y <= top {
            y
        } else if y >= ph - bottom {
            y - (ph - sph)
        } else if y > top + 1 {
            // Every middle row is the one before it.
            let at = y as usize * line;
            to.copy_within(at - line..at, at);
            continue;
        } else {
            top
        };
        let sy = sy as usize * spw * 4;
        stretch(
            &from[sy..sy + spw * 4],
            &mut to[y as usize * line..(y as usize + 1) * line],
        );
    }
    Some(out)
}

/// `shadow`'s every pixel, blurred at the size asked for.
fn blurred(w: u32, h: u32, room: u32, r: f32, blur: f32, fall: f32, c: Colour) -> Option<Pixmap> {
    let (pw, ph) = (w + 2 * room, h + 2 * room);
    let mut cover = vec![0.0f32; (pw * ph) as usize];
    let black = Colour {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
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
            black,
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
    let [cr, cg, cb, ca] = c.bytes();
    let mut out = Pixmap::new(pw, ph)?;
    for (pixel, c) in out.data_mut().chunks_exact_mut(4).zip(cover) {
        let a = c * f32::from(ca) / 255.0;
        let mul = |v: u8| (f32::from(v) * a).round() as u8;
        pixel.copy_from_slice(&[mul(cr), mul(cg), mul(cb), (a * 255.0).round() as u8]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Colour = Colour {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    const WHITE: Colour = Colour {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 0.5,
    };

    #[test]
    fn mixing_moves_a_colour_towards_another_and_is_opaque() {
        let half = mix(BLACK, WHITE, 0.5);
        assert_eq!((half.r, half.g, half.b, half.a), (0.5, 0.5, 0.5, 1.0));
        assert_eq!(mix(BLACK, WHITE, 0.0).r, 0.0);
        // `towards` keeps the first colour's alpha, for a tint.
        assert_eq!(WHITE.towards(BLACK, 1.0).a, 0.5);
    }

    #[test]
    fn a_corner_is_at_most_half_the_shorter_side() {
        let mut pixmap = Pixmap::new(40, 20).unwrap();
        fill(&mut pixmap, 0.0, 0.0, 40.0, 20.0, 100.0, BLACK);
        let alpha = |x: u32, y: u32| pixmap.pixel(x, y).unwrap().alpha();
        // A pill: its ends are round, its middle filled to the edge.
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(20, 0), 255);
        assert_eq!(alpha(20, 10), 255);
    }

    #[test]
    fn a_wide_shadow_is_its_corners_and_edges_stretched() {
        // Wider and taller than the core in turn, and both, against every
        // pixel blurred at full size.
        for (w, h) in [(300, 40), (40, 260), (300, 260)] {
            let fast = shadow(w, h, 30, 6.0, 24.0, 6.0, BLACK).unwrap();
            let whole = blurred(w, h, 30, 6.0, 24.0, 6.0, BLACK).unwrap();
            let worst = fast
                .data()
                .iter()
                .zip(whole.data())
                .map(|(a, b)| a.abs_diff(*b))
                .max();
            assert_eq!(worst, Some(0), "{w} by {h}");
        }
    }

    #[test]
    fn a_shadow_falls_below_its_shape_and_fades_out() {
        let pixmap = shadow(40, 20, 16, 4.0, 12.0, 6.0, BLACK).unwrap();
        let alpha = |x: u32, y: u32| pixmap.pixel(x, y).unwrap().alpha();
        let (above, below) = (alpha(36, 10), alpha(36, 46));
        assert!(below > above, "{below} below, {above} above");
        assert_eq!(alpha(0, 0), 0);
        assert!(alpha(36, 26) > 0);
    }
}
