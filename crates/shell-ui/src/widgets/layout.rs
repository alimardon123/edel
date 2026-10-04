//! The layout toggle (M5.3a): one button that does what Super+T does,
//! whose icon shows how the shown workspace places its windows: two
//! overlapping windows when they float, a screen split in three when they
//! tile, the button then filled with the accent. A click switches the
//! shown workspace over `edel-shell-v1` (`crate::link`), which also says
//! the policy; without it the button is not there.

use tiny_skia::{FillRule, LineCap, LineJoin, Path, PathBuilder, Stroke, Transform};

use edel::tokens::Colour;

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{mix, paint_of, rounded};

pub const WIDGET: Widget = Widget {
    name: "layout",
    needs: None,
    shows,
    width,
    draw,
    input,
};

/// The button, its corner radius and the space on each side of it.
const WIDTH: f32 = 30.0;
const HEIGHT: f32 = 26.0;
const RADIUS: f32 = 7.0;
const ROOM: f32 = 3.0;
/// The icon's size, drawn from a 24-unit square as the mockups' are, and
/// its lines' width in those units.
const ICON: f32 = 15.0;
const LINE: f32 = 1.6;

/// What it shows: the shown workspace's policy, `floating` or `tiling`,
/// or nothing before the compositor has said.
fn shows(live: &Live) -> String {
    live.policy.clone()
}

/// Its width in logical pixels.
pub fn logical_width(shown: &str) -> f32 {
    if shown.is_empty() {
        0.0
    } else {
        WIDTH + 2.0 * ROOM
    }
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    logical_width(shown) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    if shown.is_empty() {
        return;
    }
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let (bw, bh) = (WIDTH * s, HEIGHT * s);
    let bx = (x + ROOM * s).round();
    let by = canvas.top + ((canvas.height - bh) / 2.0).round();
    let tiled = shown == "tiling";
    let (ink, under) = if tiled {
        let fill = Colour {
            a: 0.18,
            ..tokens.accent
        };
        if let Some(path) = rounded(bx, by, bw, bh, RADIUS * s) {
            canvas.pixmap.fill_path(
                &path,
                &paint_of(fill),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        (tokens.accent, mix(tokens.panel, tokens.accent, 0.18))
    } else {
        (mix(tokens.panel_text, tokens.panel, 0.25), tokens.panel)
    };
    // The icon's 24 units, centred on the button.
    let unit = ICON * s / 24.0;
    let at = |ux: f32, uy: f32| {
        (
            bx + bw / 2.0 + (ux - 12.0) * unit,
            by + bh / 2.0 + (uy - 12.0) * unit,
        )
    };
    let rect = |ux: f32, uy: f32, uw: f32, uh: f32| {
        let (px, py) = at(ux, uy);
        rounded(px, py, uw * unit, uh * unit, 1.8 * unit)
    };
    let mut strokes: Vec<Path> = Vec::new();
    if tiled {
        strokes.extend(rect(3.0, 4.5, 18.0, 15.0));
        let mut lines = PathBuilder::new();
        let (a, b) = (at(12.0, 4.5), at(12.0, 19.5));
        lines.move_to(a.0, a.1);
        lines.line_to(b.0, b.1);
        let (c, d) = (at(12.0, 12.0), at(21.0, 12.0));
        lines.move_to(c.0, c.1);
        lines.line_to(d.0, d.1);
        strokes.extend(lines.finish());
    } else {
        strokes.extend(rect(3.0, 4.5, 11.5, 9.5));
        // The front window hides the back one where they overlap.
        if let Some(front) = rect(9.5, 10.0, 11.5, 9.5) {
            canvas.pixmap.fill_path(
                &front,
                &paint_of(under),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
            strokes.push(front);
        }
    }
    let stroke = Stroke {
        width: LINE * unit,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    for path in strokes {
        canvas
            .pixmap
            .stroke_path(&path, &paint_of(ink), &stroke, Transform::identity(), None);
    }
}

/// A click switches the shown workspace's policy.
fn input(shown: &str, input: Input) -> Option<Action> {
    match input {
        Input::Click(..) if !shown.is_empty() => Some(Action::TogglePolicy),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::{Look, Row, fillet_height, paint};
    use edel::presets::Edge;
    use edel::tokens::Tokens;
    use tiny_skia::Pixmap;

    fn drawn(policy: &str) -> (Pixmap, Tokens) {
        let tokens = Tokens::built_in();
        let row = Row {
            start: vec![&WIDGET],
            centre: vec![],
            end: vec![],
        };
        let live = Live {
            policy: policy.into(),
            ..Live::default()
        };
        let look = Look {
            width: 200,
            height: tokens.panel_height + fillet_height(&tokens),
            scale: 1,
            edge: Edge::Bottom,
            fillets: false,
            shown: row.shows(&live),
        };
        let mut pixmap = Pixmap::new(look.width, look.height).unwrap();
        paint(&mut pixmap, &look, &tokens, None, &row);
        (pixmap, tokens)
    }

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pixmap.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn the_button_fills_with_the_accent_only_when_tiled() {
        let (floating, tokens) = drawn("floating");
        // Inside the button, clear of the icon: 3 px in from its left
        // edge, half way down the panel.
        let y = fillet_height(&tokens) + tokens.panel_height / 2;
        let x = ROOM as u32 + 3;
        assert_eq!(pixel(&floating, x, y), tokens.panel.bytes());
        let (tiling, tokens) = drawn("tiling");
        assert_ne!(pixel(&tiling, x, y), tokens.panel.bytes());
        // The icon is drawn in its middle, in both.
        let middle = (ROOM + WIDTH / 2.0) as u32;
        assert_ne!(pixel(&tiling, middle, y), pixel(&tiling, x, y));
        assert_eq!(logical_width(""), 0.0);
        assert_eq!(logical_width("tiling"), WIDTH + 2.0 * ROOM);
    }

    #[test]
    fn a_click_switches_the_policy_once_known() {
        assert_eq!(
            input("floating", Input::Click(10.0, 36.0)),
            Some(Action::TogglePolicy)
        );
        assert_eq!(input("", Input::Click(10.0, 0.0)), None);
        assert_eq!(input("tiling", Input::Scroll(1)), None);
    }
}
