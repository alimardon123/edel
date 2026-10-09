//! One notification as it is drawn (M5.9b), by the banner (`banner.rs`)
//! and by each card of the notification centre (`centre.rs`), so the two
//! look alike and a change here reaches both: the app's icon on a tile,
//! the summary in semibold, the body in up to two lines, the actions as
//! small buttons and a close button. Plain data and drawing, tested
//! without a display: [`item`] says what shows (the texts already broken
//! into lines, the buttons measured), [`place`] where it lies (the one
//! function that lays a notification out), [`paint`] draws it, [`hit`]
//! finds what a pointer is over and [`nodes`] are what a screen reader
//! hears. A desktop notification has the mockups' sizes; below Compact's
//! edge every target is at least 44 px (M5.6c).

use accesskit::Role;
use edel::app_icons::Icons;
use edel::i18n::tr;
use edel::tokens::{Colour, Tokens};
use tiny_skia::{FilterQuality, PixmapPaint, Transform};

use crate::a11y::Item as Node;
use crate::notify::Notification;
use crate::paint::{Face, Text, fill, lit, outline};
use crate::popup::{self, Rect, dim, icon_in, veil};

/// The lines a body keeps, here and in the centre.
pub const BODY_LINES: usize = 2;

/// What one notification shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: u32,
    /// The app's name and its icon's name, to find the icon.
    pub app: String,
    pub icon: String,
    pub summary: String,
    /// The body, broken into at most [`BODY_LINES`] lines.
    pub body: Vec<String>,
    /// The buttons, one for each action but `default`.
    pub buttons: Vec<Button>,
    /// Whether the app gave a `default` action, which a click on the body
    /// invokes.
    pub default: bool,
    pub critical: bool,
    /// What a screen reader says of it.
    pub spoken: String,
}

/// One action's button: the key the app is told when it is pressed, the
/// label shown and the label's width without the room round it, logical
/// pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub key: String,
    pub label: String,
    pub width: f32,
}

/// The sizes of a notification's parts, logical pixels: the mockups' on
/// a desktop, touch sizes on a Compact screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// The room inside its edge
    pub pad: f32,
    /// The icon's tile, the room after it
    pub tile: f32,
    pub gap: f32,
    /// The close button's square, and its distance from the corner
    pub close: f32,
    pub edge: f32,
    /// A button's height, the room inside it at each end, and between
    pub button: f32,
    pub button_pad: f32,
    pub button_gap: f32,
    /// A line of the summary, and of the body
    pub summary: f32,
    pub body: f32,
    /// The app's icon on its tile
    pub icon: f32,
}

pub fn metrics(compact: bool) -> Metrics {
    if compact {
        Metrics {
            pad: 14.0,
            tile: 40.0,
            gap: 12.0,
            close: 44.0,
            edge: 4.0,
            button: 44.0,
            button_pad: 16.0,
            button_gap: 8.0,
            summary: 20.0,
            body: 17.0,
            icon: 26.0,
        }
    } else {
        Metrics {
            pad: 12.0,
            tile: 32.0,
            gap: 10.0,
            close: 24.0,
            edge: 8.0,
            button: 28.0,
            button_pad: 12.0,
            button_gap: 8.0,
            summary: 18.0,
            body: 16.0,
            icon: 22.0,
        }
    }
}

/// The size of the summary's text and the body's and buttons', logical
/// pixels, from the tokens.
pub fn sizes(tokens: &Tokens) -> (f32, f32) {
    let text = tokens.panel_text_size as f32;
    (text, text - 1.0)
}

/// How wide the summary and the body may be in a notification `width`
/// wide, logical pixels: the text's column, less the close button's.
pub fn text_room(width: f32, compact: bool) -> f32 {
    let m = metrics(compact);
    let from = m.pad + m.tile + m.gap;
    (width - m.edge - m.close - 4.0 - from).max(40.0)
}

/// `n` as an [`Item`] for a notification `width` wide: the body broken
/// into lines and each button measured, by `measure`, which gives the
/// width of a text in a face at `small` pixels (the body's and buttons'
/// size, `sizes`' second), logical pixels.
pub fn item(
    n: &Notification,
    width: f32,
    compact: bool,
    mut measure: impl FnMut(&str, Face) -> f32,
) -> Item {
    let room = text_room(width, compact);
    let body = popup::wrap(&n.body, BODY_LINES, room, |t| measure(t, Face::REGULAR));
    let buttons = n
        .buttons()
        .map(|(key, label)| Button {
            key: key.clone(),
            label: label.clone(),
            width: measure(label, Face::MEDIUM),
        })
        .collect();
    Item {
        id: n.id,
        app: n.app.clone(),
        icon: n.icon.clone(),
        summary: n.summary.clone(),
        body,
        buttons,
        default: n.has_default(),
        critical: n.urgency == crate::notify::Urgency::Critical,
        spoken: n.spoken(),
    }
}

/// Where a notification's parts lie, logical pixels from the surface's
/// corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub whole: Rect,
    pub tile: Rect,
    pub summary: Rect,
    pub body: Vec<Rect>,
    pub buttons: Vec<Rect>,
    pub close: Rect,
}

/// Lays `item` out in a notification `w` wide whose top left corner is
/// at `x`, `y`: the one function that places a notification's parts.
pub fn place(item: &Item, (x, y, w): (f32, f32, f32), compact: bool) -> Placed {
    let m = metrics(compact);
    let from = x + m.pad + m.tile + m.gap;
    let room = text_room(w, compact);
    let mut at = y + m.pad;
    let summary = Rect::new(from, at, room, m.summary);
    at += m.summary + 2.0;
    let body: Vec<Rect> = (0..item.body.len())
        .map(|i| Rect::new(from, at + i as f32 * m.body, room, m.body))
        .collect();
    at += item.body.len() as f32 * m.body;
    let mut buttons = Vec::new();
    if !item.buttons.is_empty() {
        at += if item.body.is_empty() { 6.0 } else { 8.0 };
        let avail = x + w - m.pad - from;
        let n = item.buttons.len() as f32;
        let cap = ((avail - m.button_gap * (n - 1.0)) / n).max(40.0);
        let mut bx = from;
        for button in &item.buttons {
            let width = (button.width + 2.0 * m.button_pad).min(cap);
            buttons.push(Rect::new(bx, at, width, m.button));
            bx += width + m.button_gap;
        }
        at += m.button;
    }
    let bottom = at.max(y + m.pad + m.tile) + m.pad;
    Placed {
        whole: Rect::new(x, y, w, bottom - y),
        tile: Rect::new(x + m.pad, y + m.pad, m.tile, m.tile),
        summary,
        body,
        buttons,
        close: Rect::new(x + w - m.close - m.edge, y + m.edge, m.close, m.close),
    }
}

/// What a pointer or the keyboard can be on in a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Close,
    /// The rest of it: a click invokes the `default` action
    Body,
    Button(usize),
}

/// The part of a notification at `x`, `y`, from the surface's corner.
pub fn hit(placed: &Placed, x: f32, y: f32) -> Option<Hit> {
    if placed.close.contains(x, y) {
        return Some(Hit::Close);
    }
    if let Some(i) = placed.buttons.iter().position(|b| b.contains(x, y)) {
        return Some(Hit::Button(i));
    }
    placed.whole.contains(x, y).then_some(Hit::Body)
}

/// Draws `item` laid out as `placed` into `pixmap` at scale `s`. `boxed`
/// draws it as a card of its own, as the centre has them; the banner's is
/// the popup's card, which `radius` (the card's) is for the red edge of a
/// critical one. `hover` and `focus` are the parts the pointer and the
/// keyboard are on. Without `text` only the shapes are drawn.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    pixmap: &mut tiny_skia::Pixmap,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    icons: Option<&mut Icons>,
    item: &Item,
    placed: &Placed,
    (hover, focus): (Option<Hit>, Option<Hit>),
    (boxed, radius): (bool, f32),
    compact: bool,
    s: f32,
) {
    let m = metrics(compact);
    let hair = (0.5 * s).max(1.0);
    let (wx, wy, ww, wh) = placed.whole.device(s);
    let r = radius * s;
    if boxed {
        fill(pixmap, wx, wy, ww, wh, r, veil(tokens, 0.055));
        outline(pixmap, (wx, wy, ww, wh), r, hair, tokens.line);
    }
    if hover == Some(Hit::Body) && item.default {
        fill(pixmap, wx, wy, ww, wh, r, veil(tokens, 0.04));
    }
    if item.critical {
        outline(
            pixmap,
            (wx, wy, ww, wh),
            r,
            1.5 * s,
            tokens.title_close_hover,
        );
    }
    // The app's icon on its tile, else the shell's bell in the accent.
    let (tx, ty, tw, th) = placed.tile.device(s);
    fill(
        pixmap,
        tx,
        ty,
        tw,
        th,
        tokens.radius_control as f32 * s,
        lit(tokens),
    );
    let side = (m.icon * s).round() as u32;
    let mut drawn = false;
    if let Some(icons) = icons {
        let candidates = [item.icon.clone(), item.app.to_lowercase().replace(' ', "-")];
        for name in candidates.iter().filter(|name| !name.is_empty()) {
            let Some(icon) = icons.get(name, side) else {
                continue;
            };
            let at = (
                (tx + (tw - side as f32) / 2.0).round() as i32,
                (ty + (th - side as f32) / 2.0).round() as i32,
            );
            let paint = PixmapPaint {
                quality: FilterQuality::Nearest,
                ..PixmapPaint::default()
            };
            pixmap.draw_pixmap(
                at.0,
                at.1,
                icon.as_ref(),
                &paint,
                Transform::identity(),
                None,
            );
            drawn = true;
            break;
        }
    }
    if !drawn {
        icon_in(pixmap, "bell", m.icon * 0.72, placed.tile, s, tokens.accent);
    }
    if let Some(text) = text.as_deref_mut() {
        let (big, small) = sizes(tokens);
        let mut line = text.fit_in(&item.summary, big * s, placed.summary.w * s, Face::SEMIBOLD);
        let y = popup::middle(placed.summary.y, placed.summary.h, big * s, s);
        text.draw(
            pixmap,
            &mut line,
            (placed.summary.x * s).round(),
            y,
            tokens.panel_text,
        );
        for (rect, body) in placed.body.iter().zip(&item.body) {
            let mut line = text.fit(body, small * s, rect.w * s);
            let y = popup::middle(rect.y, rect.h, small * s, s);
            text.draw(pixmap, &mut line, (rect.x * s).round(), y, dim(tokens));
        }
    }
    for (i, (rect, button)) in placed.buttons.iter().zip(&item.buttons).enumerate() {
        let (bx, by, bw, bh) = rect.device(s);
        let rr = tokens.radius_control as f32 * s;
        let first = i == 0;
        let (back, ink) = if first {
            (tokens.accent, tokens.accent_text)
        } else {
            (veil(tokens, 0.085), tokens.panel_text)
        };
        fill(pixmap, bx, by, bw, bh, rr, back);
        if hover == Some(Hit::Button(i)) {
            let shade = if first {
                Colour {
                    a: 0.12,
                    ..tokens.accent_text
                }
            } else {
                veil(tokens, 0.06)
            };
            fill(pixmap, bx, by, bw, bh, rr, shade);
        }
        if let Some(text) = text.as_deref_mut() {
            let (_, small) = sizes(tokens);
            let mut line = text.fit_in(
                &button.label,
                small * s,
                (rect.w - 2.0 * m.button_pad) * s,
                Face::MEDIUM,
            );
            let x = bx + (bw - line.width) / 2.0;
            let y = popup::middle(rect.y, rect.h, small * s, s);
            text.draw(pixmap, &mut line, x, y, ink);
        }
        if focus == Some(Hit::Button(i)) {
            outline(pixmap, (bx, by, bw, bh), rr, 2.0 * s, tokens.accent);
        }
    }
    // The close button: a cross in a square the touch size, drawn small.
    let small_close = Rect::new(
        placed.close.x + (placed.close.w - 24.0).max(0.0) / 2.0,
        placed.close.y + (placed.close.h - 24.0).max(0.0) / 2.0,
        placed.close.w.min(24.0),
        placed.close.h.min(24.0),
    );
    if hover == Some(Hit::Close) {
        let (hx, hy, hw, hh) = small_close.device(s);
        fill(
            pixmap,
            hx,
            hy,
            hw,
            hh,
            tokens.radius_control as f32 * s,
            veil(tokens, 0.085),
        );
    }
    icon_in(pixmap, "close", 12.0, small_close, s, dim(tokens));
    if focus == Some(Hit::Close) {
        let (hx, hy, hw, hh) = small_close.device(s);
        outline(
            pixmap,
            (hx, hy, hw, hh),
            tokens.radius_control as f32 * s,
            2.0 * s,
            tokens.accent,
        );
    }
}

/// What a screen reader finds, flat and in Tab's order: the notification
/// as an alert saying `Notification from APP: SUMMARY, BODY`, then each
/// button and the close button. Places are shifted by `origin`.
pub fn nodes(item: &Item, placed: &Placed, origin: (f64, f64)) -> Vec<Node> {
    let rect = |r: Rect| {
        accesskit::Rect::new(
            origin.0 + f64::from(r.x),
            origin.1 + f64::from(r.y),
            origin.0 + f64::from(r.right()),
            origin.1 + f64::from(r.y + r.h),
        )
    };
    let node = |role, label: String, r: Rect| Node {
        role,
        label,
        bounds: rect(r),
        children: Vec::new(),
        toggled: None,
        value: None,
    };
    let mut out = vec![node(Role::Alert, item.spoken.clone(), placed.whole)];
    for (button, r) in item.buttons.iter().zip(&placed.buttons) {
        out.push(node(Role::Button, button.label.clone(), *r));
    }
    out.push(node(
        Role::Button,
        tr("Close notification").to_string(),
        placed.close,
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::{Call, received};

    /// Six pixels a character, whatever the face.
    fn six(s: &str, _: Face) -> f32 {
        s.chars().count() as f32 * 6.0
    }

    fn note(summary: &str, body: &str, actions: &[&str]) -> Notification {
        received(
            4,
            &Call {
                app: "Mail",
                icon: "mail",
                summary,
                body,
                actions,
                urgency: None,
                timeout: -1,
            },
        )
    }

    #[test]
    fn an_item_has_a_body_in_two_lines_and_a_button_for_each_action() {
        let n = note(
            "New mail",
            "Ali wrote a long message about the plans for next week and more",
            &["default", "", "reply", "Reply", "read", "Mark as read"],
        );
        let i = item(&n, 360.0, false, six);
        assert_eq!(i.body.len(), BODY_LINES);
        assert!(i.default);
        assert!(!i.critical);
        let labels: Vec<(&str, &str, f32)> = i
            .buttons
            .iter()
            .map(|b| (b.key.as_str(), b.label.as_str(), b.width))
            .collect();
        assert_eq!(
            labels,
            [("reply", "Reply", 30.0), ("read", "Mark as read", 72.0)]
        );
        assert_eq!(
            i.spoken,
            "Notification from Mail: New mail, Ali wrote a long message about the plans for next week and more"
        );
        // No body and no actions: a summary alone.
        let bare = item(&note("Saved", "", &[]), 360.0, false, six);
        assert!(bare.body.is_empty() && bare.buttons.is_empty() && !bare.default);
    }

    #[test]
    fn a_desktop_notification_is_laid_out_as_the_mockups_have_it() {
        let n = note(
            "Update ready",
            "It starts on the next restart. If it fails to start you go back",
            &["now", "Restart now", "later", "Later"],
        );
        let i = item(&n, 360.0, false, six);
        let p = place(&i, (0.0, 0.0, 360.0), false);
        let m = metrics(false);
        // The tile at the corner, the text after it, the close button at the other.
        assert_eq!(p.tile, Rect::new(12.0, 12.0, 32.0, 32.0));
        assert_eq!(p.summary.x, 54.0);
        assert_eq!(p.close.right(), 360.0 - m.edge);
        assert!(
            p.summary.right() < p.close.x,
            "the summary stays clear of close"
        );
        // Body lines under the summary, buttons under the body, in a row.
        assert_eq!(p.body.len(), 2);
        assert!(p.body[0].y >= p.summary.y + p.summary.h);
        assert!(p.buttons[0].y >= p.body[1].y + p.body[1].h);
        assert_eq!(p.buttons[0].x, p.summary.x);
        assert_eq!(p.buttons[1].x, p.buttons[0].right() + m.button_gap);
        assert_eq!(p.buttons[0].h, 28.0);
        // The card ends a pad below the buttons; nothing sticks out.
        assert_eq!(p.whole.h, p.buttons[0].y + 28.0 + m.pad);
        for r in p
            .body
            .iter()
            .chain(&p.buttons)
            .chain([&p.summary, &p.tile, &p.close])
        {
            assert!(r.right() <= 360.0 && r.y + r.h <= p.whole.h, "{r:?}");
        }
        // Without body or buttons the tile sets the height.
        let bare = place(
            &item(&note("Saved", "", &[]), 360.0, false, six),
            (0.0, 0.0, 360.0),
            false,
        );
        assert_eq!(bare.whole.h, 12.0 + 32.0 + 12.0);
    }

    #[test]
    fn at_compact_width_the_targets_are_44_px() {
        let n = note("Update ready", "Restart to finish", &["now", "Restart now"]);
        let i = item(&n, 360.0, true, six);
        let p = place(&i, (0.0, 0.0, 360.0), true);
        assert_eq!(p.buttons[0].h, 44.0);
        assert_eq!((p.close.w, p.close.h), (44.0, 44.0));
        assert!(p.close.right() <= 360.0 && p.close.x >= 0.0);
        // The sheet takes the screen's width; text keeps clear of close.
        assert_eq!(p.whole.w, 360.0);
        assert!(p.summary.right() < p.close.x);
        assert!(p.body.iter().all(|b| b.right() < p.close.x));
        let m = metrics(true);
        assert!(m.button >= 44.0 && m.close >= 44.0);
    }

    #[test]
    fn many_wide_buttons_shrink_to_fit_the_row() {
        let n = note(
            "x",
            "",
            &[
                "a",
                "Aaaaaaaaaaaaaaaaaaaaaaaa",
                "b",
                "Bbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "c",
                "Cccccccccccccccccccccc",
            ],
        );
        let i = item(&n, 360.0, false, six);
        let p = place(&i, (0.0, 0.0, 360.0), false);
        assert_eq!(p.buttons.len(), 3);
        for b in &p.buttons {
            assert!(b.right() <= 360.0 - metrics(false).pad + 0.01, "{b:?}");
        }
    }

    #[test]
    fn a_pointer_finds_close_a_button_or_the_body() {
        let n = note("Update", "Restart", &["now", "Restart now"]);
        let i = item(&n, 360.0, false, six);
        let p = place(&i, (0.0, 0.0, 360.0), false);
        let c = p.close;
        assert_eq!(hit(&p, c.x + 2.0, c.y + 2.0), Some(Hit::Close));
        let b = p.buttons[0];
        assert_eq!(hit(&p, b.x + 2.0, b.y + 2.0), Some(Hit::Button(0)));
        assert_eq!(hit(&p, 20.0, 20.0), Some(Hit::Body));
        assert_eq!(hit(&p, 400.0, 20.0), None);
        assert_eq!(hit(&p, 20.0, p.whole.h + 5.0), None);
    }

    #[test]
    fn a_screen_reader_hears_the_notification_then_its_buttons() {
        let n = note("Update", "Restart", &["now", "Restart now"]);
        let i = item(&n, 360.0, false, six);
        let p = place(&i, (0.0, 0.0, 360.0), false);
        let all = nodes(&i, &p, (0.0, 0.0));
        let said: Vec<(Role, &str)> = all.iter().map(|n| (n.role, n.label.as_str())).collect();
        assert_eq!(
            said,
            [
                (Role::Alert, "Notification from Mail: Update, Restart"),
                (Role::Button, "Restart now"),
                (Role::Button, "Close notification"),
            ]
        );
        // Shifted by the card's place on its surface.
        let shifted = nodes(&i, &p, (10.0, 20.0));
        assert_eq!(shifted[0].bounds.x0, 10.0);
        assert_eq!(shifted[0].bounds.y0, 20.0);
    }

    #[test]
    fn it_draws_a_tile_buttons_and_a_cross_in_the_tokens_colours() {
        let tokens = Tokens::built_in_scheme(edel::tokens::Scheme::Light);
        let n = note(
            "Update",
            "Restart",
            &["now", "Restart now", "later", "Later"],
        );
        let i = item(&n, 360.0, false, six);
        let p = place(&i, (0.0, 0.0, 360.0), false);
        let mut pixmap = tiny_skia::Pixmap::new(360 * 2, p.whole.h.ceil() as u32 * 2).unwrap();
        popup::card(&mut pixmap, &tokens, 2.0);
        paint(
            &mut pixmap,
            &tokens,
            None,
            None,
            &i,
            &p,
            (Some(Hit::Button(1)), None),
            (false, tokens.radius_menu as f32),
            false,
            2.0,
        );
        let at = |r: Rect, dx: f32, dy: f32| {
            let c = pixmap
                .pixel(((r.x + dx) * 2.0) as u32, ((r.y + dy) * 2.0) as u32)
                .unwrap()
                .demultiply();
            [c.red(), c.green(), c.blue()]
        };
        // The first button is the accent; the second only veils the card.
        assert_eq!(at(p.buttons[0], 3.0, 3.0), tokens.accent.bytes()[..3]);
        assert_ne!(at(p.buttons[1], 3.0, 3.0), tokens.accent.bytes()[..3]);
        assert_ne!(
            at(p.buttons[1], 3.0, 3.0),
            tokens.panel.bytes()[..3],
            "hovered, so lit"
        );
        // The tile is tinted, not the card's colour; a bell sits on it.
        assert_ne!(at(p.tile, 3.0, 3.0), tokens.panel.bytes()[..3]);
    }
}
