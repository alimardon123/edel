//! The search field (M5.4c): a pill with a magnifier and the word
//! "Search", as Windows' taskbar has; Alimardon preferred this rounder,
//! narrower field to the mockup's. It is only a door: a click
//! opens the launcher, where typing searches the apps, as Super does
//! (M5.3b).

use accesskit::Role;
use edel::i18n::tr;
use tiny_skia::{LineCap, PathBuilder, Stroke, Transform};

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{fill, mix, paint_of};

pub const WIDGET: Widget = Widget {
    name: "search",
    needs: None,
    shows,
    width,
    draw,
    input,
    parts: super::no_parts,
    role: Role::Button,
    label,
};

/// The field and the room on each side of it, in logical pixels.
const WIDTH: f32 = 180.0;
const HEIGHT: f32 = 30.0;
const ROOM: f32 = 4.0;
/// The magnifier's inset from the field's left end and its size.
const PAD: f32 = 11.0;
const GLASS: f32 = 14.0;

fn label(_: &str) -> String {
    tr("Search").into()
}

/// It always shows the same field.
fn shows(_: &Live) -> String {
    String::new()
}

/// Its width in logical pixels.
pub fn logical_width() -> f32 {
    WIDTH + 2.0 * ROOM
}

fn width(canvas: &mut Canvas, _: &str) -> f32 {
    logical_width() * canvas.scale
}

/// A click opens the launcher, or closes it.
fn input(_: &str, input: Input) -> Option<Action> {
    matches!(input, Input::Click(..)).then_some(Action::Launcher)
}

fn draw(canvas: &mut Canvas, _: &str, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let (fw, fh) = ((WIDTH * s).round(), (HEIGHT * s).round());
    let fx = (x + ROOM * s).round();
    let fy = canvas.top + ((canvas.height - fh) / 2.0).round();
    let field = mix(tokens.panel, tokens.panel_text, 0.1);
    fill(canvas.pixmap, fx, fy, fw, fh, fh / 2.0, field);
    let ink = mix(tokens.panel_text, tokens.panel, 0.3);
    // The magnifier: a ring and its handle, from a 14-unit square.
    let unit = GLASS * s / 14.0;
    let (gx, gy) = (fx + PAD * s, fy + (fh - GLASS * s) / 2.0);
    let mut path = PathBuilder::new();
    path.push_circle(gx + 6.0 * unit, gy + 6.0 * unit, 4.75 * unit);
    path.move_to(gx + 9.5 * unit, gy + 9.5 * unit);
    path.line_to(gx + 13.0 * unit, gy + 13.0 * unit);
    if let Some(path) = path.finish() {
        let stroke = Stroke {
            width: 1.6 * unit,
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        canvas
            .pixmap
            .stroke_path(&path, &paint_of(ink), &stroke, Transform::identity(), None);
    }
    if let Some(text) = canvas.text.as_deref_mut() {
        let size = (tokens.panel_text_size as f32 * 0.96).round() * s;
        let tx = gx + (GLASS + 8.0) * s;
        let mut line = text.fit(tr("Search"), size, fx + fw - tx - PAD * s);
        let ty = fy + (fh - size * 1.25) / 2.0;
        text.draw(canvas.pixmap, &mut line, tx, ty, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_anywhere_opens_the_launcher_and_a_scroll_does_nothing() {
        assert_eq!(input("", Input::Click(1.0, 188.0)), Some(Action::Launcher));
        assert_eq!(input("", Input::Scroll(1)), None);
        assert_eq!(label(""), "Search");
        assert_eq!(logical_width(), 188.0);
    }
}
