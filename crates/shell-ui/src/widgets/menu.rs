//! The menu button (M5.1b): `design/icons/menu.svg`, four rounded
//! squares, in the panel text's colour, centred in a tile as the
//! mockups draw it (M5.29): `size.panel_control` high and a fifth wider,
//! `size.panel_glyph` for the icon. While the launcher is open the tile is
//! filled and the icon takes the accent, as the mockups show it. A click
//! opens the launcher, or closes it (M5.3b).

use accesskit::Role;
use edel::i18n::tr;

use edel::tokens::{Colour, Tokens};

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{self, fill};

pub const WIDGET: Widget = Widget {
    name: "menu",
    needs: None,
    shows,
    width,
    draw,
    input: |_, shown, what| input(shown, what),
    parts: super::no_parts,
    role: Role::Button,
    label,
};

fn label(_: &str) -> String {
    tr("Menu").into()
}

/// A click opens or closes the launcher.
fn input(_: &str, input: Input) -> Option<Action> {
    matches!(input, Input::Click(..)).then_some(Action::Launcher)
}

/// What it shows: `open` while the launcher is open, else nothing.
fn shows(live: &Live) -> String {
    if live.launcher {
        "open".into()
    } else {
        String::new()
    }
}

/// The room before the tile (the panel's own padding in the mockups) and
/// after it, in logical pixels.
const ROOM_START: f32 = 6.0;
const ROOM_END: f32 = 2.0;

/// The tile's width: a fifth wider than it is high, as the mockups'.
fn tile(tokens: &Tokens) -> f32 {
    (tokens.panel_control as f32 * 1.2).round()
}

/// Its width in logical pixels.
pub fn logical_width(tokens: &Tokens) -> f32 {
    ROOM_START + tile(tokens) + ROOM_END
}

fn width(canvas: &mut Canvas, _: &str) -> f32 {
    logical_width(canvas.tokens) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let open = shown == "open";
    let (tw, th) = (
        (tile(tokens) * s).round(),
        (tokens.panel_control as f32 * s).round(),
    );
    let tx = (x + ROOM_START * s).round();
    let ty = canvas.top + ((canvas.height - th) / 2.0).round();
    let ink = if open {
        let lit = Colour {
            a: 0.085,
            ..tokens.panel_text
        };
        let r = tokens.radius_control as f32 * s;
        fill(canvas.pixmap, tx, ty, tw, th, r, lit);
        tokens.accent
    } else {
        tokens.panel_text
    };
    let px = (tokens.panel_glyph as f32 * s).round();
    let ix = tx + ((tw - px) / 2.0).round();
    let iy = ty + ((th - px) / 2.0).round();
    paint::icon(canvas.pixmap, "menu", px, ix, iy, ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tile_is_a_fifth_wider_than_a_button_is_high() {
        let tokens = Tokens::built_in();
        assert_eq!(tile(&tokens), 36.0);
        assert_eq!(logical_width(&tokens), 44.0);
    }

    #[test]
    fn it_shows_whether_the_launcher_is_open() {
        let mut live = Live::default();
        assert_eq!(shows(&live), "");
        live.launcher = true;
        assert_eq!(shows(&live), "open");
    }
}
