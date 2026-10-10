//! The tray's grid (M5.9g, the fifth round's `tray.jpg`): the apps behind
//! the tray's arrow, on a frosted card, four across, with a footer that
//! says they can be dragged to the panel and a gear that opens the tray's
//! settings. On a Compact screen it is three across at touch size, with no
//! footer. Plain data and drawing, tested without a display; `tray_card.rs`
//! owns the surface.

use accesskit::Role;
use edel::i18n::tr;
use edel::tokens::Tokens;
use tiny_skia::Pixmap;

use crate::a11y::Item as Node;
use crate::paint::{Face, Text, fill};
use crate::popup::{self, Card, Rect, dim, icon_in, veil};
use crate::tray;
use crate::widgets::tray::{Glyph, dot, draw_icon};

// The sizes are the mockup's: there are no tokens for an icon's cell.

/// A desktop cell, icon, columns and the card's room: a cell's side, an
/// icon's side, how many across and the room at each side, logical pixels.
pub const CELL: f32 = 36.0;
pub const ICON: f32 = 20.0;
pub const COLUMNS: usize = 4;
pub const PAD: f32 = 10.0;
/// The footer's height: a hairline, then a row this high holding the hint
/// and the gear. The gear is a square as high as the row, since a cell
/// high gear would not fit it.
pub const FOOTER: f32 = 30.0;
/// The narrowest desktop card, so the whole hint fits beside the gear
/// (the spec's 180 px cut it short at the font's size).
const WIDTH_MIN: f32 = 210.0;
/// A Compact screen's cell, icon and columns, for touch: no footer.
pub const CELL_COMPACT: f32 = 56.0;
pub const ICON_COMPACT: f32 = 32.0;
pub const COLUMNS_COMPACT: usize = 3;

/// One app behind the arrow, as the grid shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Icon {
    /// The item's `Id` (`service/path`), which a click asks to act.
    pub id: String,
    /// The app's name for `panels.tray`, which `places` logs.
    pub app: String,
    /// What a reader calls it: the item's title, else "Tray item".
    pub title: String,
    /// The theme icon's name, or empty.
    pub name: String,
    /// The pixmap as `tray::encode` writes it, or empty.
    pub pixels: String,
    /// Whether it has news: a dot on its corner.
    pub attention: bool,
}

impl From<&tray::Item> for Icon {
    fn from(item: &tray::Item) -> Icon {
        Icon {
            id: item.id.clone(),
            app: item.app.clone(),
            title: item.label.clone(),
            name: item.name.clone(),
            pixels: item.pixels.clone(),
            attention: item.attention,
        }
    }
}

/// What the grid shows and where the pointer and the keyboard are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub icons: Vec<Icon>,
    /// A Compact screen: touch sizes, three across, no footer.
    pub compact: bool,
    pub hover: Option<Part>,
    pub focus: Option<Part>,
    /// The icon being dragged, drawn at half strength.
    pub dragging: Option<usize>,
}

/// The parts a press or the keyboard reaches: an icon by its place in
/// [`View::icons`], or the gear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Icon(usize),
    Gear,
}

/// Where the grid's parts lie, logical pixels from the card's top left
/// corner. `gear` and `hint` are none on a Compact screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub size: (u32, u32),
    pub card: Rect,
    pub cells: Vec<Rect>,
    pub gear: Option<Rect>,
    pub hint: Option<Rect>,
}

/// A key the grid takes from the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Tab,
    Activate,
    Menu,
    Escape,
}

/// What a key does: move the focus, act on a part, close the grid, or
/// nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Focus(Part),
    Act(Act),
    Close,
    Nothing,
}

/// What a key asks for on a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Ask the icon at this place to do what a click does.
    Activate(usize),
    /// Open the icon's menu.
    Menu(usize),
    /// Open the tray's settings.
    Settings,
}

/// The cell side, icon side and columns for a screen.
fn sizes(compact: bool) -> (f32, f32, usize) {
    if compact {
        (CELL_COMPACT, ICON_COMPACT, COLUMNS_COMPACT)
    } else {
        (CELL, ICON, COLUMNS)
    }
}

/// How many icons lie across in a row of this view: at most the columns,
/// at least one.
fn across(view: &View) -> usize {
    let (_, _, columns) = sizes(view.compact);
    view.icons.len().clamp(1, columns)
}

/// The grid's size and where each part lies: the one function that places
/// everything. Icons fill rows from the top left, after `PAD`; the card is
/// as wide as its columns with `PAD` at each side (at least `WIDTH_MIN` on
/// a desktop), and as high as its rows, with the footer below them.
pub fn layout(view: &View) -> Layout {
    let (cell, _, _) = sizes(view.compact);
    let columns = across(view);
    let rows = view.icons.len().div_ceil(columns);
    let mut width = PAD + columns as f32 * cell + PAD;
    if !view.compact {
        width = width.max(WIDTH_MIN);
    }
    let footer = if view.compact { 0.0 } else { FOOTER };
    let grid_bottom = PAD + rows as f32 * cell + PAD;
    let height = grid_bottom + footer;
    // The grid in the middle of a card the hint made wider.
    let left = ((width - columns as f32 * cell) / 2.0).round();
    let cells = (0..view.icons.len())
        .map(|i| {
            let (row, column) = (i / columns, i % columns);
            Rect::new(
                left + column as f32 * cell,
                PAD + row as f32 * cell,
                cell,
                cell,
            )
        })
        .collect();
    let (gear, hint) = if view.compact {
        (None, None)
    } else {
        let gear = Rect::new(width - PAD - FOOTER, grid_bottom, FOOTER, FOOTER);
        let hint = Rect::new(PAD, grid_bottom, gear.x - PAD, FOOTER);
        (Some(gear), Some(hint))
    };
    Layout {
        size: (width.ceil() as u32, height.ceil() as u32),
        card: Rect::new(0.0, 0.0, width, height),
        cells,
        gear,
        hint,
    }
}

/// The one card the grid holds, with the menus' corners.
pub fn cards(l: &Layout, tokens: &Tokens) -> Vec<Card> {
    vec![Card {
        rect: l.card,
        radius: tokens.radius_menu as f32,
    }]
}

/// The part under `x`, `y` (logical pixels from the card's corner), if
/// any: an icon's cell or the gear. The padding and the footer's hint do
/// nothing.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Part> {
    if l.gear.is_some_and(|g| g.contains(x, y)) {
        return Some(Part::Gear);
    }
    l.cells
        .iter()
        .position(|c| c.contains(x, y))
        .map(Part::Icon)
}

/// Whether `x`, `y` lies on the card.
pub fn inside(l: &Layout, x: f32, y: f32) -> bool {
    l.card.contains(x, y)
}

/// The parts in the keyboard's order: the icons, then the gear when there
/// is one.
pub fn ring(view: &View) -> Vec<Part> {
    let mut out: Vec<Part> = (0..view.icons.len()).map(Part::Icon).collect();
    if !view.compact {
        out.push(Part::Gear);
    }
    out
}

/// What a key does with `focus` as the focused part: arrows move by one
/// cell (Up and Down by a row's columns) and stop at the ends, Tab cycles
/// the ring, Activate and Menu act on an icon (the gear acts only when
/// activated), and Escape closes.
pub fn key(view: &View, focus: Option<Part>, key: Key) -> Step {
    let n = view.icons.len();
    let ring = ring(view);
    if key == Key::Escape {
        return Step::Close;
    }
    if key == Key::Tab {
        let next = match focus.and_then(|f| ring.iter().position(|p| *p == f)) {
            Some(i) => (i + 1) % ring.len().max(1),
            None => 0,
        };
        return ring.get(next).map_or(Step::Nothing, |p| Step::Focus(*p));
    }
    match (key, focus) {
        (Key::Activate, Some(Part::Icon(i))) => Step::Act(Act::Activate(i)),
        (Key::Activate, Some(Part::Gear)) => Step::Act(Act::Settings),
        (Key::Menu, Some(Part::Icon(i))) => Step::Act(Act::Menu(i)),
        (Key::Activate | Key::Menu, _) => Step::Nothing,
        (_, None) => ring.first().map_or(Step::Nothing, |p| Step::Focus(*p)),
        (Key::Left, Some(Part::Icon(i))) => Step::Focus(Part::Icon(i.saturating_sub(1))),
        (Key::Right, Some(Part::Icon(i))) => {
            Step::Focus(Part::Icon((i + 1).min(n.saturating_sub(1))))
        }
        (Key::Up, Some(Part::Icon(i))) => Step::Focus(Part::Icon(i.saturating_sub(across(view)))),
        (Key::Down, Some(Part::Icon(i))) => {
            Step::Focus(Part::Icon((i + across(view)).min(n.saturating_sub(1))))
        }
        // From the gear, the arrows go back to the last icon.
        (Key::Left | Key::Up, Some(Part::Gear)) if n > 0 => Step::Focus(Part::Icon(n - 1)),
        (_, Some(Part::Gear)) => Step::Nothing,
        (Key::Tab | Key::Escape, _) => Step::Nothing,
    }
}

/// Where the parts lie, as one log line CI reads:
/// `card WxH, icon APP X+Y+WxH, ..., gear X+Y+WxH`, logical pixels.
pub fn places(l: &Layout, view: &View) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut out = format!("card {}x{}", l.size.0, l.size.1);
    for (icon, cell) in view.icons.iter().zip(&l.cells) {
        out.push_str(&format!(", icon {} {}", icon.app, at(*cell)));
    }
    if let Some(gear) = l.gear {
        out.push_str(&format!(", gear {}", at(gear)));
    }
    out
}

/// Draws `view` at `s` into `pixmap`, which is the card's size times it:
/// the card, each icon in its cell (a veil behind the hovered or focused
/// one, the dragged one at half strength, the dot on news), then with a
/// footer the hairline, the hint and the gear.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    l: &Layout,
    tokens: &Tokens,
    icons: Option<&mut edel::app_icons::Icons>,
    text: Option<&mut Text>,
    s: f32,
) {
    let mut icons = icons;
    let mut text = text;
    popup::cards(pixmap, tokens, s, &cards(l, tokens));
    let (_, side, _) = sizes(view.compact);
    let radius = tokens.radius_control as f32 * s;
    for (i, icon) in view.icons.iter().enumerate() {
        let cell = l.cells[i];
        let (x, y, w, h) = cell.device(s);
        let part = Part::Icon(i);
        if view.hover == Some(part) || view.focus == Some(part) {
            fill(pixmap, x, y, w, h, radius, veil(tokens, 0.06));
        }
        let px = (side * s).round();
        let (ix, iy) = (x + ((w - px) / 2.0).round(), y + ((h - px) / 2.0).round());
        let title = if icon.title.is_empty() {
            tr("Tray item").to_string()
        } else {
            icon.title.clone()
        };
        let glyph = Glyph {
            name: &icon.name,
            pixels: &icon.pixels,
            title: &title,
        };
        if view.dragging == Some(i) {
            // Drawn on its own pixmap, then laid over the card at half
            // strength, so the icon fades while it is dragged.
            if let Some(mut layer) = Pixmap::new(w as u32, h as u32) {
                draw_icon(
                    &mut layer,
                    tokens,
                    icons.as_deref_mut(),
                    text.as_deref_mut(),
                    glyph,
                    (ix - x, iy - y, px),
                );
                let paint = tiny_skia::PixmapPaint {
                    opacity: 0.5,
                    ..tiny_skia::PixmapPaint::default()
                };
                pixmap.draw_pixmap(
                    x as i32,
                    y as i32,
                    layer.as_ref(),
                    &paint,
                    tiny_skia::Transform::identity(),
                    None,
                );
            }
        } else {
            draw_icon(
                pixmap,
                tokens,
                icons.as_deref_mut(),
                text.as_deref_mut(),
                glyph,
                (ix, iy, px),
            );
        }
        if icon.attention {
            dot(pixmap, tokens, ix + px, iy, s);
        }
    }
    let (Some(hint), Some(gear)) = (l.hint, l.gear) else {
        return;
    };
    // The hairline across the footer, less PAD at each side.
    let line = s.max(1.0);
    fill(
        pixmap,
        PAD * s,
        hint.y * s,
        (l.card.w - 2.0 * PAD) * s,
        line,
        0.0,
        tokens.line,
    );
    if let Some(text) = text {
        let size = tokens.panel_text_small_size as f32 * s;
        let (_, _, w, _) = hint.device(s);
        let mut words = text.fit_in(tr("Drag to the panel to keep"), size, w, Face::REGULAR);
        let y = popup::middle(hint.y, hint.h, size, s);
        text.draw(pixmap, &mut words, hint.x * s, y, dim(tokens));
    }
    if view.hover == Some(Part::Gear) || view.focus == Some(Part::Gear) {
        let (x, y, w, h) = gear.device(s);
        fill(pixmap, x, y, w, h, radius, veil(tokens, 0.06));
    }
    icon_in(
        pixmap,
        "gear",
        tokens.panel_glyph as f32 * 0.7,
        gear,
        s,
        dim(tokens),
    );
}

/// What a screen reader finds: each icon as a button named by its title
/// (or "Tray item"), then the gear as "Tray settings".
pub fn nodes(view: &View, l: &Layout) -> Vec<Node> {
    let at = |r: Rect| {
        accesskit::Rect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.y + r.h),
        )
    };
    let item = |label: String, bounds| Node {
        role: Role::Button,
        label,
        bounds,
        children: Vec::new(),
        toggled: None,
        value: None,
    };
    let mut out: Vec<Node> = view
        .icons
        .iter()
        .zip(&l.cells)
        .map(|(icon, cell)| {
            let label = if icon.title.is_empty() {
                tr("Tray item").to_string()
            } else {
                icon.title.clone()
            };
            item(label, at(*cell))
        })
        .collect();
    if let Some(gear) = l.gear {
        out.push(item(tr("Tray settings").to_string(), at(gear)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::tokens::Scheme;
    use tiny_skia::{Color, PixmapPaint, Transform};

    fn icon(app: &str, title: &str) -> Icon {
        Icon {
            id: format!("{app}/StatusNotifierItem"),
            app: app.into(),
            title: title.into(),
            ..Icon::default()
        }
    }

    /// `n` icons with no theme icon and no pixmap, so each shows its
    /// initial, as the mockup's six do.
    fn view(n: usize, compact: bool) -> View {
        let names = [
            "Dropbox",
            "Google Drive",
            "VPN",
            "Network",
            "Bluetooth",
            "Printer",
        ];
        View {
            icons: (0..n)
                .map(|i| icon(&format!("app{i}"), names[i % names.len()]))
                .collect(),
            compact,
            ..View::default()
        }
    }

    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    #[test]
    fn a_desktop_grid_is_four_across_and_at_least_as_wide_as_the_hint() {
        let one = layout(&view(1, false));
        assert_eq!(one.size, (210, 86));
        assert_eq!(one.cells[0], Rect::new(87.0, PAD, CELL, CELL), "centred");
        let four = layout(&view(4, false));
        assert_eq!(four.size, (210, 86), "four columns are 164, widened to 210");
        assert_eq!(four.cells[0].x, 33.0, "the grid centred in the card");
        assert_eq!(four.cells[3].x, 33.0 + 3.0 * CELL);
        let five = layout(&view(5, false));
        assert_eq!(five.size, (210, 122), "a second row");
        assert_eq!(five.cells[4], Rect::new(33.0, PAD + CELL, CELL, CELL));
        let six = layout(&view(6, false));
        assert_eq!(six.cells.len(), 6);
        assert_eq!(six.size, (210, 122));
    }

    #[test]
    fn a_compact_grid_is_three_by_two_at_touch_size_with_no_gear() {
        let l = layout(&view(6, true));
        assert_eq!(l.cells.len(), 6);
        assert_eq!(l.cells[0], Rect::new(PAD, PAD, CELL_COMPACT, CELL_COMPACT));
        assert_eq!(
            l.cells[3],
            Rect::new(PAD, PAD + CELL_COMPACT, CELL_COMPACT, CELL_COMPACT)
        );
        assert_eq!(l.size, (188, 132));
        assert!(l.gear.is_none() && l.hint.is_none());
        assert_eq!(ring(&view(6, true)).len(), 6);
    }

    #[test]
    fn hit_finds_an_icon_the_gear_and_nothing_in_the_padding() {
        let l = layout(&view(4, false));
        assert_eq!(hit(&l, l.cells[0].x + 5.0, PAD + 5.0), Some(Part::Icon(0)));
        let gear = l.gear.unwrap();
        assert_eq!(hit(&l, gear.x + 2.0, gear.y + 2.0), Some(Part::Gear));
        assert_eq!(hit(&l, 2.0, 2.0), None, "the padding");
        assert!(inside(&l, 2.0, 2.0));
        assert!(!inside(&l, -1.0, 2.0));
    }

    #[test]
    fn keys_move_clamp_cycle_and_act() {
        let v = view(5, false);
        // From nothing, an arrow focuses the first part.
        assert_eq!(key(&v, None, Key::Right), Step::Focus(Part::Icon(0)));
        assert_eq!(
            key(&v, Some(Part::Icon(0)), Key::Right),
            Step::Focus(Part::Icon(1))
        );
        assert_eq!(
            key(&v, Some(Part::Icon(0)), Key::Left),
            Step::Focus(Part::Icon(0)),
            "clamped"
        );
        assert_eq!(
            key(&v, Some(Part::Icon(0)), Key::Down),
            Step::Focus(Part::Icon(4))
        );
        assert_eq!(
            key(&v, Some(Part::Icon(4)), Key::Down),
            Step::Focus(Part::Icon(4)),
            "clamped"
        );
        assert_eq!(
            key(&v, Some(Part::Icon(4)), Key::Up),
            Step::Focus(Part::Icon(0))
        );
        assert_eq!(
            key(&v, Some(Part::Icon(4)), Key::Right),
            Step::Focus(Part::Icon(4)),
            "the last"
        );
        // Tab cycles the icons and the gear, and wraps.
        assert_eq!(
            key(&v, Some(Part::Icon(4)), Key::Tab),
            Step::Focus(Part::Gear)
        );
        assert_eq!(
            key(&v, Some(Part::Gear), Key::Tab),
            Step::Focus(Part::Icon(0))
        );
        assert_eq!(key(&v, None, Key::Tab), Step::Focus(Part::Icon(0)));
        // Activate and Menu act; Escape closes.
        assert_eq!(
            key(&v, Some(Part::Icon(2)), Key::Activate),
            Step::Act(Act::Activate(2))
        );
        assert_eq!(
            key(&v, Some(Part::Gear), Key::Activate),
            Step::Act(Act::Settings)
        );
        assert_eq!(
            key(&v, Some(Part::Icon(1)), Key::Menu),
            Step::Act(Act::Menu(1))
        );
        assert_eq!(key(&v, None, Key::Activate), Step::Nothing);
        assert_eq!(key(&v, Some(Part::Icon(1)), Key::Escape), Step::Close);
        // A compact grid has no gear to tab to, so Tab wraps over the icons.
        let c = view(2, true);
        assert_eq!(
            key(&c, Some(Part::Icon(1)), Key::Tab),
            Step::Focus(Part::Icon(0))
        );
    }

    #[test]
    fn places_name_each_icon_by_its_app() {
        let mut v = view(2, false);
        v.icons[0].app = "nm-applet".into();
        v.icons[1].app = "edel-testclient".into();
        let l = layout(&v);
        let text = places(&l, &v);
        assert!(
            text.starts_with(
                "card 210x86, icon nm-applet 69+10+36x36, icon edel-testclient 105+10+36x36"
            ),
            "{text}"
        );
        assert!(text.ends_with(", gear 170+56+30x30"), "{text}");
    }

    #[test]
    fn a_reader_hears_each_icon_by_its_title_and_then_the_gear() {
        let mut v = view(2, false);
        v.icons[1].title = String::new();
        let l = layout(&v);
        let heard = nodes(&v, &l);
        assert_eq!(heard.len(), 3);
        assert_eq!(heard[0].label, "Dropbox");
        assert_eq!(heard[1].label, "Tray item");
        assert_eq!(heard[2].label, "Tray settings");
        assert!(heard.iter().all(|n| n.role == Role::Button));
    }

    #[test]
    fn cards_hold_one_card_with_the_menus_corners() {
        let tokens = Tokens::built_in();
        let l = layout(&view(1, false));
        let c = cards(&l, &tokens);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].rect, l.card);
        assert_eq!(c[0].radius, tokens.radius_menu as f32);
    }

    #[test]
    fn it_draws_the_card_and_each_icon_with_no_fonts() {
        let tokens = Tokens::built_in();
        let v = view(6, false);
        let l = layout(&v);
        let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
        paint(&mut pixmap, &v, &l, &tokens, None, None, 2.0);
        let alpha = |x: f32, y: f32| {
            pixmap
                .pixel((x * 2.0) as u32, (y * 2.0) as u32)
                .unwrap()
                .alpha()
        };
        assert_eq!(alpha(0.0, 0.0), 0, "a round corner");
        let middle = l.cells[0].x + CELL / 2.0;
        assert_ne!(alpha(middle, PAD + CELL / 2.0), 0, "an icon's cell");
    }

    #[test]
    fn it_draws_light_and_dark_and_writes_pngs() {
        let Some(mut text) = fonts() else {
            return; // no fonts on this machine
        };
        for (scheme, mode) in [(Scheme::Light, "light"), (Scheme::Dark, "dark")] {
            let tokens = Tokens::built_in_scheme(scheme);
            for (attention, name) in [(false, "tray"), (true, "tray-attention")] {
                let mut v = view(6, false);
                if attention {
                    v.icons[2].attention = true;
                }
                let l = layout(&v);
                let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
                paint(&mut pixmap, &v, &l, &tokens, None, Some(&mut text), 2.0);
                let alpha = |x: f32, y: f32| {
                    pixmap
                        .pixel((x * 2.0) as u32, (y * 2.0) as u32)
                        .unwrap()
                        .alpha()
                };
                assert_eq!(alpha(0.0, 0.0), 0, "{name} {mode}: a round corner");
                for i in 0..6 {
                    let c = l.cells[i];
                    assert_ne!(
                        alpha(c.x + CELL / 2.0, c.y + CELL / 2.0),
                        0,
                        "{name} {mode}: icon {i}"
                    );
                }
                if let Some(dir) = std::env::var_os("EDEL_TRAY_PNG") {
                    let dir = std::path::PathBuf::from(dir);
                    std::fs::create_dir_all(&dir).unwrap();
                    // Over the screen's colour, so the card's edge shows.
                    let room = 24u32 * 2;
                    let mut screen =
                        Pixmap::new(pixmap.width() + 2 * room, pixmap.height() + 2 * room).unwrap();
                    screen.fill(
                        Color::from_rgba(
                            tokens.background.r,
                            tokens.background.g,
                            tokens.background.b,
                            1.0,
                        )
                        .unwrap(),
                    );
                    screen.draw_pixmap(
                        room as i32,
                        room as i32,
                        pixmap.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                    screen
                        .save_png(dir.join(format!("{name}-{mode}.png")))
                        .unwrap();
                }
            }
        }
    }
}
