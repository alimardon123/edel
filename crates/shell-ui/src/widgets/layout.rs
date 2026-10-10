//! The layout toggle (M5.3a): one button that does what Super+T does,
//! whose icon shows how the shown workspace places its windows: two
//! overlapping windows when they float, a screen split in three when they
//! tile, the button then filled with the accent. A click switches the
//! shown workspace over `edel-shell-v1` (`crate::link`), which also says
//! the policy; without it the button is not there. A right click opens
//! the tiling styles' menu (M5.16b, `crate::styles`).

use accesskit::Role;
use edel::i18n::{n_, tr, trf};
use edel::tokens::Tokens;

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{self, fill, lit, mix};

pub const WIDGET: Widget = Widget {
    name: "layout",
    title: n_("Floating or tiling"),
    needs: None,
    shows,
    width,
    draw,
    input: |_, shown, what| input(shown, what),
    parts: super::no_parts,
    role: Role::Button,
    label,
};

/// The button says how windows are placed: "Layout: tiling".
fn label(shown: &str) -> String {
    match shown {
        "tiling" => tr("Layout: tiling").into(),
        "floating" => tr("Layout: floating").into(),
        other => trf("Layout: {policy}", &[("policy", other)]),
    }
}

/// The space on each side of the button, in logical pixels. The button
/// is `size.panel_control` wide and four pixels less high, as the
/// mockups' is (30 by 26), its corners the tokens' controls'.
const ROOM: f32 = 2.0;
const SHORTER: f32 = 4.0;

/// What it shows: the shown workspace's policy, `floating` or `tiling`,
/// or nothing before the compositor has said.
fn shows(live: &Live) -> String {
    live.policy.clone()
}

/// Its width in logical pixels.
pub fn logical_width(shown: &str, tokens: &Tokens) -> f32 {
    if shown.is_empty() {
        0.0
    } else {
        tokens.panel_control as f32 + 2.0 * ROOM
    }
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    logical_width(shown, canvas.tokens) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    if shown.is_empty() {
        return;
    }
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let control = tokens.panel_control as f32;
    let (bw, bh) = ((control * s).round(), ((control - SHORTER) * s).round());
    let bx = (x + ROOM * s).round();
    let by = canvas.top + ((canvas.height - bh) / 2.0).round();
    let tiled = shown == "tiling";
    let ink = if tiled {
        let r = tokens.radius_control as f32 * s;
        fill(canvas.pixmap, bx, by, bw, bh, r, lit(tokens));
        tokens.accent
    } else {
        mix(tokens.panel_text, tokens.panel, 0.25)
    };
    let name = if tiled {
        "layout-tiling"
    } else {
        "layout-floating"
    };
    let px = (tokens.panel_glyph as f32 * s).round();
    let (ix, iy) = (
        bx + ((bw - px) / 2.0).round(),
        by + ((bh - px) / 2.0).round(),
    );
    paint::icon(canvas.pixmap, name, px, ix, iy, ink);
}

/// A click switches the shown workspace's policy; a right click opens the
/// tiling styles' menu (M5.16b).
fn input(shown: &str, input: Input) -> Option<Action> {
    match input {
        Input::Click(..) if !shown.is_empty() => Some(Action::TogglePolicy),
        Input::Menu(..) if !shown.is_empty() => Some(Action::Styles),
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
            style: edel::presets::Style::Bar,
            fillets: false,
            shown: row.shows(&live),
            editing: false,
            lifted: None,
            caret: None,
        };
        let mut pixmap = Pixmap::new(look.width, look.height).unwrap();
        paint(&mut pixmap, &look, &tokens, None, None, &row);
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
        let width = tokens.panel_control as f32;
        assert_eq!(pixel(&floating, x, y), tokens.panel.bytes());
        let (tiling, tokens) = drawn("tiling");
        assert_ne!(pixel(&tiling, x, y), tokens.panel.bytes());
        // The icon is drawn in its middle, in both.
        let middle = (ROOM + width / 2.0) as u32;
        assert_ne!(pixel(&tiling, middle, y), pixel(&tiling, x, y));
        assert_eq!(logical_width("", &tokens), 0.0);
        assert_eq!(logical_width("tiling", &tokens), width + 2.0 * ROOM);
    }

    #[test]
    fn a_click_switches_the_policy_once_known() {
        assert_eq!(
            input("floating", Input::Click(10.0, 36.0)),
            Some(Action::TogglePolicy)
        );
        assert_eq!(
            input("tiling", Input::Menu(10.0, 36.0)),
            Some(Action::Styles)
        );
        assert_eq!(input("", Input::Click(10.0, 0.0)), None);
        assert_eq!(input("tiling", Input::Scroll(1)), None);
    }
}
