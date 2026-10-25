//! The wallpaper behind the windows (M5.10c4b): `edel::wallpapers` says
//! what `appearance.wallpaper` shows, and this turns a picture, read and
//! fitted to a screen by `edel::pictures` as CSS `object-fit: cover` does,
//! on a thread (`off_loop.rs`), into the buffers the screens draw. Each screen keeps
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

/// What the wallpaper shows, and each screen's picture of it. Each
/// workspace has its own choice (M5.12b5, `workspaces.wallpapers`), made
/// by `edel::wallpapers::for_workspace`; a screen draws the choice of the
/// workspace it shows, and two workspaces that show one value share its
/// picture.
pub struct Shown {
    /// What Appearance shows: a workspace with no entry of its own.
    pub what: Wallpaper,
    /// What each workspace shows, from the first, made when the settings
    /// load; a workspace past this list shows `what`.
    pub choices: Vec<Wallpaper>,
    /// Whether each workspace has an entry of its own in the list.
    pub own: Vec<bool>,
    /// Each workspace's value as last logged: its own value, or empty for
    /// Appearance's, so a change to or from its own is logged once.
    logged: Vec<String>,
    /// By output name and the value shown there (`describe`): each
    /// screen's picture of each value, made once.
    pub screens: HashMap<(String, String), Screen>,
    /// Whether the first settings load has said what is shown.
    pub announced: bool,
    /// Counts the settings loads, so a picture read for an earlier one is
    /// not shown.
    generation: u64,
}

impl Shown {
    pub fn new(what: Wallpaper) -> Shown {
        Shown {
            what,
            choices: Vec::new(),
            own: Vec::new(),
            logged: Vec::new(),
            screens: HashMap::new(),
            announced: false,
            generation: 0,
        }
    }

    /// What workspace `index` (from 0) shows.
    pub fn on(&self, index: usize) -> &Wallpaper {
        self.choices.get(index).unwrap_or(&self.what)
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

/// The workspaces the wallpaper keeps a choice for: the most there may be.
const WORKSPACES: usize = edel::presets::MOST_WORKSPACES;

impl Edel {
    /// Shows the wallpapers the settings name (M5.10c4b, M5.12b5): each
    /// workspace's choice, logged when it is its own, and Appearance's,
    /// logged as before. Makes every screen's picture again.
    pub fn apply_wallpaper(&mut self, settings: &Settings) {
        let dark = settings.color_scheme == Scheme::Dark;
        let (light, dark_value) = (
            settings.wallpaper.as_deref(),
            settings.wallpaper_dark.as_deref(),
        );
        let value = edel::wallpapers::value_for(light, dark_value, dark);
        let what = edel::wallpapers::on_this_machine(value, dark);
        eprintln!("edel-compositor: wallpaper {}", describe(&what));
        let entries = &settings.workspace_wallpapers;
        self.wallpaper.choices = (0..WORKSPACES)
            .map(|index| {
                let value =
                    edel::wallpapers::for_workspace(entries, index, light, dark_value, dark);
                edel::wallpapers::on_this_machine(value, dark)
            })
            .collect();
        self.wallpaper.own = (0..WORKSPACES)
            .map(|index| entries.get(index).is_some_and(|e| !e.trim().is_empty()))
            .collect();
        self.wallpaper.what = what;
        self.wallpaper.screens.clear();
        self.wallpaper.generation += 1;
        self.wallpaper.announced = true;
        for index in 0..self.desks.count() {
            self.note_workspace(index);
        }
        self.repaint = true;
        self.dirty = true;
    }

    /// Logs workspace `index`'s wallpaper as `wallpaper on workspace N:
    /// VALUE` when it is its own and was not logged as it is now, and once
    /// more when a workspace goes back to Appearance's (M5.12b5).
    pub fn note_workspace(&mut self, index: usize) {
        let own = self.wallpaper.own.get(index).copied().unwrap_or(false);
        let value = describe(self.wallpaper.on(index));
        if self.wallpaper.logged.len() <= index {
            self.wallpaper.logged.resize(index + 1, String::new());
        }
        let kept = if own { value.clone() } else { String::new() };
        if self.wallpaper.logged[index] == kept {
            return;
        }
        eprintln!(
            "edel-compositor: wallpaper on workspace {}: {value}",
            index + 1
        );
        self.wallpaper.logged[index] = kept;
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
        let name = output.name();
        let index = self.desks.shown_on(&name);
        self.note_workspace(index);
        let what = self.wallpaper.on(index).clone();
        let key = (name, describe(&what));
        match what {
            Wallpaper::Background => Vec::new(),
            Wallpaper::Colour(colour) => {
                let screen = self.wallpaper.screens.entry(key).or_default();
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
            Wallpaper::Picture(path) => {
                self.picture_elements(renderer, output, key, &path, logical)
            }
        }
    }

    /// The picture of `path` on `output`, made again only when the screen's
    /// physical size changes (`Screen::size`). It is read and fitted off
    /// the frame (`off_loop`); until it comes the background shows.
    fn picture_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        key: (String, String),
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
            .get(&key)
            .is_none_or(|s| s.size != size)
        {
            self.wallpaper.screens.insert(
                key.clone(),
                Screen {
                    size,
                    ..Default::default()
                },
            );
            let (generation, key, path) =
                (self.wallpaper.generation, key.clone(), path.to_path_buf());
            self.off_loop("wallpaper", move || {
                let pixmap = load_pixmap(&path, &name, size);
                Box::new(move |state: &mut Edel| {
                    state.wallpaper_read(generation, key, size, pixmap)
                })
            });
        }
        let Some(buffer) = self
            .wallpaper
            .screens
            .get(&key)
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

impl Edel {
    /// Shows the picture read for screen `key` at `size`, unless the
    /// settings or the screen changed meanwhile.
    fn wallpaper_read(
        &mut self,
        generation: u64,
        key: (String, String),
        size: (u32, u32),
        pixmap: Option<Pixmap>,
    ) {
        if generation != self.wallpaper.generation {
            return;
        }
        let Some(screen) = self
            .wallpaper
            .screens
            .get_mut(&key)
            .filter(|s| s.size == size)
        else {
            return;
        };
        screen.picture = pixmap.as_ref().map(painted);
        self.repaint = true;
        self.dirty = true;
    }
}

/// `path` made into `size` pixels for the screen `name`, logging what
/// happened; a picture that cannot be read is `None`, so the background
/// colour shows and the picture is not read again.
fn load_pixmap(path: &Path, name: &str, size: (u32, u32)) -> Option<Pixmap> {
    match edel::pictures::load(path, size) {
        Ok(pixmap) => {
            eprintln!("edel-compositor: wallpaper on {name} {}x{}", size.0, size.1);
            Some(pixmap)
        }
        Err(why) => {
            eprintln!(
                "edel-compositor: {}",
                messages::wallpaper_not_read(path, &why)
            );
            None
        }
    }
}
