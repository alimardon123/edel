//! Server-side window frames (roadmap M4.4): a title bar above each window
//! that asks for one through xdg-decoration or lets the compositor choose,
//! and a thin border round the rest. Apps that draw their own bars, as
//! libadwaita apps do, get none, so nothing is drawn twice. The bar holds
//! the title and, at the right, maximize and close; minimize joins them
//! with the window list that brings a minimized window back (M5.2).
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

/// A button on the bar, from the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Close,
    Maximize,
    /// Since M5.2h, when the panel's window list brings a minimized
    /// window back.
    Minimize,
}

/// The buttons, rightmost first.
pub const BUTTONS: [Button; 3] = [Button::Close, Button::Maximize, Button::Minimize];

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
/// left; none inside the window or beyond the grips. A window that cannot
/// be resized (a maximized one) has no grips, and its border counts as its
/// bar.
pub fn hit(
    size: Size<i32, Logical>,
    insets: Insets,
    point: Point<f64, Logical>,
    resizable: bool,
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
        // Close is 0, the square ending at the right edge; then leftwards.
        let from_right = ((w - x) / top).ceil() as usize - 1;
        return Some(
            BUTTONS
                .get(from_right)
                .map_or(Hit::Title, |b| Hit::Button(*b)),
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
}

/// Draws the bar `look` describes into `pixels`, `look.width` by
/// `look.height` ARGB8888 in memory order (blue, green, red, alpha).
pub fn paint(pixels: &mut [u8], look: &Look, tokens: &Tokens, text: Option<&mut Text>) {
    let (w, h) = (look.width.max(0) as usize, look.height.max(0) as usize);
    if pixels.len() < w * h * 4 || w == 0 || h == 0 {
        return;
    }
    let scale = look.scale_120.max(120) as f32 / 120.0;
    let mut canvas = Canvas {
        pixels,
        width: w,
        stroke: scale,
    };
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
    for (i, button) in BUTTONS.iter().enumerate() {
        let Some(x) = w.checked_sub((i + 1) * size) else {
            break;
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
    let Some(text) = text else {
        return;
    };
    // The title is centred on the bar, moved left if it would reach the
    // buttons, and cut short with an ellipsis if it is still too long.
    let pad = (h / 2) as f32;
    let right = w as f32 - (BUTTONS.len() * size) as f32 - pad;
    let room = right - pad;
    if room <= 0.0 || look.title.is_empty() {
        return;
    }
    text.set_size(tokens.title_text_size as f32 * scale);
    let title = text.fit(&look.title, room);
    let width = text.width(&title);
    let x = ((w as f32 - width) / 2.0).min(right - width).max(pad);
    let baseline = ((h as f32 + text.ascent + text.descent) / 2.0).round();
    text.draw(&mut canvas, &title, x, baseline, ink, right);
}

struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: usize,
    /// How thick lines are, in pixels: the scale, so icons keep their weight.
    stroke: f32,
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

    /// A button's icon, drawn in the middle of the square at `x`: a cross
    /// for close, a square for maximize, two for restore, a dash across
    /// the middle for minimize.
    fn icon(&mut self, button: Button, maximized: bool, x: usize, size: usize, colour: Colour) {
        let half = (size as f32 * 0.17).round().max(3.0);
        let centre = (x as f32 + size as f32 / 2.0, size as f32 / 2.0);
        match button {
            Button::Close => {
                let (cx, cy) = centre;
                let a = half - 0.5;
                self.line((cx - a, cy - a), (cx + a, cy + a), colour);
                self.line((cx - a, cy + a), (cx + a, cy - a), colour);
            }
            Button::Maximize if maximized => {
                let step = self.stroke.round().max(1.0) as i64 * 2;
                let s = half as i64 * 2 - step;
                let x0 = (centre.0 - half) as i64;
                let y0 = (centre.1 - half) as i64 + step;
                // The window behind, then the one in front over it.
                self.outline(x0 + step, y0 - step, s, colour, Some((x0, y0, s)));
                self.outline(x0, y0, s, colour, None);
            }
            Button::Maximize => {
                let s = half as i64 * 2;
                let x0 = (centre.0 - half) as i64;
                let y0 = (centre.1 - half) as i64;
                self.outline(x0, y0, s, colour, None);
            }
            Button::Minimize => {
                let (cx, cy) = centre;
                let a = half - 0.5;
                self.line((cx - a, cy), (cx + a, cy), colour);
            }
        }
    }

    /// A square's outline, `s` wide and one stroke thick, leaving out
    /// what falls inside the square `hidden` (x, y, side).
    fn outline(
        &mut self,
        x0: i64,
        y0: i64,
        s: i64,
        colour: Colour,
        hidden: Option<(i64, i64, i64)>,
    ) {
        let t = self.stroke.round().max(1.0) as i64;
        for y in y0..y0 + s {
            for x in x0..x0 + s {
                let edge = x < x0 + t || y < y0 + t || x >= x0 + s - t || y >= y0 + s - t;
                let behind = hidden
                    .is_some_and(|(hx, hy, hs)| x >= hx && x < hx + hs && y >= hy && y < hy + hs);
                if edge && !behind {
                    self.blend(x, y, colour, 1.0);
                }
            }
        }
    }

    /// A smooth line 1.5 strokes wide from `a` to `b`.
    fn line(&mut self, a: (f32, f32), b: (f32, f32), colour: Colour) {
        let width = 1.5 * self.stroke;
        let (x0, x1) = (
            a.0.min(b.0).floor() as i64 - 1,
            a.0.max(b.0).ceil() as i64 + 1,
        );
        let (y0, y1) = (
            a.1.min(b.1).floor() as i64 - 1,
            a.1.max(b.1).ceil() as i64 + 1,
        );
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = (((px - a.0) * dx + (py - a.1) * dy) / (length * length)).clamp(0.0, 1.0);
                let (nx, ny) = (a.0 + t * dx - px, a.1 + t * dy - py);
                let distance = (nx * nx + ny * ny).sqrt();
                self.blend(x, y, colour, width / 2.0 + 0.5 - distance);
            }
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
        let at = |x: f64, y: f64| hit(size, insets(), (x, y).into(), true);
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
    fn edges_and_corners_resize_and_a_maximized_window_has_none() {
        let size = Size::from((302, 229));
        let at = |x: f64, y: f64| hit(size, insets(), (x, y).into(), true);
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
        let fixed = |x: f64, y: f64| hit(size, insets(), (x, y).into(), false);
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
        paint(&mut pixels, &look(302, true), &t, None);
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
        paint(&mut unfocused, &look(302, false), &t, None);
        let bar = t.title_bar.bytes();
        assert_eq!(pixel(&unfocused, 302, 5, 5), [bar[0], bar[1], bar[2]]);
        let mut hovered = look(302, false);
        hovered.hovered = Some(Button::Close);
        paint(&mut unfocused, &hovered, &t, None);
        let red = t.title_close_hover.bytes();
        assert_eq!(pixel(&unfocused, 302, 276, 2), [red[0], red[1], red[2]]);
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
        paint(&mut plain, &look(302, true), &t, None);
        let mut titled = vec![0; 302 * 28 * 4];
        let mut with_text = look(302, true);
        with_text.text = true;
        paint(&mut titled, &with_text, &t, Some(&mut text));
        let changed: Vec<usize> = (0..302 * 28)
            .filter(|i| plain[i * 4..i * 4 + 4] != titled[i * 4..i * 4 + 4])
            .map(|i| i % 302)
            .collect();
        assert!(changed.len() > 20, "the title is drawn");
        let (left, right) = (changed.iter().min().unwrap(), changed.iter().max().unwrap());
        assert!(*left > 120 && *right < 182, "centred: {left} to {right}");
    }

    #[test]
    fn at_scale_two_the_bar_has_twice_the_pixels_and_thicker_lines() {
        let t = tokens();
        let mut look = look(604, true);
        look.height = 56;
        look.scale_120 = 240;
        let mut pixels = vec![0; 604 * 56 * 4];
        paint(&mut pixels, &look, &t, None);
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
        paint(&mut pixels, &look(30, true), &tokens(), None);
        paint(&mut [], &look(30, true), &tokens(), None);
    }
}
