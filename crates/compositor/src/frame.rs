//! Server-side window frames (roadmap M4.4): a title bar above each window
//! that asks for one through xdg-decoration or lets the compositor choose,
//! and a thin border round the rest. Apps that draw their own bars, as
//! libadwaita apps do, get none, so nothing is drawn twice. The bar holds
//! the title and the buttons, minimize, maximize and close, at the right
//! or, as on a Mac, at the left (M5.4b): close outermost either way, and
//! minimize before maximize reading left to right.
//!
//! Geometry, hit tests and the bar's pixels are plain data and functions
//! here, tested without a display; the compositor turns the pixels into
//! one texture per window and redraws it only when what it shows changes.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::ResizeEdge;
use smithay::utils::{Logical, Point, Rectangle, Size};

use crate::tokens::{Colour, Tokens};

pub use edel::presets::Side;

/// How far outside its frame a window's edge can still be grabbed.
pub const GRIP: i32 = 6;

/// How far from a corner a grab on an edge resizes from the corner.
pub const CORNER: i32 = 16;

/// The fonts titles fall back to when the tokens' interface font is not
/// among the system's fonts: Inter (`fonts` feature), then Noto Sans.
pub const FONTS: [&str; 2] = [
    "/usr/share/fonts/inter/InterVariable.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
];

/// What a frame adds round a window, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Insets {
    /// The title bar.
    pub top: i32,
    /// The border, left and right.
    pub side: i32,
    pub bottom: i32,
}

impl Insets {
    /// A server-side frame: the bar on top, the border on the other sides.
    pub fn server_side(tokens: &Tokens) -> Insets {
        let border = tokens.border as i32;
        Insets {
            top: tokens.title_bar_height as i32,
            side: border,
            bottom: border,
        }
    }

    /// The frame round a window at `window`.
    pub fn frame(self, window: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
        Rectangle::new(
            window.loc - Point::from((self.side, self.top)),
            self.frame_size(window.size),
        )
    }

    /// The window inside the frame at `frame`.
    pub fn window(self, frame: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
        Rectangle::new(
            frame.loc + Point::from((self.side, self.top)),
            Size::from((
                (frame.size.w - 2 * self.side).max(1),
                (frame.size.h - self.top - self.bottom).max(1),
            )),
        )
    }

    pub fn frame_size(self, window: Size<i32, Logical>) -> Size<i32, Logical> {
        Size::from((window.w + 2 * self.side, window.h + self.top + self.bottom))
    }
}

/// A button on the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Close,
    Maximize,
    /// Since M5.2h, when the panel's window list brings a minimized
    /// window back.
    Minimize,
}

/// Which buttons the bars show (M5.18a): each, unless the settings file
/// hides it with `layout.close_button`, `minimize_button` or
/// `maximize_button`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    pub close: bool,
    pub minimize: bool,
    pub maximize: bool,
}

impl Default for Shown {
    fn default() -> Shown {
        Shown {
            close: true,
            minimize: true,
            maximize: true,
        }
    }
}

impl Shown {
    pub fn has(self, button: Button) -> bool {
        match button {
            Button::Close => self.close,
            Button::Minimize => self.minimize,
            Button::Maximize => self.maximize,
        }
    }

    /// The shown ones' names, for the log: `close, maximize`, or `none`.
    pub fn names(self) -> String {
        let names: Vec<&str> = [
            (self.close, "close"),
            (self.minimize, "minimize"),
            (self.maximize, "maximize"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect();
        if names.is_empty() {
            "none".to_string()
        } else {
            names.join(", ")
        }
    }
}

/// The shown buttons on `side`, from the bar's end inwards: close
/// outermost, then, reading left to right, minimize before maximize, as
/// on the desktops people know; a hidden one leaves no gap.
pub fn buttons(side: Side, shown: Shown) -> impl Iterator<Item = Button> {
    let order = match side {
        Side::Right => [Button::Close, Button::Maximize, Button::Minimize],
        Side::Left => [Button::Close, Button::Minimize, Button::Maximize],
    };
    order.into_iter().filter(move |b| shown.has(*b))
}

/// A part of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// The bar outside the buttons: dragging it moves the window.
    Title,
    Button(Button),
    /// An edge or corner: dragging it resizes the window.
    Edge(ResizeEdge),
}

/// The part of a frame of `size` at `point`, measured from the frame's top
/// left, its `shown` buttons on `buttons_side`; none inside the window or
/// beyond the grips. A window that cannot be resized (a maximized one) has no grips,
/// and its border counts as its bar.
pub fn hit(
    size: Size<i32, Logical>,
    insets: Insets,
    point: Point<f64, Logical>,
    resizable: bool,
    buttons_side: Side,
    shown: Shown,
) -> Option<Hit> {
    let (w, h) = (f64::from(size.w), f64::from(size.h));
    let (x, y) = (point.x, point.y);
    let grip = if resizable { f64::from(GRIP) } else { 0.0 };
    if x < -grip || y < -grip || x >= w + grip || y >= h + grip {
        return None;
    }
    let side = f64::from(insets.side);
    let top = f64::from(insets.top);
    let bottom = f64::from(insets.bottom);
    if x >= 0.0 && x < w && y >= 0.0 && y < top {
        // Close is 0, the square at the bar's end on that side; then
        // inwards.
        let from_end = match buttons_side {
            Side::Right => ((w - x) / top).ceil() as usize - 1,
            Side::Left => (x / top).floor() as usize,
        };
        return Some(
            buttons(buttons_side, shown)
                .nth(from_end)
                .map_or(Hit::Title, Hit::Button),
        );
    }
    if x >= side && x < w - side && y >= top && y < h - bottom {
        return None;
    }
    if !resizable {
        return Some(Hit::Title);
    }
    let corner = f64::from(CORNER);
    let (in_left, in_right) = (x < side, x >= w - side);
    let (in_top, in_bottom) = (y < 0.0, y >= h - bottom);
    let across = in_top || in_bottom;
    let along = in_left || in_right;
    let left = in_left || (across && x < corner);
    let right = !left && (in_right || (across && x >= w - corner));
    let up = in_top || (along && y < corner);
    let down = !up && (in_bottom || (along && y >= h - corner));
    Some(Hit::Edge(match (up, down, left, right) {
        (true, _, true, _) => ResizeEdge::TopLeft,
        (true, _, _, true) => ResizeEdge::TopRight,
        (_, true, true, _) => ResizeEdge::BottomLeft,
        (_, true, _, true) => ResizeEdge::BottomRight,
        (true, ..) => ResizeEdge::Top,
        (_, true, ..) => ResizeEdge::Bottom,
        (_, _, true, _) => ResizeEdge::Left,
        _ => ResizeEdge::Right,
    }))
}

/// Everything a bar shows; when it changes, the bar is drawn again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    /// In pixels on the screen: the logical size times the scale, so the
    /// bar is sharp at any scale (M4.6).
    pub width: i32,
    pub height: i32,
    /// The screen's scale, in 120ths, as `wp_fractional_scale_v1` gives it.
    pub scale_120: u32,
    pub title: String,
    pub focused: bool,
    pub maximized: bool,
    pub hovered: Option<Button>,
    /// Whether the font had loaded.
    pub text: bool,
    /// The side the buttons sit on (M5.4b).
    pub side: Side,
    /// Which buttons it shows (M5.18a).
    pub shown: Shown,
    /// The app's icon, by its name in the icon themes, shown left of the
    /// title (M5.6a); none for an app without one.
    pub icon: Option<String>,
    /// Light or dark (M5.5c): a new scheme draws the bar again.
    pub scheme: crate::tokens::Scheme,
}

/// Draws the bar `look` describes into `pixels`, `look.width` by
/// `look.height` ARGB8888 in memory order (blue, green, red, alpha).
pub fn paint(
    pixels: &mut [u8],
    look: &Look,
    tokens: &Tokens,
    text: Option<&mut Text>,
    icon: Option<&edel::app_icons::Picture>,
) {
    let (w, h) = (look.width.max(0) as usize, look.height.max(0) as usize);
    if pixels.len() < w * h * 4 || w == 0 || h == 0 {
        return;
    }
    let scale = look.scale_120.max(120) as f32 / 120.0;
    let mut canvas = Canvas { pixels, width: w };
    let bar = if look.focused {
        tokens.title_bar_focused
    } else {
        tokens.title_bar
    };
    canvas.fill(0, 0, w, h, bar);
    let ink = if look.focused {
        tokens.title_text
    } else {
        tokens.title_text_unfocused
    };
    let size = h;
    let set: Vec<Button> = buttons(look.side, look.shown).collect();
    for (i, button) in set.iter().enumerate() {
        if (i + 1) * size > w {
            break;
        }
        let x = match look.side {
            Side::Right => w - (i + 1) * size,
            Side::Left => i * size,
        };
        let mut icon = ink;
        if look.hovered == Some(*button) {
            let hover = match button {
                Button::Close => tokens.title_close_hover,
                Button::Maximize | Button::Minimize => tokens.title_button_hover,
            };
            canvas.fill(x, 0, size, h, hover);
            icon = tokens.title_text;
        }
        canvas.icon(*button, look.maximized, x, size, icon);
    }
    // A hairline under the bar, and the window's edge along its top and
    // sides, as the mockups draw them; a maximized window has no edge.
    let line = tokens.line;
    for col in 0..w {
        canvas.blend(col as i64, h as i64 - 1, line, line.a);
    }
    if !look.maximized {
        let edge = tokens.edge;
        let opaque = Colour { a: 1.0, ..edge };
        for col in 0..w {
            canvas.blend(col as i64, 0, opaque, edge.a);
        }
        for row in 1..h {
            canvas.blend(0, row as i64, opaque, edge.a);
            canvas.blend(w as i64 - 1, row as i64, opaque, edge.a);
        }
    }
    let Some(text) = text else {
        return;
    };
    // The title is centred on the bar, moved aside if it would reach the
    // buttons, and cut short with an ellipsis if it is still too long.
    let pad = (h / 2) as f32;
    let used = (set.len() * size) as f32;
    let (left, right) = match look.side {
        Side::Right => (pad, w as f32 - used - pad),
        Side::Left => (used + pad, w as f32 - pad),
    };
    let room = right - left;
    if room <= 0.0 || look.title.is_empty() {
        return;
    }
    text.set_size(tokens.title_text_size as f32 * scale);
    // The app's icon, when it has one, sits left of the title, and the
    // two are centred together (M5.6a).
    let icon = icon.filter(|p| p.height() as f32 <= h as f32);
    let (icon_w, gap) = match icon {
        Some(p) => (p.width() as f32, (7.0 * scale).round()),
        None => (0.0, 0.0),
    };
    let title = text.fit(&look.title, (room - icon_w - gap).max(0.0));
    let width = text.width(&title) + icon_w + gap;
    let x = ((w as f32 - width) / 2.0).min(right - width).max(left);
    if let Some(picture) = icon {
        let top = (h as f32 - picture.height() as f32) / 2.0;
        canvas.picture(picture, x.round() as i64, top.round() as i64);
    }
    let baseline = ((h as f32 + text.ascent + text.descent) / 2.0).round();
    text.draw(&mut canvas, &title, x + icon_w + gap, baseline, ink, right);
}

struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: usize,
}

impl Canvas<'_> {
    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, colour: Colour) {
        let [r, g, b, a] = colour.bytes();
        let height = self.pixels.len() / 4 / self.width;
        for row in y..(y + h).min(height) {
            for col in x..(x + w).min(self.width) {
                let i = (row * self.width + col) * 4;
                self.pixels[i..i + 4].copy_from_slice(&[b, g, r, a]);
            }
        }
    }

    /// Mixes `colour` into one pixel by `coverage`, 0 to 1.
    fn blend(&mut self, x: i64, y: i64, colour: Colour, coverage: f32) {
        let height = (self.pixels.len() / 4 / self.width) as i64;
        if x < 0 || y < 0 || x >= self.width as i64 || y >= height || coverage <= 0.0 {
            return;
        }
        let coverage = coverage.min(1.0);
        let i = (y as usize * self.width + x as usize) * 4;
        let [r, g, b, _] = colour.bytes();
        for (k, c) in [b, g, r].into_iter().enumerate() {
            let under = f32::from(self.pixels[i + k]);
            self.pixels[i + k] = (f32::from(c) * coverage + under * (1.0 - coverage)).round() as u8;
        }
    }

    /// A picture, premultiplied RGBA as tiny-skia keeps it, with its top
    /// left at `x`, `y`, over what is there.
    fn picture(&mut self, picture: &edel::app_icons::Picture, x: i64, y: i64) {
        let width = picture.width() as usize;
        for (i, p) in picture.data().chunks_exact(4).enumerate() {
            let alpha = f32::from(p[3]) / 255.0;
            if alpha <= 0.0 {
                continue;
            }
            let straight = |c: u8| (f32::from(c) / alpha).min(255.0) / 255.0;
            let colour = Colour {
                r: straight(p[0]),
                g: straight(p[1]),
                b: straight(p[2]),
                a: 1.0,
            };
            self.blend(
                x + (i % width) as i64,
                y + (i / width) as i64,
                colour,
                alpha,
            );
        }
    }

    /// A button's icon from `design/icons/` (M5.5d), half the square at
    /// `x` wide and in its middle: close, maximize, restore for a
    /// maximized window, minimize.
    fn icon(&mut self, button: Button, maximized: bool, x: usize, size: usize, colour: Colour) {
        let name = match button {
            Button::Close => "close",
            Button::Maximize if maximized => "restore",
            Button::Maximize => "maximize",
            Button::Minimize => "minimize",
        };
        let px = (size / 2).max(1);
        let Some(mask) = edel::icons::mask(name, px as u32) else {
            return;
        };
        let (x0, y0) = ((x + (size - px) / 2) as i64, ((size - px) / 2) as i64);
        for (i, pixel) in mask.data().chunks_exact(4).enumerate() {
            let (dx, dy) = ((i % px) as i64, (i / px) as i64);
            self.blend(x0 + dx, y0 + dy, colour, f32::from(pixel[3]) / 255.0);
        }
    }
}

/// The title's font at one size, with the letters drawn so far.
pub struct Text {
    font: fontdue::Font,
    px: f32,
    glyphs: HashMap<char, (fontdue::Metrics, Vec<u8>)>,
    /// Above the baseline, in pixels.
    ascent: f32,
    /// Below it, negative.
    descent: f32,
}

/// Where fonts live: `fonts` in each of `$XDG_DATA_DIRS`, else in
/// `/usr/local/share` and `/usr/share`, as fontconfig looks.
fn font_dirs() -> Vec<PathBuf> {
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.split(':')
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
        .map(|d| d.join("fonts"))
        .collect()
}

/// The file of `family` among the fonts under `dirs`: its variable font,
/// else its regular one, else one named just so, by the file's name with
/// case, spaces and dashes aside ("InterVariable.ttf", "NotoSans-
/// Regular.ttf", "DejaVuSans.ttf"); none if it is not there.
pub fn font_file(family: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let squash = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    let family = squash(family);
    if family.is_empty() {
        return None;
    }
    let wanted = [
        format!("{family}variable"),
        format!("{family}regular"),
        family,
    ];
    let mut best: Option<(usize, PathBuf)> = None;
    let mut stack: Vec<(PathBuf, u32)> = dirs.iter().map(|d| (d.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // Not into linked folders, and not deep: no loop, little time.
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if depth < 4 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let font = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"));
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(rank) = wanted.iter().position(|w| *w == squash(stem)) else {
                continue;
            };
            if font
                && best
                    .as_ref()
                    .is_none_or(|(r, p)| rank < *r || (rank == *r && path < *p))
            {
                best = Some((rank, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

/// How many letters [`Text`] keeps drawn; a title in a script with many
/// letters clears them rather than growing without end.
const KEPT: usize = 512;

impl Text {
    /// The tokens' interface font `family` (`font_file`), else the first
    /// of [`FONTS`] that loads, at `px` pixels per em.
    pub fn load(family: &str, px: f32) -> Result<Text> {
        let mut errors = Vec::new();
        let found = font_file(family, &font_dirs());
        let paths = found.iter().map(|p| p.to_string_lossy().into_owned());
        for path in paths.chain(FONTS.iter().map(|p| p.to_string())) {
            let path = path.as_str();
            match std::fs::read(path)
                .map_err(anyhow::Error::from)
                .and_then(|data| Text::new(&data, px))
            {
                Ok(text) => return Ok(text),
                Err(e) => errors.push(format!("{path}: {e}")),
            }
        }
        Err(anyhow!("no font for titles: {}", errors.join("; ")))
    }

    pub fn new(data: &[u8], px: f32) -> Result<Text> {
        let font = fontdue::Font::from_bytes(data, fontdue::FontSettings::default())
            .map_err(|e| anyhow!("{e}"))?;
        let metrics = font
            .horizontal_line_metrics(px)
            .ok_or_else(|| anyhow!("the font has no horizontal metrics"))?;
        Ok(Text {
            font,
            px,
            glyphs: HashMap::new(),
            ascent: metrics.ascent,
            descent: metrics.descent,
        })
    }

    /// Draws from now on at `px` pixels per em; the letters drawn at the
    /// old size are dropped.
    pub fn set_size(&mut self, px: f32) {
        if px == self.px || px <= 0.0 {
            return;
        }
        if let Some(metrics) = self.font.horizontal_line_metrics(px) {
            self.px = px;
            self.ascent = metrics.ascent;
            self.descent = metrics.descent;
            self.glyphs.clear();
        }
    }

    fn glyph(&mut self, c: char) -> &(fontdue::Metrics, Vec<u8>) {
        if self.glyphs.len() >= KEPT && !self.glyphs.contains_key(&c) {
            self.glyphs.clear();
        }
        let (font, px) = (&self.font, self.px);
        self.glyphs
            .entry(c)
            .or_insert_with(|| font.rasterize(c, px))
    }

    /// Where each letter of `text` starts, and where the last one ends.
    fn advances(&mut self, text: &str) -> (Vec<f32>, f32) {
        let mut pen = 0.0;
        let mut starts = Vec::new();
        let mut previous = None;
        for c in text.chars() {
            if let Some(p) = previous {
                pen += self.font.horizontal_kern(p, c, self.px).unwrap_or(0.0);
            }
            starts.push(pen);
            pen += self.glyph(c).0.advance_width;
            previous = Some(c);
        }
        (starts, pen)
    }

    /// The width `text` takes, in pixels.
    pub fn width(&mut self, text: &str) -> f32 {
        self.advances(text).1
    }

    /// `text`, or as much of it as fits in `room` pixels with an ellipsis.
    pub fn fit(&mut self, text: &str, room: f32) -> String {
        let text = text.trim();
        if self.width(text) <= room {
            return text.to_string();
        }
        let ellipsis = self.width("\u{2026}");
        let mut kept = String::new();
        for c in text.chars() {
            kept.push(c);
            if self.width(&kept) + ellipsis > room {
                kept.pop();
                break;
            }
        }
        format!("{}\u{2026}", kept.trim_end())
    }

    /// Above the baseline, in pixels, at the size set.
    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    /// Below the baseline (negative), in pixels, at the size set.
    pub fn descent(&self) -> f32 {
        self.descent
    }

    /// Draws `text` from `x` on `baseline` over `pixmap` (premultiplied
    /// RGBA, as tiny-skia keeps it), for the overview's labels (M5.2j-b).
    pub fn draw_on(
        &mut self,
        pixmap: &mut tiny_skia::Pixmap,
        text: &str,
        x: f32,
        baseline: f32,
        colour: Colour,
    ) {
        let (width, height) = (pixmap.width() as i64, pixmap.height() as i64);
        let [r, g, b, _] = colour.bytes();
        let (starts, _) = self.advances(text);
        let data = pixmap.data_mut();
        for (c, start) in text.chars().zip(starts) {
            let (metrics, coverage) = self.glyph(c).clone();
            let left = (x + start).round() as i64 + i64::from(metrics.xmin);
            let top = baseline as i64 - i64::from(metrics.ymin) - metrics.height as i64;
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let (px, py) = (left + col as i64, top + row as i64);
                    if px < 0 || py < 0 || px >= width || py >= height {
                        continue;
                    }
                    let a = f32::from(coverage[row * metrics.width + col]) / 255.0 * colour.a;
                    if a <= 0.0 {
                        continue;
                    }
                    let i = ((py * width + px) * 4) as usize;
                    for (k, c) in [r, g, b].into_iter().enumerate() {
                        let under = f32::from(data[i + k]);
                        data[i + k] = (f32::from(c) * a + under * (1.0 - a)).round() as u8;
                    }
                    let under = f32::from(data[i + 3]);
                    data[i + 3] = (255.0 * a + under * (1.0 - a)).round() as u8;
                }
            }
        }
    }

    /// Draws `text` from `x` on `baseline`, nothing right of `limit`.
    fn draw(
        &mut self,
        canvas: &mut Canvas,
        text: &str,
        x: f32,
        baseline: f32,
        colour: Colour,
        limit: f32,
    ) {
        let (starts, _) = self.advances(text);
        for (c, start) in text.chars().zip(starts) {
            let (metrics, coverage) = self.glyph(c).clone();
            let left = (x + start).round() as i64 + i64::from(metrics.xmin);
            let top = baseline as i64 - i64::from(metrics.ymin) - metrics.height as i64;
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let px = left + col as i64;
                    if px as f32 >= limit {
                        continue;
                    }
                    let alpha = f32::from(coverage[row * metrics.width + col]) / 255.0;
                    canvas.blend(px, top + row as i64, colour, alpha);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_family_finds_its_variable_or_regular_file() {
        let root = std::env::temp_dir().join(format!("edel-fonts-{}", std::process::id()));
        let inter = root.join("fonts/inter");
        let noto = root.join("fonts/noto");
        std::fs::create_dir_all(&inter).unwrap();
        std::fs::create_dir_all(&noto).unwrap();
        for file in [
            "Inter-Italic.ttf",
            "InterVariable.ttf",
            "Inter-Regular.otf",
            "notes.txt",
        ] {
            std::fs::write(inter.join(file), "").unwrap();
        }
        std::fs::write(noto.join("NotoSans-Regular.ttf"), "").unwrap();
        let dirs = [root.join("fonts")];
        let found =
            |family: &str| font_file(family, &dirs).map(|p| p.file_name().unwrap().to_owned());
        assert_eq!(found("Inter"), Some("InterVariable.ttf".into()));
        assert_eq!(found("Noto Sans"), Some("NotoSans-Regular.ttf".into()));
        assert_eq!(found("Fira Sans"), None);
        assert_eq!(found(" "), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn tokens() -> Tokens {
        Tokens::built_in()
    }

    fn insets() -> Insets {
        Insets::server_side(&tokens())
    }

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn a_frame_is_the_bar_above_and_the_border_round_the_rest() {
        let i = insets();
        assert_eq!((i.top, i.side, i.bottom), (28, 1, 1));
        let window = rect(522, 345, 300, 200);
        let frame = i.frame(window);
        assert_eq!(frame, rect(521, 317, 302, 229));
        assert_eq!(i.window(frame), window);
        assert_eq!(Insets::default().frame(window), window);
    }

    #[test]
    fn the_bar_has_the_title_then_minimize_maximize_and_close_at_the_right() {
        let size = Size::from((302, 229));
        let at = |x: f64, y: f64| {
            hit(
                size,
                insets(),
                (x, y).into(),
                true,
                Side::Right,
                Shown::default(),
            )
        };
        assert_eq!(at(10.0, 10.0), Some(Hit::Title));
        assert_eq!(at(301.0, 0.0), Some(Hit::Button(Button::Close)));
        assert_eq!(at(274.0, 27.0), Some(Hit::Button(Button::Close)));
        assert_eq!(at(273.9, 14.0), Some(Hit::Button(Button::Maximize)));
        assert_eq!(at(246.0, 14.0), Some(Hit::Button(Button::Maximize)));
        assert_eq!(at(245.9, 14.0), Some(Hit::Button(Button::Minimize)));
        assert_eq!(at(218.0, 14.0), Some(Hit::Button(Button::Minimize)));
        assert_eq!(at(217.9, 14.0), Some(Hit::Title));
        assert_eq!(at(150.0, 100.0), None, "the window's own");
    }

    #[test]
    fn a_hidden_button_leaves_no_gap_and_its_room_to_the_title() {
        let size = Size::from((302, 229));
        let no_minimize = Shown {
            minimize: false,
            ..Shown::default()
        };
        let at = |x: f64, shown| hit(size, insets(), (x, 14.0).into(), true, Side::Right, shown);
        assert_eq!(at(280.0, no_minimize), Some(Hit::Button(Button::Close)));
        assert_eq!(at(250.0, no_minimize), Some(Hit::Button(Button::Maximize)));
        assert_eq!(at(230.0, no_minimize), Some(Hit::Title));
        // Only close: it keeps the outer end.
        let only_close = Shown {
            close: true,
            minimize: false,
            maximize: false,
        };
        assert_eq!(at(280.0, only_close), Some(Hit::Button(Button::Close)));
        assert_eq!(at(260.0, only_close), Some(Hit::Title));
        let on_left = hit(
            size,
            insets(),
            (5.0, 14.0).into(),
            true,
            Side::Left,
            only_close,
        );
        assert_eq!(on_left, Some(Hit::Button(Button::Close)));
        let none = Shown {
            close: false,
            minimize: false,
            maximize: false,
        };
        assert_eq!(at(290.0, none), Some(Hit::Title));
        assert_eq!(none.names(), "none");
        assert_eq!(no_minimize.names(), "close, maximize");
    }

    #[test]
    fn a_hidden_button_is_not_drawn() {
        let t = tokens();
        let mut pixels = vec![0; 302 * 28 * 4];
        let mut bar = look(302, true);
        bar.shown.minimize = false;
        paint(&mut pixels, &bar, &t, None, None);
        let focused = t.title_bar_focused.bytes();
        // Where minimize's dash was, the bar.
        assert_eq!(
            pixel(&pixels, 302, 232, 14),
            [focused[0], focused[1], focused[2]]
        );
        // Maximize's square stays where it was, next to close.
        let ink = t.title_text.bytes();
        assert_eq!(pixel(&pixels, 302, 255, 14), [ink[0], ink[1], ink[2]]);
    }

    #[test]
    fn on_the_left_close_comes_first_then_minimize_and_maximize() {
        let size = Size::from((302, 229));
        let at = |x: f64, y: f64| {
            hit(
                size,
                insets(),
                (x, y).into(),
                true,
                Side::Left,
                Shown::default(),
            )
        };
        assert_eq!(at(0.0, 0.0), Some(Hit::Button(Button::Close)));
        assert_eq!(at(27.9, 27.0), Some(Hit::Button(Button::Close)));
        assert_eq!(at(28.0, 14.0), Some(Hit::Button(Button::Minimize)));
        assert_eq!(at(56.0, 14.0), Some(Hit::Button(Button::Maximize)));
        assert_eq!(at(84.0, 14.0), Some(Hit::Title));
        assert_eq!(at(301.0, 0.0), Some(Hit::Title));
    }

    #[test]
    fn edges_and_corners_resize_and_a_maximized_window_has_none() {
        let size = Size::from((302, 229));
        let at = |x: f64, y: f64| {
            hit(
                size,
                insets(),
                (x, y).into(),
                true,
                Side::Right,
                Shown::default(),
            )
        };
        assert_eq!(at(-3.0, 100.0), Some(Hit::Edge(ResizeEdge::Left)));
        assert_eq!(at(0.5, 100.0), Some(Hit::Edge(ResizeEdge::Left)));
        assert_eq!(at(304.0, 100.0), Some(Hit::Edge(ResizeEdge::Right)));
        assert_eq!(at(100.0, 230.0), Some(Hit::Edge(ResizeEdge::Bottom)));
        assert_eq!(at(100.0, -2.0), Some(Hit::Edge(ResizeEdge::Top)));
        assert_eq!(at(-2.0, -2.0), Some(Hit::Edge(ResizeEdge::TopLeft)));
        assert_eq!(at(10.0, -2.0), Some(Hit::Edge(ResizeEdge::TopLeft)));
        assert_eq!(at(-2.0, 220.0), Some(Hit::Edge(ResizeEdge::BottomLeft)));
        assert_eq!(at(295.0, 233.0), Some(Hit::Edge(ResizeEdge::BottomRight)));
        assert_eq!(at(305.0, 5.0), Some(Hit::Edge(ResizeEdge::TopRight)));
        assert_eq!(at(-7.0, 100.0), None, "beyond the grip");
        let fixed = |x: f64, y: f64| {
            hit(
                size,
                insets(),
                (x, y).into(),
                false,
                Side::Right,
                Shown::default(),
            )
        };
        assert_eq!(fixed(-3.0, 100.0), None);
        assert_eq!(fixed(0.0, 100.0), Some(Hit::Title));
        assert_eq!(fixed(301.0, 5.0), Some(Hit::Button(Button::Close)));
    }

    fn look(width: i32, focused: bool) -> Look {
        Look {
            width,
            height: 28,
            scale_120: 120,
            title: "foot".into(),
            focused,
            maximized: false,
            hovered: None,
            text: false,
            side: Side::Right,
            shown: Shown::default(),
            icon: None,
            scheme: crate::tokens::Scheme::Dark,
        }
    }

    fn pixel(pixels: &[u8], width: i32, x: i32, y: i32) -> [u8; 3] {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i + 2], pixels[i + 1], pixels[i]]
    }

    #[test]
    fn the_bar_is_the_token_colour_with_its_buttons_drawn() {
        let t = tokens();
        let mut pixels = vec![0; 302 * 28 * 4];
        paint(&mut pixels, &look(302, true), &t, None, None);
        let focused = t.title_bar_focused.bytes();
        assert_eq!(
            pixel(&pixels, 302, 5, 5),
            [focused[0], focused[1], focused[2]]
        );
        assert_eq!(pixels[3], 0xff, "opaque");
        // The cross passes through the close button's centre, the square's
        // outline is crisp at its left edge.
        let ink = t.title_text.bytes();
        assert_ne!(
            pixel(&pixels, 302, 288, 14),
            [focused[0], focused[1], focused[2]]
        );
        assert_eq!(pixel(&pixels, 302, 255, 14), [ink[0], ink[1], ink[2]]);
        // Minimize's dash runs across its square's middle, nowhere else.
        let bar = [focused[0], focused[1], focused[2]];
        assert_ne!(pixel(&pixels, 302, 232, 14), bar);
        assert_eq!(pixel(&pixels, 302, 232, 8), bar);
        let mut unfocused = vec![0; 302 * 28 * 4];
        paint(&mut unfocused, &look(302, false), &t, None, None);
        let bar = t.title_bar.bytes();
        assert_eq!(pixel(&unfocused, 302, 5, 5), [bar[0], bar[1], bar[2]]);
        let mut hovered = look(302, false);
        hovered.hovered = Some(Button::Close);
        paint(&mut unfocused, &hovered, &t, None, None);
        let red = t.title_close_hover.bytes();
        assert_eq!(pixel(&unfocused, 302, 276, 2), [red[0], red[1], red[2]]);
    }

    #[test]
    fn on_the_left_the_buttons_are_drawn_from_the_left_end() {
        let t = tokens();
        let mut pixels = vec![0; 302 * 28 * 4];
        let mut left = look(302, false);
        left.side = Side::Left;
        left.hovered = Some(Button::Close);
        paint(&mut pixels, &left, &t, None, None);
        let red = t.title_close_hover.bytes();
        assert_eq!(pixel(&pixels, 302, 2, 2), [red[0], red[1], red[2]]);
        let bar = t.title_bar.bytes();
        assert_eq!(pixel(&pixels, 302, 276, 2), [bar[0], bar[1], bar[2]]);
        // Minimize's dash crosses the second square's middle.
        assert_ne!(pixel(&pixels, 302, 42, 14), [bar[0], bar[1], bar[2]]);
    }

    /// A font from the image, or one most build machines have; without one
    /// the test says so and passes, and CI's desktop test checks the title.
    fn some_text() -> Option<Text> {
        let paths = FONTS
            .iter()
            .chain(&["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"]);
        let found = paths
            .filter_map(|p| std::fs::read(p).ok())
            .find_map(|data| Text::new(&data, 13.0).ok());
        if found.is_none() {
            eprintln!("no font here; the title text is left to CI's desktop test");
        }
        found
    }

    #[test]
    fn titles_are_drawn_and_cut_short_to_fit() {
        let Some(mut text) = some_text() else {
            return;
        };
        let short = text.fit("foot", 200.0);
        assert_eq!(short, "foot");
        let long = text.fit("a title far too long for the little room it gets", 100.0);
        assert!(long.ends_with('\u{2026}') && long.len() > 3, "{long}");
        assert!(text.width(&long) <= 100.0);
        let t = tokens();
        let mut plain = vec![0; 302 * 28 * 4];
        paint(&mut plain, &look(302, true), &t, None, None);
        let mut titled = vec![0; 302 * 28 * 4];
        let mut with_text = look(302, true);
        with_text.text = true;
        paint(&mut titled, &with_text, &t, Some(&mut text), None);
        let changed: Vec<usize> = (0..302 * 28)
            .filter(|i| plain[i * 4..i * 4 + 4] != titled[i * 4..i * 4 + 4])
            .map(|i| i % 302)
            .collect();
        assert!(changed.len() > 20, "the title is drawn");
        let (left, right) = (changed.iter().min().unwrap(), changed.iter().max().unwrap());
        assert!(*left > 120 && *right < 182, "centred: {left} to {right}");
        // With the app's icon (M5.6a), the icon comes first, left of the
        // title, and the two stay centred together.
        let mut red = edel::app_icons::Picture::new(16, 16).unwrap();
        for p in red.data_mut().chunks_exact_mut(4) {
            p.copy_from_slice(&[255, 0, 0, 255]);
        }
        let mut iconed = vec![0; 302 * 28 * 4];
        with_text.icon = Some("red".into());
        paint(&mut iconed, &with_text, &t, Some(&mut text), Some(&red));
        let reds: Vec<usize> = (0..302 * 28)
            .filter(|i| iconed[i * 4..i * 4 + 4] == [0, 0, 255, 255])
            .map(|i| i % 302)
            .collect();
        assert_eq!(reds.len(), 16 * 16, "the whole icon is drawn");
        let icon_right = *reds.iter().max().unwrap();
        let title_left = (0..302 * 28)
            .filter(|i| {
                plain[i * 4..i * 4 + 4] != iconed[i * 4..i * 4 + 4]
                    && iconed[i * 4..i * 4 + 4] != [0, 0, 255, 255]
            })
            .map(|i| i % 302)
            .min()
            .unwrap();
        assert!(
            icon_right + 5 < title_left,
            "the icon comes before the title"
        );
        assert!(
            *reds.iter().min().unwrap() < *left,
            "the pair is centred together"
        );
    }

    #[test]
    fn at_scale_two_the_bar_has_twice_the_pixels_and_thicker_lines() {
        let t = tokens();
        let mut look = look(604, true);
        look.height = 56;
        look.scale_120 = 240;
        let mut pixels = vec![0; 604 * 56 * 4];
        paint(&mut pixels, &look, &t, None, None);
        let bar = t.title_bar_focused.bytes();
        assert_eq!(pixel(&pixels, 604, 10, 50), [bar[0], bar[1], bar[2]]);
        // Close is the rightmost 56 px; its cross crosses at the middle,
        // and the square's outline is two pixels thick.
        let ink = t.title_text.bytes();
        let ink = [ink[0], ink[1], ink[2]];
        assert_eq!(pixel(&pixels, 604, 576, 28), ink);
        let left = 604 - 2 * 56 + 28 - 10;
        assert_eq!(pixel(&pixels, 604, left, 28), ink);
        assert_eq!(pixel(&pixels, 604, left + 1, 28), ink);
        assert_eq!(pixel(&pixels, 604, left + 2, 28), [bar[0], bar[1], bar[2]]);
    }

    #[test]
    fn a_bar_too_narrow_for_its_buttons_is_still_drawn() {
        let mut pixels = vec![0; 30 * 28 * 4];
        paint(&mut pixels, &look(30, true), &tokens(), None, None);
        paint(&mut [], &look(30, true), &tokens(), None, None);
    }
}
