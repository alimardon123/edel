//! Asking the compositor to blur behind popups (M5.14, built in M8.21 part
//! 21d), with ext-background-effect-v1. The compositor says with its
//! `capabilities` event whether it blurs (its Full tier only). Where it
//! does and the panels are see-through, each popup's cards get a blur
//! region and are drawn as glass: their fill at the panels' opacity, so the
//! blurred desktop shows through as frosted glass. `glass` makes that from
//! the finished picture, and is pure and tested without a display.

use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle, WEnum};
use smithay_client_toolkit::reexports::protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{self, Capability, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::{self, ExtBackgroundEffectSurfaceV1},
};
use tiny_skia::{BlendMode, FillRule, Paint, Pixmap, Transform};

use edel::tokens::Colour;

use crate::Shell;

/// The compositor's background-effect manager, version 1, or `None` when
/// it has none: then no popup asks for blur and every card stays opaque.
pub fn bind(globals: &GlobalList, qh: &QueueHandle<Shell>) -> Option<ExtBackgroundEffectManagerV1> {
    globals
        .bind(qh, 1..=1, ())
        .inspect_err(|_| {
            eprintln!(
                "edel-shell-ui: the compositor offers no ext_background_effect_manager_v1, so no live blur"
            )
        })
        .ok()
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for Shell {
    /// The compositor's capabilities: a change to whether it blurs turns
    /// the glass on or off, and redraws the popups in it.
    fn event(
        shell: &mut Shell,
        _: &ExtBackgroundEffectManagerV1,
        event: ext_background_effect_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        if let ext_background_effect_manager_v1::Event::Capabilities { flags } = event {
            let blur = matches!(flags, WEnum::Value(c) if c.contains(Capability::Blur));
            if blur != shell.live_blur {
                shell.live_blur = blur;
                let state = if blur { "on" } else { "off" };
                eprintln!("edel-shell-ui: live blur {state}");
                shell.glass_changed();
            }
        }
    }
}

impl Dispatch<ExtBackgroundEffectSurfaceV1, ()> for Shell {
    /// The surface's effect has no events.
    fn event(
        _: &mut Shell,
        _: &ExtBackgroundEffectSurfaceV1,
        _: ext_background_effect_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
    }
}

/// The rectangles, `(x, y, w, h)` in whole logical pixels from 0,0, that
/// make a `w` by `h` rounded rectangle with corners `radius` for a
/// `wl_region`: each corner's rows as rectangles inset by the arc, rows of
/// one inset merged, and the middle full width. A radius of zero, or less,
/// is the whole rectangle.
pub fn rows(w: i32, h: i32, radius: f32) -> Vec<(i32, i32, i32, i32)> {
    if w <= 0 || h <= 0 {
        return Vec::new();
    }
    let r = radius.min(w as f32 / 2.0).min(h as f32 / 2.0);
    if r <= 0.0 {
        return vec![(0, 0, w, h)];
    }
    let n = r.ceil() as i32;
    // The inset of row `y` from the card's side at its corner, never past
    // the middle.
    let inset = |y: i32| {
        let d = r - y as f32 - 0.5;
        ((r - (r * r - d * d).max(0.0).sqrt()).ceil() as i32).min(w / 2)
    };
    let insets: Vec<i32> = (0..n).map(inset).collect();
    let mut out = Vec::new();
    let mut start = 0;
    while start < n {
        let i = insets[start as usize];
        let mut end = start + 1;
        while end < n && insets[end as usize] == i {
            end += 1;
        }
        let len = end - start;
        out.push((i, start, w - 2 * i, len));
        // The bottom rows mirror the top ones: rows `h - end` to `h - start`.
        out.push((i, h - end, w - 2 * i, len));
        start = end;
    }
    if h - 2 * n > 0 {
        out.push((0, n, w, h - 2 * n));
    }
    out
}

/// Whether the point `(px, py)` lies within `grow` pixels of the rounded
/// rectangle `(x, y, w, h, radius)`.
fn inside(px: f32, py: f32, (x, y, w, h, radius): (f32, f32, f32, f32, f32), grow: f32) -> bool {
    let r = radius.clamp(0.0, (w.min(h) / 2.0).max(0.0));
    let qx = ((px - (x + w / 2.0)).abs() - (w / 2.0 - r)).max(0.0);
    let qy = ((py - (y + h / 2.0)).abs() - (h / 2.0 - r)).max(0.0);
    (qx * qx + qy * qy).sqrt() <= r + grow
}

/// Turns the cards of `pixmap` into glass (M8.21 21d). Each `cards` entry
/// is a rounded rectangle `(x, y, w, h, radius)` in device pixels from the
/// pixmap's top left corner. Inside it, every pixel is taken through colour
/// to alpha against the panel's colour `panel`, as GIMP's does: its alpha
/// is the largest distance from the panel's colour, in the channels' own
/// scale. The panel part of it then fades to `opacity` (0 to 1), so a
/// card's plain fill becomes the panel at that opacity, while text, icons
/// and lit rows keep their colour. Pixels outside every card are left as
/// they are. The pixmap holds premultiplied colour, and its edges are kept
/// soft by the pixel's own alpha.
pub fn glass(
    pixmap: &mut Pixmap,
    panel: Colour,
    opacity: f32,
    cards: &[(f32, f32, f32, f32, f32)],
) {
    let o = opacity.clamp(0.0, 1.0);
    let c = [panel.r, panel.g, panel.b];
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let data = pixmap.data_mut();
    for &card in cards {
        let (x, y, w, h, _) = card;
        // One pixel of slack, for the anti-aliased edge.
        let x0 = (x.floor() as i32 - 1).clamp(0, pw);
        let y0 = (y.floor() as i32 - 1).clamp(0, ph);
        let x1 = (((x + w).ceil() as i32) + 1).clamp(0, pw);
        let y1 = (((y + h).ceil() as i32) + 1).clamp(0, ph);
        for py in y0..y1 {
            for px in x0..x1 {
                if !inside(px as f32 + 0.5, py as f32 + 0.5, card, 1.0) {
                    continue;
                }
                let at = ((py * pw + px) * 4) as usize;
                let pixel = &mut data[at..at + 4];
                let source = pixel[3] as f32 / 255.0;
                if source <= 0.0 {
                    continue;
                }
                let p = [0, 1, 2].map(|k| (pixel[k] as f32 / 255.0 / source).min(1.0));
                // Colour to alpha: the panel's colour is what becomes clear,
                // a channel that cannot be divided by its panel value counts
                // as no change.
                let mut a = 0.0f32;
                for k in 0..3 {
                    let (v, ck) = (p[k], c[k]);
                    let t = if v > ck && ck < 1.0 {
                        (v - ck) / (1.0 - ck)
                    } else if v < ck && ck > 0.0 {
                        (ck - v) / ck
                    } else {
                        0.0
                    };
                    a = a.max(t);
                }
                let a = a.clamp(0.0, 1.0);
                // The panel part at opacity `o`; the colour and alpha
                // carry the source pixel's own alpha too.
                let alpha = (a + (1.0 - a) * o) * source;
                let alpha_byte = (alpha * 255.0).round().min(255.0) as u8;
                for k in 0..3 {
                    let col = if a > 0.0 {
                        ((p[k] - c[k]) / a + c[k]).clamp(0.0, 1.0)
                    } else {
                        c[k]
                    };
                    let premultiplied = (col * a + c[k] * (1.0 - a) * o) * source;
                    pixel[k] = (premultiplied * 255.0).round().min(alpha_byte as f32) as u8;
                }
                pixel[3] = alpha_byte;
            }
        }
    }
}

/// Clears the rounded rectangles `(x, y, w, h, radius)` out of `pixmap`
/// (M8.21 21d): a glass card's shadow must not show through it.
pub fn cut(pixmap: &mut Pixmap, shapes: &[(f32, f32, f32, f32, f32)]) {
    let paint = Paint {
        blend_mode: BlendMode::Clear,
        anti_alias: true,
        ..Paint::default()
    };
    for &(x, y, w, h, r) in shapes {
        if let Some(path) = edel::draw::rounded(x, y, w, h, r) {
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether two channel values are within `d` of each other.
    fn near(a: u8, b: u8, d: u8) -> bool {
        a.abs_diff(b) <= d
    }

    /// A `w` by `h` pixmap filled with the premultiplied pixel `px`.
    fn filled(w: u32, h: u32, px: [u8; 4]) -> Pixmap {
        let mut pixmap = Pixmap::new(w, h).unwrap();
        for pixel in pixmap.data_mut().chunks_exact_mut(4) {
            pixel.copy_from_slice(&px);
        }
        pixmap
    }

    fn at(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * pixmap.width() + x) * 4) as usize;
        pixmap.data()[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn a_radius_of_nothing_is_the_whole_rectangle() {
        assert_eq!(rows(30, 20, 0.0), vec![(0, 0, 30, 20)]);
        assert_eq!(rows(30, 20, -3.0), vec![(0, 0, 30, 20)]);
    }

    #[test]
    fn rounded_rows_stay_inside_and_cover_most_of_the_rectangle() {
        for (w, h, r) in [
            (40, 30, 8),
            (100, 24, 12),
            (20, 20, 10),
            (64, 64, 16),
            (9, 40, 4),
        ] {
            let rects = rows(w, h, r as f32);
            let mut area = 0;
            for &(x, y, rw, rh) in &rects {
                assert!(x >= 0 && y >= 0 && rw >= 0 && rh > 0, "{rects:?}");
                assert!(x + rw <= w && y + rh <= h, "{rects:?}");
                area += rw * rh;
            }
            assert!(area < w * h, "{rects:?}");
            assert!(
                area as f32 > (w * h) as f32 - 4.0 * (r * r) as f32,
                "{rects:?}"
            );
        }
    }

    #[test]
    fn the_top_row_is_inset_by_the_corner_and_the_middle_is_full_width() {
        let rects = rows(40, 30, 8.0);
        let top = rects.iter().find(|r| r.1 == 0).unwrap();
        assert!(top.0 > 0 && top.0 < 8, "{rects:?}");
        assert!(rects.contains(&(0, 8, 40, 14)), "{rects:?}");
    }

    #[test]
    fn a_pixel_of_the_panel_is_glass_at_the_panels_opacity() {
        // The panel's colour, 0.2 in each channel, opaque.
        let panel = Colour {
            r: 0.2,
            g: 0.2,
            b: 0.2,
            a: 1.0,
        };
        let mut pixmap = filled(4, 4, [51, 51, 51, 255]);
        glass(&mut pixmap, panel, 0.6, &[(0.0, 0.0, 4.0, 4.0, 0.0)]);
        let got = at(&pixmap, 2, 2);
        // Alpha 0.6 and the panel's colour at that alpha, premultiplied.
        assert!(near(got[3], 153, 2), "{got:?}");
        assert!(near(got[0], 31, 2) && near(got[2], 31, 2), "{got:?}");
    }

    #[test]
    fn text_on_the_panel_keeps_its_colour_and_stays_opaque() {
        let panel = Colour {
            r: 0.2,
            g: 0.2,
            b: 0.2,
            a: 1.0,
        };
        let mut pixmap = filled(4, 4, [255, 255, 255, 255]);
        glass(&mut pixmap, panel, 0.6, &[(0.0, 0.0, 4.0, 4.0, 0.0)]);
        assert_eq!(at(&pixmap, 1, 1), [255, 255, 255, 255]);
    }

    #[test]
    fn a_pixel_outside_every_card_is_untouched() {
        let panel = Colour {
            r: 0.2,
            g: 0.2,
            b: 0.2,
            a: 1.0,
        };
        let mut pixmap = filled(10, 10, [51, 51, 51, 255]);
        glass(&mut pixmap, panel, 0.6, &[(0.0, 0.0, 4.0, 4.0, 0.0)]);
        assert_eq!(at(&pixmap, 9, 9), [51, 51, 51, 255]);
        assert_eq!(at(&pixmap, 6, 6), [51, 51, 51, 255]);
    }

    #[test]
    fn a_cut_clears_the_shape_and_nothing_else() {
        let mut pixmap = filled(10, 10, [0, 0, 0, 255]);
        cut(&mut pixmap, &[(2.0, 2.0, 4.0, 4.0, 0.0)]);
        assert_eq!(at(&pixmap, 3, 3), [0, 0, 0, 0]);
        assert_eq!(at(&pixmap, 0, 0), [0, 0, 0, 255]);
    }
}
