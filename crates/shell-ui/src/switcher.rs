//! The window switcher's list (M5.3c): while Alt+Tab is held, the
//! compositor sends the windows' titles, the most recently used first,
//! and which is chosen; shell-ui draws them in the middle of the screen
//! and hides them when told. The compositor owns the order and the
//! choice, so this is only a picture. Drawing is plain and tested
//! without a display.

use tiny_skia::{FillRule, Pixmap, Transform};

use edel::tokens::{Colour, Tokens};

use crate::paint::{Text, paint_of, rounded};

/// Its width, a row's height and the room around the rows, in logical
/// pixels; its height follows the titles, at most ten.
pub const WIDTH: u32 = 420;
const ROW: f32 = 36.0;
const PAD: f32 = 8.0;
const RADIUS: f32 = 12.0;
const CONTROL: f32 = 7.0;
const INSET: f32 = 12.0;

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

/// Its height for `rows` titles.
pub fn height(rows: usize) -> u32 {
    (2.0 * PAD + rows as f32 * ROW) as u32
}

/// Draws `view` at `scale` into `pixmap`, `WIDTH` by its height times
/// `scale`.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    text: Option<&mut Text>,
    scale: f32,
) {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let s = scale;
    let fill = |pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Colour| {
        if let Some(path) = rounded(x * s, y * s, w * s, h * s, r * s) {
            pixmap.fill_path(
                &path,
                &paint_of(c),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    };
    let w = WIDTH as f32;
    let h = height(view.titles.len()) as f32;
    fill(pixmap, 0.0, 0.0, w, h, RADIUS, tokens.panel);
    if view.chosen < view.titles.len() {
        let chosen = Colour {
            a: 0.18,
            ..tokens.accent
        };
        let y = PAD + view.chosen as f32 * ROW;
        fill(pixmap, PAD, y, w - 2.0 * PAD, ROW, CONTROL, chosen);
    }
    let Some(text) = text else {
        return;
    };
    let size = tokens.panel_text_size as f32 * s;
    let room = (w - 2.0 * (PAD + INSET)) * s;
    for (i, title) in view.titles.iter().enumerate() {
        let mut line = text.fit(title, size, room);
        let top = PAD + i as f32 * ROW;
        let y = (top + ROW / 2.0) * s - size * 0.625;
        text.draw(pixmap, &mut line, (PAD + INSET) * s, y, tokens.panel_text);
    }
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
        assert_eq!(height(3), 124);
        let mut pixmap = Pixmap::new(WIDTH, height(3)).unwrap();
        paint(&mut pixmap, &view, &tokens, None, 1.0);
        assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "a round corner");
        let back = tokens.panel.bytes();
        assert_eq!(pixel(&pixmap, WIDTH / 2, 4), back);
        let row = |i: u32| pixel(&pixmap, PAD as u32 + 4, PAD as u32 + i * ROW as u32 + 18);
        assert_eq!(row(0), back);
        assert_ne!(row(1), back);
        assert_eq!(row(2), back);
        // An untitled window keeps its row, the last one too.
        assert_eq!(View::from_event("away\n", 1).titles, ["away", ""]);
        assert_eq!(View::from_event("", 0).titles, [""]);
    }
}
