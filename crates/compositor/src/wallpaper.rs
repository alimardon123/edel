//! The wallpaper behind the windows (M5.10c4b): `edel::wallpapers` says
//! what `appearance.wallpaper` shows, and this turns a picture, read and
//! fitted to a screen by `edel::pictures` as CSS `object-fit: cover` does,
//! into the buffers the screens draw. Each screen keeps
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
use tiny_skia::Pixmap;

use edel::wallpapers::Wallpaper;
use edel_compositor::messages;

use crate::render::{Drawn, Element};
use crate::state::Edel;
use edel_compositor::settings::{Scheme, Settings};

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
    match edel::pictures::load(path, size) {
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
