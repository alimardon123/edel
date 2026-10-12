//! The pointer on screen (roadmap M4.6b): the cursor a window sets, a
//! surface or a named shape (`wp_cursor_shape_v1`), drawn at the pointer.
//! A named shape comes from the xcursor theme (`appearance.cursor_theme`,
//! else `XCURSOR_THEME`, else `default`) when one is installed, at the size
//! `appearance.cursor_size` gives (M5.12b5 part A), else it is the arrow
//! `cursor.rs` draws, so there is always a pointer to see. The cursor goes
//! on the GPU's cursor plane, so moving it redraws nothing, unless
//! `EDEL_SOFTWARE_CURSOR=1` asks for it to be drawn into the frame, as
//! screenshots need and as GPUs with a broken cursor plane do.

use std::collections::HashMap;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::input::pointer::{CursorImageStatus, CursorImageSurfaceData};

use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{IsAlive, Logical, Physical, Point, Transform};
use smithay::wayland::compositor::with_states;

use std::time::Duration;

use smithay::desktop::utils::send_frames_surface_tree;
use smithay::output::Output;

use edel_compositor::cursor;

use crate::state::Edel;

/// The cursor's height in logical pixels when no file sets
/// `appearance.cursor_size` (M5.5, M5.12b5 part A).
pub const SIZE: u32 = edel::settings::CURSOR_SIZE_DEFAULT;

/// One cursor image, ready to draw.
pub struct Image {
    pub buffer: MemoryRenderBuffer,
    /// In the image's own pixels.
    pub hotspot: Point<i32, Physical>,
    pub width: i32,
    pub height: i32,
}

pub struct Cursors {
    /// What the window under the pointer asked for.
    pub status: CursorImageStatus,
    /// Draw the cursor into the frame instead of on the cursor plane.
    pub software: bool,
    /// The xcursor theme in use and the cursor's height in logical pixels,
    /// from the settings file (M5.12b5 part A).
    pub name: String,
    pub size: u32,
    /// Whether the log has said which cursor is in use (at start, then on
    /// each change).
    pub announced: bool,
    theme: xcursor::CursorTheme,
    /// By shape name and size in pixels.
    images: HashMap<(String, u32), Image>,
    told: bool,
}

impl Cursors {
    pub fn new() -> Cursors {
        let name = theme_name(None);
        Cursors {
            status: CursorImageStatus::default_named(),
            software: std::env::var_os("EDEL_SOFTWARE_CURSOR").is_some_and(|v| v == "1"),
            theme: xcursor::CursorTheme::load(&name),
            name,
            size: SIZE,
            announced: false,
            images: HashMap::new(),
            told: false,
        }
    }

    /// The theme and size the settings file asks for (M5.12b5 part A); the
    /// image cache is dropped when the theme changes. Returns whether either
    /// changed.
    pub fn set(&mut self, theme: Option<&str>, size: Option<u32>) -> bool {
        let name = theme_name(theme);
        let size = size.unwrap_or(SIZE);
        let changed = name != self.name || size != self.size;
        if name != self.name {
            self.theme = xcursor::CursorTheme::load(&name);
            self.images.clear();
            self.told = false;
        }
        self.name = name;
        self.size = size;
        changed
    }

    /// The variables the programs the compositor starts are given, so their
    /// own cursors match the compositor's (M5.12b5 part A). The size is in
    /// logical pixels; apps already running keep theirs.
    pub fn env(&self) -> [(&'static str, String); 2] {
        [
            ("XCURSOR_THEME", self.name.clone()),
            ("XCURSOR_SIZE", self.size.to_string()),
        ]
    }

    /// The image for the shape `name` at `px` pixels tall: the theme's
    /// nearest size, else its default arrow, else ours.
    pub fn image(&mut self, name: &str, px: u32) -> &Image {
        let key = (name.to_string(), px);
        if !self.images.contains_key(&key) {
            let themed = [name, "default", "left_ptr"]
                .iter()
                .find_map(|n| self.themed(n, px));
            if !self.told {
                self.told = true;
                match &themed {
                    Some(_) => eprintln!("edel-compositor: cursors from the xcursor theme"),
                    None => eprintln!("edel-compositor: no xcursor theme, so the drawn arrow"),
                }
            }
            let image = themed.unwrap_or_else(|| {
                let arrow = cursor::arrow(px);
                image(
                    &arrow.pixels,
                    arrow.width,
                    arrow.height,
                    (arrow.hotspot.0, arrow.hotspot.1),
                )
            });
            self.images.insert(key.clone(), image);
        }
        &self.images[&key]
    }

    fn themed(&self, name: &str, px: u32) -> Option<Image> {
        let path = self.theme.load_icon(name)?;
        let data = std::fs::read(path).ok()?;
        let images = xcursor::parser::parse_xcursor(&data)?;
        let nearest = images
            .iter()
            .min_by_key(|i| (i64::from(i.size) - i64::from(px)).abs())?;
        // An animated cursor shows its first frame.
        Some(image(
            &nearest.pixels_rgba,
            nearest.width,
            nearest.height,
            (nearest.xhot, nearest.yhot),
        ))
    }

    /// The surface a window gave as its cursor, while it lives.
    pub fn surface(&mut self) -> Option<WlSurface> {
        if let CursorImageStatus::Surface(surface) = &self.status {
            if surface.alive() {
                return Some(surface.clone());
            }
            self.status = CursorImageStatus::default_named();
        }
        None
    }
}

/// The theme a settings file names, else `XCURSOR_THEME` when a session
/// sets it, else the release's (M5.12b5 part A).
fn theme_name(key: Option<&str>) -> String {
    key.map(str::to_string)
        .or_else(|| {
            std::env::var("XCURSOR_THEME")
                .ok()
                .filter(|n| !n.is_empty())
        })
        .unwrap_or_else(|| edel::settings::CURSOR_THEME_DEFAULT.into())
}

impl Edel {
    /// The cursor the settings files ask for (M5.12b5 part A), at start and
    /// on each change: logged as `cursor THEME at N px`, and drawn again.
    /// Apps already running keep the cursor they had.
    pub fn cursor_settings(&mut self, theme: Option<&str>, size: Option<u32>) {
        let changed = self.cursors.set(theme, size);
        if changed || !self.cursors.announced {
            self.cursors.announced = true;
            eprintln!(
                "edel-compositor: cursor {} at {} px",
                self.cursors.name, self.cursors.size
            );
            self.dirty = true;
        }
    }
}

/// Pixels in xcursor's order, which is ARGB8888's in memory.
fn image(pixels: &[u8], width: u32, height: u32, hotspot: (u32, u32)) -> Image {
    Image {
        buffer: MemoryRenderBuffer::from_slice(
            pixels,
            Fourcc::Argb8888,
            (width as i32, height as i32),
            1,
            Transform::Normal,
            None,
        ),
        hotspot: (hotspot.0 as i32, hotspot.1 as i32).into(),
        width: width as i32,
        height: height as i32,
    }
}

impl Edel {
    /// A window's cursor surface hears a frame was shown, so an animated
    /// cursor goes on.
    pub fn cursor_frame(&mut self, output: &Output, now: Duration) {
        if let Some(surface) = self.cursors.surface() {
            send_frames_surface_tree(&surface, output, now, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
    }
}

/// Where a window's cursor surface puts the pointer, from its top left.
pub fn surface_hotspot(surface: &WlSurface) -> Point<i32, Logical> {
    with_states(surface, |states| {
        states
            .data_map
            .get::<CursorImageSurfaceData>()
            .and_then(|data| data.lock().ok().map(|d| d.hotspot))
    })
    .unwrap_or_default()
}
