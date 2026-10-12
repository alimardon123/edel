//! The glass behind see-through panels (M5.12b3, ADR-002's glass decision):
//! the wallpaper blurred once, at a quarter of the screen's size, and then
//! only a picture, so it costs no more to show on Lite than a flat colour.
//! Shell-ui draws it under its panel's own fill, because only it knows the
//! panel's shape. The lock screen's blurred backgrounds are made here too,
//! so the blur has one copy (ADR-010).

use std::path::Path;

use edel::pictures;
use edel::tokens::Colour;
use edel::wallpapers::Wallpaper;
use tiny_skia::{FillRule, Mask, Pixmap, PixmapPaint, Transform};

/// The blur is made from a picture this many times smaller than the screen
/// in each direction (M5.10c5c).
pub const SHRINK: u32 = 4;

/// The blur radius for `blur` percent, in the quarter-size picture's pixels:
/// the release's 50 is 12, the lock screen's own radius, and 100 is 24.
pub fn radius(blur: u32) -> f32 {
    blur as f32 * 0.24
}

/// What a glass is kept under for `wallpaper`: a picture's path, a colour's
/// `#rrggbb`, or nothing for the background, so a changed wallpaper makes a
/// new one.
pub fn behind(wallpaper: &Wallpaper) -> String {
    match wallpaper {
        Wallpaper::Background => String::new(),
        Wallpaper::Colour(colour) => colour.hex(),
        Wallpaper::Picture(path) => path.display().to_string(),
    }
}

/// A panel's glass as `paint` draws it: the screen's glass, the scale that
/// stretches it over the screen, and the panel's top left on the screen, in
/// device pixels.
pub type Behind<'a> = (&'a Pixmap, (f32, f32), (f32, f32));

/// The glass behind the panels of one screen (M5.12b3). Made again only
/// when the wallpaper, the screen's size or the blur changes, and kept
/// until then, so a panel's frame costs no more than a copy. It is kept
/// as small as it was made, a blurred picture at a quarter of the screen
/// (about 250 KB at 1280 by 800, 2 MB at 4K) and a colour as one pixel,
/// with the scale that draws it over the whole screen (ADR-002's glass
/// decision).
#[derive(Default)]
pub struct Glass {
    key: String,
    picture: Option<(Pixmap, (f32, f32))>,
    /// How many times a glass was made, counted by the tests.
    #[cfg(test)]
    makes: u32,
}

impl Glass {
    /// The whole screen's glass for `size` device pixels, `wallpaper` and
    /// `blur` percent (0 sharp), with the scale that stretches it over the
    /// screen. None for the background, which has no glass, and for a
    /// picture that cannot be read, which is logged.
    pub fn get(
        &mut self,
        wallpaper: &Wallpaper,
        size: (u32, u32),
        blur: u32,
    ) -> Option<(&Pixmap, (f32, f32))> {
        if *wallpaper == Wallpaper::Background {
            self.key.clear();
            self.picture = None;
            return None;
        }
        let key = format!("{}|{}x{}|{blur}", behind(wallpaper), size.0, size.1);
        if key != self.key {
            self.key = key;
            self.picture = make(wallpaper, size, blur);
            #[cfg(test)]
            {
                self.makes += 1;
            }
            if self.picture.is_some() {
                eprintln!("edel-shell-ui: glass blur {blur} on {}x{}", size.0, size.1);
            }
        }
        self.picture
            .as_ref()
            .map(|(picture, scale)| (picture, *scale))
    }
}

/// Makes the glass of `wallpaper` at `size` and `blur` (`Glass::get`).
fn make(wallpaper: &Wallpaper, size: (u32, u32), blur: u32) -> Option<(Pixmap, (f32, f32))> {
    match wallpaper {
        Wallpaper::Background => None,
        Wallpaper::Colour(colour) => {
            Some((solid((1, 1), *colour)?, (size.0 as f32, size.1 as f32)))
        }
        Wallpaper::Picture(path) if blur == 0 => Some((sharp(path, size)?, (1.0, 1.0))),
        Wallpaper::Picture(path) => blurred_small(path, size, radius(blur)),
    }
}

/// Draws `glass`, stretched by `scale` over the screen, into `pixmap` as
/// the screen behind it: `origin` is the pixmap's top left on the screen,
/// in device pixels, and the glass shows only inside `clip`, with smooth
/// edges.
pub fn under(
    pixmap: &mut Pixmap,
    glass: &Pixmap,
    scale: (f32, f32),
    origin: (f32, f32),
    clip: &tiny_skia::Path,
) {
    let Some(mut mask) = Mask::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    mask.fill_path(clip, FillRule::Winding, true, Transform::identity());
    pixmap.draw_pixmap(
        0,
        0,
        glass.as_ref(),
        &PixmapPaint {
            quality: tiny_skia::FilterQuality::Bilinear,
            ..PixmapPaint::default()
        },
        Transform::from_scale(scale.0, scale.1).post_translate(-origin.0, -origin.1),
        Some(&mask),
    );
}

/// A pixmap of `size` device pixels filled with `colour`.
pub fn solid(size: (u32, u32), colour: Colour) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(size.0, size.1)?;
    pixmap.fill(tiny_skia::Color::from_rgba(
        colour.r, colour.g, colour.b, 1.0,
    )?);
    Some(pixmap)
}

/// The picture at `path` covering `size`, or None when it cannot be read,
/// which is logged.
pub fn sharp(path: &Path, size: (u32, u32)) -> Option<Pixmap> {
    pictures::load(path, size)
        .inspect_err(|why| picture_failed(path, why))
        .ok()
}

/// The picture at `path` blurred for glass: made at a quarter of `size`,
/// blurred with `radius` in those pixels, then drawn up to `size` with
/// smooth edges. None when it cannot be read, which is logged.
pub fn blurred(path: &Path, size: (u32, u32), radius: f32) -> Option<Pixmap> {
    let (low, _) = blurred_small(path, size, radius)?;
    let small = (low.width(), low.height());
    let mut out = Pixmap::new(size.0, size.1)?;
    let smooth = PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    let up = Transform::from_scale(
        size.0 as f32 / small.0 as f32,
        size.1 as f32 / small.1 as f32,
    );
    out.draw_pixmap(0, 0, low.as_ref(), &smooth, up, None);
    Some(out)
}

/// The picture at `path` blurred for glass at a quarter of `size`, with
/// `radius` in those pixels, and the scale that stretches it back to
/// `size`. None when it cannot be read, which is logged.
pub fn blurred_small(path: &Path, size: (u32, u32), radius: f32) -> Option<(Pixmap, (f32, f32))> {
    let small = ((size.0 / SHRINK).max(1), (size.1 / SHRINK).max(1));
    let mut low = pictures::load(path, small)
        .inspect_err(|why| picture_failed(path, why))
        .ok()?;
    pictures::blur(&mut low, radius);
    let scale = (
        size.0 as f32 / small.0 as f32,
        size.1 as f32 / small.1 as f32,
    );
    Some((low, scale))
}

/// Logs a picture of the glass or the lock screen that cannot be read.
fn picture_failed(path: &Path, why: &str) {
    eprintln!(
        "edel-shell-ui: {}",
        crate::messages::picture_not_read(&path.display().to_string(), why)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{ColorU8, PathBuilder, Rect};

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pixmap.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    /// A 4 by 4 glass whose left two columns are `left` and right two `right`.
    fn halves(left: [u8; 4], right: [u8; 4]) -> Pixmap {
        let mut glass = Pixmap::new(4, 4).unwrap();
        for y in 0..4usize {
            for x in 0..4usize {
                let c = if x < 2 { left } else { right };
                glass.pixels_mut()[y * 4 + x] =
                    ColorU8::from_rgba(c[0], c[1], c[2], c[3]).premultiply();
            }
        }
        glass
    }

    fn whole(w: f32, h: f32) -> tiny_skia::Path {
        let mut p = PathBuilder::new();
        p.push_rect(Rect::from_xywh(0.0, 0.0, w, h).unwrap());
        p.finish().unwrap()
    }

    #[test]
    fn the_radius_is_the_lock_screens_at_half_and_scales_to_100() {
        assert_eq!(radius(0), 0.0);
        assert!((radius(50) - 12.0).abs() < 1e-4, "{}", radius(50));
        assert!((radius(100) - 24.0).abs() < 1e-4, "{}", radius(100));
    }

    #[test]
    fn under_takes_the_glass_moved_by_the_origin() {
        let glass = halves([255, 0, 0, 255], [0, 0, 255, 255]);
        let mut pixmap = Pixmap::new(2, 2).unwrap();
        under(
            &mut pixmap,
            &glass,
            (1.0, 1.0),
            (2.0, 0.0),
            &whole(2.0, 2.0),
        );
        assert_eq!(pixel(&pixmap, 0, 0), [0, 0, 255, 255]);
        assert_eq!(pixel(&pixmap, 1, 1), [0, 0, 255, 255]);
    }

    #[test]
    fn under_draws_only_inside_its_clip() {
        let glass = halves([255, 0, 0, 255], [0, 0, 255, 255]);
        let mut pixmap = Pixmap::new(4, 4).unwrap();
        let clip = {
            let mut p = PathBuilder::new();
            p.push_rect(Rect::from_xywh(0.0, 0.0, 2.0, 4.0).unwrap());
            p.finish().unwrap()
        };
        under(&mut pixmap, &glass, (1.0, 1.0), (0.0, 0.0), &clip);
        assert_eq!(pixel(&pixmap, 1, 3), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixmap, 3, 3), [0, 0, 0, 0]);
    }

    #[test]
    fn the_background_has_no_glass_and_a_glass_is_made_once_for_its_key() {
        let mut glass = Glass::default();
        let red = Wallpaper::Colour(Colour {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        assert!(glass.get(&Wallpaper::Background, (8, 8), 50).is_none());
        assert_eq!(glass.makes, 0);
        assert!(glass.get(&red, (8, 8), 50).is_some());
        assert_eq!(glass.makes, 1);
        assert!(glass.get(&red, (8, 8), 50).is_some());
        assert_eq!(glass.makes, 1, "the same key makes no new glass");
        assert!(glass.get(&red, (16, 8), 50).is_some());
        assert_eq!(glass.makes, 2, "a new size makes one");
        assert!(glass.get(&Wallpaper::Background, (16, 8), 50).is_none());
        assert!(glass.get(&red, (16, 8), 50).is_some());
        assert_eq!(glass.makes, 3, "the background drops the glass");
    }

    /// With `EDEL_GLASS_PNG` set, writes a bottom bar over the glass of the
    /// aurora wallpaper as `panel-glass.png` (blur 50) and `panel-sharp.png`
    /// (blur 0) there, for a look.
    #[test]
    fn glass_png() {
        let Some(dir) = std::env::var_os("EDEL_GLASS_PNG") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let picture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../features/wallpapers/usr/share/edel/wallpapers/aurora-light.jpg");
        let tokens = edel::tokens::Tokens::built_in();
        for (name, blur) in [("panel-glass", 50), ("panel-sharp", 0)] {
            let mut glass = Glass::default();
            let (screen, scale) = glass
                .get(&Wallpaper::Picture(picture.clone()), (1280, 800), blur)
                .map(|(p, scale)| (p.clone(), scale))
                .unwrap();
            let mut bar = Pixmap::new(1280, 52).unwrap();
            under(
                &mut bar,
                &screen,
                scale,
                (0.0, 800.0 - 52.0),
                &whole(1280.0, 52.0),
            );
            let fill = Colour {
                a: tokens.panel.a * 0.5,
                ..tokens.panel
            };
            crate::paint::fill(&mut bar, 0.0, 0.0, 1280.0, 52.0, 0.0, fill);
            bar.save_png(dir.join(format!("{name}.png"))).unwrap();
        }
    }

    #[test]
    fn a_blurred_picture_is_softer_than_a_sharp_one() {
        let picture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../features/wallpapers/usr/share/edel/wallpapers/aurora-light.jpg");
        let sharp = sharp(&picture, (320, 200)).unwrap();
        let soft = blurred(&picture, (320, 200), radius(50)).unwrap();
        assert_eq!((soft.width(), soft.height()), (320, 200));
        assert_ne!(sharp.data(), soft.data());
    }
}
