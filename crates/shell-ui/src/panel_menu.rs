//! The panel's menu (M5.31b): a right click on a panel where no widget
//! gives an action for it (empty space, or a widget without a menu of its
//! own) opens a small card at the pointer's place along the panel, with a
//! row for each thing the panels do. One row now, Edit panels; M5.31c adds
//! rows to `ROWS`. Like the tiling styles' menu it is a card of rows, the
//! keyboard exclusive, Escape closing it, and it holds its surface only
//! while open. Plain data and drawing, tested without a display;
//! `panel_menu_card.rs` owns the surface.

use edel::i18n::tr;
use edel::tokens::Tokens;
use tiny_skia::Pixmap;

use crate::paint::Text;
use crate::popup::{self, PAD, Rect};

/// What a row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// Opens the editor (`editor_card.rs`).
    Edit,
}

/// The rows, top to bottom.
pub const ROWS: &[Row] = &[Row::Edit];

/// Its width in logical pixels.
pub const WIDTH: u32 = 200;

/// The row as people read it.
pub fn label(row: Row) -> String {
    match row {
        Row::Edit => tr("Edit panels").into(),
    }
}

/// Its size in logical pixels: a row for each entry, with `PAD` round them.
pub fn size(tokens: &Tokens) -> (u32, u32) {
    let rows = ROWS.len() as f32 * tokens.row as f32;
    (WIDTH, (PAD + rows + PAD) as u32)
}

/// What the menu shows: the row the pointer or the arrows are on.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub lit: usize,
}

/// The row at `y` logical pixels from the card's top, if any.
pub fn row_at(y: f32, tokens: &Tokens) -> Option<usize> {
    if y < PAD {
        return None;
    }
    let row = ((y - PAD) / tokens.row as f32) as usize;
    (row < ROWS.len()).then_some(row)
}

/// Where row `i` lies, logical pixels from the card's top left corner.
pub fn row_rect(i: usize, tokens: &Tokens) -> Rect {
    let row = tokens.row as f32;
    Rect::new(PAD, PAD + i as f32 * row, WIDTH as f32 - 2.0 * PAD, row)
}

/// Where the card and its rows lie, as the one log line CI reads:
/// `card WxH, row edit X+Y+WxH`, logical pixels from the card's corner.
pub fn places(tokens: &Tokens) -> String {
    let (w, h) = size(tokens);
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    format!("card {w}x{h}, row edit {}", at(row_rect(0, tokens)))
}

/// Draws `view` at `scale` into `pixmap`, its size times `scale`: the card
/// with each row's label, the lit row filled.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    text: Option<&mut Text>,
    scale: f32,
) {
    popup::card(pixmap, tokens, scale);
    let labels: Vec<String> = ROWS.iter().map(|row| label(*row)).collect();
    popup::rows(
        pixmap,
        tokens,
        text,
        labels.iter().map(String::as_str),
        PAD,
        Some(view.lit),
        scale,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_has_one_row_that_edits_the_panels() {
        assert_eq!(ROWS, &[Row::Edit]);
        assert_eq!(label(Row::Edit), "Edit panels");
        let tokens = Tokens::built_in();
        assert_eq!(size(&tokens).0, WIDTH);
        assert_eq!(row_at(PAD + 1.0, &tokens), Some(0));
        assert_eq!(row_at(PAD - 1.0, &tokens), None, "the padding above");
        assert_eq!(
            row_at(PAD + tokens.row as f32 + 1.0, &tokens),
            None,
            "no second row"
        );
    }

    #[test]
    fn its_places_say_where_the_row_is() {
        let tokens = Tokens::built_in();
        let row = tokens.row;
        let (w, h) = size(&tokens);
        assert_eq!(
            places(&tokens),
            format!("card {w}x{h}, row edit 8+8+184x{row}")
        );
    }
}
