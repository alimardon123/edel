//! Done's Undo bar (M5.31b, part 2b): after Done writes a change to the
//! panels, a small card where the drawer was, "Panels changed" and an Undo
//! pill, for `SECONDS` (10 s). Plain data and drawing, tested without a
//! display; `editor_card.rs` owns the surface, its timer and what Undo
//! does, and draws the pill as the drawer's Undo button is drawn.

use accesskit::Role;
use edel::i18n::tr;
use edel::tokens::Tokens;
use tiny_skia::Pixmap;

use crate::a11y::Item;
use crate::paint::{Text, fill};
use crate::popup::{self, Card, Rect, raised, veil};
use crate::quick;

/// How long the bar stays after Done, in seconds.
pub const SECONDS: u64 = 10;
/// The room round the text and the pill, and between them, logical pixels.
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
/// The room for "Panels changed", logical pixels; a longer text is cut
/// short with an ellipsis.
const TEXT: f32 = 128.0;
/// The Undo pill as wide as the drawer's, and 36 px high; on a Compact
/// screen 44 px high, where a finger lands.
const PILL_WIDTH: f32 = 88.0;
const PILL: f32 = 36.0;
const PILL_TOUCH: f32 = 44.0;

/// What the bar shows: whether the pointer is on Undo, and the screen's
/// width in logical pixels, 0 when unknown (a Compact screen's touch sizes).
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub hover: bool,
    pub width: u32,
}

/// Where the bar's parts lie, logical pixels from its top left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub size: (u32, u32),
    pub card: Rect,
    pub text: Rect,
    pub undo: Rect,
}

/// Whether the screen is narrower than quick settings' Compact width.
fn compact(view: &View) -> bool {
    view.width > 0 && view.width < quick::COMPACT_BELOW
}

/// The bar's size and where its parts lie: the text on the left, the pill
/// on the right, both centred in the card's padding.
pub fn layout(view: &View) -> Layout {
    let pill = if compact(view) { PILL_TOUCH } else { PILL };
    let (w, h) = (PAD + TEXT + GAP + PILL_WIDTH + PAD, pill + 2.0 * PAD);
    Layout {
        size: (w.ceil() as u32, h.ceil() as u32),
        card: Rect::new(0.0, 0.0, w, h),
        text: Rect::new(PAD, PAD, TEXT, pill),
        undo: Rect::new(PAD + TEXT + GAP, PAD, PILL_WIDTH, pill),
    }
}

/// The card, with the menus' corners.
pub fn cards(l: &Layout, tokens: &Tokens) -> Vec<Card> {
    vec![Card {
        rect: l.card,
        radius: tokens.radius_menu as f32,
    }]
}

/// Whether `x`, `y` (logical pixels from the card's corner) is on Undo.
pub fn hit(l: &Layout, x: f32, y: f32) -> bool {
    l.undo.contains(x, y)
}

/// Where the parts lie, as one log line CI reads:
/// `card WxH, undo X+Y+WxH`, logical pixels from the card's corner.
pub fn places(l: &Layout) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    format!("card {}x{}, undo {}", l.size.0, l.size.1, at(l.undo))
}

/// What a screen reader finds: the text, then the Undo button.
pub fn nodes(l: &Layout) -> Vec<Item> {
    let at = |r: Rect| {
        accesskit::Rect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.y + r.h),
        )
    };
    let item = |role, label: &str, bounds| Item {
        role,
        label: label.to_string(),
        bounds,
        children: Vec::new(),
        toggled: None,
        value: None,
    };
    vec![
        item(Role::Label, tr("Panels changed"), at(l.text)),
        item(Role::Button, tr("Undo"), at(l.undo)),
    ]
}

/// Draws `view` at `s` into `pixmap`, which is the bar's size times it: the
/// card, the text in the panel's text colour, and the pill raised with its
/// word, veiled a little under the pointer. Without `text`, the shapes only.
pub fn paint(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, text: Option<&mut Text>, s: f32) {
    let l = layout(view);
    popup::cards(pixmap, tokens, s, &cards(&l, tokens));
    let size = tokens.panel_text_size as f32 * s;
    let mut text = text;
    if let Some(text) = text.as_deref_mut() {
        let mut line = text.fit(tr("Panels changed"), size, l.text.w * s);
        let y = popup::middle(l.text.y, l.text.h, size, s);
        text.draw(pixmap, &mut line, l.text.x * s, y, tokens.panel_text);
    }
    let (x, y, w, h) = l.undo.device(s);
    let radius = tokens.radius_control as f32 * s;
    fill(pixmap, x, y, w, h, radius, raised(tokens));
    if view.hover {
        fill(pixmap, x, y, w, h, radius, veil(tokens, 0.06));
    }
    if let Some(text) = text {
        let mut line = text.line(tr("Undo"), size);
        let at = x + ((w - line.width) / 2.0).round();
        let baseline = popup::middle(l.undo.y, l.undo.h, size, s);
        text.draw(pixmap, &mut line, at, baseline, tokens.panel_text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::tokens::Scheme;

    fn view(width: u32) -> View {
        View {
            hover: false,
            width,
        }
    }

    #[test]
    fn the_bar_is_the_text_and_a_pill_in_a_card_of_the_menus_padding() {
        let l = layout(&view(1280));
        assert_eq!(l.size, (248, 60));
        assert_eq!(l.undo, Rect::new(148.0, 12.0, 88.0, 36.0));
        let touch = layout(&view(360));
        assert_eq!(touch.size, (248, 68), "a Compact pill is 44 px high");
        assert_eq!(touch.undo.h, 44.0);
    }

    #[test]
    fn hit_finds_the_pill_and_nothing_else() {
        let l = layout(&view(1280));
        assert!(hit(&l, 150.0, 14.0));
        assert!(!hit(&l, 20.0, 30.0), "the text");
        assert!(!hit(&l, 2.0, 2.0), "the padding");
    }

    #[test]
    fn places_name_the_pill_in_the_log_line_ci_reads() {
        assert_eq!(
            places(&layout(&view(1280))),
            "card 248x60, undo 148+12+88x36"
        );
    }

    #[test]
    fn the_nodes_are_a_label_and_an_undo_button() {
        let nodes = nodes(&layout(&view(1280)));
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].role, Role::Label);
        assert_eq!(nodes[1].role, Role::Button);
        assert_eq!(nodes[1].label, "Undo");
    }

    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    #[test]
    fn it_draws_light_and_dark_and_writes_pngs() {
        let Some(mut text) = fonts() else {
            return; // no fonts on this machine
        };
        for (scheme, mode) in [(Scheme::Light, "light"), (Scheme::Dark, "dark")] {
            let tokens = Tokens::built_in_scheme(scheme);
            let v = View {
                hover: true,
                width: 1280,
            };
            let l = layout(&v);
            let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
            paint(&mut pixmap, &v, &tokens, Some(&mut text), 2.0);
            let alpha = |x: f32, y: f32| {
                pixmap
                    .pixel((x * 2.0) as u32, (y * 2.0) as u32)
                    .unwrap()
                    .alpha()
            };
            assert_eq!(alpha(0.0, 0.0), 0, "{mode}: a round corner");
            assert_ne!(alpha(l.card.w / 2.0, 3.0), 0, "{mode}: the card");
            if let Some(dir) = std::env::var_os("EDEL_EDITOR_PNG") {
                let dir = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&dir).unwrap();
                pixmap
                    .save_png(dir.join(format!("undo-bar-{mode}.png")))
                    .unwrap();
            }
        }
    }
}
