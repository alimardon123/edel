//! The shell's own icons (M5.5d): the menu button, the layout toggle, the
//! status area's and the title bar buttons, one SVG file each in
//! `design/icons/`, built into the compositor and shell-ui, so changing one
//! file changes every place that shows it. Each is drawn as a shape in black and coloured by the
//! caller with a token, as GNOME's symbolic icons are.

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

use crate::tokens::Colour;

/// Each icon's name and its file.
pub const BUILT_IN: [(&str, &str); 50] = [
    ("menu", include_str!("../../../design/icons/menu.svg")),
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
    (
        "page-displays",
        include_str!("../../../design/icons/page-displays.svg"),
    ),
    (
        "page-about",
        include_str!("../../../design/icons/page-about.svg"),
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
    (
        "chevron-right",
        include_str!("../../../design/icons/chevron-right.svg"),
    ),
    (
        "chevron-down",
        include_str!("../../../design/icons/chevron-down.svg"),
    ),
    ("gear", include_str!("../../../design/icons/gear.svg")),
    // The notification centre's and the Do not disturb tile's (M5.9b).
    ("bell", include_str!("../../../design/icons/bell.svg")),
    (
        "do-not-disturb",
        include_str!("../../../design/icons/do-not-disturb.svg"),
    ),
    (
        "chevron-left",
        include_str!("../../../design/icons/chevron-left.svg"),
    ),
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

#[cfg(test)]
mod tests {
    use super::*;

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
