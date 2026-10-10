//! The keyboard layout indicator (roadmap M5.9f): a button showing the
//! layout in use, `EN` or `DE`, shown only when the keyboard has two or
//! more layouts (`region.keyboard`, M5.21). A click asks the compositor
//! for the next one, as Super+Space does (`crate::link`). It is drawn
//! as the layout toggle is: a button `size.panel_control` high with the
//! controls' radius, filled faintly with the panel's text.

use accesskit::Role;
use edel::i18n::{n_, trf};
use edel::tokens::Colour;

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{Face, fill};

pub const WIDGET: Widget = Widget {
    name: "keyboard",
    title: n_("Keyboard layout"),
    needs: None,
    shows,
    width,
    draw,
    input: |_, shown, what| input(shown, what),
    parts: super::no_parts,
    role: Role::Button,
    label,
};

/// The room outside the button, in logical pixels, and the space the
/// text keeps on each side inside it.
const ROOM: f32 = 2.0;
const PAD: f32 = 10.0;

/// The short name of an xkb layout: `EN` for the English layouts, else
/// its first two letters in capitals (`de` gives `DE`, `latam` `LA`). A
/// variant, `us(intl)`, is its layout.
pub fn short(name: &str) -> String {
    let layout = name.split('(').next().unwrap_or_default();
    match layout {
        "us" | "gb" | "au" | "ca" => "EN".to_string(),
        other => other.chars().take(2).collect::<String>().to_uppercase(),
    }
}

/// What it shows: the short name of the layout in use, a tab, then the
/// layouts' names, comma separated, so a change of either redraws it.
/// Nothing with fewer than two layouts.
fn shows(live: &Live) -> String {
    let (layouts, active) = &live.keyboard;
    match layouts.get(*active) {
        Some(name) if layouts.len() >= 2 => format!("{}\t{}", short(name), layouts.join(",")),
        _ => String::new(),
    }
}

/// The short name it shows, if it shows anything.
fn decode(shown: &str) -> Option<&str> {
    shown.split('\t').next().filter(|short| !short.is_empty())
}

/// The button's width in pixels, without its room: the text's width at
/// `size.panel_text_small`, semibold, plus `PAD` on each side, and never
/// less than `size.panel_control`.
fn button(canvas: &mut Canvas, short: &str) -> f32 {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let size = tokens.panel_text_small_size as f32 * s;
    let text = match canvas.text.as_deref_mut() {
        Some(text) => text.line_in(short, size, Face::SEMIBOLD).width,
        None => 0.0,
    };
    (text + 2.0 * PAD * s)
        .max(tokens.panel_control as f32 * s)
        .round()
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    let Some(short) = decode(shown) else {
        return 0.0;
    };
    let room = (ROOM * canvas.scale).round();
    button(canvas, short) + 2.0 * room
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let Some(short) = decode(shown) else {
        return;
    };
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let room = (ROOM * s).round();
    let bw = button(canvas, short);
    let bh = (tokens.panel_control as f32 * s).round();
    let (bx, by) = (
        (x + room).round(),
        canvas.top + ((canvas.height - bh) / 2.0).round(),
    );
    let lit = Colour {
        a: 0.06,
        ..tokens.panel_text
    };
    fill(
        canvas.pixmap,
        bx,
        by,
        bw,
        bh,
        tokens.radius_control as f32 * s,
        lit,
    );
    let size = tokens.panel_text_small_size as f32 * s;
    if let Some(text) = canvas.text.as_deref_mut() {
        let mut line = text.line_in(short, size, Face::SEMIBOLD);
        let (tx, ty) = (
            bx + ((bw - line.width) / 2.0).round(),
            by + ((bh - size * 1.25) / 2.0).round(),
        );
        text.draw(canvas.pixmap, &mut line, tx, ty, tokens.panel_text);
    }
}

/// A click asks for the next layout.
fn input(shown: &str, input: Input) -> Option<Action> {
    match input {
        Input::Click(..) if decode(shown).is_some() => Some(Action::NextKeyboardLayout),
        _ => None,
    }
}

/// `Keyboard layout: DE, click for the next`.
fn label(shown: &str) -> String {
    match decode(shown) {
        Some(short) => trf(
            "Keyboard layout: {name}, click for the next",
            &[("name", short)],
        ),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::tokens::Tokens;
    use tiny_skia::Pixmap;

    fn live(layouts: &[&str], active: usize) -> Live {
        Live {
            keyboard: (layouts.iter().map(|l| l.to_string()).collect(), active),
            ..Live::default()
        }
    }

    #[test]
    fn short_names_the_english_layouts_en_and_the_rest_by_two_letters() {
        assert_eq!(short("us"), "EN");
        assert_eq!(short("us(intl)"), "EN");
        assert_eq!(short("gb"), "EN");
        assert_eq!(short("de"), "DE");
        assert_eq!(short("ru"), "RU");
        assert_eq!(short("latam"), "LA");
    }

    #[test]
    fn it_shows_only_with_two_layouts_and_names_the_one_in_use() {
        assert_eq!(shows(&live(&[], 0)), "");
        assert_eq!(shows(&live(&["de"], 0)), "");
        assert_eq!(shows(&live(&["de", "us"], 0)), "DE\tde,us");
        assert_eq!(shows(&live(&["de", "us"], 1)), "EN\tde,us");
        assert_eq!(shows(&live(&["de", "us"], 5)), "");
    }

    #[test]
    fn its_width_is_nothing_while_it_shows_nothing() {
        let tokens = Tokens::built_in();
        let mut pixmap = Pixmap::new(1, 1).unwrap();
        let mut canvas = Canvas {
            pixmap: &mut pixmap,
            tokens: &tokens,
            text: None,
            icons: None,
            scale: 1.0,
            top: 0.0,
            height: 1.0,
            dock: false,
            along_top: false,
        };
        assert_eq!(width(&mut canvas, ""), 0.0);
        // Without the fonts the text measures nothing, so the button is
        // `size.panel_control` wide, with its room on each side.
        assert_eq!(
            width(&mut canvas, "DE\tde,us"),
            tokens.panel_control as f32 + 2.0 * ROOM
        );
    }

    #[test]
    fn a_click_asks_for_the_next_layout_only_while_it_shows() {
        assert_eq!(
            input("DE\tde,us", Input::Click(5.0, 34.0)),
            Some(Action::NextKeyboardLayout)
        );
        assert_eq!(input("", Input::Click(5.0, 0.0)), None);
        assert_eq!(input("DE\tde,us", Input::Scroll(1)), None);
    }
}
