//! The lock screen's bottom right corner (M5.10c, `docs/mockups/shell/lock.jpg`):
//! round frosted buttons, the keyboard layout (shown with two or more) and
//! Power, and Power's menu above them with Suspend, Restart and Shut down.
//! The login screen adds its session chip left of them (M5.10c3b,
//! `docs/mockups/shell/greeter-sessions.jpg`) and the chip's menu above it.
//! Plain data and drawing, tested without a display. Restart carries a
//! quiet "Update waiting" while an update waits for a restart. Logs are
//! written by `lock_card.rs` and `greeter.rs`, and `places` is the line CI
//! reads.

use edel::i18n::tr;
use edel::tokens::{Colour, Tokens};
use tiny_skia::Pixmap;

use crate::lock::glass;
use crate::paint::{Face, Text, fill, icon, mix};
use crate::popup::Rect;
use crate::power::{CHOICES, Choice};

/// A button of the corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Layout,
    Power,
    /// The login screen's session chip (M5.10c3b).
    Session,
}

/// What the corner shows now.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The short name of the layout in use, `None` with fewer than two.
    pub layout: Option<String>,
    /// The power menu is open.
    pub menu: bool,
    /// An update waits for a restart: the menu's Restart says so.
    pub restart_waiting: bool,
    /// The button or row under the pointer.
    pub hover: Option<Hit>,
    /// The login screen's session chip: the chosen session's name, `None`
    /// with fewer than two sessions (M5.10c3b).
    pub session: Option<String>,
    /// The chip's width, as `chip_width` measures its name.
    pub chip_w: f32,
    /// The session menu is open: the sessions' names and the chosen one's
    /// place. It opens only while the power menu is shut.
    pub sessions: Option<(Vec<String>, usize)>,
}

/// What a point on the corner is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Button(Button),
    Row(usize),
    /// A row of the session menu.
    Session(usize),
}

/// A button's side, logical pixels (touch-sized, M5.6c).
pub const BUTTON: f32 = 40.0;
/// Between two buttons.
pub const GAP: f32 = 10.0;
/// From the screen's right and bottom edges.
pub const EDGE: f32 = 24.0;
/// The menu's width.
pub const MENU_WIDE: f32 = 216.0;
/// A menu row's height.
pub const ROW: f32 = 40.0;
/// Inside the menu, round its rows.
pub const MENU_PAD: f32 = 6.0;
/// Between the menu's bottom and the buttons' top.
pub const MENU_GAP: f32 = 10.0;
/// A button's icon.
pub const GLYPH: f32 = 18.0;
/// A row's icon.
pub const ROW_ICON: f32 = 16.0;
/// A row's label.
pub const ROW_TEXT: f32 = 14.0;
/// The quiet note at a row's right end.
pub const NOTE_TEXT: f32 = 11.0;
/// The menu's corners.
pub const MENU_RADIUS: f32 = 16.0;
/// A row's corners.
pub const ROW_RADIUS: f32 = 10.0;
/// The session menu's header line, "Desktop for this login" (M5.10c3b).
pub const HEADER_H: f32 = 28.0;
/// The header's text.
pub const HEADER_TEXT: f32 = 11.0;
/// The chip's text, and its face: bold, as the mockup sets it.
pub const CHIP_TEXT: f32 = 14.0;
const CHIP_FACE: Face = Face {
    weight: 700,
    tabular: false,
};
/// Inside the chip, from its left and right ends.
pub const CHIP_PAD: f32 = 14.0;
/// The chip's icon, `page-displays`.
pub const CHIP_ICON: f32 = 16.0;
/// The chip's chevron, `chevron-up`.
pub const CHIP_CHEVRON: f32 = 12.0;
/// Between the chip's icon, name and chevron.
pub const CHIP_GAP: f32 = 8.0;
/// The chosen session's check mark, and how far it is from a row's end.
pub const CHECK: f32 = 14.0;
pub const CHECK_RIGHT: f32 = 12.0;

/// A row's icon is this far from its left edge.
const ROW_ICON_LEFT: f32 = 14.0;
/// A row's label is this far from its left edge.
const LABEL_LEFT: f32 = 44.0;
/// The note is this far from a row's right end.
const NOTE_RIGHT: f32 = 12.0;
/// How much the glass under the pointer or a pressed button is lightened
/// again (M5.10c).
const HOVER_TINT: f32 = 0.12;
/// Where the corner's parts lie.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Power first, then the layout button when shown, then the session
    /// chip (the leftmost) when shown.
    pub buttons: Vec<(Button, Rect)>,
    pub menu: Option<Rect>,
    /// The three rows, top to bottom, in `CHOICES` order.
    pub rows: Vec<Rect>,
    /// The session chip's name, and the session menu's names, as shown.
    pub chip_name: Option<String>,
    pub session_names: Vec<String>,
    /// The session menu and its rows, top to bottom (M5.10c3b).
    pub sessions: Option<Rect>,
    pub session_rows: Vec<Rect>,
}

/// The one function that places the corner on a screen `w` by `h`.
pub fn layout(w: f32, h: f32, view: &View) -> Layout {
    let power = Rect::new(w - EDGE - BUTTON, h - EDGE - BUTTON, BUTTON, BUTTON);
    let mut buttons = vec![(Button::Power, power)];
    if view.layout.is_some() {
        let left = Rect::new(power.x - GAP - BUTTON, power.y, BUTTON, BUTTON);
        buttons.push((Button::Layout, left));
    }
    if view.session.is_some() {
        let left = buttons.last().map_or(power, |(_, r)| *r);
        let chip = Rect::new(left.x - GAP - view.chip_w, power.y, view.chip_w, BUTTON);
        buttons.push((Button::Session, chip));
    }
    let chip = buttons
        .iter()
        .find(|(b, _)| *b == Button::Session)
        .map(|(_, r)| *r);
    // Only one menu is laid out: the power menu, when it is open, wins.
    let (sessions, session_rows) = match (&view.sessions, chip) {
        (Some((names, _)), Some(chip)) if !view.menu => {
            let high = 2.0 * MENU_PAD + HEADER_H + names.len() as f32 * ROW;
            let menu = Rect::new(
                chip.right() - MENU_WIDE,
                chip.y - MENU_GAP - high,
                MENU_WIDE,
                high,
            );
            let rows = (0..names.len())
                .map(|i| {
                    Rect::new(
                        menu.x + MENU_PAD,
                        menu.y + MENU_PAD + HEADER_H + i as f32 * ROW,
                        MENU_WIDE - 2.0 * MENU_PAD,
                        ROW,
                    )
                })
                .collect();
            (Some(menu), rows)
        }
        _ => (None, Vec::new()),
    };
    let (menu, rows) = if view.menu {
        let high = 2.0 * MENU_PAD + CHOICES.len() as f32 * ROW;
        let menu = Rect::new(
            power.right() - MENU_WIDE,
            power.y - MENU_GAP - high,
            MENU_WIDE,
            high,
        );
        let rows = (0..CHOICES.len())
            .map(|i| {
                Rect::new(
                    menu.x + MENU_PAD,
                    menu.y + MENU_PAD + i as f32 * ROW,
                    MENU_WIDE - 2.0 * MENU_PAD,
                    ROW,
                )
            })
            .collect();
        (Some(menu), rows)
    } else {
        (None, Vec::new())
    };
    Layout {
        buttons,
        menu,
        rows,
        chip_name: view.session.clone(),
        session_names: view
            .sessions
            .as_ref()
            .map_or(Vec::new(), |(names, _)| names.clone()),
        sessions,
        session_rows,
    }
}

/// The width of the session chip for `name` (M5.10c3b): its padding, icon,
/// chevron and gaps, and the name in its face.
pub fn chip_width(text: &mut Text, name: &str) -> f32 {
    let line = text.line_in(name, CHIP_TEXT, CHIP_FACE);
    2.0 * CHIP_PAD + CHIP_ICON + CHIP_CHEVRON + 2.0 * CHIP_GAP + line.width
}

/// What lies under the point (`x`, `y`): a session row, then a row of
/// Power's menu, then a button.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Hit> {
    if let Some(i) = l.session_rows.iter().position(|r| r.contains(x, y)) {
        return Some(Hit::Session(i));
    }
    if let Some(i) = l.rows.iter().position(|r| r.contains(x, y)) {
        return Some(Hit::Row(i));
    }
    l.buttons
        .iter()
        .find(|(_, r)| r.contains(x, y))
        .map(|(b, _)| Hit::Button(*b))
}

/// `X+Y+WxH`, whole logical pixels, as the places lines write them.
fn place(r: &Rect) -> String {
    format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h)
}

/// The log line CI reads: `power X+Y+WxH`, `layout X+Y+WxH` when shown,
/// `session X+Y+WxH NAME` for the chip (M5.10c3b), then with the power
/// menu open `menu X+Y+WxH` and each row by its choice's name (M5.10c),
/// or with the session menu open `sessions X+Y+WxH` and each row as
/// `sessionN X+Y+WxH NAME`.
pub fn places(l: &Layout) -> String {
    let mut parts: Vec<String> = l
        .buttons
        .iter()
        .map(|(b, r)| match b {
            Button::Power => format!("power {}", place(r)),
            Button::Layout => format!("layout {}", place(r)),
            Button::Session => format!(
                "session {} {}",
                place(r),
                l.chip_name.as_deref().unwrap_or_default()
            ),
        })
        .collect();
    if let Some(menu) = &l.menu {
        parts.push(format!("menu {}", place(menu)));
        for (choice, row) in CHOICES.iter().zip(&l.rows) {
            parts.push(format!("{} {}", choice.name(), place(row)));
        }
    }
    if let Some(menu) = &l.sessions {
        parts.push(format!("sessions {}", place(menu)));
        for (i, (name, row)) in l.session_names.iter().zip(&l.session_rows).enumerate() {
            parts.push(format!("session{i} {} {name}", place(row)));
        }
    }
    parts.join(", ")
}

/// Draws the corner into `pixmap` at scale `s`: each button as frosted
/// glass with its icon or short name, lighter under the pointer; the menu
/// as a card with its rows (M5.10c).
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    l: &Layout,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let ink = tokens.backdrop_text;
    let quiet = mix(ink, tokens.lock_backdrop_deep, 0.35);
    let hover_tint = Colour {
        a: HOVER_TINT,
        ..ink
    };
    for (button, rect) in &l.buttons {
        let (x, y, w, h) = rect.device(s);
        // A round button is as high as wide; the chip is a pill.
        let r = h / 2.0;
        glass(pixmap, (x, y, w, h), r, s, tokens);
        if view.hover == Some(Hit::Button(*button)) {
            fill(pixmap, x, y, w, h, r, hover_tint);
        }
        match button {
            Button::Power => {
                // While the menu is open the button is filled once more, so
                // it reads as pressed.
                if view.menu {
                    fill(pixmap, x, y, w, h, r, hover_tint);
                }
                let side = GLYPH * s;
                let (ix, iy) = (x + (w - side) / 2.0, y + (h - side) / 2.0);
                icon(pixmap, "power", side, ix, iy, ink);
            }
            Button::Layout => {
                if let Some(short) = &view.layout {
                    let size = tokens.panel_text_small_size as f32;
                    let mut line = text.line_in(short, size * s, Face::SEMIBOLD);
                    let left = x + w / 2.0 - line.width / 2.0;
                    let top = y + h / 2.0 - size * s * 0.62;
                    text.draw(pixmap, &mut line, left, top, ink);
                }
            }
            Button::Session => {
                if view.sessions.is_some() {
                    fill(pixmap, x, y, w, h, r, hover_tint);
                }
                if let Some(name) = &view.session {
                    let side = CHIP_ICON * s;
                    let icon_x = x + CHIP_PAD * s;
                    icon(
                        pixmap,
                        "page-displays",
                        side,
                        icon_x,
                        y + (h - side) / 2.0,
                        ink,
                    );
                    let mut line = text.line_in(name, CHIP_TEXT * s, CHIP_FACE);
                    let left = icon_x + (CHIP_ICON + CHIP_GAP) * s;
                    let top = y + h / 2.0 - CHIP_TEXT * s * 0.62;
                    text.draw(pixmap, &mut line, left, top, ink);
                    let chevron = CHIP_CHEVRON * s;
                    let chevron_x = x + w - CHIP_PAD * s - chevron;
                    icon(
                        pixmap,
                        "chevron-up",
                        chevron,
                        chevron_x,
                        y + (h - chevron) / 2.0,
                        ink,
                    );
                }
            }
        }
    }
    if let Some(menu) = l.menu {
        let (x, y, w, h) = menu.device(s);
        let r = MENU_RADIUS * s;
        crate::lock::frosted(pixmap, (x, y, w, h), r, s, tokens);
        for (i, (choice, row)) in CHOICES.iter().zip(&l.rows).enumerate() {
            let (rx, ry, rw, rh) = row.device(s);
            if view.hover == Some(Hit::Row(i)) {
                fill(pixmap, rx, ry, rw, rh, ROW_RADIUS * s, hover_tint);
            }
            let side = ROW_ICON * s;
            let iy = (row.y + (ROW - ROW_ICON) / 2.0) * s;
            icon(
                pixmap,
                choice.icon(),
                side,
                (row.x + ROW_ICON_LEFT) * s,
                iy,
                ink,
            );
            let room = (row.w - LABEL_LEFT - NOTE_RIGHT) * s;
            let mut label = text.fit_in(tr(choice.label()), ROW_TEXT * s, room, Face::MEDIUM);
            let top = (row.y + (ROW - ROW_TEXT * 1.25) / 2.0) * s;
            text.draw(pixmap, &mut label, (row.x + LABEL_LEFT) * s, top, ink);
            if *choice == Choice::Restart && view.restart_waiting {
                let mut note = text.line_in(tr("Update waiting"), NOTE_TEXT * s, Face::MEDIUM);
                let right = (row.x + row.w - NOTE_RIGHT) * s;
                let top = (row.y + (ROW - NOTE_TEXT * 1.25) / 2.0) * s;
                let left = right - note.width;
                text.draw(pixmap, &mut note, left, top, quiet);
            }
        }
    }
    if let Some(menu) = l.sessions {
        let (x, y, w, h) = menu.device(s);
        crate::lock::frosted(pixmap, (x, y, w, h), MENU_RADIUS * s, s, tokens);
        // The header sits in the menu's top padding, quiet, as the notes do.
        let left = (menu.x + MENU_PAD + ROW_ICON_LEFT) * s;
        let room = (menu.w - 2.0 * MENU_PAD - 2.0 * ROW_ICON_LEFT) * s;
        let mut head = text.fit_in(
            tr("Desktop for this login"),
            HEADER_TEXT * s,
            room,
            Face::MEDIUM,
        );
        let top = (menu.y + MENU_PAD + (HEADER_H - HEADER_TEXT * 1.25) / 2.0) * s;
        text.draw(pixmap, &mut head, left, top, quiet);
        let chosen = view.sessions.as_ref().map(|(_, chosen)| *chosen);
        for (i, (name, row)) in l.session_names.iter().zip(&l.session_rows).enumerate() {
            let (rx, ry, rw, rh) = row.device(s);
            if view.hover == Some(Hit::Session(i)) || chosen == Some(i) {
                fill(pixmap, rx, ry, rw, rh, ROW_RADIUS * s, hover_tint);
            }
            let iy = (row.y + (ROW - ROW_ICON) / 2.0) * s;
            icon(
                pixmap,
                "page-displays",
                ROW_ICON * s,
                (row.x + ROW_ICON_LEFT) * s,
                iy,
                ink,
            );
            let room = (row.w - LABEL_LEFT - CHECK_RIGHT - CHECK) * s;
            let mut label = text.fit_in(name, ROW_TEXT * s, room, Face::MEDIUM);
            let top = (row.y + (ROW - ROW_TEXT * 1.25) / 2.0) * s;
            text.draw(pixmap, &mut label, (row.x + LABEL_LEFT) * s, top, ink);
            if chosen == Some(i) {
                let check_x = (row.x + row.w - CHECK_RIGHT - CHECK) * s;
                let check_y = (row.y + (ROW - CHECK) / 2.0) * s;
                icon(pixmap, "check", CHECK * s, check_x, check_y, tokens.accent);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock;

    fn view(layout: Option<&str>, menu: bool) -> View {
        View {
            layout: layout.map(str::to_string),
            menu,
            restart_waiting: false,
            hover: None,
            session: None,
            chip_w: 0.0,
            sessions: None,
        }
    }

    /// The login screen's view: a chip named `name`, `chip_w` wide, and the
    /// session menu open with `n` sessions and `chosen` (or shut).
    fn login(name: &str, chip_w: f32, menu: Option<(usize, usize)>) -> View {
        View {
            session: Some(name.to_string()),
            chip_w,
            sessions: menu.map(|(n, chosen)| {
                let names = (0..n).map(|i| format!("Session {i}")).collect();
                (names, chosen)
            }),
            ..view(None, false)
        }
    }

    fn overlaps(a: &Rect, b: (f32, f32, f32, f32)) -> bool {
        a.x < b.0 + b.2 && b.0 < a.x + a.w && a.y < b.1 + b.3 && b.1 < a.y + a.h
    }

    #[test]
    fn the_corner_lies_at_the_bottom_right_with_its_menu_above() {
        let all = layout(1280.0, 800.0, &view(Some("EN"), true));
        let rects: Vec<(Button, Rect)> = all.buttons.clone();
        assert_eq!(rects[0].0, Button::Power);
        assert_eq!(rects[0].1, Rect::new(1216.0, 736.0, 40.0, 40.0));
        assert_eq!(rects[1].0, Button::Layout);
        assert_eq!(rects[1].1, Rect::new(1166.0, 736.0, 40.0, 40.0));
        let menu = all.menu.expect("the menu is open");
        assert_eq!(menu, Rect::new(1040.0, 594.0, 216.0, 132.0));
        assert_eq!(menu.right(), 1256.0);
        assert_eq!(menu.y + menu.h, 726.0);
        assert_eq!(all.rows.len(), 3);
        for row in &all.rows {
            assert!(menu.contains(row.x, row.y));
            assert!(menu.contains(row.x, row.y + row.h - 0.5));
            assert_eq!(row.w, 204.0);
            assert_eq!(row.h, 40.0);
        }
    }

    #[test]
    fn the_layout_button_and_the_menu_come_only_when_shown() {
        let plain = layout(1280.0, 800.0, &view(None, false));
        assert_eq!(plain.buttons.len(), 1);
        assert_eq!(plain.buttons[0].0, Button::Power);
        assert!(plain.menu.is_none());
        assert!(plain.rows.is_empty());
    }

    #[test]
    fn a_point_finds_each_button_and_row() {
        let l = layout(1280.0, 800.0, &view(Some("EN"), true));
        let (cx, cy) = (1236.0, 756.0);
        assert_eq!(hit(&l, cx, cy), Some(Hit::Button(Button::Power)));
        assert_eq!(hit(&l, 1186.0, 756.0), Some(Hit::Button(Button::Layout)));
        for i in 0..3 {
            let row = l.rows[i];
            assert_eq!(hit(&l, row.x + 20.0, row.y + 20.0), Some(Hit::Row(i)));
        }
        assert_eq!(hit(&l, 10.0, 10.0), None);
        let closed = layout(1280.0, 800.0, &view(Some("EN"), false));
        assert_eq!(hit(&closed, 1100.0, 600.0), None);
    }

    #[test]
    fn the_places_line_names_each_part() {
        let l = layout(1280.0, 800.0, &view(Some("EN"), true));
        assert_eq!(
            places(&l),
            "power 1216+736+40x40, layout 1166+736+40x40, menu 1040+594+216x132, suspend 1046+600+204x40, restart 1046+640+204x40, shutdown 1046+680+204x40"
        );
        let closed = layout(1280.0, 800.0, &view(None, false));
        assert_eq!(places(&closed), "power 1216+736+40x40");
    }

    #[test]
    fn every_part_stays_on_a_screen_of_any_size() {
        for (w, h) in [(1280.0, 800.0), (1024.0, 768.0), (360.0, 740.0)] {
            for layout_name in [None, Some("EN")] {
                for menu in [false, true] {
                    let l = layout(w, h, &view(layout_name, menu));
                    let mut parts: Vec<Rect> = l.buttons.iter().map(|(_, r)| *r).collect();
                    parts.extend(l.menu);
                    parts.extend(l.rows.iter().copied());
                    for r in parts {
                        assert!(r.x >= 0.0 && r.y >= 0.0, "{w}x{h} {r:?}");
                        assert!(r.x + r.w <= w && r.y + r.h <= h, "{w}x{h} {r:?}");
                    }
                }
            }
            // The login screen's chip and its session menu, shut and open.
            for chip_w in [120.0, 260.0] {
                for menu in [None, Some((3, 1))] {
                    let l = layout(w, h, &login("Edel OS", chip_w, menu));
                    let mut parts: Vec<Rect> = l.buttons.iter().map(|(_, r)| *r).collect();
                    parts.extend(l.sessions);
                    parts.extend(l.session_rows.iter().copied());
                    for r in parts {
                        assert!(r.x >= 0.0 && r.y >= 0.0, "{w}x{h} {r:?}");
                        assert!(r.x + r.w <= w && r.y + r.h <= h, "{w}x{h} {r:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn no_chip_without_a_session() {
        let l = layout(1280.0, 800.0, &view(Some("EN"), false));
        assert!(l.buttons.iter().all(|(b, _)| *b != Button::Session));
        assert!(l.sessions.is_none() && l.session_rows.is_empty());
        assert_eq!(places(&l), "power 1216+736+40x40, layout 1166+736+40x40");
    }

    #[test]
    fn the_chip_lies_left_of_the_buttons_and_inside_the_screen() {
        let plain = layout(1280.0, 800.0, &login("Edel OS", 120.0, None));
        let (button, chip) = plain.buttons[1];
        assert_eq!(button, Button::Session);
        assert_eq!(chip, Rect::new(1086.0, 736.0, 120.0, 40.0));
        assert_eq!(chip.right() + GAP, 1216.0, "left of Power, GAP apart");
        // With the layout button too, the chip lies left of that one.
        let both = View {
            layout: Some("EN".into()),
            ..login("Edel OS", 120.0, None)
        };
        let l = layout(1280.0, 800.0, &both);
        let (_, layout_button) = l.buttons[1];
        let (_, chip) = l.buttons[2];
        assert_eq!(chip.right() + GAP, layout_button.x);
        assert!(chip.x >= 0.0 && chip.right() <= 1280.0);
    }

    #[test]
    fn the_chip_names_itself_in_the_places_line() {
        let l = layout(1280.0, 800.0, &login("Edel OS", 120.0, None));
        assert_eq!(
            places(&l),
            "power 1216+736+40x40, session 1086+736+120x40 Edel OS"
        );
    }

    #[test]
    fn the_session_menu_lies_above_the_chip_and_its_rows_are_hit() {
        let open = layout(1280.0, 800.0, &login("Edel OS", 120.0, Some((2, 0))));
        let (_, chip) = open.buttons[1];
        let menu = open.sessions.expect("the session menu is open");
        assert_eq!(menu.right(), chip.right(), "right edges line up");
        assert_eq!(menu.w, MENU_WIDE);
        assert_eq!(menu.y + menu.h, chip.y - MENU_GAP);
        assert_eq!(open.session_rows.len(), 2);
        for (i, row) in open.session_rows.iter().enumerate() {
            assert!(menu.contains(row.x, row.y));
            assert_eq!(row.y - menu.y, MENU_PAD + HEADER_H + i as f32 * ROW);
            assert_eq!(
                hit(&open, row.x + 20.0, row.y + 20.0),
                Some(Hit::Session(i))
            );
        }
        assert_eq!(
            hit(&open, chip.x + 10.0, chip.y + 10.0),
            Some(Hit::Button(Button::Session))
        );
        let line = places(&open);
        assert!(line.contains(", sessions 990+606+216x120"), "{line}");
        assert!(line.contains(", session0 "), "{line}");
        assert!(line.ends_with(" Session 1"), "{line}");
    }

    #[test]
    fn only_one_menu_is_laid_out() {
        let both = View {
            menu: true,
            ..login("Edel OS", 120.0, Some((2, 0)))
        };
        let l = layout(1280.0, 800.0, &both);
        assert!(l.menu.is_some(), "the power menu is open");
        assert!(l.sessions.is_none() && l.session_rows.is_empty());
        assert_eq!(l.rows.len(), 3);
        let shut = layout(1280.0, 800.0, &login("Edel OS", 120.0, None));
        assert!(shut.menu.is_none() && shut.sessions.is_none());
    }

    #[test]
    fn the_corner_keeps_clear_of_the_field_and_the_note() {
        for (w, h) in [(1280.0, 800.0), (1024.0, 768.0), (360.0, 740.0)] {
            let tokens = Tokens::built_in();
            let mut text = Text::load(&tokens.font);
            let l = layout(w, h, &view(Some("EN"), false));
            let at = lock::screen_layout(w, h, &tokens, true, None, None, false, &l, &mut text);
            let f = at.field;
            for (_, r) in &l.buttons {
                assert!(!overlaps(r, (f.x, f.y, f.w, f.h)), "{w}x{h} field");
                assert!(!overlaps(r, (0.0, at.note, w, 20.0)), "{w}x{h} note");
            }
        }
    }

    #[test]
    fn the_power_button_is_frosted_glass_over_the_backdrop() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let (w, h) = (1280.0, 800.0);
        let shown = view(None, false);
        let l = layout(w, h, &shown);
        let mut pixmap = Pixmap::new(1280, 800).unwrap();
        fill(&mut pixmap, 0.0, 0.0, w, h, 0.0, tokens.lock_backdrop_deep);
        paint(&mut pixmap, &shown, &l, &tokens, &mut text, 1.0);
        // Six pixels inside the button's left edge, at its middle, and six
        // pixels left of it on the backdrop.
        let sum = |x: u32, y: u32| {
            let p = pixmap.pixel(x, y).unwrap();
            p.red() as u32 + p.green() as u32 + p.blue() as u32
        };
        let inside = sum(1222, 756);
        let outside = sum(1210, 756);
        assert!(inside > outside, "glass {inside} over backdrop {outside}");
        if let Ok(dir) = std::env::var("EDEL_LOCK_PNG") {
            let shown = View {
                layout: Some("EN".into()),
                menu: true,
                restart_waiting: true,
                hover: None,
                session: None,
                chip_w: 0.0,
                sessions: None,
            };
            let l = layout(w, h, &shown);
            let mut pixmap = Pixmap::new(1280, 800).unwrap();
            let mut screen =
                lock::View::new("14:06".into(), "Saturday 3 October".into(), "Ali".into());
            screen.dots = 7;
            let below = layout(w, h, &view(None, false));
            let at =
                lock::screen_layout(w, h, &tokens, false, None, None, false, &below, &mut text);
            lock::paint(
                &mut pixmap,
                &screen,
                &at,
                &tokens,
                &mut text,
                None,
                &mut lock::Pictures::default(),
                1.0,
            );
            paint(&mut pixmap, &shown, &l, &tokens, &mut text, 1.0);
            pixmap.save_png(format!("{dir}/corner.png")).unwrap();
            // The login screen's chip with its menu open (M5.10c3b).
            let shown = View {
                layout: Some("EN".into()),
                menu: false,
                restart_waiting: false,
                hover: None,
                session: Some("Edel OS".into()),
                chip_w: chip_width(&mut text, "Edel OS"),
                sessions: Some((vec!["Edel OS".into(), "Console".into()], 0)),
            };
            let l = layout(w, h, &shown);
            let mut pixmap = Pixmap::new(1280, 800).unwrap();
            let at = lock::screen_layout(w, h, &tokens, false, None, None, false, &l, &mut text);
            lock::paint(
                &mut pixmap,
                &screen,
                &at,
                &tokens,
                &mut text,
                None,
                &mut lock::Pictures::default(),
                1.0,
            );
            paint(&mut pixmap, &shown, &l, &tokens, &mut text, 1.0);
            pixmap
                .save_png(format!("{dir}/corner-sessions.png"))
                .unwrap();
        }
    }
}
