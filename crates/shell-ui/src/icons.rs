//! App icons (M5.4c), found as the icon theme spec says for the theme
//! every app installs into, hicolor, and in `/usr/share/pixmaps`: a PNG at
//! least as big as wanted, else an SVG, else the biggest smaller PNG. Each
//! is drawn once at the size the panel asks and kept, PNGs through
//! tiny-skia and SVGs through `edel::icons` (resvg, which draws with the
//! same tiny-skia).
//! A name that finds nothing is remembered as nothing, so it is looked
//! for once. Plain files, tested without a display.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

/// hicolor's fixed sizes, small to big.
const SIZES: [u32; 10] = [16, 22, 24, 32, 48, 64, 96, 128, 256, 512];

/// Icons drawn so far, by name and size in pixels.
pub struct Icons {
    /// The data directories, first wins.
    dirs: Vec<PathBuf>,
    drawn: HashMap<(String, u32), Option<Pixmap>>,
}

impl Icons {
    pub fn new(dirs: Vec<PathBuf>) -> Icons {
        Icons {
            dirs,
            drawn: HashMap::new(),
        }
    }

    /// `icon`, a name or a file's path, `px` pixels square; none when
    /// there is no such icon or it cannot be read.
    pub fn get(&mut self, icon: &str, px: u32) -> Option<&Pixmap> {
        let key = (icon.to_string(), px);
        if !self.drawn.contains_key(&key) {
            let found = find(&self.dirs, icon, px).and_then(|path| draw(&path, px));
            self.drawn.insert(key.clone(), found);
        }
        self.drawn.get(&key).and_then(Option::as_ref)
    }
}

/// The file for `icon` at `px`, as the order above says.
pub fn find(dirs: &[PathBuf], icon: &str, px: u32) -> Option<PathBuf> {
    let path = Path::new(icon);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    // A name never has an extension, though some files write one.
    let name = icon
        .strip_suffix(".png")
        .or_else(|| icon.strip_suffix(".svg"))
        .unwrap_or(icon);
    let themes: Vec<PathBuf> = dirs.iter().map(|d| d.join("icons/hicolor")).collect();
    let png = |size: u32| {
        themes
            .iter()
            .map(|t| t.join(format!("{size}x{size}/apps/{name}.png")))
            .find(|p| p.is_file())
    };
    let svg = || {
        themes
            .iter()
            .map(|t| t.join(format!("scalable/apps/{name}.svg")))
            .find(|p| p.is_file())
    };
    SIZES
        .iter()
        .filter(|s| **s >= px)
        .find_map(|s| png(*s))
        .or_else(svg)
        .or_else(|| {
            SIZES
                .iter()
                .rev()
                .filter(|s| **s < px)
                .find_map(|s| png(*s))
        })
        .or_else(|| {
            ["png", "svg"]
                .iter()
                .map(|ext| PathBuf::from(format!("/usr/share/pixmaps/{name}.{ext}")))
                .find(|p| p.is_file())
        })
}

/// The icon at `path` drawn `px` pixels square, kept to its shape and
/// centred; an SVG through `edel::icons`, as the shell's own icons are.
pub fn draw(path: &Path, px: u32) -> Option<Pixmap> {
    if path.extension().is_some_and(|e| e == "svg") {
        return edel::icons::svg_pixmap(&std::fs::read(path).ok()?, px);
    }
    let mut out = Pixmap::new(px, px)?;
    let image = Pixmap::load_png(path).ok()?;
    let (scale, dx, dy) = edel::icons::fit(image.width() as f32, image.height() as f32, px);
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    let at = Transform::from_translate(dx, dy).pre_scale(scale, scale);
    out.draw_pixmap(0, 0, image.as_ref(), &paint, at, None);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ff0000"/></svg>"##;

    /// An empty directory of its own for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-icons-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn theme(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join("icons/hicolor").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn png(size: u32) -> Vec<u8> {
        let mut pixmap = Pixmap::new(size, size).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(0, 0, 255, 255));
        pixmap.encode_png().unwrap()
    }

    #[test]
    fn a_big_enough_png_comes_first_then_an_svg_then_a_smaller_png() {
        let root = scratch("order");
        let dirs = vec![root.clone()];
        assert_eq!(find(&dirs, "foot", 32), None);
        theme(&root, "16x16/apps/foot.png", &png(16));
        assert!(
            find(&dirs, "foot", 32)
                .unwrap()
                .ends_with("16x16/apps/foot.png")
        );
        theme(&root, "scalable/apps/foot.svg", SVG.as_bytes());
        assert!(
            find(&dirs, "foot.svg", 32)
                .unwrap()
                .ends_with("scalable/apps/foot.svg")
        );
        theme(&root, "48x48/apps/foot.png", &png(48));
        assert!(
            find(&dirs, "foot", 32)
                .unwrap()
                .ends_with("48x48/apps/foot.png")
        );
    }

    #[test]
    fn pngs_and_svgs_are_drawn_at_the_size_asked() {
        let root = scratch("drawn");
        theme(&root, "48x48/apps/blue.png", &png(48));
        theme(&root, "scalable/apps/red.svg", SVG.as_bytes());
        let mut icons = Icons::new(vec![root]);
        let blue = icons.get("blue", 24).unwrap().pixel(12, 12).unwrap();
        assert_eq!((blue.red(), blue.blue(), blue.alpha()), (0, 255, 255));
        let red = icons.get("red", 24).unwrap();
        assert_eq!(red.width(), 24);
        let mid = red.pixel(12, 12).unwrap();
        assert_eq!((mid.red(), mid.blue(), mid.alpha()), (255, 0, 255));
        assert!(icons.get("missing", 24).is_none());
    }
}
