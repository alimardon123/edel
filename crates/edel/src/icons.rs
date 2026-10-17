//! The shell's own icons (M5.5d): the menu button, the layout toggle, the
//! status area's and the title bar buttons, one SVG file each in
//! `design/icons/`, built into the compositor and shell-ui, so changing one
//! file changes every place that shows it. Each is drawn as a shape in black and coloured by the
//! caller with a token, as GNOME's symbolic icons are.

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

use crate::tokens::Colour;

/// Each icon's name and its file.
pub const BUILT_IN: [(&str, &str); 78] = [
    ("menu", include_str!("../../../design/icons/menu.svg")),
    ("more", include_str!("../../../design/icons/more.svg")),
    // Customize, a look's edit (M5.10c7b).
    ("edit", include_str!("../../../design/icons/edit.svg")),
    // An app with no icon of its own, in the window list (M5.29).
    (
        "app-generic",
        include_str!("../../../design/icons/app-generic.svg"),
    ),
    (
        "layout-floating",
        include_str!("../../../design/icons/layout-floating.svg"),
    ),
    (
        "layout-tiling",
        include_str!("../../../design/icons/layout-tiling.svg"),
    ),
    (
        "minimize",
        include_str!("../../../design/icons/minimize.svg"),
    ),
    (
        "maximize",
        include_str!("../../../design/icons/maximize.svg"),
    ),
    ("restore", include_str!("../../../design/icons/restore.svg")),
    ("close", include_str!("../../../design/icons/close.svg")),
    // Settings' (M5.6a): its pages, its search field, Copy as command,
    // the chosen card, the way back from a page on a narrow window and a
    // value chosen from a few.
    (
        "page-layout",
        include_str!("../../../design/icons/page-layout.svg"),
    ),
    // The Panels, Workspaces and Appearance pages' (M5.2q).
    (
        "page-panels",
        include_str!("../../../design/icons/page-panels.svg"),
    ),
    (
        "page-workspaces",
        include_str!("../../../design/icons/page-workspaces.svg"),
    ),
    (
        "page-appearance",
        include_str!("../../../design/icons/page-appearance.svg"),
    ),
    (
        "page-displays",
        include_str!("../../../design/icons/page-displays.svg"),
    ),
    (
        "page-about",
        include_str!("../../../design/icons/page-about.svg"),
    ),
    // Settings' Lock screen page (M5.32c).
    (
        "page-lock-screen",
        include_str!("../../../design/icons/page-lock-screen.svg"),
    ),
    // The Sound page's (M5.7b).
    (
        "page-sound",
        include_str!("../../../design/icons/page-sound.svg"),
    ),
    // The Network and Bluetooth pages' (M5.8a), and the padlock of a Wi-Fi
    // network that asks for a password.
    (
        "page-network",
        include_str!("../../../design/icons/page-network.svg"),
    ),
    (
        "page-bluetooth",
        include_str!("../../../design/icons/page-bluetooth.svg"),
    ),
    // Settings' Region page (M6.11): a globe.
    (
        "page-region",
        include_str!("../../../design/icons/page-region.svg"),
    ),
    ("lock", include_str!("../../../design/icons/lock.svg")),
    // The Power and Users pages' (M5.8b).
    (
        "page-power",
        include_str!("../../../design/icons/page-power.svg"),
    ),
    (
        "page-users",
        include_str!("../../../design/icons/page-users.svg"),
    ),
    // Settings' installer page, shown on a stick (M6.6a).
    (
        "page-install",
        include_str!("../../../design/icons/page-install.svg"),
    ),
    // The Updates and System pages' (M5.8c).
    (
        "page-updates",
        include_str!("../../../design/icons/page-updates.svg"),
    ),
    (
        "page-system",
        include_str!("../../../design/icons/page-system.svg"),
    ),
    ("search", include_str!("../../../design/icons/search.svg")),
    ("copy", include_str!("../../../design/icons/copy.svg")),
    ("check", include_str!("../../../design/icons/check.svg")),
    ("back", include_str!("../../../design/icons/back.svg")),
    ("updown", include_str!("../../../design/icons/updown.svg")),
    // The Updates page's big status icons (M5.8d).
    (
        "status-ok",
        include_str!("../../../design/icons/status-ok.svg"),
    ),
    (
        "status-sync",
        include_str!("../../../design/icons/status-sync.svg"),
    ),
    (
        "status-new",
        include_str!("../../../design/icons/status-new.svg"),
    ),
    (
        "status-restart",
        include_str!("../../../design/icons/status-restart.svg"),
    ),
    (
        "status-problem",
        include_str!("../../../design/icons/status-problem.svg"),
    ),
    // The panel's status area and quick settings (M5.9a): the network by
    // its kind and signal, the volume by its level, the battery and the
    // marks of a charging one, and the tiles' and buttons' own.
    (
        "net-wired",
        include_str!("../../../design/icons/net-wired.svg"),
    ),
    (
        "net-wifi-1",
        include_str!("../../../design/icons/net-wifi-1.svg"),
    ),
    (
        "net-wifi-2",
        include_str!("../../../design/icons/net-wifi-2.svg"),
    ),
    (
        "net-wifi-3",
        include_str!("../../../design/icons/net-wifi-3.svg"),
    ),
    (
        "net-offline",
        include_str!("../../../design/icons/net-offline.svg"),
    ),
    (
        "airplane",
        include_str!("../../../design/icons/airplane.svg"),
    ),
    (
        "volume-muted",
        include_str!("../../../design/icons/volume-muted.svg"),
    ),
    (
        "volume-low",
        include_str!("../../../design/icons/volume-low.svg"),
    ),
    (
        "volume-medium",
        include_str!("../../../design/icons/volume-medium.svg"),
    ),
    (
        "volume-high",
        include_str!("../../../design/icons/volume-high.svg"),
    ),
    (
        "brightness",
        include_str!("../../../design/icons/brightness.svg"),
    ),
    ("battery", include_str!("../../../design/icons/battery.svg")),
    (
        "battery-level",
        include_str!("../../../design/icons/battery-level.svg"),
    ),
    (
        "battery-bolt",
        include_str!("../../../design/icons/battery-bolt.svg"),
    ),
    ("moon", include_str!("../../../design/icons/moon.svg")),
    // Power: the lock screen's corner button and its menu (M5.10c).
    ("power", include_str!("../../../design/icons/power.svg")),
    ("restart", include_str!("../../../design/icons/restart.svg")),
    ("person", include_str!("../../../design/icons/person.svg")),
    (
        "arrow-right",
        include_str!("../../../design/icons/arrow-right.svg"),
    ),
    (
        "caps-lock",
        include_str!("../../../design/icons/caps-lock.svg"),
    ),
    (
        "chevron-right",
        include_str!("../../../design/icons/chevron-right.svg"),
    ),
    (
        "chevron-down",
        include_str!("../../../design/icons/chevron-down.svg"),
    ),
    (
        "chevron-up",
        include_str!("../../../design/icons/chevron-up.svg"),
    ),
    ("gear", include_str!("../../../design/icons/gear.svg")),
    // The notification centre's and the Do not disturb tile's (M5.9b).
    ("bell", include_str!("../../../design/icons/bell.svg")),
    // What an app may use (M6.14a), each permission's own; the bell is
    // the notifications'.
    ("camera", include_str!("../../../design/icons/camera.svg")),
    (
        "microphone",
        include_str!("../../../design/icons/microphone.svg"),
    ),
    (
        "screenshot",
        include_str!("../../../design/icons/screenshot.svg"),
    ),
    ("folder", include_str!("../../../design/icons/folder.svg")),
    (
        "do-not-disturb",
        include_str!("../../../design/icons/do-not-disturb.svg"),
    ),
    // Developer mode in effect, in the panel's status pill (M7.1).
    (
        "developer",
        include_str!("../../../design/icons/developer.svg"),
    ),
    (
        "chevron-left",
        include_str!("../../../design/icons/chevron-left.svg"),
    ),
    // Now playing's buttons (M5.9d): play, pause and the two track buttons.
    (
        "media-play",
        include_str!("../../../design/icons/media-play.svg"),
    ),
    (
        "media-pause",
        include_str!("../../../design/icons/media-pause.svg"),
    ),
    (
        "media-previous",
        include_str!("../../../design/icons/media-previous.svg"),
    ),
    (
        "media-next",
        include_str!("../../../design/icons/media-next.svg"),
    ),
    // The edit mode's parts (M5.10c6c3): the lock screen's clock, date,
    // message, picture and now playing, in its drawer and on its tiles.
    ("clock", include_str!("../../../design/icons/clock.svg")),
    (
        "calendar",
        include_str!("../../../design/icons/calendar.svg"),
    ),
    ("message", include_str!("../../../design/icons/message.svg")),
    ("picture", include_str!("../../../design/icons/picture.svg")),
    ("music", include_str!("../../../design/icons/music.svg")),
];

/// The icon `name` drawn `px` pixels square in `colour`; none for a name
/// this release lacks.
pub fn draw(name: &str, px: u32, colour: Colour) -> Option<Pixmap> {
    let mut pixmap = mask(name, px)?;
    tint(&mut pixmap, colour);
    Some(pixmap)
}

/// The icon `name` drawn `px` pixels square in black: its alpha is how
/// much of each pixel the shape covers.
pub fn mask(name: &str, px: u32) -> Option<Pixmap> {
    let (_, svg) = BUILT_IN.iter().find(|(n, _)| *n == name)?;
    svg_pixmap(svg.as_bytes(), px)
}

/// An SVG drawn `px` pixels square, kept to its shape and centred; app
/// icons (M5.4c) come through here too.
pub fn svg_pixmap(data: &[u8], px: u32) -> Option<Pixmap> {
    let tree = Tree::from_data(data, &Options::default()).ok()?;
    let size = tree.size();
    let (scale, dx, dy) = fit(size.width(), size.height(), px);
    let mut out = Pixmap::new(px, px)?;
    let at = Transform::from_translate(dx, dy).pre_scale(scale, scale);
    resvg::render(&tree, at, &mut out.as_mut());
    Some(out)
}

/// The scale that fits a `w` by `h` picture in `px` square, and where it
/// starts, centred.
pub fn fit(w: f32, h: f32, px: u32) -> (f32, f32, f32) {
    let px = px as f32;
    let scale = px / w.max(h).max(1.0);
    (scale, (px - w * scale) / 2.0, (px - h * scale) / 2.0)
}

/// Paints every pixel `colour`, keeping how much of it the shape covers.
fn tint(pixmap: &mut Pixmap, colour: Colour) {
    let [r, g, b, a] = colour.bytes();
    for pixel in pixmap.data_mut().chunks_exact_mut(4) {
        let cover = u32::from(pixel[3]) * u32::from(a) / 255;
        let mul = |c: u8| (u32::from(c) * cover / 255) as u8;
        pixel.copy_from_slice(&[mul(r), mul(g), mul(b), cover as u8]);
    }
}

/// The window glyphs of our title bars under the names GTK and libadwaita
/// ask their icon theme for (M6.16): each of our names and the freedesktop
/// name its copy has in the `Edel` icon theme, `features/shell/usr/share/icons/Edel`,
/// so apps' own header bars draw the glyphs our bars draw.
pub const SYMBOLIC: [(&str, &str); 4] = [
    ("close", "window-close-symbolic"),
    ("minimize", "window-minimize-symbolic"),
    ("maximize", "window-maximize-symbolic"),
    ("restore", "window-restore-symbolic"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The `Edel` icon theme's index (M6.16): it inherits Adwaita, which the
    /// apps' own glyphs come from, and hicolor, and holds one scalable
    /// context of the UI's icons from 8 to 512 px.
    fn index_theme() -> String {
        "# Edel OS's icon theme (M6.16): the shell's own icons under the names\n\
         # apps ask for. Written from design/icons/ by edel::icons (a cargo test\n\
         # keeps it so: EDEL_WRITE_DOCS=1 cargo test -p edel the_edel_icon_theme).\n\
         [Icon Theme]\n\
         Name=Edel\n\
         Comment=Edel OS's own icons\n\
         Inherits=Adwaita,hicolor\n\
         Directories=scalable/ui\n\n\
         [scalable/ui]\n\
         Size=16\n\
         MinSize=8\n\
         MaxSize=512\n\
         Type=Scalable\n\
         Context=UI\n"
            .to_string()
    }

    /// `svg` with every stroke turned into the filled shape it draws, as one
    /// path filled black: GTK fills a symbolic icon's shapes in the text's
    /// colour and draws no strokes (M6.16), so a stroked cross showed
    /// nothing and a stroked square a solid one. Coordinates are in the
    /// source's own units, kept to three decimals.
    fn outlined(svg: &[u8]) -> String {
        use resvg::tiny_skia::PathSegment;
        use resvg::usvg::{Group, Node};
        fn shapes(group: &Group, out: &mut String) {
            for node in group.children() {
                match node {
                    Node::Group(inner) => shapes(inner, out),
                    Node::Path(path) => {
                        let mut parts = Vec::new();
                        if path.fill().is_some() {
                            parts.push(path.data().clone());
                        }
                        if let Some(stroke) = path.stroke() {
                            parts.extend(path.data().stroke(&stroke.to_tiny_skia(), 16.0));
                        }
                        for part in parts {
                            let Some(part) = part.transform(path.abs_transform()) else {
                                continue;
                            };
                            for segment in part.segments() {
                                let points = match segment {
                                    PathSegment::MoveTo(p) => ("M", vec![p]),
                                    PathSegment::LineTo(p) => ("L", vec![p]),
                                    PathSegment::QuadTo(a, p) => ("Q", vec![a, p]),
                                    PathSegment::CubicTo(a, b, p) => ("C", vec![a, b, p]),
                                    PathSegment::Close => ("Z", vec![]),
                                };
                                out.push_str(points.0);
                                let numbers: Vec<String> = points
                                    .1
                                    .iter()
                                    .flat_map(|p| [p.x, p.y])
                                    .map(|n| {
                                        let text = format!("{n:.3}");
                                        let text = text.trim_end_matches('0').trim_end_matches('.');
                                        if text == "-0" {
                                            "0".to_string()
                                        } else {
                                            text.to_string()
                                        }
                                    })
                                    .collect();
                                out.push_str(&numbers.join(" "));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let tree = Tree::from_data(svg, &Options::default()).unwrap();
        let mut d = String::new();
        shapes(tree.root(), &mut d);
        let size = tree.size();
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\">\n\
             \x20 <!-- Written from design/icons/ by edel::icons, its strokes filled (M6.16). -->\n\
             \x20 <path fill=\"#000\" d=\"{d}\"/>\n\
             </svg>\n",
            size.width(),
            size.height()
        )
    }

    /// The `Edel` icon theme holds the window glyphs of [`SYMBOLIC`], each
    /// `design/icons/`'s glyph with its strokes filled ([`outlined`]), and
    /// its index (M6.16). With EDEL_WRITE_DOCS set, the test writes the
    /// glyphs and the index.
    #[test]
    fn the_edel_icon_theme_is_the_shells_icons() {
        let root = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../features/shell/usr/share/icons/Edel"
        );
        let ui = format!("{root}/scalable/ui");
        let write = std::env::var_os("EDEL_WRITE_DOCS").is_some();
        if write {
            std::fs::create_dir_all(&ui).unwrap();
            std::fs::write(format!("{root}/index.theme"), index_theme()).unwrap();
        }
        let shipped_index = std::fs::read_to_string(format!("{root}/index.theme"));
        assert!(
            shipped_index.unwrap_or_default() == index_theme(),
            "the Edel icon theme's index differs; run EDEL_WRITE_DOCS=1 cargo test -p edel the_edel_icon_theme"
        );
        let mut want = Vec::new();
        for (ours, freedesktop) in SYMBOLIC {
            let source = format!(
                "{}/../../design/icons/{ours}.svg",
                env!("CARGO_MANIFEST_DIR")
            );
            let source = outlined(&std::fs::read(&source).unwrap());
            let copy = format!("{ui}/{freedesktop}.svg");
            if write {
                std::fs::write(&copy, &source).unwrap();
            }
            assert!(
                std::fs::read_to_string(&copy).unwrap_or_default() == source,
                "{copy} differs from design/icons/{ours}.svg; run EDEL_WRITE_DOCS=1 cargo test -p edel the_edel_icon_theme"
            );
            want.push(format!("{freedesktop}.svg"));
        }
        let mut shipped: Vec<String> = std::fs::read_dir(&ui)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        shipped.sort();
        want.sort();
        assert_eq!(
            shipped, want,
            "the theme holds exactly the icons SYMBOLIC names"
        );
    }

    /// Each glyph of the theme draws, filled, what our title bars draw
    /// from its strokes: at 64 px, at most 2 percent of the pixels differ
    /// by more than a quarter of their cover, and the theme's file has no
    /// stroke for GTK to leave out.
    #[test]
    fn the_theme_glyphs_draw_what_the_strokes_draw() {
        for (ours, _) in SYMBOLIC {
            let path = format!(
                "{}/../../design/icons/{ours}.svg",
                env!("CARGO_MANIFEST_DIR")
            );
            let source = std::fs::read(&path).unwrap();
            let filled = outlined(&source);
            assert!(!filled.contains("stroke="), "{ours}: {filled}");
            let a = svg_pixmap(&source, 64).unwrap();
            let b = svg_pixmap(filled.as_bytes(), 64).unwrap();
            let differ = a
                .pixels()
                .iter()
                .zip(b.pixels())
                .filter(|(x, y)| x.alpha().abs_diff(y.alpha()) > 64)
                .count();
            assert!(covered(&a) > 0, "{ours} draws nothing");
            assert!(
                differ * 50 <= a.pixels().len(),
                "{ours}: {differ} of {} pixels differ",
                a.pixels().len()
            );
        }
    }

    fn covered(pixmap: &Pixmap) -> usize {
        pixmap.data().chunks_exact(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn every_icon_draws_from_its_file_in_the_colour_asked() {
        let red = Colour::parse("#ff0000").unwrap();
        for (name, _) in BUILT_IN {
            let icon = draw(name, 28, red).unwrap_or_else(|| panic!("{name} did not draw"));
            let n = covered(&icon);
            assert!(n > 10 && n < 28 * 28, "{name} covers {n} pixels");
            // Premultiplied red: no green or blue anywhere.
            assert!(
                icon.data()
                    .chunks_exact(4)
                    .all(|p| p[1] == 0 && p[2] == 0 && p[0] == p[3]),
                "{name} is not all red"
            );
        }
        assert!(draw("nope", 28, red).is_none());
    }

    #[test]
    fn every_file_in_design_icons_is_built_in() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/icons");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_stem().unwrap().to_str().unwrap().to_string();
            assert!(
                BUILT_IN.iter().any(|(n, _)| *n == name),
                "design/icons/{name}.svg is not in icons::BUILT_IN"
            );
        }
    }

    #[test]
    fn a_changed_file_draws_differently() {
        let (_, svg) = BUILT_IN[0];
        let changed = svg.replace("rx=\"1.8\"", "rx=\"0\"");
        let before = svg_pixmap(svg.as_bytes(), 30).unwrap();
        let after = svg_pixmap(changed.as_bytes(), 30).unwrap();
        assert_ne!(before.data(), after.data());
    }
}
