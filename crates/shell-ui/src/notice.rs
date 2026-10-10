//! One notification as it is drawn (M5.9b), by the banner (`banner.rs`)
//! and by each card of the notification centre (`centre.rs`), so the two
//! look alike and a change here reaches both: a head row with the app's
//! icon on a tile, its name and a close button; the summary in semibold;
//! the body in up to two lines; the actions as pill buttons that share
//! the width. Plain data and drawing, tested without a display: [`item`]
//! says what shows (the texts broken into lines, the buttons' labels),
//! [`place`] where it lies (the one function that lays a notification
//! out), [`paint`] draws it, [`hit`] finds what a pointer is over and
//! [`nodes`] are what a screen reader hears. The close button is drawn
//! only while the pointer or the keyboard is on the notification, and
//! always on a Compact screen, but it takes clicks either way. A desktop
//! notification has the mockups' sizes; on a Compact screen every target
//! is at least 44 px (M5.6c).

use accesskit::Role;
use edel::app_icons::Icons;
use edel::i18n::tr;
use edel::tokens::{Colour, Tokens};
use tiny_skia::{FilterQuality, PixmapPaint, Transform};

use crate::a11y::Item as Node;
use crate::notify::Notification;
use crate::paint::{Face, Text, fill, mix, outline};
use crate::popup::{self, Rect, dim, icon_in, raised, veil};

/// The lines a body keeps, here and in the centre.
pub const BODY_LINES: usize = 2;

/// The corner radius of a notification drawn as a card of its own (the
/// centre's), logical pixels.
pub const CARD_RADIUS: f32 = 20.0;

/// The corner radius of the app's icon tile, logical pixels.
const TILE_RADIUS: f32 = 5.0;

/// The round of the close button's circle, logical pixels: it is drawn
/// this size inside its touch-sized square.
const CLOSE_CIRCLE: f32 = 22.0;

/// The room a button's label keeps inside its button at each side,
/// logical pixels.
const BUTTON_ROOM: f32 = 6.0;

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

/// One action's button: the key the app is told when it is pressed and
/// the label shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub key: String,
    pub label: String,
}

/// The sizes of a notification's parts, logical pixels: the mockups' on
/// a desktop, touch sizes on a Compact screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// The room at each side of the text and the tile, and the room
    /// above the head row and below the last part
    pub pad_x: f32,
    pub pad_y: f32,
    /// The head row: the tile, the app's name and the close button
    pub head: f32,
    pub tile: f32,
    /// The app's icon on its tile
    pub icon: f32,
    /// The app's name after the tile
    pub name_gap: f32,
    /// The close button's square, at the head row's right end
    pub close: f32,
    /// Between the head row and the summary
    pub summary_gap: f32,
    /// A line of the summary, and of the body (its line height)
    pub summary: f32,
    pub body: f32,
    /// Between the summary and the body
    pub body_gap: f32,
    /// Between the text and the buttons
    pub buttons_gap: f32,
    /// A button's height, and the room between buttons
    pub button: f32,
    pub button_gap: f32,
}

pub fn metrics(compact: bool) -> Metrics {
    if compact {
        Metrics {
            pad_x: 14.0,
            pad_y: 14.0,
            head: 44.0,
            tile: 28.0,
            icon: 24.0,
            name_gap: 8.0,
            close: 44.0,
            summary_gap: 10.0,
            summary: 22.0,
            body: 20.0,
            body_gap: 2.0,
            buttons_gap: 12.0,
            button: 44.0,
            button_gap: 8.0,
        }
    } else {
        Metrics {
            pad_x: 14.0,
            pad_y: 12.0,
            head: 20.0,
            tile: 20.0,
            icon: 20.0,
            name_gap: 6.0,
            close: 22.0,
            summary_gap: 7.0,
            summary: 18.0,
            body: 17.5,
            body_gap: 2.0,
            buttons_gap: 10.0,
            button: 32.0,
            button_gap: 8.0,
        }
    }
}

/// The sizes of the summary, the body, the buttons' labels and the app's
/// name, logical pixels, from the tokens' panel text.
pub fn sizes(tokens: &Tokens) -> (f32, f32, f32, f32) {
    let text = tokens.panel_text_size as f32;
    (text + 0.5, text - 0.5, text - 1.0, text - 1.5)
}

/// How wide the summary and the body may be in a notification `width`
/// wide, logical pixels: the card's width less its padding at each side.
pub fn text_room(width: f32, compact: bool) -> f32 {
    (width - 2.0 * metrics(compact).pad_x).max(40.0)
}

/// `n` as an [`Item`] for a notification `width` wide: the body broken
/// into lines, by `measure`, which gives the width of a text in a face at
/// the body's size, logical pixels (see `sizes`).
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
    /// The app's name, in the head row between the tile and the close
    /// button
    pub app: Rect,
    pub summary: Rect,
    pub body: Vec<Rect>,
    pub buttons: Vec<Rect>,
    pub close: Rect,
}

/// Lays `item` out in a notification `w` wide whose top left corner is
/// at `x`, `y`: the one function that places a notification's parts.
/// The head row holds the tile, the name and the close button at the
/// right end; the summary, the body and the buttons take the full text
/// width, the buttons sharing it equally.
pub fn place(item: &Item, (x, y, w): (f32, f32, f32), compact: bool) -> Placed {
    let m = metrics(compact);
    let left = x + m.pad_x;
    let text_w = w - 2.0 * m.pad_x;
    let head = y + m.pad_y;
    let close = Rect::new(
        x + w - m.pad_x - m.close,
        head + (m.head - m.close) / 2.0,
        m.close,
        m.close,
    );
    let tile = Rect::new(left, head + (m.head - m.tile) / 2.0, m.tile, m.tile);
    let name_x = left + m.tile + m.name_gap;
    let app = Rect::new(name_x, head, (close.x - 8.0 - name_x).max(0.0), m.head);
    let mut at = head + m.head + m.summary_gap;
    let summary = Rect::new(left, at, text_w, m.summary);
    at += m.summary;
    let body: Vec<Rect> = (0..item.body.len())
        .map(|i| Rect::new(left, at + m.body_gap + i as f32 * m.body, text_w, m.body))
        .collect();
    if !item.body.is_empty() {
        at += m.body_gap + item.body.len() as f32 * m.body;
    }
    let mut buttons = Vec::new();
    if !item.buttons.is_empty() {
        at += m.buttons_gap;
        let n = item.buttons.len() as f32;
        let each = (text_w - m.button_gap * (n - 1.0)) / n;
        for i in 0..item.buttons.len() {
            let bx = left + i as f32 * (each + m.button_gap);
            buttons.push(Rect::new(bx, at, each, m.button));
        }
        at += m.button;
    }
    let bottom = at + m.pad_y;
    Placed {
        whole: Rect::new(x, y, w, bottom - y),
        tile,
        app,
        summary,
        body,
        buttons,
        close,
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
/// draws it as a card of its own, as the centre has them, with `radius`
/// its corners; the banner's is the popup's card, drawn by the banner
/// with `radius` (the card's) for the red edge of a critical one. `hover`
/// and `focus` are the parts the pointer and the keyboard are on. Without
/// `text` only the shapes are drawn.
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
        fill(pixmap, wx, wy, ww, wh, r, raised(tokens));
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
    // The app's icon on its tile; without one, the tile in the accent
    // with the app's first letter.
    let (tx, ty, tw, th) = placed.tile.device(s);
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
        fill(pixmap, tx, ty, tw, th, TILE_RADIUS * s, tokens.accent);
    }
    let (summary_px, body_px, button_px, app_px) = sizes(tokens);
    if let Some(text) = text.as_deref_mut() {
        if !drawn {
            if let Some(letter) = item.app.chars().next() {
                let letter = letter.to_uppercase().to_string();
                let mut line = text.line_in(&letter, 10.0 * s, Face::SEMIBOLD);
                let x = tx + (tw - line.width) / 2.0;
                let y = popup::middle(placed.tile.y, placed.tile.h, 10.0 * s, s);
                text.draw(pixmap, &mut line, x, y, tokens.accent_text);
            }
        }
        if !item.app.is_empty() {
            let mut line = text.fit_in(&item.app, app_px * s, placed.app.w * s, Face::SEMIBOLD);
            let y = popup::middle(placed.app.y, placed.app.h, app_px * s, s);
            text.draw(
                pixmap,
                &mut line,
                (placed.app.x * s).round(),
                y,
                dim(tokens),
            );
        }
        let mut line = text.fit_in(
            &item.summary,
            summary_px * s,
            placed.summary.w * s,
            Face::SEMIBOLD,
        );
        let y = popup::middle(placed.summary.y, placed.summary.h, summary_px * s, s);
        text.draw(
            pixmap,
            &mut line,
            (placed.summary.x * s).round(),
            y,
            tokens.panel_text,
        );
        let soft = mix(tokens.panel_text, tokens.panel, 0.2);
        for (rect, body) in placed.body.iter().zip(&item.body) {
            let mut line = text.fit(body, body_px * s, rect.w * s);
            let y = popup::middle(rect.y, rect.h, body_px * s, s);
            text.draw(pixmap, &mut line, (rect.x * s).round(), y, soft);
        }
    }
    for (i, (rect, button)) in placed.buttons.iter().zip(&item.buttons).enumerate() {
        let (bx, by, bw, bh) = rect.device(s);
        let pill = bh / 2.0;
        let first = i == 0;
        let (back, ink) = if first {
            (tokens.accent, tokens.accent_text)
        } else {
            (veil(tokens, 0.06), tokens.panel_text)
        };
        fill(pixmap, bx, by, bw, bh, pill, back);
        if hover == Some(Hit::Button(i)) {
            let shade = if first {
                Colour {
                    a: 0.12,
                    ..tokens.accent_text
                }
            } else {
                veil(tokens, 0.06)
            };
            fill(pixmap, bx, by, bw, bh, pill, shade);
        }
        if let Some(text) = text.as_deref_mut() {
            let mut line = text.fit_in(
                &button.label,
                button_px * s,
                (rect.w - 2.0 * BUTTON_ROOM) * s,
                Face::SEMIBOLD,
            );
            let x = bx + (bw - line.width) / 2.0;
            let y = popup::middle(rect.y, rect.h, button_px * s, s);
            text.draw(pixmap, &mut line, x, y, ink);
        }
        if focus == Some(Hit::Button(i)) {
            outline(pixmap, (bx, by, bw, bh), pill, 2.0 * s, tokens.accent);
        }
    }
    // The close button: a round veil with a cross, shown while the
    // notification is hovered or focused, and always on Compact.
    let shown = hover.is_some() || focus.is_some() || compact;
    let circle = Rect::new(
        placed.close.x + (placed.close.w - CLOSE_CIRCLE) / 2.0,
        placed.close.y + (placed.close.h - CLOSE_CIRCLE) / 2.0,
        CLOSE_CIRCLE,
        CLOSE_CIRCLE,
    );
    let (cx, cy, cw, ch) = circle.device(s);
    if shown {
        fill(pixmap, cx, cy, cw, ch, cw / 2.0, veil(tokens, 0.08));
        icon_in(pixmap, "close", 10.0, circle, s, dim(tokens));
    }
    if focus == Some(Hit::Close) {
        outline(pixmap, (cx, cy, cw, ch), cw / 2.0, 2.0 * s, tokens.accent);
    }
    if focus == Some(Hit::Body) {
        outline(pixmap, (wx, wy, ww, wh), r, 2.0 * s, tokens.accent);
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
        let labels: Vec<(&str, &str)> = i
            .buttons
            .iter()
            .map(|b| (b.key.as_str(), b.label.as_str()))
            .collect();
        assert_eq!(labels, [("reply", "Reply"), ("read", "Mark as read")]);
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
        // The head row: the tile at the corner, the name after it, the
        // close button at the right end, vertically centred on the row.
        assert_eq!(p.tile, Rect::new(14.0, 12.0, 20.0, 20.0));
        assert_eq!(p.app.x, 14.0 + 20.0 + 6.0);
        assert_eq!(p.close, Rect::new(360.0 - 14.0 - 22.0, 11.0, 22.0, 22.0));
        assert!(p.app.right() < p.close.x);
        // The summary and the body take the full text width, 7 px and 2 px apart.
        assert_eq!(p.summary, Rect::new(14.0, 39.0, 332.0, 18.0));
        assert_eq!(p.body.len(), 2);
        assert_eq!(p.body[0].y, 57.0 + 2.0);
        assert!(p.body.iter().all(|b| b.w == 332.0));
        // Two buttons share the text width, 8 px apart, 10 px below the text.
        assert_eq!(p.buttons.len(), 2);
        assert_eq!(p.buttons[0].y, p.body[1].y + p.body[1].h + 10.0);
        assert_eq!(p.buttons[0].h, 32.0);
        assert_eq!(p.buttons[0].w, (332.0 - 8.0) / 2.0);
        assert_eq!(p.buttons[1].x, p.buttons[0].right() + 8.0);
        assert_eq!(p.buttons[1].right(), 346.0);
        // The card ends 12 px below the buttons; nothing sticks out.
        assert_eq!(p.whole.h, p.buttons[0].y + 32.0 + 12.0);
        for r in p
            .body
            .iter()
            .chain(&p.buttons)
            .chain([&p.summary, &p.tile, &p.close, &p.app])
        {
            assert!(r.right() <= 360.0 && r.y + r.h <= p.whole.h, "{r:?}");
        }
        // Without body or buttons the head row and the summary set the height.
        let bare = place(
            &item(&note("Saved", "", &[]), 360.0, false, six),
            (0.0, 0.0, 360.0),
            false,
        );
        assert_eq!(bare.whole.h, 12.0 + 20.0 + 7.0 + 18.0 + 12.0);
    }

    #[test]
    fn at_compact_width_the_targets_are_44_px() {
        let n = note("Update ready", "Restart to finish", &["now", "Restart now"]);
        let i = item(&n, 360.0, true, six);
        let p = place(&i, (0.0, 0.0, 360.0), true);
        assert_eq!(p.buttons[0].h, 44.0);
        assert_eq!((p.close.w, p.close.h), (44.0, 44.0));
        assert!(p.close.right() <= 360.0 && p.close.x >= 0.0);
        // The sheet takes the screen's width; the summary sits below the head row.
        assert_eq!(p.whole.w, 360.0);
        assert!(p.summary.y >= p.close.y + p.close.h);
        assert!(p.body.iter().all(|b| b.right() <= 360.0 - 14.0 + 0.01));
        let m = metrics(true);
        assert!(m.button >= 44.0 && m.close >= 44.0 && m.head >= 44.0);
    }

    #[test]
    fn many_buttons_share_the_row_and_stay_inside_it() {
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
        assert!(
            (p.buttons[0].w - p.buttons[2].w).abs() < 0.01,
            "equal shares"
        );
        for b in &p.buttons {
            assert!(b.right() <= 360.0 - 14.0 + 0.01, "{b:?}");
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
    fn the_close_button_is_drawn_only_when_the_notification_is_on() {
        let tokens = Tokens::built_in_scheme(edel::tokens::Scheme::Light);
        let n = note("Update", "Restart", &["now", "Restart now"]);
        let i = item(&n, 360.0, false, six);
        let p = place(&i, (0.0, 0.0, 360.0), false);
        let draw = |hover: Option<Hit>| {
            let mut pixmap = tiny_skia::Pixmap::new(360 * 2, p.whole.h.ceil() as u32 * 2).unwrap();
            popup::card(&mut pixmap, &tokens, 2.0);
            paint(
                &mut pixmap,
                &tokens,
                None,
                None,
                &i,
                &p,
                (hover, None),
                (false, 22.0),
                false,
                2.0,
            );
            // Inside the close button's circle, off its cross.
            let c = p.close;
            let c = popup::Rect::new(c.x + 2.0, c.y + c.h / 2.0, 1.0, 1.0);
            let px = pixmap
                .pixel((c.x * 2.0) as u32, (c.y * 2.0) as u32)
                .unwrap()
                .demultiply();
            [px.red(), px.green(), px.blue()]
        };
        assert_eq!(draw(None), tokens.panel.bytes()[..3], "at rest, bare");
        assert_ne!(
            draw(Some(Hit::Body)),
            tokens.panel.bytes()[..3],
            "on, veiled"
        );
    }

    #[test]
    fn it_draws_tile_buttons_and_a_cross_in_the_tokens_colours() {
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
        // Sampled at each pill's middle, its ends being round.
        let mid = |r: Rect| (r.w / 2.0, r.h / 2.0);
        let (dx, dy) = mid(p.buttons[0]);
        assert_eq!(at(p.buttons[0], dx, dy), tokens.accent.bytes()[..3]);
        let (dx, dy) = mid(p.buttons[1]);
        assert_ne!(at(p.buttons[1], dx, dy), tokens.accent.bytes()[..3]);
        assert_ne!(
            at(p.buttons[1], dx, dy),
            tokens.panel.bytes()[..3],
            "hovered, so lit"
        );
        // Without an icon the tile is the accent.
        assert_eq!(at(p.tile, 1.0, 10.0), tokens.accent.bytes()[..3]);
    }

    #[test]
    fn a_raised_card_is_lighter_than_the_panel_on_the_light_scheme() {
        let light = Tokens::built_in_scheme(edel::tokens::Scheme::Light);
        let dark = Tokens::built_in_scheme(edel::tokens::Scheme::Dark);
        let lum = |c: Colour| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        assert!(lum(raised(&light)) > lum(light.panel));
        assert_ne!(raised(&dark).bytes()[..3], dark.panel.bytes()[..3]);
    }
}
