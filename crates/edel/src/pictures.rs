//! Pictures for the wallpaper and the overview (M5.10c4c): `load` reads a
//! JPEG or PNG and fits it to a screen as CSS `object-fit: cover` does, and
//! `blur` softens one for the frosted glass behind the overview (ADR-002).
//! `box_blur`, the blur shell-ui's shadows use, is kept here once (ADR-010).
//! The compositor and shell-ui link this module through the `pictures`
//! feature.

use std::path::Path;

use tiny_skia::{FilterQuality, IntSize, Pixmap, PixmapPaint};

/// The scale and the x and y offsets, in pixels of the screen, that make a
/// picture of `src` pixels fill `screen` keeping its proportions, centred
/// and cropping the excess (CSS `object-fit: cover`).
pub fn cover(src: (u32, u32), screen: (u32, u32)) -> (f32, f32, f32) {
    let (sw, sh) = (src.0 as f32, src.1 as f32);
    let (dw, dh) = (screen.0 as f32, screen.1 as f32);
    let scale = (dw / sw).max(dh / sh);
    let x = (dw - sw * scale) / 2.0;
    let y = (dh - sh * scale) / 2.0;
    (scale, x, y)
}

/// The picture at `path`, scaled and cropped to fill a screen of `screen`
/// pixels, as an opaque pixmap of that size: black where the picture does
/// not reach (a PNG's transparency shows that black).
pub fn load(path: &Path, screen: (u32, u32)) -> Result<Pixmap, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("the file could not be read: {e}"))?;
    let picture = if path.extension().is_some_and(|e| e == "png") {
        Pixmap::decode_png(&bytes).map_err(|e| format!("it is not a JPEG or PNG picture: {e}"))?
    } else {
        decode_jpeg(&bytes, screen)?
    };
    let mut out = Pixmap::new(screen.0, screen.1)
        .ok_or_else(|| format!("the screen has no size, {}x{}", screen.0, screen.1))?;
    out.fill(tiny_skia::Color::BLACK);
    let (scale, dx, dy) = cover((picture.width(), picture.height()), screen);
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..Default::default()
    };
    let transform = tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, dx, dy);
    out.draw_pixmap(0, 0, picture.as_ref(), &paint, transform, None);
    Ok(out)
}

/// A JPEG decoded at no less than `screen` pixels, as an opaque pixmap.
fn decode_jpeg(bytes: &[u8], screen: (u32, u32)) -> Result<Pixmap, String> {
    let mut decoder = jpeg_decoder::Decoder::new(bytes);
    decoder
        .read_info()
        .map_err(|e| format!("it is not a JPEG or PNG picture: {e}"))?;
    let want = |n: u32| u16::try_from(n).unwrap_or(u16::MAX);
    decoder
        .scale(want(screen.0), want(screen.1))
        .map_err(|e| format!("the JPEG could not be scaled: {e}"))?;
    let pixels = decoder
        .decode()
        .map_err(|e| format!("the JPEG could not be decoded: {e}"))?;
    let info = decoder
        .info()
        .ok_or_else(|| "the JPEG has no picture information".to_string())?;
    let rgba = opaque_rgba(&pixels, info.pixel_format)?;
    let size = IntSize::from_wh(u32::from(info.width), u32::from(info.height))
        .ok_or_else(|| "the JPEG has no size".to_string())?;
    Pixmap::from_vec(rgba, size).ok_or_else(|| "the JPEG pixels do not fit its size".to_string())
}

/// Grey or RGB pixels as opaque RGBA bytes, the layout a pixmap keeps.
fn opaque_rgba(pixels: &[u8], format: jpeg_decoder::PixelFormat) -> Result<Vec<u8>, String> {
    use jpeg_decoder::PixelFormat;
    match format {
        PixelFormat::RGB24 => Ok(pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect()),
        PixelFormat::L8 => Ok(pixels.iter().flat_map(|&l| [l, l, l, 255]).collect()),
        other => Err(format!(
            "the picture's pixels are {other:?}, which a wallpaper cannot show; use a JPEG in RGB or grey, or a PNG"
        )),
    }
}

/// `pixmap` blurred in place, each channel by three box blurs of a third of
/// `radius` (close to a Gaussian), for frosted glass over a wallpaper (ADR-002's
/// glass decision). Opaque pictures only: alpha is left as it is.
pub fn blur(pixmap: &mut Pixmap, radius: f32) {
    let r = (radius / 3.0).round() as usize;
    if r == 0 {
        return;
    }
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    // A box blur counts the pixels past an edge as black, so the picture is
    // first widened by copies of its edge pixels, 3 r on each side, which the
    // three passes reach at most; an edge then keeps its own colour.
    let pad = 3 * r;
    let (pw, ph) = (w + 2 * pad, h + 2 * pad);
    let data = pixmap.data_mut();
    for channel in 0..3 {
        let mut values = vec![0.0_f32; pw * ph];
        for y in 0..ph {
            let sy = y.saturating_sub(pad).min(h - 1);
            for x in 0..pw {
                let sx = x.saturating_sub(pad).min(w - 1);
                values[y * pw + x] = f32::from(data[(sy * w + sx) * 4 + channel]);
            }
        }
        for _ in 0..3 {
            box_blur(&mut values, pw, ph, r);
        }
        for y in 0..h {
            for x in 0..w {
                let v = values[(y + pad) * pw + x + pad];
                data[(y * w + x) * 4 + channel] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Blurs `values`, a `w` by `h` grid, by averaging each with its
/// neighbours up to `radius` away, across and then down.
pub fn box_blur(values: &mut [f32], w: usize, h: usize, radius: usize) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_picture_fills_a_square_screen_by_its_height() {
        assert_eq!(cover((2000, 1000), (1000, 1000)), (1.0, -500.0, 0.0));
    }

    #[test]
    fn a_tall_picture_fills_a_wide_screen_by_its_width() {
        assert_eq!(cover((1000, 1000), (2000, 1000)), (2.0, 0.0, -500.0));
    }

    #[test]
    fn a_png_is_scaled_to_the_screen_with_its_halves_kept() {
        let path = std::env::temp_dir().join("edel-wallpaper-halves-test.png");
        let (w, h) = (40u32, 20u32);
        let mut picture = Pixmap::new(w, h).unwrap();
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let rgba = if x < 20 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                };
                picture.data_mut()[i..i + 4].copy_from_slice(&rgba);
            }
        }
        picture.save_png(&path).unwrap();
        let shown = load(&path, (20, 20)).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((shown.width(), shown.height()), (20, 20));
        // The picture is twice as wide as the screen, so the screen shows its
        // middle: the red half on the left, the blue half on the right.
        let pixel = |x: u32, y: u32| {
            let i = ((y * 20 + x) * 4) as usize;
            shown.data()[i..i + 4].to_vec()
        };
        let left = pixel(2, 10);
        let right = pixel(17, 10);
        assert!(left[0] > 200 && left[2] < 60, "left is red: {left:?}");
        assert!(right[2] > 200 && right[0] < 60, "right is blue: {right:?}");
    }

    #[test]
    fn a_missing_file_is_an_error_saying_it_could_not_be_read() {
        let path = std::env::temp_dir().join("edel-wallpaper-no-such-file.jpg");
        let why = load(&path, (20, 20)).unwrap_err();
        assert!(why.starts_with("the file could not be read: "), "{why}");
    }

    #[test]
    fn a_blur_turns_a_hard_edge_grey_and_keeps_the_far_sides() {
        let (w, h) = (40u32, 4u32);
        let mut picture = Pixmap::new(w, h).unwrap();
        for (i, px) in picture.data_mut().chunks_exact_mut(4).enumerate() {
            let v = if (i as u32 % w) < 20 { 0 } else { 255 };
            px.copy_from_slice(&[v, v, v, 255]);
        }
        blur(&mut picture, 12.0);
        let at = |x: u32| picture.data()[((2 * w + x) * 4) as usize];
        assert!(at(0) <= 2, "the far left stays black: {}", at(0));
        assert!(at(39) >= 253, "the far right stays white: {}", at(39));
        for x in [19, 20] {
            assert!(
                at(x) > 0 && at(x) < 255,
                "the middle is grey at {x}: {}",
                at(x)
            );
        }
    }

    #[test]
    fn a_blur_of_radius_zero_changes_nothing() {
        let mut picture = Pixmap::new(8, 8).unwrap();
        for (i, px) in picture.data_mut().chunks_exact_mut(4).enumerate() {
            px.copy_from_slice(&[i as u8, 255 - i as u8, 7, 255]);
        }
        let before = picture.data().to_vec();
        blur(&mut picture, 0.0);
        assert_eq!(picture.data(), &before[..]);
    }
}
