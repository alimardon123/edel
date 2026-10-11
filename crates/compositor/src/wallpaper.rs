//! The wallpaper behind the windows (M5.10c4b): `edel::wallpapers` says
//! what `appearance.wallpaper` shows, and this turns a picture into pixels
//! that fill a screen as CSS `object-fit: cover` does. Each screen keeps
//! its picture's buffer until the picture or the screen's size changes, so
//! an idle desktop draws nothing.

use std::collections::HashMap;
use std::path::Path;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::solid::{SolidColorBuffer, SolidColorRenderElement};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::output::Output;
use smithay::utils::{Logical, Physical, Point, Rectangle, Size, Transform};
use tiny_skia::{FilterQuality, IntSize, Pixmap, PixmapPaint};

use edel::wallpapers::Wallpaper;
use edel_compositor::messages;

use crate::render::{Drawn, Element};
use crate::state::Edel;
use edel_compositor::settings::{Scheme, Settings};

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

/// One screen's wallpaper, kept until it changes.
#[derive(Default)]
pub struct Screen {
    /// The physical size the picture was made for.
    pub size: (u32, u32),
    /// The picture, or `None` when it could not be read at this size, so
    /// the background colour shows and the picture is not read again.
    pub picture: Option<MemoryRenderBuffer>,
    /// The colour, when the wallpaper is one.
    pub colour: SolidColorBuffer,
}

/// What the wallpaper shows, and each screen's picture of it.
pub struct Shown {
    pub what: Wallpaper,
    /// By output name.
    pub screens: HashMap<String, Screen>,
    /// Whether the first settings load has said what is shown.
    pub announced: bool,
}

impl Shown {
    pub fn new(what: Wallpaper) -> Shown {
        Shown {
            what,
            screens: HashMap::new(),
            announced: false,
        }
    }
}

/// The wallpaper as the log names it: a picture's path, the colour, or
/// `none` for the background.
pub fn describe(what: &Wallpaper) -> String {
    match what {
        Wallpaper::Background => "none".to_string(),
        Wallpaper::Colour(colour) => format!("colour {}", colour.hex()),
        Wallpaper::Picture(path) => path.display().to_string(),
    }
}

/// The picture as a buffer that keeps its id while it lives.
fn painted(pixmap: &Pixmap) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        pixmap.data(),
        Fourcc::Abgr8888,
        (pixmap.width() as i32, pixmap.height() as i32),
        1,
        Transform::Normal,
        None,
    )
}

impl Edel {
    /// Shows the wallpaper the settings name (M5.10c4b), logging it, and
    /// makes every screen's picture again.
    pub fn apply_wallpaper(&mut self, settings: &Settings) {
        let dark = settings.color_scheme == Scheme::Dark;
        let value = edel::wallpapers::value_for(
            settings.wallpaper.as_deref(),
            settings.wallpaper_dark.as_deref(),
            dark,
        );
        let what = edel::wallpapers::on_this_machine(value, dark);
        eprintln!("edel-compositor: wallpaper {}", describe(&what));
        self.wallpaper.what = what;
        self.wallpaper.screens.clear();
        self.wallpaper.announced = true;
        self.repaint = true;
        self.dirty = true;
    }

    /// The wallpaper of `output`, drawn behind everything (M5.10c4b).
    /// `logical` is the screen's size in logical pixels. Nothing is drawn
    /// for the background colour.
    pub fn wallpaper_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        logical: Size<i32, Logical>,
        scale: f64,
    ) -> Vec<Drawn> {
        match self.wallpaper.what.clone() {
            Wallpaper::Background => Vec::new(),
            Wallpaper::Colour(colour) => {
                let screen = self.wallpaper.screens.entry(output.name()).or_default();
                screen.colour.update(logical, colour.rgba());
                let solid = SolidColorRenderElement::from_buffer(
                    &screen.colour,
                    Point::<i32, Physical>::from((0, 0)),
                    scale,
                    1.0,
                    Kind::Unspecified,
                );
                vec![Drawn::Plain(Element::Border(solid))]
            }
            Wallpaper::Picture(path) => self.picture_elements(renderer, output, &path, logical),
        }
    }

    /// The picture of `path` on `output`, made again only when the screen's
    /// physical size changes (`Screen::size`).
    fn picture_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        path: &Path,
        logical: Size<i32, Logical>,
    ) -> Vec<Drawn> {
        let name = output.name();
        let Some(mode) = output.current_mode() else {
            return Vec::new();
        };
        let size = (mode.size.w.max(0) as u32, mode.size.h.max(0) as u32);
        if self
            .wallpaper
            .screens
            .get(&name)
            .is_none_or(|s| s.size != size)
        {
            let screen = load_screen(path, &name, size);
            self.wallpaper.screens.insert(name.clone(), screen);
        }
        let Some(buffer) = self
            .wallpaper
            .screens
            .get(&name)
            .and_then(|s| s.picture.as_ref())
        else {
            return Vec::new();
        };
        let src = Rectangle::from_size((size.0 as f64, size.1 as f64).into());
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, Physical>::from((0.0, 0.0)),
            buffer,
            None,
            Some(src),
            Some(logical),
            Kind::Unspecified,
        ) {
            Ok(element) => vec![Drawn::Plain(Element::Bar(element))],
            Err(e) => {
                eprintln!("edel-compositor: drawing the wallpaper failed: {e}");
                Vec::new()
            }
        }
    }
}

/// `path` made into `size` pixels for the screen `name`, logging what
/// happened; a picture that cannot be read is kept as `None`.
fn load_screen(path: &Path, name: &str, size: (u32, u32)) -> Screen {
    match load(path, size) {
        Ok(pixmap) => {
            eprintln!("edel-compositor: wallpaper on {name} {}x{}", size.0, size.1);
            Screen {
                size,
                picture: Some(painted(&pixmap)),
                ..Default::default()
            }
        }
        Err(why) => {
            eprintln!(
                "edel-compositor: {}",
                messages::wallpaper_not_read(path, &why)
            );
            Screen {
                size,
                ..Default::default()
            }
        }
    }
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
}
