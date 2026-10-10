//! The tray (M5.2e): one icon for each app that registered a
//! StatusNotifierItem, in a row beside the clock. The apps the person
//! kept in the panel (`layout.tray_in_panel`, M5.9g) show their icons in
//! that order; every other app waits behind the row's first cell, an
//! arrow that opens a grid of them (`trayview.rs`, `tray_card.rs`). The
//! arrow shows a dot while one of them has news. It shows a line per
//! item, `ID\TITLE\tICON NAME\tPIXMAP` for the kept items, after an
//! optional `arrow\tHIDDEN\tOPEN\tDOT` line; the icon is the item's
//! `IconName` found in the icon themes as the apps' are, else its
//! `IconPixmap`, else the first letter of its title on an accent tile.
//! A click asks the item to activate, a right click for its menu. With no
//! item the widget has no width. A screen reader hears the row as a group
//! and each icon as a button named by its item's title.

use accesskit::Role;
use edel::i18n::{tr, trf};
use edel::tokens::Tokens;
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use super::{Action, Canvas, Input, Live, Part, Widget};
use crate::paint::{self, Text, fill, mix};
use crate::popup::veil;
use crate::tray::Item;

pub const WIDGET: Widget = Widget {
    name: "tray",
    needs: None,
    shows,
    width,
    draw,
    input: |_, shown, what| input(shown, what),
    parts,
    role: Role::Group,
    label,
};

/// A cell's side, the room at each end of the row and the icon, in
/// logical pixels. The icon is the size the specification's items draw
/// theirs; there is no token for an icon's size or a gap yet, so these
/// are the widget's own, as the apps widget's are.
const CELL: f32 = 30.0;
const EDGE: f32 = 2.0;
const ICON: f32 = 22.0;

/// The middle of the arrow along the widget, in logical pixels: where the
/// grid opens from (M5.9g).
pub fn arrow_middle() -> f32 {
    EDGE + CELL / 2.0
}

/// The arrow's glyph is this share of the panel's glyph size (M5.9g).
const ARROW_SHARE: f32 = 0.7;
/// The attention dot's diameter and its distance from a cell's top right
/// corner, in logical pixels (M5.9g).
const DOT: f32 = 6.0;
const DOT_IN: f32 = 4.0;

/// One item of the row.
#[derive(Debug, PartialEq)]
struct Cell<'a> {
    id: &'a str,
    title: &'a str,
    name: &'a str,
    pixels: &'a str,
}

/// What an icon is drawn from: its theme icon's name, its pixmap as
/// `tray::encode` writes it, and its title, whose first letter stands for
/// it on an accent tile. The tray's row and the grid (`trayview.rs`) draw
/// one icon the same way through [`draw_icon`].
pub(crate) struct Glyph<'a> {
    pub name: &'a str,
    pub pixels: &'a str,
    pub title: &'a str,
}

/// The items kept in the panel, in `tray_in_panel`'s order, and the items
/// behind the arrow, in the order they came (M5.9g). A name the tray lacks
/// keeps nothing; an item kept once is not kept again.
pub fn split(live: &Live) -> (Vec<&Item>, Vec<&Item>) {
    let mut taken = vec![false; live.tray.len()];
    let mut kept = Vec::new();
    for name in &live.tray_in_panel {
        let found = live
            .tray
            .iter()
            .enumerate()
            .find(|(i, item)| !taken[*i] && item.app == *name);
        if let Some((i, item)) = found {
            taken[i] = true;
            kept.push(item);
        }
    }
    let hidden = live
        .tray
        .iter()
        .enumerate()
        .filter(|(i, _)| !taken[*i])
        .map(|(_, item)| item)
        .collect();
    (kept, hidden)
}

/// One kept item's line, as `shows` writes it.
fn line(item: &Item) -> String {
    let clean = |s: &str| s.replace(['\t', '\n'], " ");
    format!(
        "{}\t{}\t{}\t{}",
        clean(&item.id),
        clean(&item.label),
        clean(&item.name),
        item.pixels
    )
}

fn shows(live: &Live) -> String {
    let (kept, hidden) = split(live);
    let mut lines = Vec::new();
    if !hidden.is_empty() {
        let dot = hidden.iter().any(|item| item.attention);
        lines.push(format!(
            "arrow\t{}\t{}\t{}",
            hidden.len(),
            u8::from(live.tray_open),
            u8::from(dot)
        ));
    }
    lines.extend(kept.into_iter().map(line));
    lines.join("\n")
}

/// The words of the arrow for a reader and its tooltip: how many items lie
/// behind it (M5.9h).
pub fn hidden_label(count: usize) -> String {
    trf("Hidden icons ({count})", &[("count", &count.to_string())])
}

/// How many items lie behind the arrow, when `at` logical pixels along the
/// widget is on the arrow's cell, None when it is not on it or no arrow
/// shows (M5.9h, the tooltip).
pub fn behind_arrow(shown: &str, at: f32) -> Option<usize> {
    let (hidden, ..) = arrow(shown)?;
    (EDGE..EDGE + CELL).contains(&at).then_some(hidden)
}

/// `shows`' arrow line read back: how many items lie behind the arrow,
/// whether its grid is open and whether one of them has news. None when
/// the widget shows no arrow.
fn arrow(shown: &str) -> Option<(usize, bool, bool)> {
    let rest = shown.lines().next()?.strip_prefix("arrow\t")?;
    let mut parts = rest.split('\t');
    let hidden = parts.next()?.parse().ok()?;
    let open = parts.next()? == "1";
    let dot = parts.next()? == "1";
    Some((hidden, open, dot))
}

/// `shows`' kept items' lines read back.
fn read(shown: &str) -> Vec<Cell<'_>> {
    shown
        .lines()
        .filter(|line| !line.starts_with("arrow\t"))
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            Some(Cell {
                id: parts.next()?,
                title: parts.next()?,
                name: parts.next()?,
                pixels: parts.next()?,
            })
        })
        .collect()
}

/// What a reader calls an item: its title, else "Tray item".
fn title(cell: &Cell) -> String {
    if cell.title.is_empty() {
        tr("Tray item").into()
    } else {
        cell.title.to_string()
    }
}

/// "Tray: Network, Volume", with ", N hidden" when some lie behind the
/// arrow.
fn label(shown: &str) -> String {
    let names: Vec<String> = read(shown).iter().map(title).collect();
    let names = names.join(", ");
    match arrow(shown) {
        None => trf("Tray: {names}", &[("names", &names)]),
        Some((hidden, ..)) => {
            let count = hidden.to_string();
            if names.is_empty() {
                trf("Tray: {count} hidden", &[("count", &count)])
            } else {
                trf(
                    "Tray: {names}, {count} hidden",
                    &[("names", &names), ("count", &count)],
                )
            }
        }
    }
}

/// Each part for a reader, where its cell lies in the widget: the arrow
/// first while one is shown, then the kept items.
fn parts(shown: &str) -> Vec<Part> {
    let mut out = Vec::new();
    let mut x = EDGE;
    if let Some((hidden, ..)) = arrow(shown) {
        out.push(Part {
            label: hidden_label(hidden),
            x,
            width: CELL,
        });
        x += CELL;
    }
    for cell in read(shown) {
        out.push(Part {
            label: title(&cell),
            x,
            width: CELL,
        });
        x += CELL;
    }
    out
}

/// The widget's width in logical pixels: a cell for each kept item and one
/// more for the arrow when one is shown, 0 when there are neither.
pub fn logical_width(kept: usize, arrow: bool) -> f32 {
    let cells = kept + usize::from(arrow);
    if cells == 0 {
        0.0
    } else {
        2.0 * EDGE + cells as f32 * CELL
    }
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    logical_width(read(shown).len(), arrow(shown).is_some()) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let px = (ICON * s).round();
    let side = CELL * s;
    let cy = canvas.top + ((canvas.height - side) / 2.0).round();
    let mut first = 0;
    if let Some((_, open, dot)) = arrow(shown) {
        let cx = (x + EDGE * s).round();
        draw_arrow(canvas, cx, cy, side, (open, dot));
        first = 1;
    }
    for (i, cell) in read(shown).iter().enumerate() {
        let cx = (x + EDGE * s + (i + first) as f32 * side).round();
        let ix = cx + ((side - px) / 2.0).round();
        let iy = cy + ((side - px) / 2.0).round();
        let title = title(cell);
        let glyph = Glyph {
            name: cell.name,
            pixels: cell.pixels,
            title: &title,
        };
        draw_icon(
            canvas.pixmap,
            tokens,
            canvas.icons.as_deref_mut(),
            canvas.text.as_deref_mut(),
            glyph,
            (ix, iy, px),
        );
    }
}

/// The arrow in its cell at `cx`, `cy` of `side` device pixels: when its
/// grid is open a rounded square inside the cell, lit as the status pill
/// is, and the chevron, pointing down from a panel along the top of the
/// screen and up from any other; with news, the dot.
fn draw_arrow(canvas: &mut Canvas, cx: f32, cy: f32, side: f32, (open, dot): (bool, bool)) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    if open {
        let inset = (2.0 * s).round();
        fill(
            canvas.pixmap,
            cx + inset,
            cy + inset,
            side - 2.0 * inset,
            side - 2.0 * inset,
            tokens.radius_control as f32 * s,
            veil(tokens, 0.085),
        );
    }
    let glyph = (tokens.panel_glyph as f32 * ARROW_SHARE * s).round();
    let at = ((side - glyph) / 2.0).round();
    let name = if canvas.along_top {
        "chevron-down"
    } else {
        "chevron-up"
    };
    paint::icon(
        canvas.pixmap,
        name,
        glyph,
        cx + at,
        cy + at,
        tokens.panel_text,
    );
    if dot {
        self::dot(canvas.pixmap, tokens, cx + side, cy, s);
    }
}

/// The attention dot at the top right corner of a square whose right edge
/// is `right` and top `top` device pixels: `DOT` logical pixels across,
/// `DOT_IN` in from both edges, in the title's close colour (the critical
/// notification's edge, M5.9b).
pub(crate) fn dot(pixmap: &mut Pixmap, tokens: &Tokens, right: f32, top: f32, s: f32) {
    let d = (DOT * s).round();
    let inset = (DOT_IN * s).round();
    fill(
        pixmap,
        right - inset - d,
        top + inset,
        d,
        d,
        d / 2.0,
        tokens.title_close_hover,
    );
}

/// Draws one icon `glyph` in the square at `square`, its left, top and
/// side in device pixels: its theme icon when the icon themes have it,
/// else its pixmap fitted into the square, else the first letter of its
/// title on an accent tile. Shared by the row and the grid.
pub(crate) fn draw_icon(
    pixmap: &mut Pixmap,
    tokens: &Tokens,
    icons: Option<&mut edel::app_icons::Icons>,
    text: Option<&mut Text>,
    glyph: Glyph,
    square: (f32, f32, f32),
) {
    let (ix, iy, px) = square;
    let from_theme = icons
        .filter(|_| !glyph.name.is_empty())
        .and_then(|icons| icons.get(glyph.name, px as u32))
        .map(|icon| {
            let paint = PixmapPaint {
                quality: FilterQuality::Nearest,
                ..PixmapPaint::default()
            };
            pixmap.draw_pixmap(
                ix as i32,
                iy as i32,
                icon.as_ref(),
                &paint,
                Transform::identity(),
                None,
            );
        })
        .is_some();
    if from_theme {
        return;
    }
    if let Some(icon) = crate::tray::decode(glyph.pixels) {
        // Fitted into the icon's square, centred, smoothly unless it is
        // that size already.
        let big = icon.width().max(icon.height()) as f32;
        let k = px / big;
        let quality = if (k - 1.0).abs() < f32::EPSILON {
            FilterQuality::Nearest
        } else {
            FilterQuality::Bilinear
        };
        let paint = PixmapPaint {
            quality,
            ..PixmapPaint::default()
        };
        let tx = ix + ((px - icon.width() as f32 * k) / 2.0).round();
        let ty = iy + ((px - icon.height() as f32 * k) / 2.0).round();
        pixmap.draw_pixmap(
            0,
            0,
            icon.as_ref(),
            &paint,
            Transform::from_row(k, 0.0, 0.0, k, tx, ty),
            None,
        );
        return;
    }
    // A tile in the accent with the title's first letter.
    let tile = mix(tokens.panel, tokens.accent, 0.7);
    fill(pixmap, ix, iy, px, px, (px * 0.25).round(), tile);
    if let Some(text) = text {
        let initial: String = glyph
            .title
            .chars()
            .take(1)
            .collect::<String>()
            .to_uppercase();
        let size = (px * 0.6).round();
        let mut line = text.line(&initial, size);
        let lx = ix + (px - line.width) / 2.0;
        let ly = iy + (px - size * 1.25) / 2.0;
        text.draw(pixmap, &mut line, lx, ly, tokens.panel);
    }
}

/// A click on the arrow opens or closes its grid (M5.9g); a click on an
/// icon asks its app to activate, a right click opens its menu; the margin
/// at each end does nothing, and a right click on the arrow does nothing.
fn input(shown: &str, input: Input) -> Option<Action> {
    let (at, menu) = match input {
        Input::Click(x, _) => (x, false),
        Input::Menu(x, _) => (x, true),
        Input::Scroll(_) => return None,
    };
    let i = ((at - EDGE) / CELL).floor();
    if i < 0.0 {
        return None;
    }
    let mut i = i as usize;
    if arrow(shown).is_some() {
        if i == 0 {
            return (!menu).then_some(Action::TrayOpen);
        }
        i -= 1;
    }
    let cells = read(shown);
    let cell = cells.get(i)?;
    Some(Action::Tray(cell.id.to_string(), menu))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::{Look, Row, fillet_height, paint};
    use edel::presets::Edge;

    /// Item `n`, a green pixmap, known to its app as `app{n}`.
    fn item(n: usize) -> Item {
        Item {
            id: format!(":1.{n}/StatusNotifierItem"),
            label: format!("item {n}"),
            name: String::new(),
            pixels: crate::tray::encode(&{
                let mut p = tiny_skia::Pixmap::new(22, 22).unwrap();
                p.fill(tiny_skia::Color::from_rgba8(0x33, 0xaa, 0x66, 255));
                p
            }),
            app: format!("app{n}"),
            attention: false,
        }
    }

    fn live(count: usize) -> Live {
        Live {
            tray: (0..count).map(item).collect(),
            ..Live::default()
        }
    }

    /// `count` items, all of them kept in the panel.
    fn kept(count: usize) -> Live {
        Live {
            tray_in_panel: (0..count).map(|n| format!("app{n}")).collect(),
            ..live(count)
        }
    }

    #[test]
    fn it_has_no_width_without_items_and_a_cell_for_each() {
        assert_eq!(shows(&Live::default()), "");
        assert_eq!(logical_width(0, false), 0.0);
        assert_eq!(logical_width(3, false), 94.0);
        assert_eq!(logical_width(0, true), 34.0, "the arrow alone");
        let shown = shows(&kept(2));
        assert!(arrow(&shown).is_none(), "every item is kept, no arrow");
        let cells = read(&shown);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[1].id, ":1.1/StatusNotifierItem");
        assert_eq!(cells[1].title, "item 1");
        assert!(cells[1].pixels.starts_with("22x22:33aa66ff"));
    }

    #[test]
    fn a_click_reaches_the_icon_under_it() {
        let shown = shows(&kept(2));
        let at = |x: f32, menu: bool| {
            input(
                &shown,
                if menu {
                    Input::Menu(x, 64.0)
                } else {
                    Input::Click(x, 64.0)
                },
            )
        };
        let item = |n: usize| format!(":1.{n}/StatusNotifierItem");
        assert_eq!(at(10.0, false), Some(Action::Tray(item(0), false)));
        assert_eq!(at(40.0, false), Some(Action::Tray(item(1), false)));
        assert_eq!(at(40.0, true), Some(Action::Tray(item(1), true)));
        // The margin at each end and beyond the last icon do nothing.
        assert_eq!(at(1.0, false), None);
        assert_eq!(at(63.0, false), None);
        assert_eq!(input(&shown, Input::Scroll(1)), None);
    }

    #[test]
    fn an_arrow_shows_for_an_app_behind_it_and_a_click_opens_the_grid() {
        let shown = shows(&live(1));
        assert_eq!(shown, "arrow\t1\t0\t0");
        assert_eq!(arrow(&shown), Some((1, false, false)));
        assert!(read(&shown).is_empty());
        let width = logical_width(read(&shown).len(), arrow(&shown).is_some());
        assert_eq!(width, 34.0);
        let click = |x: f32| input(&shown, Input::Click(x, width));
        assert_eq!(click(17.0), Some(Action::TrayOpen));
        assert_eq!(click(1.0), None, "the margin at the start");
        assert_eq!(click(33.0), None, "beyond the arrow there is no icon");
        assert_eq!(input(&shown, Input::Menu(17.0, width)), None);
        // The arrow says whether its grid is open.
        let open = Live {
            tray_open: true,
            ..live(1)
        };
        assert_eq!(shows(&open), "arrow\t1\t1\t0");
    }

    #[test]
    fn with_a_kept_app_the_arrow_comes_first_and_the_kept_app_after_it() {
        let live = Live {
            tray_in_panel: vec!["app0".into()],
            ..live(2)
        };
        let shown = shows(&live);
        assert_eq!(arrow(&shown), Some((1, false, false)));
        let width = logical_width(read(&shown).len(), arrow(&shown).is_some());
        assert_eq!(width, 64.0);
        assert_eq!(
            input(&shown, Input::Click(17.0, 64.0)),
            Some(Action::TrayOpen)
        );
        assert_eq!(
            input(&shown, Input::Click(47.0, 64.0)),
            Some(Action::Tray(":1.0/StatusNotifierItem".into(), false))
        );
    }

    #[test]
    fn with_every_item_kept_there_is_no_arrow() {
        let shown = shows(&kept(2));
        assert_eq!(arrow(&shown), None);
        assert_eq!(logical_width(2, false), 64.0);
    }

    #[test]
    fn the_dot_shows_only_for_a_hidden_item_with_attention() {
        // Item 1 is kept, item 0 is behind the arrow.
        let mut live = Live {
            tray_in_panel: vec!["app1".into()],
            ..live(2)
        };
        assert_eq!(arrow(&shows(&live)), Some((1, false, false)));
        live.tray[1].attention = true;
        assert_eq!(
            arrow(&shows(&live)),
            Some((1, false, false)),
            "a kept item's news is no dot"
        );
        live.tray[1].attention = false;
        live.tray[0].attention = true;
        assert_eq!(arrow(&shows(&live)), Some((1, false, true)));
    }

    #[test]
    fn split_keeps_the_panels_order_and_the_rest_in_order() {
        let live = Live {
            tray_in_panel: vec!["app2".into(), "app0".into(), "gone".into()],
            ..live(4)
        };
        let (kept, hidden) = split(&live);
        let ids = |items: &[&Item]| items.iter().map(|i| i.label.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&kept), ["item 2", "item 0"]);
        assert_eq!(ids(&hidden), ["item 1", "item 3"]);
    }

    #[test]
    fn a_reader_hears_each_item_by_its_title() {
        let shown = shows(&kept(2));
        assert_eq!(label(&shown), "Tray: item 0, item 1");
        let parts = parts(&shown);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].label, "item 1");
        assert_eq!((parts[1].x, parts[1].width), (32.0, 30.0));
        // An item with no title is still named.
        let nameless = format!("{}\t\t\t", ":1.5/Item");
        assert_eq!(label(&nameless), "Tray: Tray item");
    }

    #[test]
    fn a_reader_hears_the_arrow_first_and_the_hidden_count() {
        let one_kept = Live {
            tray_in_panel: vec!["app0".into()],
            ..live(2)
        };
        let shown = shows(&one_kept);
        assert_eq!(label(&shown), "Tray: item 0, 1 hidden");
        let parts = parts(&shown);
        assert_eq!(parts[0].label, "Hidden icons (1)");
        assert_eq!((parts[0].x, parts[0].width), (2.0, 30.0));
        assert_eq!(parts[1].label, "item 0");
        assert_eq!(parts[1].x, 32.0);
        // With nothing kept, the label counts what lies behind the arrow.
        assert_eq!(label(&shows(&live(2))), "Tray: 2 hidden");
    }

    #[test]
    fn an_items_pixmap_is_drawn_in_the_middle_of_its_cell() {
        let tokens = Tokens::built_in();
        let row = Row {
            start: vec![],
            centre: vec![],
            end: vec![&WIDGET],
        };
        let live = kept(1);
        let look = Look {
            width: 200,
            height: tokens.panel_height + fillet_height(&tokens),
            scale: 1,
            edge: Edge::Bottom,
            style: edel::presets::Style::Bar,
            fillets: false,
            shown: row.shows(&live),
        };
        let mut pixmap = tiny_skia::Pixmap::new(look.width, look.height).unwrap();
        let places = paint(&mut pixmap, &look, &tokens, None, None, &row);
        let (x, w) = places[0];
        assert_eq!(w, logical_width(1, false));
        let strip = fillet_height(&tokens);
        let (mx, my) = ((x + w / 2.0) as u32, strip + tokens.panel_height / 2);
        let c = pixmap.pixel(mx, my).unwrap();
        assert_eq!([c.red(), c.green(), c.blue()], [0x33, 0xaa, 0x66]);
        // Beside the icon, in the cell's margin, is the panel.
        let c = pixmap.pixel(x as u32 + 5, my).unwrap();
        assert_eq!([c.red(), c.green(), c.blue()], tokens.panel.bytes()[..3]);
    }

    #[test]
    fn the_arrow_is_lit_when_open_and_the_dot_marks_news() {
        let tokens = Tokens::built_in();
        let row = Row {
            start: vec![],
            centre: vec![],
            end: vec![&WIDGET],
        };
        let strip = fillet_height(&tokens);
        let my = strip + tokens.panel_height / 2;
        let draw = |live: &Live| {
            let look = Look {
                width: 200,
                height: tokens.panel_height + strip,
                scale: 1,
                edge: Edge::Bottom,
                style: edel::presets::Style::Bar,
                fillets: false,
                shown: row.shows(live),
            };
            let mut pixmap = tiny_skia::Pixmap::new(look.width, look.height).unwrap();
            let places = paint(&mut pixmap, &look, &tokens, None, None, &row);
            (pixmap, places[0].0)
        };
        let rgb = |pixmap: &tiny_skia::Pixmap, x: u32, y: u32| {
            let c = pixmap.pixel(x, y).unwrap();
            [c.red(), c.green(), c.blue()]
        };
        // Shut: the cell's margin is the panel; open: it is lit.
        let (shut, x) = draw(&live(1));
        let x = x as u32;
        assert_eq!(rgb(&shut, x + 3, my), tokens.panel.bytes()[..3]);
        let open = Live {
            tray_open: true,
            ..live(1)
        };
        let (lit, x) = draw(&open);
        // The lit square sits 2 px inside the cell, which starts 2 px in.
        assert_ne!(rgb(&lit, x as u32 + 5, my), tokens.panel.bytes()[..3]);
        // With news, the dot is in the cell's top right corner.
        let mut news = live(1);
        news.tray[0].attention = true;
        let (dotted, x) = draw(&news);
        let (dx, dy) = (x as u32 + 30 - 4 - 3, strip + 9 + 3);
        assert_eq!(rgb(&dotted, dx, dy), tokens.title_close_hover.bytes()[..3]);
        assert_eq!(rgb(&shut, x as u32 + 3, my), tokens.panel.bytes()[..3]);
    }
}
