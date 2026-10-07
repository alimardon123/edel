//! The tray (M5.2e): one icon for each app that registered a
//! StatusNotifierItem (`crate::tray`), in a row beside the clock. It
//! shows a line per item, `ID\tTITLE\tICON NAME\tPIXMAP`; the icon is
//! the item's `IconName` found in the icon themes as the apps' are, else
//! its `IconPixmap`, else the first letter of its title on an accent tile.
//! A click asks the item to activate, a right click for its menu. With no
//! item the widget has no width. A screen reader hears the row as a group
//! and each icon as a button named by its item's title.

use accesskit::Role;
use edel::i18n::{tr, trf};
use tiny_skia::{FilterQuality, PixmapPaint, Transform};

use super::{Action, Canvas, Input, Live, Part, Widget};
use crate::paint::{fill, mix};

pub const WIDGET: Widget = Widget {
    name: "tray",
    needs: None,
    shows,
    width,
    draw,
    input,
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

/// One item of the row.
#[derive(Debug, PartialEq)]
struct Cell<'a> {
    id: &'a str,
    title: &'a str,
    name: &'a str,
    pixels: &'a str,
}

fn shows(live: &Live) -> String {
    let clean = |s: &str| s.replace(['\t', '\n'], " ");
    let lines: Vec<String> = live
        .tray
        .iter()
        .map(|item| {
            format!(
                "{}\t{}\t{}\t{}",
                clean(&item.id),
                clean(&item.label),
                clean(&item.name),
                item.pixels
            )
        })
        .collect();
    lines.join("\n")
}

/// `shows`' text read back.
fn read(shown: &str) -> Vec<Cell<'_>> {
    shown
        .lines()
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

/// "Tray: Network, Volume".
fn label(shown: &str) -> String {
    let names: Vec<String> = read(shown).iter().map(title).collect();
    trf("Tray: {names}", &[("names", &names.join(", "))])
}

/// Each item for a reader, where its cell lies in the widget.
fn parts(shown: &str) -> Vec<Part> {
    read(shown)
        .iter()
        .enumerate()
        .map(|(i, cell)| Part {
            label: title(cell),
            x: EDGE + i as f32 * CELL,
            width: CELL,
        })
        .collect()
}

/// The widget's width in logical pixels for `count` items.
pub fn logical_width(count: usize) -> f32 {
    if count == 0 {
        0.0
    } else {
        2.0 * EDGE + count as f32 * CELL
    }
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    logical_width(read(shown).len()) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let px = (ICON * s).round();
    let side = CELL * s;
    for (i, cell) in read(shown).iter().enumerate() {
        let cx = (x + EDGE * s + i as f32 * side).round();
        let cy = canvas.top + ((canvas.height - side) / 2.0).round();
        let ix = cx + ((side - px) / 2.0).round();
        let iy = cy + ((side - px) / 2.0).round();
        let from_theme = canvas
            .icons
            .as_deref_mut()
            .filter(|_| !cell.name.is_empty())
            .and_then(|icons| icons.get(cell.name, px as u32))
            .map(|icon| {
                let paint = PixmapPaint {
                    quality: FilterQuality::Nearest,
                    ..PixmapPaint::default()
                };
                canvas.pixmap.draw_pixmap(
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
            continue;
        }
        if let Some(icon) = crate::tray::decode(cell.pixels) {
            // Fitted into the icon's square, centred, smoothly unless it
            // is that size already.
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
            canvas.pixmap.draw_pixmap(
                0,
                0,
                icon.as_ref(),
                &paint,
                Transform::from_row(k, 0.0, 0.0, k, tx, ty),
                None,
            );
            continue;
        }
        // A tile in the accent with the title's first letter.
        let tile = mix(tokens.panel, tokens.accent, 0.7);
        fill(canvas.pixmap, ix, iy, px, px, (px * 0.25).round(), tile);
        if let Some(text) = canvas.text.as_deref_mut() {
            let initial: String = title(cell)
                .chars()
                .take(1)
                .collect::<String>()
                .to_uppercase();
            let size = (px * 0.6).round();
            let mut line = text.line(&initial, size);
            let lx = ix + (px - line.width) / 2.0;
            let ly = iy + (px - size * 1.25) / 2.0;
            text.draw(canvas.pixmap, &mut line, lx, ly, tokens.panel);
        }
    }
}

/// A click on an icon asks its app to activate, a right click opens its
/// menu; the margin at each end does nothing.
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
    let cells = read(shown);
    let cell = cells.get(i as usize)?;
    Some(Action::Tray(cell.id.to_string(), menu))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::{Look, Row, fillet_height, paint};
    use crate::tray::Item;
    use edel::presets::Edge;
    use edel::tokens::Tokens;

    fn live(count: usize) -> Live {
        let item = |n: usize| Item {
            id: format!(":1.{n}/StatusNotifierItem"),
            label: format!("item {n}"),
            name: String::new(),
            pixels: crate::tray::encode(&{
                let mut p = tiny_skia::Pixmap::new(22, 22).unwrap();
                p.fill(tiny_skia::Color::from_rgba8(0x33, 0xaa, 0x66, 255));
                p
            }),
        };
        Live {
            tray: (0..count).map(item).collect(),
            ..Live::default()
        }
    }

    #[test]
    fn it_has_no_width_without_items_and_a_cell_for_each() {
        assert_eq!(shows(&Live::default()), "");
        assert_eq!(logical_width(0), 0.0);
        assert_eq!(logical_width(3), 94.0);
        let shown = shows(&live(2));
        let cells = read(&shown);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[1].id, ":1.1/StatusNotifierItem");
        assert_eq!(cells[1].title, "item 1");
        assert!(cells[1].pixels.starts_with("22x22:33aa66ff"));
    }

    #[test]
    fn a_click_reaches_the_icon_under_it() {
        let shown = shows(&live(2));
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
    fn a_reader_hears_each_item_by_its_title() {
        let shown = shows(&live(2));
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
    fn an_items_pixmap_is_drawn_in_the_middle_of_its_cell() {
        let tokens = Tokens::built_in();
        let row = Row {
            start: vec![],
            centre: vec![],
            end: vec![&WIDGET],
        };
        let live = live(1);
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
        assert_eq!(w, logical_width(1));
        let strip = fillet_height(&tokens);
        let (mx, my) = ((x + w / 2.0) as u32, strip + tokens.panel_height / 2);
        let c = pixmap.pixel(mx, my).unwrap();
        assert_eq!([c.red(), c.green(), c.blue()], [0x33, 0xaa, 0x66]);
        // Beside the icon, in the cell's margin, is the panel.
        let c = pixmap.pixel(x as u32 + 5, my).unwrap();
        assert_eq!([c.red(), c.green(), c.blue()], tokens.panel.bytes()[..3]);
    }
}
