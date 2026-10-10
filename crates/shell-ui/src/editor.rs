//! Edit panels (M5.31b): the drawer a panel's menu opens. It is a card of
//! every widget the release has, each by its title and a small picture of
//! it as it is on a panel, the ones already on a panel dimmed, then a
//! hairline and Undo and Done. While it is open the panels show their
//! widgets as tiles (`paint::Look::editing`). Plain data and drawing,
//! tested without a display; `editor_card.rs` owns the surface and the
//! panels. The sizes are the drawer's own: the mockups have none for it.

use accesskit::Role;
use edel::i18n::{tr, trf};
use edel::panel_edit::Group;
use edel::tokens::Tokens;
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use crate::a11y::Item as Node;
use crate::paint::{EDIT_PAD, Face, LIFTED, Text, fill, outline};
use crate::popup::{self, Card, Rect, dim, veil, wrap};
use crate::trayview::Key;
use crate::widgets::{Canvas, Widget};

/// How far the pointer moves, pressed, before a press becomes a drag, in
/// logical pixels (M5.31b).
pub const DRAG_START: f32 = 6.0;

/// The margin round the drawer's content, logical pixels.
pub const PAD: f32 = 16.0;
/// The drawer's widest on a desktop.
pub const WIDTH: f32 = 560.0;
/// The room the drawer keeps at each side of a screen narrower than it.
const SIDE: f32 = 32.0;
/// A tile's size on a desktop; a compact drawer shrinks its width to fit.
pub const TILE: (f32, f32) = (112.0, 64.0);
/// The room between tiles, rows and columns alike.
const GAP: f32 = 8.0;
/// The heading's line, and the hint under it: two lines at most.
const HEADING: f32 = 22.0;
const HINT_GAP: f32 = 4.0;
const HINT_LINE: f32 = 15.0;
const HINT_LINES: usize = 2;
/// A card at least this wide holds the hint on one line.
const ONE_LINE_HINT: f32 = 480.0;

/// How many lines the hint takes in a card of `view`'s width: one on a
/// desktop's card, two on a narrow one.
fn hint_lines(view: &View) -> usize {
    if card_width(view) >= ONE_LINE_HINT {
        1
    } else {
        HINT_LINES
    }
}
/// The room between the hint and the tiles, and between the tiles and the
/// hairline.
const SPACE: f32 = 12.0;
/// The footer below the hairline, and its buttons' size on a desktop and
/// on a touch screen (Compact), logical pixels.
const FOOTER: f32 = 44.0;
const BUTTON_WIDTH: f32 = 88.0;
const BUTTON: f32 = 32.0;
const BUTTON_TOUCH: f32 = 44.0;
/// Three tiles across on a Compact screen.
const COLUMNS_COMPACT: usize = 3;
/// Where a tile's picture starts from the tile's top, where its title line
/// starts and how high it is, logical pixels.
const PICTURE_TOP: f32 = 10.0;
const TITLE_TOP: f32 = 44.0;
const TITLE_LINE: f32 = 16.0;

/// One widget in the drawer.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    /// Its name in the widget table, which its picture is drawn from.
    pub name: &'static str,
    /// Its title as people read it.
    pub title: String,
    /// What it shows now, as its picture shows it.
    pub shown: String,
    /// Whether a panel holds it already, so it is drawn dimmed.
    pub placed: bool,
}

/// What the drawer shows and where the pointer and the keyboard are.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    pub tiles: Vec<Tile>,
    /// A Compact screen: touch sizes, three tiles across.
    pub compact: bool,
    /// The screen's width in logical pixels, 0 when unknown.
    pub width: u32,
    pub hover: Option<Part>,
    pub focus: Option<Part>,
    /// A drawer tile being dragged, by its index: drawn at 35 percent.
    pub lifted: Option<usize>,
    /// A widget of a panel is dragged over the drawer: it gets an outline
    /// and the hint says it will be taken off the panel.
    pub taking: bool,
}

/// Where a widget dragged to a panel would land there (M5.31b): the group,
/// the widget it lands before (`None`: the group's end), and the caret's
/// place in the panel's logical x.
#[derive(Debug, Clone, PartialEq)]
pub struct Landing {
    pub group: Group,
    pub before: Option<&'static str>,
    pub caret: f32,
}

/// Where a dragged widget would land on a panel (M5.31b). `row` is each
/// widget the panel shows, in order, with its group and its place (left and
/// width, logical pixels from the panel's left); `empty` is each group's
/// place while the panel is edited; `x` is the pointer along the panel;
/// `lifted` is the widget being dragged off it, which is not a place to
/// land by. Inside an empty group's place the widget lands in that group's
/// end. Otherwise the nearest widget decides: before its middle it lands
/// before it, after its middle before the next widget of its group (the
/// group's end when none). `None` when nothing is there to land by.
pub fn landing(
    row: &[(&'static str, Group, f32, f32)],
    empty: [Option<(f32, f32)>; 3],
    x: f32,
    lifted: Option<&str>,
) -> Option<Landing> {
    for (k, place) in empty.iter().enumerate() {
        if let Some((left, width)) = *place {
            if x >= left && x <= left + width {
                return Some(Landing {
                    group: Group::ALL[k],
                    before: None,
                    caret: left + width / 2.0,
                });
            }
        }
    }
    let distance = |left: f32, width: f32| {
        if x < left {
            left - x
        } else if x > left + width {
            x - left - width
        } else {
            0.0
        }
    };
    let (index, _) = row
        .iter()
        .enumerate()
        .filter(|(_, (name, ..))| Some(*name) != lifted)
        .map(|(i, &(_, _, left, width))| (i, distance(left, width)))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    let (name, group, left, width) = row[index];
    if x < left + width / 2.0 {
        return Some(Landing {
            group,
            before: Some(name),
            caret: left,
        });
    }
    let after = row[index + 1..]
        .iter()
        .find(|(other, g, ..)| *g == group && Some(*other) != lifted);
    Some(Landing {
        group,
        before: after.map(|(other, ..)| *other),
        caret: left + width,
    })
}

/// The parts a press or the keyboard reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Tile(usize),
    Undo,
    Done,
}

/// Where everything lies, logical pixels from the card's top left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub size: (u32, u32),
    pub card: Rect,
    pub tiles: Vec<Rect>,
    pub undo: Rect,
    pub done: Rect,
}

/// What a key asks for: Undo or Done as if pressed, or closing the drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Undo,
    Done,
    Close,
}

/// What a key does: move the focus, act, or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Focus(Part),
    Act(Act),
    Nothing,
}

/// The widget called `name` in the table, if the release has it.
fn widget_of(name: &str) -> Option<&'static Widget> {
    crate::widgets::TABLE.iter().find(|w| w.name == name)
}

/// The drawer's width: the screen's on a Compact screen, else `WIDTH`, or
/// the screen less `SIDE` when that is narrower.
fn card_width(view: &View) -> f32 {
    let screen = view.width as f32;
    match (view.compact, screen > 0.0) {
        (true, true) => screen,
        (_, true) => WIDTH.min(screen - SIDE),
        _ => WIDTH,
    }
}

/// How many tiles lie across.
fn columns(view: &View) -> usize {
    if view.compact {
        return COLUMNS_COMPACT;
    }
    let inner = card_width(view) - 2.0 * PAD;
    ((inner + GAP) / (TILE.0 + GAP)).floor().max(1.0) as usize
}

/// The drawer's size and where each part lies: the one function that
/// places everything. Tiles fill rows from the top, centred across the
/// card; the footer holds Undo and Done at the right.
pub fn layout(view: &View) -> Layout {
    let width = card_width(view);
    let columns = columns(view);
    let tile_w = if view.compact {
        (width - 2.0 * PAD - (columns - 1) as f32 * GAP) / columns as f32
    } else {
        TILE.0
    };
    let rows = view.tiles.len().max(1).div_ceil(columns);
    let used = columns as f32 * tile_w + (columns - 1) as f32 * GAP;
    let left = ((width - used) / 2.0).round();
    let top = PAD + HEADING + HINT_GAP + hint_lines(view) as f32 * HINT_LINE + SPACE;
    let tiles = (0..view.tiles.len())
        .map(|i| {
            let (row, column) = (i / columns, i % columns);
            Rect::new(
                left + column as f32 * (tile_w + GAP),
                top + row as f32 * (TILE.1 + GAP),
                tile_w,
                TILE.1,
            )
        })
        .collect();
    let tiles_bottom = top + rows as f32 * TILE.1 + (rows - 1) as f32 * GAP;
    // The hairline is one pixel high, then the footer.
    let footer_top = tiles_bottom + SPACE + 1.0;
    let height = footer_top + FOOTER;
    let button = if view.compact { BUTTON_TOUCH } else { BUTTON };
    let y = footer_top + (FOOTER - button) / 2.0;
    let done = Rect::new(width - PAD - BUTTON_WIDTH, y, BUTTON_WIDTH, button);
    let undo = Rect::new(done.x - GAP - BUTTON_WIDTH, y, BUTTON_WIDTH, button);
    Layout {
        size: (width.ceil() as u32, height.ceil() as u32),
        card: Rect::new(0.0, 0.0, width, height),
        tiles,
        undo,
        done,
    }
}

/// The card, with the menus' corners.
pub fn cards(l: &Layout, tokens: &Tokens) -> Vec<Card> {
    vec![Card {
        rect: l.card,
        radius: tokens.radius_menu as f32,
    }]
}

/// The part under `x`, `y` (logical pixels from the card's corner), if
/// any. The padding, the hint and the hairline do nothing.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Part> {
    if l.undo.contains(x, y) {
        return Some(Part::Undo);
    }
    if l.done.contains(x, y) {
        return Some(Part::Done);
    }
    l.tiles
        .iter()
        .position(|t| t.contains(x, y))
        .map(Part::Tile)
}

/// The parts in the keyboard's order: the tiles, then Undo and Done.
pub fn ring(view: &View) -> Vec<Part> {
    let mut out: Vec<Part> = (0..view.tiles.len()).map(Part::Tile).collect();
    out.push(Part::Undo);
    out.push(Part::Done);
    out
}

/// What a key does with `focus` as the focused part: arrows move between
/// tiles (Up and Down by a row), Undo and Done sit below the last row, Tab
/// cycles the ring, Return acts on Undo or Done, and Escape closes. Return
/// on a tile does nothing yet: dragging a widget comes with the editor's
/// drag (M5.31b).
pub fn key(view: &View, focus: Option<Part>, key: Key) -> Step {
    if key == Key::Escape {
        return Step::Act(Act::Close);
    }
    let ring = ring(view);
    if key == Key::Tab {
        let next = match focus.and_then(|f| ring.iter().position(|p| *p == f)) {
            Some(i) => (i + 1) % ring.len(),
            None => 0,
        };
        return Step::Focus(ring[next]);
    }
    let n = view.tiles.len();
    let columns = columns(view);
    match (key, focus) {
        (Key::Activate, Some(Part::Undo)) => Step::Act(Act::Undo),
        (Key::Activate, Some(Part::Done)) => Step::Act(Act::Done),
        (_, None) => ring.first().map_or(Step::Nothing, |p| Step::Focus(*p)),
        (Key::Left, Some(Part::Tile(i))) => Step::Focus(Part::Tile(i.saturating_sub(1))),
        (Key::Right, Some(Part::Tile(i))) => {
            Step::Focus(Part::Tile((i + 1).min(n.saturating_sub(1))))
        }
        (Key::Up, Some(Part::Tile(i))) => Step::Focus(Part::Tile(i.saturating_sub(columns))),
        (Key::Down, Some(Part::Tile(i))) if i + columns < n => Step::Focus(Part::Tile(i + columns)),
        (Key::Down, Some(Part::Tile(_))) => Step::Focus(Part::Undo),
        (Key::Left | Key::Up, Some(Part::Done)) => Step::Focus(Part::Undo),
        (Key::Right | Key::Down, Some(Part::Undo)) => Step::Focus(Part::Done),
        (Key::Left | Key::Up, Some(Part::Undo)) if n > 0 => Step::Focus(Part::Tile(n - 1)),
        _ => Step::Nothing,
    }
}

/// Where the parts lie, as one log line CI reads:
/// `card WxH, tile NAME X+Y+WxH, ..., undo X+Y+WxH, done X+Y+WxH`, logical
/// pixels from the card's corner.
pub fn places(l: &Layout, view: &View) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut out = format!("card {}x{}", l.size.0, l.size.1);
    for (tile, rect) in view.tiles.iter().zip(&l.tiles) {
        out.push_str(&format!(", tile {} {}", tile.name, at(*rect)));
    }
    out.push_str(&format!(", undo {}, done {}", at(l.undo), at(l.done)));
    out
}

/// A tile's picture (M5.31b): the widget drawn whole into a pixmap its
/// natural width by `panel_control` high, then laid into the tile's `layer`
/// centred in its top part, scaled down to fit the tile less `EDIT_PAD` at
/// each side when it is wider (never up), so nothing spills out of a tile.
fn picture(
    layer: &mut Pixmap,
    widget: &Widget,
    shown: &str,
    tokens: &Tokens,
    text: Option<&mut Text>,
    icons: Option<&mut edel::app_icons::Icons>,
    s: f32,
) {
    let (mut text, mut icons) = (text, icons);
    let height = tokens.panel_control as f32 * s;
    let top = PICTURE_TOP * s;
    let natural = {
        let mut canvas = Canvas {
            pixmap: layer,
            tokens,
            text: text.as_deref_mut(),
            icons: icons.as_deref_mut(),
            scale: s,
            top,
            height,
            dock: false,
            along_top: false,
        };
        (widget.width)(&mut canvas, shown)
    };
    let Some(mut piece) = Pixmap::new(natural.ceil().max(1.0) as u32, height.ceil() as u32) else {
        return;
    };
    {
        let mut canvas = Canvas {
            pixmap: &mut piece,
            tokens,
            text,
            icons,
            scale: s,
            top: 0.0,
            height,
            dock: false,
            along_top: false,
        };
        (widget.draw)(&mut canvas, shown, 0.0);
    }
    let lw = layer.width() as f32;
    let room = lw - 2.0 * EDIT_PAD * s;
    let fit = (room / natural.max(1.0)).min(1.0);
    let (w, h) = (natural * fit, height * fit);
    let x = ((lw - w) / 2.0).round();
    let y = top + ((height - h) / 2.0).round();
    let paint = PixmapPaint {
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    layer.draw_pixmap(
        0,
        0,
        piece.as_ref(),
        &paint,
        Transform::from_row(fit, 0.0, 0.0, fit, x, y),
        None,
    );
}

/// Draws `view` at `s` into `pixmap`, which is the card's size times it:
/// the card (outlined when a panel's widget is dragged over it), the
/// heading and its hint (at most two lines), each tile (a raised tile with
/// the widget's picture in its top part and its title under it, at half
/// strength when a panel holds it and at 35 percent while it is dragged, a
/// veil when the pointer or the keyboard is on it), the hairline and the
/// two buttons.
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
    if view.taking {
        let (x, y, w, h) = l.card.device(s);
        outline(
            pixmap,
            (x, y, w, h),
            tokens.radius_menu as f32 * s,
            2.0 * s,
            tokens.accent,
        );
    }
    let heading = tokens.panel_text_size as f32 * s;
    let small = tokens.panel_text_small_size as f32 * s;
    if let Some(text) = text.as_deref_mut() {
        let mut line = text.line_in(tr("Edit panels"), heading, Face::SEMIBOLD);
        let y = popup::middle(PAD, HEADING, heading, s);
        text.draw(pixmap, &mut line, PAD * s, y, tokens.panel_text);
        let room = (l.card.w - 2.0 * PAD) * s;
        let hint = if view.taking {
            tr("Drop here to take it off the panel")
        } else {
            tr(
                "Drag a widget along the panels or onto another, from here to add it, or here to take it away",
            )
        };
        let lines = wrap(hint, hint_lines(view), room, |t| text.line(t, small).width);
        for (i, words) in lines.iter().enumerate() {
            let mut line = text.fit(words, small, room);
            let top = PAD + HEADING + HINT_GAP + i as f32 * HINT_LINE;
            let y = popup::middle(top, HINT_LINE, small, s);
            text.draw(pixmap, &mut line, PAD * s, y, dim(tokens));
        }
    }
    for (i, (tile, rect)) in view.tiles.iter().zip(&l.tiles).enumerate() {
        let (x, y, w, h) = rect.device(s);
        let Some(mut layer) = Pixmap::new(w.round() as u32, h.round() as u32) else {
            continue;
        };
        let (lw, lh) = (layer.width() as f32, layer.height() as f32);
        let radius = tokens.radius_control as f32 * s;
        fill(&mut layer, 0.0, 0.0, lw, lh, radius, popup::raised(tokens));
        let part = Part::Tile(i);
        if view.hover == Some(part) || view.focus == Some(part) {
            fill(&mut layer, 0.0, 0.0, lw, lh, radius, veil(tokens, 0.06));
        }
        if let Some(widget) = widget_of(tile.name) {
            picture(
                &mut layer,
                widget,
                &tile.shown,
                tokens,
                text.as_deref_mut(),
                icons.as_deref_mut(),
                s,
            );
        }
        if let Some(text) = text.as_deref_mut() {
            let mut words = text.fit(&tile.title, small, lw - 2.0 * popup::PAD * s);
            let y = popup::middle(TITLE_TOP, TITLE_LINE, small, s);
            let at = ((lw - words.width) / 2.0).round();
            text.draw(&mut layer, &mut words, at, y, tokens.panel_text);
        }
        let opacity = if view.lifted == Some(i) {
            LIFTED
        } else if tile.placed {
            0.5
        } else {
            1.0
        };
        let paint = PixmapPaint {
            opacity,
            ..PixmapPaint::default()
        };
        pixmap.draw_pixmap(
            x as i32,
            y as i32,
            layer.as_ref(),
            &paint,
            Transform::identity(),
            None,
        );
    }
    let rule = l.card.h - FOOTER - 1.0;
    fill(
        pixmap,
        PAD * s,
        rule * s,
        (l.card.w - 2.0 * PAD) * s,
        s.max(1.0),
        0.0,
        tokens.line,
    );
    for (part, rect, words, ink, back) in [
        (
            Part::Undo,
            l.undo,
            tr("Undo"),
            tokens.panel_text,
            popup::raised(tokens),
        ),
        (
            Part::Done,
            l.done,
            tr("Done"),
            tokens.accent_text,
            tokens.accent,
        ),
    ] {
        let (x, y, w, h) = rect.device(s);
        let radius = tokens.radius_control as f32 * s;
        fill(pixmap, x, y, w, h, radius, back);
        if view.hover == Some(part) || view.focus == Some(part) {
            fill(pixmap, x, y, w, h, radius, veil(tokens, 0.06));
        }
        if let Some(text) = text.as_deref_mut() {
            let mut line = text.line(words, heading);
            let at = x + ((w - line.width) / 2.0).round();
            let baseline = popup::middle(rect.y, rect.h, heading, s);
            text.draw(pixmap, &mut line, at, baseline, ink);
        }
    }
}

/// What a screen reader finds: each tile as a button, named by its title
/// and whether a panel holds it, then Undo and Done.
pub fn nodes(view: &View, l: &Layout) -> Vec<Node> {
    let at = |r: Rect| {
        accesskit::Rect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.y + r.h),
        )
    };
    let button = |label: String, bounds| Node {
        role: Role::Button,
        label,
        bounds,
        children: Vec::new(),
        toggled: None,
        value: None,
    };
    let mut out: Vec<Node> = view
        .tiles
        .iter()
        .zip(&l.tiles)
        .map(|(tile, rect)| {
            let label = if tile.placed {
                trf("{title}, on a panel", &[("title", &tile.title)])
            } else {
                trf("Add {title}", &[("title", &tile.title)])
            };
            button(label, at(*rect))
        })
        .collect();
    out.push(button(tr("Undo").into(), at(l.undo)));
    out.push(button(tr("Done").into(), at(l.done)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::tokens::Scheme;

    fn view(n: usize, width: u32, compact: bool) -> View {
        View {
            tiles: (0..n)
                .map(|i| Tile {
                    name: "clock",
                    title: format!("Widget {i}"),
                    shown: String::new(),
                    placed: i == 0,
                })
                .collect(),
            compact,
            width,
            hover: None,
            focus: None,
            lifted: None,
            taking: false,
        }
    }

    #[test]
    fn a_desktop_drawer_is_four_tiles_across_and_centred_in_560() {
        let l = layout(&view(6, 1280, false));
        assert_eq!(l.size.0, 560);
        assert_eq!(
            l.tiles[0],
            Rect::new(44.0, 69.0, TILE.0, TILE.1),
            "the hint on one line"
        );
        assert_eq!(l.tiles[3].x, 44.0 + 3.0 * (TILE.0 + GAP));
        assert_eq!(l.tiles[4].y, 69.0 + TILE.1 + GAP, "the second row");
    }

    #[test]
    fn a_compact_drawer_is_the_screen_wide_with_three_tiles_across() {
        let l = layout(&view(7, 360, true));
        assert_eq!(l.size.0, 360);
        assert_eq!(l.tiles[0], Rect::new(PAD, 84.0, 104.0, TILE.1));
        assert_eq!(l.tiles[2].x, PAD + 2.0 * (104.0 + GAP));
        assert_eq!(l.done.h, BUTTON_TOUCH, "touch-sized buttons");
    }

    #[test]
    fn a_narrow_desktop_keeps_32_px_at_the_sides() {
        assert_eq!(layout(&view(1, 500, false)).size.0, 468);
    }

    #[test]
    fn hit_finds_a_tile_undo_and_done_and_nothing_in_the_padding() {
        let l = layout(&view(6, 1280, false));
        let tile = l.tiles[1];
        assert_eq!(hit(&l, tile.x + 1.0, tile.y + 1.0), Some(Part::Tile(1)));
        assert_eq!(hit(&l, l.undo.x + 1.0, l.undo.y + 1.0), Some(Part::Undo));
        assert_eq!(hit(&l, l.done.x + 1.0, l.done.y + 1.0), Some(Part::Done));
        assert_eq!(hit(&l, 2.0, 2.0), None, "the padding");
        assert_eq!(hit(&l, tile.x - 2.0, tile.y), None, "the gap beside a tile");
    }

    #[test]
    fn keys_move_between_tiles_and_the_footer_and_act() {
        let v = view(6, 1280, false);
        assert_eq!(
            key(&v, Some(Part::Tile(0)), Key::Right),
            Step::Focus(Part::Tile(1))
        );
        assert_eq!(
            key(&v, Some(Part::Tile(0)), Key::Down),
            Step::Focus(Part::Tile(4))
        );
        assert_eq!(
            key(&v, Some(Part::Tile(4)), Key::Down),
            Step::Focus(Part::Undo)
        );
        assert_eq!(
            key(&v, Some(Part::Undo), Key::Right),
            Step::Focus(Part::Done)
        );
        assert_eq!(
            key(&v, Some(Part::Done), Key::Tab),
            Step::Focus(Part::Tile(0)),
            "Tab wraps"
        );
        assert_eq!(
            key(&v, Some(Part::Undo), Key::Activate),
            Step::Act(Act::Undo)
        );
        assert_eq!(
            key(&v, Some(Part::Done), Key::Activate),
            Step::Act(Act::Done)
        );
        assert_eq!(key(&v, Some(Part::Tile(0)), Key::Activate), Step::Nothing);
        assert_eq!(key(&v, None, Key::Left), Step::Focus(Part::Tile(0)));
        assert_eq!(
            key(&v, Some(Part::Tile(2)), Key::Escape),
            Step::Act(Act::Close)
        );
    }

    /// A panel's row for the landing tests: a start group of `menu` and
    /// `windows`, and the end group's `clock`, with their places.
    fn row() -> Vec<(&'static str, Group, f32, f32)> {
        vec![
            ("menu", Group::Start, 0.0, 40.0),
            ("windows", Group::Start, 40.0, 100.0),
            ("clock", Group::End, 1200.0, 60.0),
        ]
    }

    const NO_EMPTY: [Option<(f32, f32)>; 3] = [None, None, None];

    #[test]
    fn a_drop_before_the_first_widget_lands_before_it() {
        let l = landing(&row(), NO_EMPTY, 5.0, None).unwrap();
        assert_eq!(
            l,
            Landing {
                group: Group::Start,
                before: Some("menu"),
                caret: 0.0
            }
        );
    }

    #[test]
    fn a_drop_after_the_last_of_a_group_lands_at_its_end() {
        let l = landing(&row(), NO_EMPTY, 130.0, None).unwrap();
        assert_eq!(
            l,
            Landing {
                group: Group::Start,
                before: None,
                caret: 140.0
            },
            "after windows' middle, the start group's end"
        );
        let l = landing(&row(), NO_EMPTY, 1230.0, None).unwrap();
        assert_eq!(l.group, Group::End);
        assert_eq!(l.before, None);
    }

    #[test]
    fn a_drop_inside_an_empty_group_lands_in_it_at_its_middle() {
        let mut empty = NO_EMPTY;
        empty[1] = Some((600.0, 40.0));
        let l = landing(&row(), empty, 610.0, None).unwrap();
        assert_eq!(
            l,
            Landing {
                group: Group::Centre,
                before: None,
                caret: 620.0
            }
        );
    }

    #[test]
    fn the_lifted_widget_is_not_a_place_to_land_by() {
        let r = vec![
            ("menu", Group::Start, 0.0, 40.0),
            ("windows", Group::Start, 40.0, 40.0),
        ];
        // Over windows, which is lifted: menu is the nearest, and after its
        // middle the group's end is the next widget left, none.
        let l = landing(&r, NO_EMPTY, 60.0, Some("windows")).unwrap();
        assert_eq!(
            l,
            Landing {
                group: Group::Start,
                before: None,
                caret: 40.0
            }
        );
    }

    #[test]
    fn a_gap_between_groups_goes_to_the_nearer_widget() {
        let r = vec![
            ("menu", Group::Start, 0.0, 40.0),
            ("clock", Group::End, 100.0, 40.0),
        ];
        let before_clock = landing(&r, NO_EMPTY, 80.0, None).unwrap();
        assert_eq!(
            before_clock,
            Landing {
                group: Group::End,
                before: Some("clock"),
                caret: 100.0
            },
            "80 is 20 from the clock and 40 from the menu"
        );
        let after_menu = landing(&r, NO_EMPTY, 60.0, None).unwrap();
        assert_eq!(after_menu.group, Group::Start, "60 is 20 from the menu");
        assert_eq!(after_menu.caret, 40.0);
    }

    #[test]
    fn an_empty_row_gives_no_landing() {
        assert_eq!(landing(&[], NO_EMPTY, 10.0, None), None);
        assert_eq!(landing(&[], NO_EMPTY, 10.0, Some("menu")), None);
    }

    #[test]
    fn places_name_every_tile_and_the_footer() {
        let v = view(2, 1280, false);
        let text = places(&layout(&v), &v);
        assert!(text.starts_with("card 560x"), "{text}");
        assert_eq!(text.matches(", tile clock ").count(), 2, "{text}");
        assert!(text.contains(", undo "), "{text}");
        assert!(text.contains(", done "), "{text}");
    }

    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    #[test]
    fn it_draws_light_and_dark_and_writes_pngs() {
        let Some(mut text) = fonts() else {
            return; // no fonts on this machine
        };
        // Classic's widgets are placed; Apps and Search are not.
        let panels = edel::presets::named(Some("classic")).0.panels;
        let placed: Vec<String> = panels
            .iter()
            .flat_map(|p| p.widgets())
            .map(String::from)
            .collect();
        let live = crate::widgets::Live::default();
        for (scheme, mode) in [(Scheme::Light, "light"), (Scheme::Dark, "dark")] {
            let tokens = Tokens::built_in_scheme(scheme);
            let tiles = crate::widgets::TABLE
                .iter()
                .filter(|w| w.name != "apps" && w.name != "search")
                .map(|w| Tile {
                    name: w.name,
                    title: tr(w.title).to_string(),
                    shown: (w.shows)(&live),
                    placed: placed.iter().any(|p| p.as_str() == w.name),
                })
                .collect();
            let v = View {
                tiles,
                compact: false,
                width: 1280,
                hover: None,
                focus: Some(Part::Done),
                lifted: None,
                taking: false,
            };
            let l = layout(&v);
            let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
            paint(&mut pixmap, &v, &l, &tokens, None, Some(&mut text), 2.0);
            let alpha = |x: f32, y: f32| {
                pixmap
                    .pixel((x * 2.0) as u32, (y * 2.0) as u32)
                    .unwrap()
                    .alpha()
            };
            assert_eq!(alpha(0.0, 0.0), 0, "{mode}: a round corner");
            let t = l.tiles[0];
            assert_ne!(alpha(t.x + 3.0, t.y + 3.0), 0, "{mode}: the first tile");
            if let Some(dir) = std::env::var_os("EDEL_EDITOR_PNG") {
                let dir = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&dir).unwrap();
                pixmap
                    .save_png(dir.join(format!("editor-{mode}.png")))
                    .unwrap();
            }
        }
    }
}
