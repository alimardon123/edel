//! The menu button (M5.1b): `design/icons/menu.svg`, four rounded
//! squares, in the panel text's colour, centred in a square as tall as the
//! panel (M5.5d). A click opens the launcher, or closes it (M5.3b).

use accesskit::Role;
use edel::i18n::tr;

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint;

pub const WIDGET: Widget = Widget {
    name: "menu",
    needs: None,
    shows,
    width,
    draw,
    input,
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

/// It always shows the same icon.
fn shows(_: &Live) -> String {
    String::new()
}

fn width(canvas: &mut Canvas, _: &str) -> f32 {
    canvas.height
}

/// The size of one square and the gap between them, and the icon's inset
/// from its square, for a panel `height` high.
pub fn icon(height: f32) -> (f32, f32, f32) {
    let cell = (height * 0.16).round();
    let gap = (cell * 0.5).round();
    let inset = ((height - (2.0 * cell + gap)) / 2.0).round();
    (cell, gap, inset)
}

fn draw(canvas: &mut Canvas, _: &str, x: f32) {
    let (cell, gap, inset) = icon(canvas.height);
    let (px, top) = (2.0 * cell + gap, canvas.top + inset);
    let ink = canvas.tokens.panel_text;
    paint::icon(canvas.pixmap, "menu", px, x + inset, top, ink);
}
