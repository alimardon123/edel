//! App icons (M5.4c), for shell-ui's panel and launcher and the
//! compositor's title bars (M5.6a), found in the icon theme the person
//! chose (`appearance.icon_theme`, M8.21 part g), then the themes it
//! inherits, depth first in the order written, then hicolor, the theme
//! every app installs into, and last `/usr/share/pixmaps`. Within a theme a
//! PNG at least as big as wanted comes first, then an SVG, then the biggest
//! smaller PNG (an SVG in a fixed folder counts with the PNGs, as the spec's
//! fixed folders do). Each is drawn once at the size the panel asks and
//! kept, PNGs through tiny-skia and SVGs through `edel::icons` (resvg, which
//! draws with the same tiny-skia). A name that finds nothing is remembered
//! as nothing, so it is looked for once. Plain files, tested without a
//! display.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use resvg::tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use crate::icon_theme;

/// What an icon is drawn into: premultiplied RGBA, as tiny-skia keeps it.
pub use resvg::tiny_skia::Pixmap as Picture;

/// hicolor's fixed sizes, small to big.
const SIZES: [u32; 10] = [16, 22, 24, 32, 48, 64, 96, 128, 256, 512];

/// The theme every app installs into, searched last of the icon themes.
const HICOLOR: &str = "hicolor";

/// The most themes one chain holds, the chosen one and what it inherits
/// (hicolor not counted), so a loop of inheritance cannot grow it.
const MOST_THEMES: usize = 16;

/// Icons drawn so far, by name and size in pixels.
pub struct Icons {
    /// The icon theme's folder name, as the person chose it.
    theme: String,
    /// The chosen theme, its inherited themes in order, then hicolor.
    chain: Vec<Theme>,
    drawn: HashMap<(String, u32), Option<Pixmap>>,
}

/// One icon theme as the chain searches it: the folders it has, and the
/// app folders of its index (or hicolor's fixed list).
struct Theme {
    /// Every `icons/NAME` folder of the data directories that exists.
    roots: Vec<PathBuf>,
    /// The folders that hold app icons, in index order.
    dirs: Vec<icon_theme::Dir>,
}

impl Icons {
    /// The icons of the theme `theme` (a folder name, `appearance.icon_theme`)
    /// under `dirs`, the chain built once here.
    pub fn new(dirs: Vec<PathBuf>, theme: &str) -> Icons {
        let mut seen = Vec::new();
        let mut chain = Vec::new();
        gather(&dirs, theme, &mut seen, &mut chain);
        chain.push(hicolor(&dirs));
        Icons {
            theme: theme.to_string(),
            chain,
            drawn: HashMap::new(),
        }
    }

    /// The icon theme these icons come from, as [`Icons::new`] was given it.
    pub fn theme(&self) -> &str {
        &self.theme
    }

    /// `icon`, a name or a file's path, `px` pixels square; none when
    /// there is no such icon or it cannot be read.
    pub fn get(&mut self, icon: &str, px: u32) -> Option<&Pixmap> {
        let key = (icon.to_string(), px);
        if !self.drawn.contains_key(&key) {
            let found = self.find(icon, px).and_then(|path| draw(&path, px));
            self.drawn.insert(key.clone(), found);
        }
        self.drawn.get(&key).and_then(Option::as_ref)
    }

    /// The file for `icon` at `px`, as the order above says.
    pub fn find(&self, icon: &str, px: u32) -> Option<PathBuf> {
        let path = Path::new(icon);
        if path.is_absolute() {
            return path.is_file().then(|| path.to_path_buf());
        }
        // A name never has an extension, though some files write one.
        let name = icon
            .strip_suffix(".png")
            .or_else(|| icon.strip_suffix(".svg"))
            .unwrap_or(icon);
        self.chain
            .iter()
            .find_map(|theme| theme.find(name, px))
            .or_else(|| {
                ["png", "svg"]
                    .iter()
                    .map(|ext| PathBuf::from(format!("/usr/share/pixmaps/{name}.{ext}")))
                    .find(|p| p.is_file())
            })
    }
}

impl Theme {
    /// The file for `name` at `px` in this theme: the fixed folders at least
    /// as big, smallest first, then the scalable ones, then the smaller fixed
    /// folders, biggest first. Among fixed folders of one size, an unscaled
    /// one comes before a scaled one (M8.21 part g).
    fn find(&self, name: &str, px: u32) -> Option<PathBuf> {
        let in_dir = |dir: &icon_theme::Dir, exts: &[&str]| {
            exts.iter().find_map(|ext| {
                self.roots
                    .iter()
                    .map(|root| root.join(&dir.path).join(format!("{name}.{ext}")))
                    .find(|p| p.is_file())
            })
        };
        let mut fixed: Vec<&icon_theme::Dir> = self.dirs.iter().filter(|d| !d.scalable).collect();
        fixed.sort_by_key(|d| (d.size, d.scale != 1));
        fixed
            .iter()
            .filter(|d| d.size >= px)
            .find_map(|d| in_dir(d, &["png", "svg"]))
            .or_else(|| {
                self.dirs
                    .iter()
                    .filter(|d| d.scalable)
                    .find_map(|d| in_dir(d, &["svg"]))
            })
            .or_else(|| {
                fixed
                    .iter()
                    .rev()
                    .filter(|d| d.size < px)
                    .find_map(|d| in_dir(d, &["svg", "png"]))
            })
    }
}

/// Adds the theme `name` and then the themes it inherits, depth first in the
/// order written, each once. hicolor is left to [`Icons::new`], which adds it
/// last; a theme with no index is left out, as the spec says (M8.21 part g).
fn gather(dirs: &[PathBuf], name: &str, seen: &mut Vec<String>, chain: &mut Vec<Theme>) {
    if chain.len() >= MOST_THEMES || name == HICOLOR || seen.iter().any(|s| s == name) {
        return;
    }
    seen.push(name.to_string());
    let Some(index) = index_of(dirs, name) else {
        return;
    };
    let icon_theme::Index {
        inherits,
        dirs: folders,
        ..
    } = index;
    chain.push(Theme {
        roots: roots_of(dirs, name),
        dirs: keep_apps(folders),
    });
    for parent in &inherits {
        gather(dirs, parent, seen, chain);
    }
}

/// hicolor as the chain searches it: its index's app folders, or the fixed
/// list when it has no index (`NxN/apps` for each size, and `scalable/apps`).
fn hicolor(dirs: &[PathBuf]) -> Theme {
    let folders = match index_of(dirs, HICOLOR) {
        Some(index) => index.dirs,
        None => SIZES
            .iter()
            .map(|&size| icon_theme::Dir {
                path: format!("{size}x{size}/apps"),
                size,
                scale: 1,
                scalable: false,
                apps: true,
            })
            .chain(std::iter::once(icon_theme::Dir {
                path: "scalable/apps".to_string(),
                size: 0,
                scale: 1,
                scalable: true,
                apps: true,
            }))
            .collect(),
    };
    Theme {
        roots: roots_of(dirs, HICOLOR),
        dirs: keep_apps(folders),
    }
}

/// The index of the theme `name`: the first `icons/NAME/index.theme` the
/// data directories give that reads.
fn index_of(dirs: &[PathBuf], name: &str) -> Option<icon_theme::Index> {
    dirs.iter()
        .find_map(|dir| icon_theme::read(&dir.join("icons").join(name)))
}

/// Every `icons/NAME` folder of the data directories that exists.
fn roots_of(dirs: &[PathBuf], name: &str) -> Vec<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join("icons").join(name))
        .filter(|path| path.is_dir())
        .collect()
}

/// The folders that hold app icons; the rest (places, UI, devices) are never
/// searched, which keeps the stat count low on the compositor's thread.
fn keep_apps(folders: Vec<icon_theme::Dir>) -> Vec<icon_theme::Dir> {
    folders.into_iter().filter(|d| d.apps).collect()
}

/// The icon at `path` drawn `px` pixels square, kept to its shape and
/// centred; an SVG through `edel::icons`, as the shell's own icons are.
pub fn draw(path: &Path, px: u32) -> Option<Pixmap> {
    if path.extension().is_some_and(|e| e == "svg") {
        return crate::icons::svg_pixmap(&std::fs::read(path).ok()?, px);
    }
    let mut out = Pixmap::new(px, px)?;
    let image = Pixmap::load_png(path).ok()?;
    let (scale, dx, dy) = crate::icons::fit(image.width() as f32, image.height() as f32, px);
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
    use resvg::tiny_skia;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#ff0000"/></svg>"##;

    /// An empty directory of its own for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-icons-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A file in the theme `theme` under `root`.
    fn put(root: &Path, theme: &str, rel: &str, bytes: &[u8]) {
        let path = root.join("icons").join(theme).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn theme(root: &Path, rel: &str, bytes: &[u8]) {
        put(root, HICOLOR, rel, bytes);
    }

    fn png(size: u32) -> Vec<u8> {
        let mut pixmap = Pixmap::new(size, size).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(0, 0, 255, 255));
        pixmap.encode_png().unwrap()
    }

    #[test]
    fn a_big_enough_png_comes_first_then_an_svg_then_a_smaller_png() {
        let root = scratch("order");
        // The folders are read when the icons are made, so each step makes
        // its own.
        let find = |icon: &str| Icons::new(vec![root.clone()], HICOLOR).find(icon, 32);
        assert_eq!(find("foot"), None);
        theme(&root, "16x16/apps/foot.png", &png(16));
        assert!(find("foot").unwrap().ends_with("16x16/apps/foot.png"));
        theme(&root, "scalable/apps/foot.svg", SVG.as_bytes());
        assert!(
            find("foot.svg")
                .unwrap()
                .ends_with("scalable/apps/foot.svg")
        );
        theme(&root, "48x48/apps/foot.png", &png(48));
        assert!(find("foot").unwrap().ends_with("48x48/apps/foot.png"));
    }

    #[test]
    fn pngs_and_svgs_are_drawn_at_the_size_asked() {
        let root = scratch("drawn");
        theme(&root, "48x48/apps/blue.png", &png(48));
        theme(&root, "scalable/apps/red.svg", SVG.as_bytes());
        let mut icons = Icons::new(vec![root], HICOLOR);
        let blue = icons.get("blue", 24).unwrap().pixel(12, 12).unwrap();
        assert_eq!((blue.red(), blue.blue(), blue.alpha()), (0, 255, 255));
        let red = icons.get("red", 24).unwrap();
        assert_eq!(red.width(), 24);
        let mid = red.pixel(12, 12).unwrap();
        assert_eq!((mid.red(), mid.blue(), mid.alpha()), (255, 0, 255));
        assert!(icons.get("missing", 24).is_none());
    }

    /// An index with the given text, for the theme `theme` under `root`.
    fn index(root: &Path, theme: &str, text: &str) {
        put(root, theme, "index.theme", text.as_bytes());
    }

    #[test]
    fn the_chosen_themes_icon_comes_before_hicolors() {
        let root = scratch("chosen");
        index(
            &root,
            "T",
            "[Icon Theme]\nDirectories=48x48/apps\n[48x48/apps]\nSize=48\nContext=Applications\n",
        );
        put(&root, "T", "48x48/apps/foot.png", &png(48));
        theme(&root, "48x48/apps/foot.png", &png(48));
        let dirs = vec![root.clone()];
        let chosen = Icons::new(dirs.clone(), "T").find("foot", 32).unwrap();
        assert!(
            chosen.ends_with("icons/T/48x48/apps/foot.png"),
            "{chosen:?}"
        );
        let hicolor_only = Icons::new(dirs, HICOLOR).find("foot", 32).unwrap();
        assert!(hicolor_only.ends_with("icons/hicolor/48x48/apps/foot.png"));
    }

    #[test]
    fn inherited_themes_are_searched_in_order_and_a_loop_ends() {
        let root = scratch("inherits");
        index(
            &root,
            "A",
            "[Icon Theme]\nInherits=B\nDirectories=48x48/apps\n[48x48/apps]\nSize=48\nContext=Applications\n",
        );
        index(
            &root,
            "B",
            "[Icon Theme]\nInherits=A\nDirectories=48x48/apps\n[48x48/apps]\nSize=48\nContext=Applications\n",
        );
        put(&root, "B", "48x48/apps/only-b.png", &png(48));
        let found = Icons::new(vec![root], "A").find("only-b", 32);
        assert!(found.unwrap().ends_with("icons/B/48x48/apps/only-b.png"));
    }

    #[test]
    fn a_theme_without_an_app_folder_falls_to_hicolor() {
        let root = scratch("no-apps");
        index(
            &root,
            "T",
            "[Icon Theme]\nDirectories=16x16/ui\n[16x16/ui]\nSize=16\nContext=UI\n",
        );
        put(&root, "T", "16x16/ui/foot.png", &png(16));
        theme(&root, "16x16/apps/foot.png", &png(16));
        let found = Icons::new(vec![root], "T").find("foot", 16).unwrap();
        assert!(
            found.ends_with("icons/hicolor/16x16/apps/foot.png"),
            "{found:?}"
        );
    }

    #[test]
    fn svgs_in_fixed_folders_are_found() {
        let root = scratch("papirus");
        index(
            &root,
            "P",
            "[Icon Theme]\nDirectories=64x64/apps\n[64x64/apps]\nSize=64\nType=Fixed\nContext=Applications\n",
        );
        put(&root, "P", "64x64/apps/foot.svg", SVG.as_bytes());
        let found = Icons::new(vec![root], "P").find("foot", 32).unwrap();
        assert!(found.ends_with("icons/P/64x64/apps/foot.svg"), "{found:?}");
    }

    #[test]
    fn an_unscaled_folder_beats_a_scaled_one_of_the_same_size() {
        let root = scratch("scaled");
        index(
            &root,
            "S",
            "[Icon Theme]\nDirectories=16x16@2/apps,32x32/apps\n\
             [16x16@2/apps]\nSize=16\nScale=2\nContext=Applications\n\
             [32x32/apps]\nSize=32\nContext=Applications\n",
        );
        put(&root, "S", "16x16@2/apps/foot.png", &png(32));
        put(&root, "S", "32x32/apps/foot.png", &png(32));
        let found = Icons::new(vec![root], "S").find("foot", 32).unwrap();
        assert!(found.ends_with("icons/S/32x32/apps/foot.png"), "{found:?}");
    }
}
