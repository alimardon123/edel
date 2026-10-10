//! The banner (M5.9b): the card a new notification shows in the panel's
//! corner for a few seconds. It is one notification, drawn as
//! `notice.rs` draws every notification, on a card of its own with
//! [`RADIUS`] corners: its head row with the app and the close button,
//! the summary, the body in up to two lines, the actions as pill buttons.
//! [`layout`] is the one function that says how big the card is and where
//! its parts lie, so a redesign of the banner replaces it alone. On a
//! Compact screen the card is as wide as the screen and every target is
//! at least 44 px. Plain data and drawing, tested without a display;
//! `notify_card.rs` owns the surface, which exists only while a banner
//! shows, and gives it [`card`] as its one card.

use edel::app_icons::Icons;
use edel::tokens::Tokens;
use tiny_skia::Pixmap;

use crate::a11y::Item as Node;
use crate::notice::{self, Hit, Placed};
use crate::notify::Notification;
use crate::paint::{Face, Text};
use crate::popup::{self, Card, Rect};

/// The card's width on a screen of any size from Compact up, logical
/// pixels; below `COMPACT_BELOW` it is as wide as the screen.
pub const WIDTH: u32 = 360;
/// A screen narrower than this is Compact (M5.6c's size classes), as
/// quick settings count it.
pub const COMPACT_BELOW: u32 = crate::quick::COMPACT_BELOW;
/// The card's corner radius, logical pixels.
pub const RADIUS: f32 = 22.0;

/// The card's width and whether it is a sheet, for a screen `screen`
/// logical pixels wide (0 when not yet known: the desktop's card).
pub fn width_for(screen: u32) -> (u32, bool) {
    if screen > 0 && screen < COMPACT_BELOW {
        (screen, true)
    } else {
        (WIDTH, false)
    }
}

/// What the banner shows: one notification and where the pointer is.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub width: u32,
    pub compact: bool,
    pub item: notice::Item,
    pub hover: Option<Hit>,
}

/// The banner for `n` on a screen `screen` wide; `measure` gives the
/// width of a text in a face at the body's size (see `notice::item`).
pub fn view(n: &Notification, screen: u32, measure: impl FnMut(&str, Face) -> f32) -> View {
    let (width, compact) = width_for(screen);
    View {
        width,
        compact,
        item: notice::item(n, width as f32, compact, measure),
        hover: None,
    }
}

/// Where the banner's parts lie.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The card's size, logical pixels.
    pub size: (u32, u32),
    pub notice: Placed,
}

/// The card's size and where each part of its notification lies: the
/// notification as wide as the card, from its top left corner.
pub fn layout(view: &View) -> Layout {
    let notice = notice::place(&view.item, (0.0, 0.0, view.width as f32), view.compact);
    Layout {
        size: (view.width, notice.whole.h.ceil() as u32),
        notice,
    }
}

/// The card a banner holds, as `notify_card.rs` gives its popup.
pub fn card(layout: &Layout) -> Card {
    Card {
        rect: Rect::new(0.0, 0.0, layout.size.0 as f32, layout.size.1 as f32),
        radius: RADIUS,
    }
}

/// The part of the banner at `x`, `y`, logical pixels from the card's
/// corner.
pub fn hit(layout: &Layout, x: f32, y: f32) -> Option<Hit> {
    notice::hit(&layout.notice, x, y)
}

/// Where the card's parts lie, as one log line CI reads:
/// `card WxH, close X+Y+WxH, button N X+Y+WxH, ...`, logical pixels from
/// the card's corner.
pub fn places(layout: &Layout) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut parts = vec![
        format!("card {}x{}", layout.size.0, layout.size.1),
        format!("close {}", at(layout.notice.close)),
    ];
    for (i, b) in layout.notice.buttons.iter().enumerate() {
        parts.push(format!("button{i} {}", at(*b)));
    }
    parts.join(", ")
}

/// What a screen reader finds, in Tab's order: see `notice::nodes`.
pub fn nodes(view: &View, layout: &Layout) -> Vec<Node> {
    notice::nodes(&view.item, &layout.notice, (0.0, 0.0))
}

/// Draws `view` at `scale` into `pixmap`, which is the card's size times
/// it: the card in the menus' colour, then the notification on it; without
/// `text` everything but the words.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    text: Option<&mut Text>,
    icons: Option<&mut Icons>,
    scale: f32,
) {
    let l = layout(view);
    popup::cards(pixmap, tokens, scale, &[card(&l)]);
    notice::paint(
        pixmap,
        tokens,
        text,
        icons,
        &view.item,
        &l.notice,
        (view.hover, None),
        (false, RADIUS),
        view.compact,
        scale,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::{Call, received};

    fn six(s: &str, _: Face) -> f32 {
        s.chars().count() as f32 * 6.0
    }

    fn update() -> Notification {
        received(
            9,
            &Call {
                app: "Edel OS",
                icon: "",
                summary: "Update ready",
                body: "Edel OS 2026.11 starts on the next restart. If it fails to start, your computer goes back to this version by itself.",
                actions: &["now", "Restart now", "later", "Later"],
                urgency: None,
                timeout: -1,
            },
        )
    }

    #[test]
    fn the_card_is_360_wide_on_a_desktop_and_the_screens_width_when_compact() {
        assert_eq!(width_for(1280), (360, false));
        assert_eq!(width_for(600), (360, false));
        assert_eq!(width_for(599), (599, true));
        assert_eq!(width_for(360), (360, true));
        assert_eq!(width_for(0), (360, false), "before a panel says");
    }

    #[test]
    fn at_1280_the_card_holds_every_part_and_nothing_sticks_out() {
        let v = view(&update(), 1280, six);
        let l = layout(&v);
        assert_eq!(l.size.0, 360);
        assert_eq!(l.size.1, l.notice.whole.h.ceil() as u32);
        let n = &l.notice;
        assert_eq!(n.body.len(), 2);
        assert_eq!(n.buttons.len(), 2);
        for r in n
            .body
            .iter()
            .chain(&n.buttons)
            .chain([&n.summary, &n.tile, &n.close, &n.app])
        {
            assert!(r.x >= 0.0 && r.right() <= 360.0, "{r:?}");
            assert!(r.y >= 0.0 && r.y + r.h <= l.size.1 as f32, "{r:?}");
        }
        assert_eq!(n.buttons[0].h, 32.0);
        // A click on its parts is told apart.
        assert_eq!(hit(&l, n.close.x + 1.0, n.close.y + 1.0), Some(Hit::Close));
        assert_eq!(
            hit(&l, n.buttons[1].x + 1.0, n.buttons[1].y + 1.0),
            Some(Hit::Button(1))
        );
        assert_eq!(hit(&l, 100.0, 15.0), Some(Hit::Body));
    }

    #[test]
    fn the_card_is_the_banner_with_its_corners() {
        let l = layout(&view(&update(), 1280, six));
        let c = card(&l);
        assert_eq!(c.radius, 22.0);
        assert_eq!(c.rect, Rect::new(0.0, 0.0, 360.0, l.size.1 as f32));
    }

    #[test]
    fn at_compact_width_it_spans_the_screen_with_44_px_targets() {
        let v = view(&update(), 360, six);
        assert!(v.compact);
        let l = layout(&v);
        assert_eq!(l.size.0, 360);
        let n = &l.notice;
        assert!(n.buttons.iter().all(|b| b.h >= 44.0));
        assert!(n.close.w >= 44.0 && n.close.h >= 44.0);
        for r in n
            .body
            .iter()
            .chain(&n.buttons)
            .chain([&n.summary, &n.close])
        {
            assert!(r.x >= 0.0 && r.right() <= 360.0, "{r:?}");
        }
        // Taller than the desktop's, for the bigger targets.
        let desktop = layout(&view(&update(), 1280, six));
        assert!(l.size.1 > desktop.size.1);
    }

    #[test]
    fn its_places_are_one_line_for_ci() {
        let l = layout(&view(&update(), 1280, six));
        let line = places(&l);
        assert!(
            line.starts_with(&format!(
                "card 360x{}, close 324+11+22x22, button0 ",
                l.size.1
            )),
            "{line}"
        );
        assert!(line.contains("button1 "), "{line}");
    }

    #[test]
    fn a_screen_reader_hears_the_notification_from_its_app() {
        let v = view(&update(), 1280, six);
        let nodes = nodes(&v, &layout(&v));
        assert_eq!(nodes[0].role, accesskit::Role::Alert);
        assert!(
            nodes[0]
                .label
                .starts_with("Notification from Edel OS: Update ready, Edel OS 2026.11")
        );
        assert_eq!(nodes.len(), 4, "the notification, two buttons and close");
    }

    #[test]
    fn it_draws_light_and_dark_at_both_scales() {
        for scheme in [edel::tokens::Scheme::Light, edel::tokens::Scheme::Dark] {
            let tokens = Tokens::built_in_scheme(scheme);
            for (screen, scale) in [(1280, 1.0), (1280, 2.0), (360, 2.0)] {
                let v = view(&update(), screen, six);
                let l = layout(&v);
                let mut pixmap = Pixmap::new(
                    (l.size.0 as f32 * scale) as u32,
                    (l.size.1 as f32 * scale) as u32,
                )
                .unwrap();
                paint(&mut pixmap, &v, &tokens, None, None, scale);
                let corner = pixmap.pixel(0, 0).unwrap();
                assert_eq!(corner.alpha(), 0, "a round corner");
                let middle = pixmap
                    .pixel(pixmap.width() / 2, pixmap.height() - 2)
                    .unwrap()
                    .demultiply();
                assert_eq!(
                    [middle.red(), middle.green(), middle.blue()],
                    tokens.panel.bytes()[..3],
                    "the menus' card under the parts"
                );
            }
        }
    }
}
