//! The app windows menu (M6.19): when an app on the panel has two or more
//! windows, a click lists them by title, and a row brings that window
//! forward.
//!
//! Plain data and drawing, as the styles' menu is: the card and its rows
//! come from `popup`, the sizes from the tokens, and what a screen reader
//! reads is `nodes`. `app_windows_card.rs` holds the surface and the
//! keyboard. Tested without a display.

use accesskit::{Rect, Role};
use tiny_skia::Pixmap;

use edel::tokens::Tokens;

use crate::a11y::Item;
use crate::paint::{Text, fill};
use crate::popup::{self, INSET};

/// Its width in logical pixels.
pub const WIDTH: u32 = 280;
/// The most rows it shows: more windows than this are left out of the menu.
pub const MOST: usize = 10;
/// The room round the rows, the same as the menus' (`popup::PAD`), so the
/// rows the menu draws and the places it names agree.
const PAD: f32 = popup::PAD;
/// The mark of the focused window: a dot this wide, in the room the row's
/// text leaves on its left.
const DOT: f32 = 6.0;

/// What the menu shows: the windows' titles in order, the row of the
/// focused window if one is focused and not minimized, and the row the
/// pointer or the arrows are on.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub titles: Vec<String>,
    pub focused: Option<usize>,
    pub lit: usize,
}

/// Its size in logical pixels: a row for each window shown, at least one.
pub fn size(count: usize, tokens: &Tokens) -> (u32, u32) {
    let rows = count.clamp(1, MOST) as f32;
    (WIDTH, (PAD + rows * tokens.row as f32 + PAD) as u32)
}

/// The row at `y` logical pixels from the card's top, if any of the `count`
/// windows shown is there.
pub fn row_at(y: f32, count: usize, tokens: &Tokens) -> Option<usize> {
    if y < PAD {
        return None;
    }
    let row = ((y - PAD) / tokens.row as f32) as usize;
    (row < count.min(MOST)).then_some(row)
}

/// What a screen reader reads of the menu (M6.9): one list item a window,
/// named by its title; the focus is the lit row.
pub fn nodes(view: &View, tokens: &Tokens) -> (Vec<Item>, Option<usize>) {
    let (left, row) = (PAD as f64, tokens.row as f64);
    let items = view
        .titles
        .iter()
        .take(MOST)
        .enumerate()
        .map(|(i, title)| {
            let top = PAD as f64 + i as f64 * row;
            Item {
                role: Role::ListItem,
                label: title.clone(),
                bounds: Rect::new(left, top, WIDTH as f64 - left, top + row),
                children: vec![],
                toggled: None,
                value: None,
                value_text: None,
            }
        })
        .collect();
    (items, Some(view.lit))
}

/// Draws `view` at `scale` into `pixmap`, its size times `scale`: the card,
/// the titles as rows (cut where they are too long, as `popup::rows` does),
/// the lit row's light, and the focused window's dot.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    text: Option<&mut Text>,
    scale: f32,
) {
    let s = scale;
    popup::card(pixmap, tokens, s);
    let labels = view.titles.iter().take(MOST).map(|t| t.as_str());
    popup::rows(pixmap, tokens, text, labels, PAD, Some(view.lit), s);
    if let Some(focused) = view.focused.filter(|&f| f < MOST) {
        let row = tokens.row as f32;
        let x = PAD + (INSET - DOT) / 2.0;
        let y = PAD + focused as f32 * row + (row - DOT) / 2.0;
        fill(
            pixmap,
            x * s,
            y * s,
            DOT * s,
            DOT * s,
            DOT * s / 2.0,
            tokens.accent,
        );
    }
}

/// The log line CI reads: the card's size, then each row's place in
/// logical pixels from the card's top left corner, as `row0 X+Y+WxH`.
pub fn places(view: &View, tokens: &Tokens) -> String {
    let (w, h) = size(view.titles.len(), tokens);
    let row = tokens.row;
    let pad = PAD as u32;
    let width = WIDTH - 2 * pad;
    let rows = (0..view.titles.len().min(MOST)).map(|i| {
        let top = pad + i as u32 * row;
        format!("row{i} {pad}+{top}+{width}x{row}")
    });
    let mut parts = vec![format!("card {w}x{h}")];
    parts.extend(rows);
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(titles: &[&str], focused: Option<usize>, lit: usize) -> View {
        View {
            titles: titles.iter().map(|t| t.to_string()).collect(),
            focused,
            lit,
        }
    }

    #[test]
    fn the_size_grows_with_the_rows_and_stops_at_most() {
        let tokens = Tokens::built_in();
        let row = tokens.row as f32;
        assert_eq!(size(0, &tokens), size(1, &tokens), "one row at least");
        let (w, h2) = size(2, &tokens);
        assert_eq!(w, WIDTH);
        assert_eq!(h2, (2.0 * PAD + 2.0 * row) as u32);
        assert!(size(3, &tokens).1 > h2);
        assert_eq!(size(MOST, &tokens), size(MOST + 5, &tokens));
    }

    #[test]
    fn a_row_is_found_under_the_pointer() {
        let tokens = Tokens::built_in();
        let row = tokens.row as f32;
        assert_eq!(row_at(PAD - 1.0, 3, &tokens), None);
        assert_eq!(row_at(PAD + 1.0, 3, &tokens), Some(0));
        assert_eq!(row_at(PAD + row + 1.0, 3, &tokens), Some(1));
        assert_eq!(row_at(PAD + 2.0 * row + 1.0, 3, &tokens), Some(2));
        assert_eq!(row_at(PAD + 3.0 * row + 1.0, 3, &tokens), None);
        assert_eq!(row_at(PAD + 1.0, 0, &tokens), None, "no window, no row");
        assert_eq!(
            row_at(PAD + MOST as f32 * row + 1.0, MOST + 4, &tokens),
            None,
            "past the most shown"
        );
    }

    #[test]
    fn places_names_every_row() {
        let tokens = Tokens::built_in();
        let row = tokens.row;
        let text = places(&view(&["Files", "Mail", "Notes"], Some(0), 1), &tokens);
        let (w, h) = size(3, &tokens);
        assert!(text.starts_with(&format!("card {w}x{h}, ")), "{text}");
        let pad = PAD as u32;
        let width = WIDTH - 2 * pad;
        for i in 0..3u32 {
            let top = pad + i * row;
            assert!(
                text.contains(&format!("row{i} {pad}+{top}+{width}x{row}")),
                "{text}"
            );
        }
        assert!(!text.contains("row3"), "{text}");
    }

    #[test]
    fn nodes_has_one_item_per_title_with_the_lit_one_focused() {
        let tokens = Tokens::built_in();
        let (items, focus) = nodes(&view(&["Files", "Mail"], Some(0), 1), &tokens);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Files", "Mail"]);
        assert!(items.iter().all(|i| i.role == Role::ListItem));
        assert!(items.iter().all(|i| i.toggled.is_none()));
        assert_eq!(focus, Some(1), "the lit row holds the focus");
    }

    #[test]
    fn the_focused_window_has_its_mark_in_the_accent() {
        let tokens = Tokens::built_in();
        let (w, h) = size(2, &tokens);
        let mut pixmap = Pixmap::new(w, h).unwrap();
        paint(
            &mut pixmap,
            &view(&["Files", "Mail"], Some(0), 1),
            &tokens,
            None,
            1.0,
        );
        let at = |x: f32, y: f32| {
            let c = pixmap.pixel(x as u32, y as u32).unwrap().demultiply();
            [c.red(), c.green(), c.blue()]
        };
        let accent = {
            let b = tokens.accent.bytes();
            [b[0], b[1], b[2]]
        };
        let row = tokens.row as f32;
        let dot = PAD + (INSET - DOT) / 2.0 + DOT / 2.0;
        assert_eq!(at(dot, PAD + row / 2.0), accent, "row 0 is marked");
        assert_ne!(at(dot, PAD + row + row / 2.0), accent, "row 1 is not");
    }
}
