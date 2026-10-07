//! The layout button's menu (M5.16b): a right click on the layout button
//! opens a small card beside it with a row for each tiling style, its
//! picture, its name and a check on the one in use. A click, or Up, Down
//! and Return, chooses one, which is written to the person's settings
//! file as `layout.tiling_style` with the function Settings and
//! `edel settings set` write with; the compositor follows the file, and
//! nothing else changes, so a floating workspace stays floating. Escape
//! or a click elsewhere closes it. The styles are the key's values in
//! `edel::settings`, so a new style is a new row here with no line
//! changed. Like the launcher it holds its surface only while open.
//! Drawing is plain and tested without a display.

use tiny_skia::Pixmap;

use edel::tokens::Tokens;

use crate::paint::{self, Text, fill, mix};
use crate::popup::{self, INSET, PAD, middle};

/// The key the menu chooses.
pub const KEY: &str = "layout.tiling_style";

/// Its width in logical pixels.
pub const WIDTH: u32 = 200;
/// A style's picture, logical pixels, and the room after it.
const PICTURE: (f32, f32) = (26.0, 16.0);
const GAP: f32 = 10.0;
/// The check on the style in use.
const CHECK: f32 = 14.0;

/// The styles the menu offers, in the key table's order.
pub fn styles() -> &'static [&'static str] {
    edel::settings::choices(KEY)
}

/// A style as people read it: `split` is Split.
pub fn label(style: &str) -> String {
    let mut chars = style.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Its size in logical pixels: a row for each style.
pub fn size(tokens: &Tokens) -> (u32, u32) {
    let rows = styles().len() as f32 * tokens.row as f32;
    (WIDTH, (PAD + rows + PAD) as u32)
}

/// What the menu shows: the style in use and the row the pointer or the
/// arrows are on.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub chosen: usize,
    pub lit: usize,
}

/// The row at `y` logical pixels from the card's top, if any.
pub fn row_at(y: f32, tokens: &Tokens) -> Option<usize> {
    if y < PAD {
        return None;
    }
    let row = ((y - PAD) / tokens.row as f32) as usize;
    (row < styles().len()).then_some(row)
}

/// The style the files choose now: the person's over the machine's, else
/// the first, the release's default.
pub fn in_use(machine: Option<&str>, person: Option<&str>) -> usize {
    edel::settings::chosen(KEY, machine, person)
        .and_then(|s| styles().iter().position(|v| *v == s))
        .unwrap_or(0)
}

/// Draws `view` at `scale` into `pixmap`, its size times `scale`.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    text: Option<&mut Text>,
    scale: f32,
) {
    let s = scale;
    popup::card(pixmap, tokens, s);
    let names = styles().iter().map(|_| "");
    popup::rows(pixmap, tokens, None, names, PAD, Some(view.lit), s);
    let row = tokens.row as f32;
    let left = PAD + INSET;
    for (i, style) in styles().iter().enumerate() {
        let top = PAD + i as f32 * row;
        let y = top + (row - PICTURE.1) / 2.0;
        picture(pixmap, style, left * s, y * s, s, tokens);
        if i == view.chosen {
            let px = CHECK * s;
            let x = (WIDTH as f32 - PAD - INSET) * s - px;
            paint::icon(
                pixmap,
                "check",
                px,
                x,
                (top + (row - CHECK) / 2.0) * s,
                tokens.accent,
            );
        }
    }
    let Some(text) = text else {
        return;
    };
    let size = tokens.panel_text_size as f32 * s;
    let x = (left + PICTURE.0 + GAP) * s;
    let room = (WIDTH as f32 - left - PICTURE.0 - GAP - CHECK - PAD - INSET) * s;
    for (i, style) in styles().iter().enumerate() {
        let mut line = text.fit(&label(style), size, room);
        let y = middle(PAD + i as f32 * row, row, size, s);
        text.draw(pixmap, &mut line, x, y, tokens.panel_text);
    }
}

/// A style's picture at `x`, `y`: a screen's outline with its windows as
/// the style lays out three or four, so each row shows what it does.
fn picture(pixmap: &mut Pixmap, style: &str, x: f32, y: f32, s: f32, tokens: &Tokens) {
    let (w, h) = (PICTURE.0 * s, PICTURE.1 * s);
    let frame = mix(tokens.panel_text, tokens.panel, 0.7);
    let tile = mix(tokens.panel_text, tokens.panel, 0.45);
    let r = 2.0 * s;
    fill(pixmap, x, y, w, h, r, frame);
    let g = (1.5 * s).max(1.0);
    let inner = (x + g, y + g, w - 2.0 * g, h - 2.0 * g);
    let half = (inner.2 - g) / 2.0;
    let right = inner.0 + half + g;
    let tall = (inner.3 - g) / 2.0;
    let small = r / 2.0;
    // Both: the main window on the left half.
    fill(pixmap, inner.0, inner.1, half, inner.3, small, tile);
    match style {
        // Split: the right half halved, its lower half halved again.
        "split" => {
            fill(pixmap, right, inner.1, half, tall, small, tile);
            let quarter = (half - g) / 2.0;
            let low = inner.1 + tall + g;
            fill(pixmap, right, low, quarter, tall, small, tile);
            fill(pixmap, right + quarter + g, low, quarter, tall, small, tile);
        }
        // Stack, and any style without a picture of its own: the rest
        // stacked on the right.
        _ => {
            let third = (inner.3 - 2.0 * g) / 3.0;
            for k in 0..3 {
                let ty = inner.1 + k as f32 * (third + g);
                fill(pixmap, right, ty, half, third, small, tile);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_offers_the_keys_styles_and_marks_the_one_in_use() {
        assert_eq!(styles(), ["stack", "split"]);
        assert_eq!(label("split"), "Split");
        let machine = "format = 1\n[layout]\ntiling_style = \"split\"\n";
        let person = "format = 1\n[layout]\ntiling_style = \"stack\"\n";
        assert_eq!(in_use(None, None), 0, "absent is the first, stack");
        assert_eq!(in_use(Some(machine), None), 1);
        assert_eq!(in_use(Some(machine), Some(person)), 0, "the person's wins");
        assert_eq!(
            in_use(
                Some("format = 1\n[layout]\ntiling_style = \"spiral\"\n"),
                None
            ),
            0
        );
    }

    #[test]
    fn a_row_is_found_under_the_pointer() {
        let tokens = Tokens::built_in();
        let row = tokens.row as f32;
        assert_eq!(row_at(PAD - 1.0, &tokens), None);
        assert_eq!(row_at(PAD + 1.0, &tokens), Some(0));
        assert_eq!(row_at(PAD + row + 1.0, &tokens), Some(1));
        assert_eq!(row_at(PAD + 2.0 * row + 1.0, &tokens), None);
        let (w, h) = size(&tokens);
        assert_eq!((w, h), (WIDTH, (2.0 * PAD + 2.0 * row) as u32));
    }

    #[test]
    fn the_lit_row_and_the_pictures_are_drawn() {
        let tokens = Tokens::built_in();
        let (w, h) = size(&tokens);
        let mut pixmap = Pixmap::new(w, h).unwrap();
        paint(&mut pixmap, &View { chosen: 1, lit: 0 }, &tokens, None, 1.0);
        let at = |x: f32, y: f32| {
            let c = pixmap.pixel(x as u32, y as u32).unwrap().demultiply();
            [c.red(), c.green(), c.blue()]
        };
        let back = {
            let b = tokens.panel.bytes();
            [b[0], b[1], b[2]]
        };
        let row = tokens.row as f32;
        // The lit row is not the card's colour; the other row is, beside
        // its picture.
        assert_ne!(at(PAD + 3.0, PAD + row / 2.0), back);
        assert_eq!(at(PAD + 3.0, PAD + row + row / 2.0), back);
        // Each row's picture is drawn left of its name.
        let px = PAD + INSET + 4.0;
        assert_ne!(at(px, PAD + row + row / 2.0), back);
    }
}
