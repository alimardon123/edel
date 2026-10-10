//! The lock screen's bottom right corner (M5.10c, `docs/mockups/shell/lock.jpg`):
//! round frosted buttons, the keyboard layout (shown with two or more) and
//! Power, and Power's menu above them with Suspend, Restart and Shut down.
//! Plain data and drawing, tested without a display. Restart carries a
//! quiet "Update waiting" while an update waits for a restart. Logs are
//! written by `lock_card.rs`, and `places` is the line CI reads.

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
}

/// What a point on the corner is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Button(Button),
    Row(usize),
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

/// A row's icon is this far from its left edge.
const ROW_ICON_LEFT: f32 = 14.0;
/// A row's label is this far from its left edge.
const LABEL_LEFT: f32 = 44.0;
/// The note is this far from a row's right end.
const NOTE_RIGHT: f32 = 12.0;
/// How much the glass under the pointer or a pressed button is lightened
/// again (M5.10c).
const HOVER_TINT: f32 = 0.12;
/// The menu's solid base, under its glass, so its text reads (M5.10c).
const MENU_SOLID: f32 = 0.85;

/// Where the corner's parts lie.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Power first, then the layout button when shown.
    pub buttons: Vec<(Button, Rect)>,
    pub menu: Option<Rect>,
    /// The three rows, top to bottom, in `CHOICES` order.
    pub rows: Vec<Rect>,
}

/// The one function that places the corner on a screen `w` by `h`.
pub fn layout(w: f32, h: f32, view: &View) -> Layout {
    let power = Rect::new(w - EDGE - BUTTON, h - EDGE - BUTTON, BUTTON, BUTTON);
    let mut buttons = vec![(Button::Power, power)];
    if view.layout.is_some() {
        let left = Rect::new(power.x - GAP - BUTTON, power.y, BUTTON, BUTTON);
        buttons.push((Button::Layout, left));
    }
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
    }
}

/// What lies under the point (`x`, `y`): a row first, then a button.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Hit> {
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
/// then with the menu open `menu X+Y+WxH` and each row by its choice's
/// name (M5.10c).
pub fn places(l: &Layout) -> String {
    let mut parts: Vec<String> = l
        .buttons
        .iter()
        .map(|(b, r)| {
            let name = match b {
                Button::Power => "power",
                Button::Layout => "layout",
            };
            format!("{name} {}", place(r))
        })
        .collect();
    if let Some(menu) = &l.menu {
        parts.push(format!("menu {}", place(menu)));
        for (choice, row) in CHOICES.iter().zip(&l.rows) {
            parts.push(format!("{} {}", choice.name(), place(row)));
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
        let r = w / 2.0;
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
        }
    }
    let Some(menu) = l.menu else {
        return;
    };
    let (x, y, w, h) = menu.device(s);
    let r = MENU_RADIUS * s;
    let solid = Colour {
        a: MENU_SOLID,
        ..mix(tokens.lock_backdrop_deep, tokens.lock_backdrop, 0.35)
    };
    fill(pixmap, x, y, w, h, r, solid);
    glass(pixmap, (x, y, w, h), r, s, tokens);
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
        }
    }

    #[test]
    fn the_corner_keeps_clear_of_the_field_and_the_note() {
        for (w, h) in [(1280.0, 800.0), (1024.0, 768.0), (360.0, 740.0)] {
            let at = lock::layout(w, h, &Tokens::built_in(), true);
            let l = layout(w, h, &view(Some("EN"), false));
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
            };
            let l = layout(w, h, &shown);
            let mut pixmap = Pixmap::new(1280, 800).unwrap();
            let mut screen =
                lock::View::new("14:06".into(), "Saturday 3 October".into(), "Ali".into());
            screen.dots = 7;
            lock::paint(&mut pixmap, &screen, &tokens, &mut text, None, 1.0);
            paint(&mut pixmap, &shown, &l, &tokens, &mut text, 1.0);
            pixmap.save_png(format!("{dir}/corner.png")).unwrap();
        }
    }
}
