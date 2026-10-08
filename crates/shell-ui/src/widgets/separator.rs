//! The separator (M5.29): a thin vertical line between groups of widgets,
//! as the mockups draw one after the menu button and before the status
//! icons, in the panel text's colour at 15 percent, a little over half a
//! button tall, with room on both sides. It shows nothing that changes,
//! says nothing to a screen reader and takes no input; a preset or
//! `layout.panels` places it by the name `separator`.

use accesskit::Role;

use super::{Canvas, Live, Widget, no_input};
use crate::paint::fill;
use edel::tokens::Colour;

pub const WIDGET: Widget = Widget {
    name: "separator",
    needs: None,
    shows,
    width,
    draw,
    input: no_input,
    parts: super::no_parts,
    role: Role::GenericContainer,
    label: |_| String::new(),
};

/// The room on each side of the line, in logical pixels.
const ROOM: f32 = 6.0;
/// The line's height as a share of a button's.
const HEIGHT: f32 = 0.6;

/// It always shows the same line.
fn shows(_: &Live) -> String {
    String::new()
}

/// The line is one screen pixel wide at any scale, in the room.
fn width(canvas: &mut Canvas, _: &str) -> f32 {
    2.0 * (ROOM * canvas.scale).round() + 1.0
}

fn draw(canvas: &mut Canvas, _: &str, x: f32) {
    let tokens = canvas.tokens;
    let s = canvas.scale;
    let room = (ROOM * s).round();
    let h = (tokens.panel_control as f32 * HEIGHT * s).round();
    let y = canvas.top + ((canvas.height - h) / 2.0).round();
    let ink = Colour {
        a: 0.15,
        ..tokens.panel_text
    };
    fill(canvas.pixmap, x + room, y, 1.0, h, 0.0, ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_a_line_with_room_on_both_sides_and_no_words() {
        assert_eq!((WIDGET.label)(""), "");
        assert_eq!(shows(&Live::default()), "");
    }
}
