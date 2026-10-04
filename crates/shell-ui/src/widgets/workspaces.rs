//! The workspace switcher (M5.2c): round buttons numbered as Super+1 to
//! Super+9 are, the shown workspace a wider pill in the accent colour. At
//! most three show at once, the shown workspace and its neighbours, so the
//! panel keeps its width whatever the preset's count; a dot at a side says
//! more lie there, which the wheel or a touchpad scrolls to. A click shows
//! its workspace through ext-workspace-v1 (`crate::workspaces`). Sizes are
//! logical pixels, drawn at the panel's scale.

use tiny_skia::{FillRule, Transform};

use edel::tokens::Colour;

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{paint_of, rounded};

pub const WIDGET: Widget = Widget {
    name: "workspaces",
    needs: None,
    shows,
    width,
    draw,
    input,
};

/// How many buttons show at once.
pub const SHOWN: usize = 3;
/// A button's height, and its width unless it is the shown workspace's.
const BUTTON: f32 = 20.0;
/// The shown workspace's pill.
const PILL: f32 = 32.0;
const GAP: f32 = 5.0;
/// Where a dot says more workspaces lie.
const HINT: f32 = 8.0;
/// Space on each side, between it and its neighbours.
const ROOM: f32 = 6.0;

/// What it shows: the first button's index, then every workspace's name,
/// the shown one starred: `0;1*,2,3,4`. Empty without workspaces, as when
/// the compositor offers no ext-workspace-v1.
fn shows(live: &Live) -> String {
    if live.workspaces.is_empty() {
        return String::new();
    }
    let active = live.workspaces.iter().position(|(_, on)| *on).unwrap_or(0);
    let first = first_shown(active, live.workspaces.len(), live.view);
    let names: Vec<String> = live
        .workspaces
        .iter()
        .map(|(name, on)| format!("{name}{}", if *on { "*" } else { "" }))
        .collect();
    format!("{first};{}", names.join(","))
}

/// The first button shown: the one before the shown workspace, or where
/// a scroll left the view, kept so that three show when there are three.
pub fn first_shown(active: usize, count: usize, view: Option<usize>) -> usize {
    view.unwrap_or(active.saturating_sub(1))
        .min(count.saturating_sub(SHOWN))
}

/// `shows`' text read back: the first shown and every workspace.
fn read(shown: &str) -> (usize, Vec<(&str, bool)>) {
    let Some((first, names)) = shown.split_once(';') else {
        return (0, Vec::new());
    };
    let names = names
        .split(',')
        .map(|n| match n.strip_suffix('*') {
            Some(name) => (name, true),
            None => (n, false),
        })
        .collect();
    (first.parse().unwrap_or(0), names)
}

/// The buttons shown, each with its name, whether it is the shown
/// workspace, and its left edge and width from the widget's left, in
/// logical pixels.
pub fn buttons(shown: &str) -> Vec<(&str, bool, f32, f32)> {
    let (first, names) = read(shown);
    let mut x = ROOM + if names.len() > SHOWN { HINT } else { 0.0 };
    let mut out = Vec::new();
    for &(name, on) in names.iter().skip(first).take(SHOWN) {
        let width = if on { PILL } else { BUTTON };
        out.push((name, on, x, width));
        x += width + GAP;
    }
    out
}

/// Its width in logical pixels: fixed for three or more workspaces, so
/// nothing beside it moves as the shown one changes.
pub fn logical_width(shown: &str) -> f32 {
    let (_, names) = read(shown);
    if names.is_empty() {
        return 0.0;
    }
    let shown_count = names.len().min(SHOWN) as f32;
    let hints = if names.len() > SHOWN { 2.0 * HINT } else { 0.0 };
    2.0 * ROOM + hints + PILL + (shown_count - 1.0) * (BUTTON + GAP)
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    logical_width(shown) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let height = BUTTON * s;
    let y = canvas.top + ((canvas.height - height) / 2.0).round();
    let off = Colour {
        a: 0.14,
        ..tokens.panel_text
    };
    let size = (tokens.panel_text_size as f32 * 0.82).round() * s;
    for (name, on, left, w) in buttons(shown) {
        let (bx, bw) = ((x + left * s).round(), w * s);
        if let Some(path) = rounded(bx, y, bw, height, height / 2.0) {
            let fill = paint_of(if on { tokens.accent } else { off });
            canvas
                .pixmap
                .fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);
        }
        if let Some(text) = canvas.text.as_deref_mut() {
            let mut line = text.line(name, size);
            let tx = bx + (bw - line.width) / 2.0;
            let ty = y + (height - size * 1.25) / 2.0;
            let ink = if on { tokens.panel } else { tokens.panel_text };
            text.draw(canvas.pixmap, &mut line, tx, ty, ink);
        }
    }
    // A dot on each side where more workspaces lie.
    let (first, names) = read(shown);
    let dot = Colour {
        a: 0.5,
        ..tokens.panel_text
    };
    let r = 1.5 * s;
    let mid = canvas.top + canvas.height / 2.0;
    let total = logical_width(shown) * s;
    let mut dots = Vec::new();
    if first > 0 {
        dots.push(x + (ROOM + HINT / 2.0) * s);
    }
    if first + SHOWN < names.len() {
        dots.push(x + total - (ROOM + HINT / 2.0) * s);
    }
    for cx in dots {
        if let Some(path) = rounded(cx - r, mid - r, 2.0 * r, 2.0 * r, r) {
            canvas.pixmap.fill_path(
                &path,
                &paint_of(dot),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

/// A click shows the workspace under it; a scroll moves the view by its
/// steps, keeping three in sight.
fn input(shown: &str, input: Input) -> Option<Action> {
    match input {
        Input::Click(at, _) => buttons(shown)
            .into_iter()
            .find(|(_, _, left, w)| (*left..left + w).contains(&at))
            .map(|(name, ..)| Action::Show(name.to_string())),
        Input::Scroll(steps) => {
            let (first, names) = read(shown);
            let last = names.len().saturating_sub(SHOWN) as i64;
            let to = (first as i64 + i64::from(steps)).clamp(0, last) as usize;
            (to != first).then_some(Action::View(to))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(count: usize, active: usize, view: Option<usize>) -> Live {
        Live {
            workspaces: (1..=count)
                .map(|n| (n.to_string(), n == active + 1))
                .collect(),
            view,
            ..Live::default()
        }
    }

    #[test]
    fn three_show_at_once_round_the_shown_workspace() {
        assert_eq!(shows(&live(4, 0, None)), "0;1*,2,3,4");
        assert_eq!(shows(&live(4, 2, None)), "1;1,2,3*,4");
        // The last: still three in sight.
        assert_eq!(shows(&live(4, 3, None)), "1;1,2,3,4*");
        assert_eq!(shows(&live(9, 6, None)), "5;1,2,3,4,5,6,7*,8,9");
        assert_eq!(shows(&live(2, 1, None)), "0;1,2*");
        assert_eq!(shows(&live(0, 0, None)), "");
        // A scroll's view holds until the shown workspace changes.
        assert_eq!(shows(&live(9, 0, Some(4))), "4;1*,2,3,4,5,6,7,8,9");
        assert_eq!(shows(&live(9, 0, Some(8))), "6;1*,2,3,4,5,6,7,8,9");
    }

    #[test]
    fn the_shown_one_is_a_wider_pill_and_the_width_stays() {
        let first = buttons("0;1*,2,3,4");
        let names: Vec<_> = first.iter().map(|b| (b.0, b.1)).collect();
        assert_eq!(names, [("1", true), ("2", false), ("3", false)]);
        // Room, then the hint's place on the left, as more lie to the right.
        assert_eq!((first[0].2, first[0].3), (ROOM + HINT, PILL));
        assert_eq!(first[1].2, ROOM + HINT + PILL + GAP);
        assert_eq!(
            logical_width("0;1*,2,3,4"),
            logical_width("1;1,2,3*,4"),
            "the widget keeps its width as the shown workspace changes"
        );
        assert_eq!(logical_width(""), 0.0);
        assert!(logical_width("0;1*,2") < logical_width("0;1*,2,3"));
    }

    #[test]
    fn a_click_shows_its_workspace_and_a_scroll_moves_the_view() {
        let shown = "0;1*,2,3,4";
        let [_, two, three] = buttons(shown)[..] else {
            panic!()
        };
        let middle = |b: (&str, bool, f32, f32)| b.2 + b.3 / 2.0;
        assert_eq!(
            input(shown, Input::Click(middle(three), 0.0)),
            Some(Action::Show("3".into()))
        );
        assert_eq!(
            input(shown, Input::Click(middle(two), 0.0)),
            Some(Action::Show("2".into()))
        );
        assert_eq!(
            input(shown, Input::Click(0.0, 0.0)),
            None,
            "the room is no button"
        );
        assert_eq!(input(shown, Input::Scroll(1)), Some(Action::View(1)));
        assert_eq!(
            input(shown, Input::Scroll(5)),
            Some(Action::View(1)),
            "only as far as the last three"
        );
        assert_eq!(
            input(shown, Input::Scroll(-1)),
            None,
            "already at the first"
        );
        assert_eq!(input("0;1*,2", Input::Scroll(1)), None);
    }
}
