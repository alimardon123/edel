//! The apps widget (M5.4c): the preset's pinned apps, then every other
//! app with a window open, one icon each in a square cell, as a taskbar
//! or a dock shows them. A running app has a dot under its icon; the
//! focused one's cell is lit and marked by a short accent line. In a dock
//! (M5.4d) the cells and icons are bigger and only dots mark them: the
//! panel's colour faded for a running app, the accent for the focused
//! one. An app without an icon shows its initial on a tile. A click brings the app's
//! window forward, back if minimized, or minimizes it when it is the
//! focused one, or starts the app when no window of it is open.

use accesskit::Role;
use tiny_skia::{FilterQuality, PixmapPaint, Rect, Transform};

use edel::tokens::Colour;

use super::{Action, Canvas, Input, Live, Pin, Widget};
use crate::paint::{fill, mix, paint_of};

pub const WIDGET: Widget = Widget {
    name: "apps",
    needs: None,
    shows,
    width,
    draw,
    input,
    role: Role::Group,
    label,
};

/// A cell's side and the room at each end of the row, in logical pixels.
const CELL: f32 = 40.0;
const EDGE: f32 = 4.0;
/// The lit square behind the focused app and the icon inside it.
const LIT: f32 = 36.0;
const ICON: f32 = 24.0;
/// The marks along a cell's foot, as the window list's: the focused
/// app's line and a running app's dot, 2 px high.
const LINE: f32 = 14.0;
const DOT: f32 = 4.0;
/// In a dock: the cell is the dock's height less this, and the icon this
/// big, its dot round, 4 px across.
const DOCK_INSET: f32 = 8.0;
const DOCK_ICON: f32 = 40.0;

/// A cell's side and its icon's in logical pixels, in a bar or a dock.
fn sizes(canvas: &Canvas) -> (f32, f32) {
    if canvas.dock {
        (canvas.height / canvas.scale - DOCK_INSET, DOCK_ICON)
    } else {
        (CELL, ICON)
    }
}

/// One cell: the app's state, its id, icon and name.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell<'a> {
    /// `*` focused, `+` running, ` ` pinned and not running.
    pub state: char,
    pub id: &'a str,
    pub icon: &'a str,
    pub name: &'a str,
}

/// Whether a window whose app id is `app_id` belongs to the app with
/// desktop file id `id`: the same, ignoring case, or the last part of a
/// reverse-domain id (`firefox` for `org.mozilla.firefox`).
pub fn belongs(app_id: &str, id: &str) -> bool {
    if app_id.is_empty() || id.is_empty() {
        return false;
    }
    let last = |s: &str| s.rsplit('.').next().unwrap_or(s).to_lowercase();
    app_id.eq_ignore_ascii_case(id) || last(app_id) == last(id)
}

/// What it shows: a line per cell, `state\tid\ticon\tname`.
fn shows(live: &Live) -> String {
    let state = |id: &str| {
        let mine = live.windows.iter().filter(|t| belongs(&t.app_id, id));
        let mut state = ' ';
        for task in mine {
            if task.focused && !task.minimized {
                return '*';
            }
            state = '+';
        }
        state
    };
    let mut cells: Vec<Pin> = live.pinned.clone();
    for task in &live.windows {
        if task.app_id.is_empty() || cells.iter().any(|p| belongs(&task.app_id, &p.id)) {
            continue;
        }
        // An app with a window but no pin: its installed name and icon,
        // or its app id for both.
        let pin = live
            .installed
            .iter()
            .find(|p| belongs(&task.app_id, &p.id))
            .cloned()
            .unwrap_or_else(|| Pin {
                id: task.app_id.clone(),
                name: task.app_id.clone(),
                icon: task.app_id.clone(),
            });
        cells.push(pin);
    }
    let clean = |s: &str| s.replace(['\t', '\n'], " ");
    let lines: Vec<String> = cells
        .iter()
        .map(|p| {
            format!(
                "{}\t{}\t{}\t{}",
                state(&p.id),
                clean(&p.id),
                clean(&p.icon),
                clean(&p.name)
            )
        })
        .collect();
    lines.join("\n")
}

/// `shows`' text read back.
pub fn read(shown: &str) -> Vec<Cell<'_>> {
    shown
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let state = parts.next()?.chars().next().unwrap_or(' ');
            Some(Cell {
                state,
                id: parts.next()?,
                icon: parts.next()?,
                name: parts.next()?,
            })
        })
        .collect()
}

/// "Apps: Files (running), Mail (focused), Music".
fn label(shown: &str) -> String {
    let names: Vec<String> = read(shown)
        .iter()
        .map(|c| match c.state {
            '*' => format!("{} (focused)", c.name),
            '+' => format!("{} (running)", c.name),
            _ => c.name.to_string(),
        })
        .collect();
    format!("Apps: {}", names.join(", "))
}

/// The widget's width in logical pixels for `count` cells `cell` wide.
pub fn logical_width(count: usize, cell: f32) -> f32 {
    if count == 0 {
        0.0
    } else {
        2.0 * EDGE + count as f32 * cell
    }
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    let (cell, _) = sizes(canvas);
    logical_width(read(shown).len(), cell) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let dock = canvas.dock;
    let (cell_w, icon) = sizes(canvas);
    let lit = Colour {
        a: 0.08,
        ..tokens.panel_text
    };
    let px = (icon * s).round();
    let side = cell_w * s;
    for (i, cell) in read(shown).iter().enumerate() {
        let cx = (x + EDGE * s + i as f32 * side).round();
        let cy = canvas.top + ((canvas.height - side) / 2.0).round();
        if cell.state == '*' && !dock {
            let inset = ((CELL - LIT) / 2.0 * s).round();
            let r = tokens.radius_control as f32 * s;
            let lit_side = (LIT * s).round();
            fill(
                canvas.pixmap,
                cx + inset,
                cy + inset,
                lit_side,
                lit_side,
                r,
                lit,
            );
        }
        let ix = cx + ((side - px) / 2.0).round();
        // The icon sits a little high, leaving room for its mark.
        let iy = cy + ((side - px) / 2.0).round() - (if dock { 3.0 } else { 2.0 } * s).round();
        let drawn = canvas
            .icons
            .as_deref_mut()
            .and_then(|icons| icons.get(cell.icon, px as u32))
            .map(|icon| {
                let paint = PixmapPaint {
                    quality: FilterQuality::Nearest,
                    ..PixmapPaint::default()
                };
                canvas.pixmap.draw_pixmap(
                    ix as i32,
                    iy as i32,
                    icon.as_ref(),
                    &paint,
                    Transform::identity(),
                    None,
                );
            })
            .is_some();
        if !drawn {
            // A tile in the accent with the app's initial.
            let tile = mix(tokens.panel, tokens.accent, 0.7);
            fill(canvas.pixmap, ix, iy, px, px, (px * 0.25).round(), tile);
            if let Some(text) = canvas.text.as_deref_mut() {
                let initial: String = cell.name.chars().take(1).collect::<String>().to_uppercase();
                let size = (px * 0.6).round();
                let mut line = text.line(&initial, size);
                let lx = ix + (px - line.width) / 2.0;
                let ly = iy + (px - size * 1.25) / 2.0;
                text.draw(canvas.pixmap, &mut line, lx, ly, tokens.panel);
            }
        }
        let faded = mix(tokens.panel_text, tokens.panel, 0.45);
        if dock {
            // A round dot under the icon: the accent for the focused app.
            let colour = match cell.state {
                '*' => Some(tokens.accent),
                '+' => Some(faded),
                _ => None,
            };
            if let Some(colour) = colour {
                let d = (DOT * s).round();
                let dx = (cx + (side - d) / 2.0).round();
                let dy = (iy + px + 2.0 * s).round();
                fill(canvas.pixmap, dx, dy, d, d, d / 2.0, colour);
            }
            continue;
        }
        let foot = match cell.state {
            '*' => Some((LINE, tokens.accent)),
            '+' => Some((DOT, faded)),
            _ => None,
        };
        if let Some((w, colour)) = foot {
            let (fw, fh) = ((w * s).round(), (2.0 * s).round());
            let fx = (cx + (side - fw) / 2.0).round();
            let fy = cy + side - 2.0 * fh;
            if let Some(rect) = Rect::from_xywh(fx, fy, fw, fh) {
                canvas
                    .pixmap
                    .fill_rect(rect, &paint_of(colour), Transform::identity(), None);
            }
        }
    }
}

/// A click on a cell opens, raises or minimizes its app.
fn input(shown: &str, input: Input) -> Option<Action> {
    let Input::Click(at, width) = input else {
        return None;
    };
    let cells = read(shown);
    if at < EDGE || cells.is_empty() {
        return None;
    }
    // The cells share the widget's width, in a bar or a dock alike.
    let cell = (width - 2.0 * EDGE) / cells.len() as f32;
    let i = ((at - EDGE) / cell).floor() as usize;
    cells.get(i).map(|c| Action::App(c.id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Task;

    fn pin(id: &str, name: &str) -> Pin {
        Pin {
            id: id.into(),
            name: name.into(),
            icon: id.into(),
        }
    }

    fn task(app_id: &str, focused: bool, minimized: bool) -> Task {
        Task {
            title: app_id.into(),
            app_id: app_id.into(),
            focused,
            minimized,
        }
    }

    #[test]
    fn a_window_belongs_to_its_app_by_id_or_by_the_ids_last_part() {
        assert!(belongs("foot", "foot"));
        assert!(belongs("Foot", "foot"));
        assert!(belongs("firefox", "org.mozilla.firefox"));
        assert!(belongs("org.gnome.Nautilus", "org.gnome.Nautilus"));
        assert!(!belongs("foot", "footclient-like"));
        assert!(!belongs("", "foot"));
    }

    #[test]
    fn pinned_apps_come_first_then_running_ones_each_once_with_their_state() {
        let live = Live {
            pinned: vec![pin("org.gnome.Nautilus", "Files"), pin("foot", "Foot")],
            installed: vec![pin("org.mozilla.firefox", "Firefox")],
            windows: vec![
                task("foot", false, false),
                task("firefox", true, false),
                task("firefox", false, false),
                task("xclock", false, true),
            ],
            ..Live::default()
        };
        let shown = shows(&live);
        let cells = read(&shown);
        let got: Vec<(char, &str, &str)> = cells.iter().map(|c| (c.state, c.id, c.name)).collect();
        assert_eq!(
            got,
            [
                (' ', "org.gnome.Nautilus", "Files"),
                ('+', "foot", "Foot"),
                ('*', "org.mozilla.firefox", "Firefox"),
                ('+', "xclock", "xclock"),
            ]
        );
        assert_eq!(
            label(&shown),
            "Apps: Files, Foot (running), Firefox (focused), xclock (running)"
        );
    }

    #[test]
    fn a_click_picks_the_cell_under_it() {
        let shown = "+\tfoot\tfoot\tFoot\n \tmail\tmail\tMail";
        assert_eq!(input(shown, Input::Click(2.0, 88.0)), None);
        assert_eq!(
            input(shown, Input::Click(EDGE + 1.0, 88.0)),
            Some(Action::App("foot".into()))
        );
        assert_eq!(
            input(shown, Input::Click(EDGE + CELL + 1.0, 88.0)),
            Some(Action::App("mail".into()))
        );
        assert_eq!(
            input(shown, Input::Click(EDGE + 2.0 * CELL + 1.0, 88.0)),
            None
        );
        assert_eq!(input(shown, Input::Scroll(1)), None);
    }

    #[test]
    fn no_apps_take_no_room() {
        assert_eq!(logical_width(0, CELL), 0.0);
        assert_eq!(logical_width(3, CELL), 2.0 * EDGE + 3.0 * CELL);
        assert!(read("").is_empty());
    }
}
