//! The window list (M5.2h): a button for each window on a screen, in the
//! order the windows opened, each its title on one line, cut short with
//! an ellipsis. The focused window's button is lit and marked by a short
//! accent line along its foot; another window's by a dot; a minimized
//! window's has no mark and dimmer text. Buttons share one width, at most
//! 180 logical pixels, and together take at most 45% of the panel, so
//! they never jump as titles change. A click on the focused window
//! minimizes it, and on any other brings it forward, back if minimized,
//! over wlr-foreign-toplevel-management (`crate::toplevels`). App icons
//! join with the launcher (M5.3), which reads them.

use tiny_skia::{FillRule, Rect, Transform};

use edel::tokens::Colour;

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{mix, paint_of, rounded};

pub const WIDGET: Widget = Widget {
    name: "windows",
    needs: None,
    shows,
    width,
    draw,
    input,
};

/// The widest a button is.
const WIDEST: f32 = 180.0;
/// The narrowest, when many windows share the room.
const NARROWEST: f32 = 32.0;
/// How much of the panel the buttons take at most.
const SHARE: f32 = 0.45;
/// Space at each end and between buttons.
const EDGE: f32 = 4.0;
const GAP: f32 = 4.0;
/// A button's height and corner radius.
const HEIGHT: f32 = 30.0;
const RADIUS: f32 = 7.0;
/// The title's inset from each side of its button.
const PAD: f32 = 10.0;
/// The marks along a button's foot: the focused window's line and other
/// windows' dot, 2 px high, 2 px above the foot.
const LINE: f32 = 14.0;
const DOT: f32 = 4.0;

/// What it shows: a line for each window, its first character `*` for
/// the focused one, `-` for a minimized one, else a space, then its
/// title. Empty without windows.
fn shows(live: &Live) -> String {
    let lines: Vec<String> = live
        .windows
        .iter()
        .map(|task| {
            let mark = match (task.focused, task.minimized) {
                (_, true) => '-',
                (true, false) => '*',
                _ => ' ',
            };
            // A title is one line here.
            let title: String = task
                .title
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            format!("{mark}{title}")
        })
        .collect();
    lines.join("\n")
}

/// `shows`' text read back: each window's mark and title.
fn read(shown: &str) -> Vec<(char, &str)> {
    if shown.is_empty() {
        return Vec::new();
    }
    shown
        .split('\n')
        .map(|line| {
            let mut chars = line.chars();
            let mark = chars.next().unwrap_or(' ');
            (mark, chars.as_str())
        })
        .collect()
}

/// A button's width for `count` windows on a panel `panel` logical pixels
/// wide.
pub fn button_width(count: usize, panel: f32) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let n = count as f32;
    ((panel * SHARE - 2.0 * EDGE - (n - 1.0) * GAP) / n)
        .clamp(NARROWEST, WIDEST)
        .floor()
}

/// The whole list's width for `count` buttons `button` wide.
pub fn logical_width(count: usize, button: f32) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let n = count as f32;
    2.0 * EDGE + n * button + (n - 1.0) * GAP
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    let count = read(shown).len();
    let panel = canvas.pixmap.width() as f32 / canvas.scale;
    logical_width(count, button_width(count, panel)) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let windows = read(shown);
    let panel = canvas.pixmap.width() as f32 / s;
    let button = button_width(windows.len(), panel);
    let height = HEIGHT * s;
    let y = canvas.top + ((canvas.height - height) / 2.0).round();
    let lit = Colour {
        a: 0.08,
        ..tokens.panel_text
    };
    let size = (tokens.panel_text_size as f32 * 0.96).round() * s;
    for (i, (mark, title)) in windows.into_iter().enumerate() {
        let bx = (x + (EDGE + i as f32 * (button + GAP)) * s).round();
        let bw = (button * s).round();
        if mark == '*' {
            if let Some(path) = rounded(bx, y, bw, height, RADIUS * s) {
                canvas.pixmap.fill_path(
                    &path,
                    &paint_of(lit),
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
        let (ink, foot) = match mark {
            '*' => (tokens.panel_text, Some((LINE, tokens.accent))),
            '-' => (mix(tokens.panel_text, tokens.panel, 0.5), None),
            _ => (
                mix(tokens.panel_text, tokens.panel, 0.25),
                Some((DOT, mix(tokens.panel_text, tokens.panel, 0.45))),
            ),
        };
        if let Some((w, colour)) = foot {
            let (fw, fh) = ((w * s).round(), (2.0 * s).round());
            let fx = (bx + (bw - fw) / 2.0).round();
            let fy = y + height - 2.0 * fh;
            if let Some(rect) = Rect::from_xywh(fx, fy, fw, fh) {
                canvas
                    .pixmap
                    .fill_rect(rect, &paint_of(colour), Transform::identity(), None);
            }
        }
        if let Some(text) = canvas.text.as_deref_mut() {
            let mut line = text.fit(title, size, bw - 2.0 * PAD * s);
            let ty = y + (height - size * 1.25) / 2.0;
            text.draw(canvas.pixmap, &mut line, bx + PAD * s, ty, ink);
        }
    }
}

/// A click on the focused window's button minimizes it; on another's,
/// brings that window forward.
fn input(shown: &str, input: Input) -> Option<Action> {
    let Input::Click(at, width) = input else {
        return None;
    };
    let windows = read(shown);
    let n = windows.len();
    if n == 0 {
        return None;
    }
    // The button width as drawn, from the list's own width.
    let button = (width - 2.0 * EDGE - (n as f32 - 1.0) * GAP) / n as f32;
    let along = at - EDGE;
    if along < 0.0 {
        return None;
    }
    let i = (along / (button + GAP)) as usize;
    if i >= n || along - i as f32 * (button + GAP) >= button {
        return None;
    }
    Some(match windows[i].0 {
        '*' => Action::Minimize(i),
        _ => Action::Activate(i),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Task;

    fn task(title: &str, focused: bool, minimized: bool) -> Task {
        Task {
            title: title.into(),
            focused,
            minimized,
        }
    }

    fn live(windows: Vec<Task>) -> Live {
        Live {
            windows,
            ..Live::default()
        }
    }

    #[test]
    fn each_window_shows_its_mark_and_title_on_one_line() {
        let shown = shows(&live(vec![
            task("foot", false, false),
            task("two\nlines", true, false),
            task("away", true, true),
        ]));
        assert_eq!(shown, " foot\n*two lines\n-away");
        assert_eq!(
            read(&shown),
            [(' ', "foot"), ('*', "two lines"), ('-', "away")]
        );
        assert_eq!(shows(&live(Vec::new())), "");
        assert!(read("").is_empty());
    }

    #[test]
    fn buttons_share_their_room_and_never_grow_past_the_widest() {
        // On a 1280 px panel, three take their widest; ten share 576 px.
        assert_eq!(button_width(3, 1280.0), WIDEST);
        assert_eq!(button_width(10, 1280.0), 53.0);
        assert_eq!(button_width(40, 1280.0), NARROWEST);
        assert_eq!(button_width(0, 1280.0), 0.0);
        assert_eq!(logical_width(3, 180.0), 556.0);
        assert!(logical_width(10, button_width(10, 1280.0)) <= 1280.0 * SHARE);
        assert_eq!(logical_width(0, 0.0), 0.0);
    }

    #[test]
    fn a_click_minimizes_the_focused_window_and_brings_others_forward() {
        let shown = " foot\n*one\n-away";
        let width = logical_width(3, 180.0);
        let middle = |i: f32| EDGE + i * (180.0 + GAP) + 90.0;
        let click = |at| input(shown, Input::Click(at, width));
        assert_eq!(click(middle(0.0)), Some(Action::Activate(0)));
        assert_eq!(click(middle(1.0)), Some(Action::Minimize(1)));
        assert_eq!(click(middle(2.0)), Some(Action::Activate(2)));
        assert_eq!(click(1.0), None, "the edge is no button");
        assert_eq!(click(EDGE + 181.0), None, "nor the gap");
        assert_eq!(click(width + 1.0), None);
        assert_eq!(input("", Input::Click(10.0, 0.0)), None);
        assert_eq!(input(shown, Input::Scroll(1)), None);
    }
}
