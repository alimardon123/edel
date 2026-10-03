//! The menu button (M5.1b): four rounded squares in the panel text's
//! colour, centred in a square as tall as the panel. The launcher it opens
//! comes with M5.3.

use accesskit::Role;
use tiny_skia::{FillRule, Transform};

use super::{Canvas, Widget};
use crate::paint::{paint_of, rounded};

pub const WIDGET: Widget = Widget {
    name: "menu",
    needs: None,
    shows,
    width,
    draw,
    role: Role::Button,
    label,
};

fn label(_: &str) -> String {
    "Menu".into()
}

/// It always shows the same icon.
fn shows() -> String {
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
    let paint = paint_of(canvas.tokens.panel_text);
    for (i, j) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let left = x + inset + i * (cell + gap);
        let top = canvas.top + inset + j * (cell + gap);
        if let Some(square) = rounded(left, top, cell, cell, cell * 0.3) {
            canvas.pixmap.fill_path(
                &square,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}
