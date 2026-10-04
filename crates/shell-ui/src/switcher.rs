//! The window switcher's list (M5.3c): while Alt+Tab is held, the
//! compositor sends the windows' titles, the most recently used first,
//! and which is chosen; shell-ui draws them in the middle of the screen
//! and hides them when told. The compositor owns the order and the
//! choice, so this is only a picture. Drawing is plain and tested
//! without a display.

use tiny_skia::Pixmap;

use edel::tokens::Tokens;

use crate::paint::Text;
use crate::popup::{self, PAD};

/// Its width in logical pixels; its height follows the titles, at most
/// ten.
pub const WIDTH: u32 = 420;
pub const MOST: usize = 10;

/// What the switcher shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    pub titles: Vec<String>,
    pub chosen: usize,
}

impl View {
    /// From the compositor's event: titles one per line, an untitled
    /// window's empty, the last one too.
    pub fn from_event(titles: &str, chosen: u32) -> View {
        View {
            titles: titles.split('\n').map(str::to_string).collect(),
            chosen: chosen as usize,
        }
    }
}

/// Its size for `rows` titles, in logical pixels.
pub fn size(rows: usize, tokens: &Tokens) -> (u32, u32) {
    (WIDTH, (2.0 * PAD) as u32 + rows as u32 * tokens.row)
}

/// Draws `view` at `scale` into `pixmap`, its size times `scale`.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    text: Option<&mut Text>,
    scale: f32,
) {
    popup::card(pixmap, tokens, scale);
    let chosen = (view.chosen < view.titles.len()).then_some(view.chosen);
    let titles = view.titles.iter().map(String::as_str);
    popup::rows(pixmap, tokens, text, titles, PAD, chosen, scale);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pixmap.pixel(x, y).unwrap().demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn the_chosen_title_is_lit_and_the_height_follows_the_titles() {
        let tokens = Tokens::built_in();
        let view = View::from_event("away\none\nfoot", 1);
        assert_eq!(view.titles, ["away", "one", "foot"]);
        assert_eq!(size(3, &tokens), (WIDTH, 124));
        let mut pixmap = Pixmap::new(WIDTH, 124).unwrap();
        paint(&mut pixmap, &view, &tokens, None, 1.0);
        assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "a round corner");
        let back = tokens.panel.bytes();
        assert_eq!(pixel(&pixmap, WIDTH / 2, 4), back);
        let row = |i: u32| pixel(&pixmap, PAD as u32 + 4, PAD as u32 + i * tokens.row + 18);
        assert_eq!(row(0), back);
        assert_ne!(row(1), back);
        assert_eq!(row(2), back);
        // An untitled window keeps its row, the last one too.
        assert_eq!(View::from_event("away\n", 1).titles, ["away", ""]);
        assert_eq!(View::from_event("", 0).titles, [""]);
    }
}
