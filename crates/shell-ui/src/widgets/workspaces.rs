//! The workspace switcher (M5.2c): round buttons numbered as Super+1 to
//! Super+9 are, the shown workspace a wider pill in the accent colour. At
//! most three show at once, the shown workspace and its neighbours, so the
//! panel keeps its width whatever the preset's count; where more lie, the
//! next one's button peeks in at that side, fading out, as the mockups
//! draw it, and the wheel or a touchpad scrolls to it, or a click on it
//! does. A click on a button shows its workspace through ext-workspace-v1
//! (`crate::workspaces`). Sizes are logical pixels, drawn at the panel's
//! scale.

use accesskit::Role;
use edel::i18n::{tr, trf};
use tiny_skia::{FillRule, Pixmap, PixmapPaint, Transform};

use edel::tokens::Colour;

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{Text, paint_of, rounded};

pub const WIDGET: Widget = Widget {
    name: "workspaces",
    needs: None,
    shows,
    width,
    draw,
    input,
    role: Role::Group,
    label,
};

/// The shown workspace among how many: "Workspaces: 2 shown, 2 of 4".
fn label(shown: &str) -> String {
    let names: Vec<&str> = shown
        .split_once(';')
        .map_or_else(Vec::new, |(_, names)| names.split(',').collect());
    match names.iter().position(|name| name.ends_with('*')) {
        Some(i) => trf(
            "Workspaces: {name} shown, {n} of {count}",
            &[
                ("name", names[i].trim_end_matches('*')),
                ("n", &(i + 1).to_string()),
                ("count", &names.len().to_string()),
            ],
        ),
        None => tr("Workspaces").into(),
    }
}

/// How many buttons show at once.
pub const SHOWN: usize = 3;
/// A button's height, and its width unless it is the shown workspace's.
const BUTTON: f32 = 20.0;
/// The shown workspace's pill.
const PILL: f32 = 32.0;
const GAP: f32 = 5.0;
/// How much of the next button peeks in where more workspaces lie,
/// fading out towards the widget's end.
const PEEK: f32 = 14.0;
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
    let mut x = ROOM + if names.len() > SHOWN { PEEK + GAP } else { 0.0 };
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
    let peeks = if names.len() > SHOWN {
        2.0 * (PEEK + GAP)
    } else {
        0.0
    };
    2.0 * ROOM + peeks + PILL + (shown_count - 1.0) * (BUTTON + GAP)
}

/// The buttons peeking in: the one before the first shown, on the left,
/// and the one after the last, on the right, each with its name, the
/// left edge of its strip and whether it is on the left.
pub fn peeks(shown: &str) -> Vec<(&str, f32, bool)> {
    let (first, names) = read(shown);
    let mut out = Vec::new();
    if first > 0 {
        out.push((names[first - 1].0, ROOM, true));
    }
    if let Some(&(name, _)) = names.get(first + SHOWN) {
        out.push((name, logical_width(shown) - ROOM - PEEK, false));
    }
    out
}

/// Draws a workspace's button, `on` if it is the shown one, at `x`, `y`
/// in `pixmap`, `w` by `h` pixels.
#[allow(clippy::too_many_arguments)]
fn button(
    pixmap: &mut Pixmap,
    text: Option<&mut Text>,
    tokens: &edel::tokens::Tokens,
    name: &str,
    on: bool,
    (x, y, w, h): (f32, f32, f32, f32),
    size: f32,
) {
    let off = Colour {
        a: 0.14,
        ..tokens.panel_text
    };
    if let Some(path) = rounded(x, y, w, h, h / 2.0) {
        let fill = paint_of(if on { tokens.accent } else { off });
        pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);
    }
    if let Some(text) = text {
        let mut line = text.line(name, size);
        let tx = x + (w - line.width) / 2.0;
        let ty = y + (h - size * 1.25) / 2.0;
        let ink = if on { tokens.panel } else { tokens.panel_text };
        text.draw(pixmap, &mut line, tx, ty, ink);
    }
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    logical_width(shown) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let height = BUTTON * s;
    let y = canvas.top + ((canvas.height - height) / 2.0).round();
    let size = (tokens.panel_text_size as f32 * 0.82).round() * s;
    for (name, on, left, w) in buttons(shown) {
        let place = ((x + left * s).round(), y, w * s, height);
        button(
            canvas.pixmap,
            canvas.text.as_deref_mut(),
            tokens,
            name,
            on,
            place,
            size,
        );
    }
    // The next button at each side where more lie: drawn whole into a
    // strip PEEK wide, the part beyond it cut off, then faded towards
    // the widget's end.
    let strip = (PEEK * s).round();
    for (name, left, on_left) in peeks(shown) {
        let Some(mut peek) = Pixmap::new(strip as u32, height.ceil() as u32) else {
            continue;
        };
        let bx = if on_left { strip - BUTTON * s } else { 0.0 };
        button(
            &mut peek,
            canvas.text.as_deref_mut(),
            tokens,
            name,
            false,
            (bx, 0.0, BUTTON * s, height),
            size,
        );
        fade(&mut peek, on_left);
        canvas.pixmap.draw_pixmap(
            (x + left * s).round() as i32,
            y as i32,
            peek.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
}

/// Fades `pixmap` out towards its left side if `to_left`, else its right:
/// full at the inner side, nothing at the outer one.
fn fade(pixmap: &mut Pixmap, to_left: bool) {
    let w = pixmap.width() as usize;
    for (i, pixel) in pixmap.data_mut().chunks_exact_mut(4).enumerate() {
        let column = (i % w) as f32 + 0.5;
        let kept = if to_left {
            column / w as f32
        } else {
            1.0 - column / w as f32
        };
        for channel in pixel {
            *channel = (f32::from(*channel) * kept).round() as u8;
        }
    }
}

/// A click shows the workspace under it; a scroll moves the view by its
/// steps, keeping three in sight.
fn input(shown: &str, input: Input) -> Option<Action> {
    match input {
        Input::Click(at, _) => {
            if let Some(&(_, _, on_left)) = peeks(shown)
                .iter()
                .find(|(_, left, _)| (*left..left + PEEK).contains(&at))
            {
                // A click on a peeking button scrolls one step towards it.
                let (first, _) = read(shown);
                return Some(Action::View(if on_left { first - 1 } else { first + 1 }));
            }
            buttons(shown)
                .into_iter()
                .find(|(_, _, left, w)| (*left..left + w).contains(&at))
                .map(|(name, ..)| Action::Show(name.to_string()))
        }
        Input::Scroll(steps) => {
            let (first, names) = read(shown);
            let last = names.len().saturating_sub(SHOWN) as i64;
            let to = (first as i64 + i64::from(steps)).clamp(0, last) as usize;
            (to != first).then_some(Action::View(to))
        }
        Input::Menu(..) => None,
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
        // Room, then the peek's place on the left, as more lie to the
        // right.
        assert_eq!((first[0].2, first[0].3), (ROOM + PEEK + GAP, PILL));
        assert_eq!(first[1].2, ROOM + PEEK + GAP + PILL + GAP);
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

    #[test]
    fn the_next_workspace_peeks_in_where_more_lie_and_a_click_scrolls_to_it() {
        // At the start, only 4 peeks in, at the right end.
        let start = "0;1*,2,3,4";
        let width = logical_width(start);
        assert_eq!(peeks(start), [("4", width - ROOM - PEEK, false)]);
        // In the middle of nine, one at each side.
        let middle = "3;1,2,3,4,5*,6,7,8,9";
        assert_eq!(
            peeks(middle),
            [("3", ROOM, true), ("7", width - ROOM - PEEK, false)]
        );
        assert_eq!(
            input(middle, Input::Click(ROOM + 1.0, 0.0)),
            Some(Action::View(2))
        );
        assert_eq!(
            input(middle, Input::Click(width - ROOM - 1.0, 0.0)),
            Some(Action::View(4))
        );
        assert!(peeks("0;1*,2,3").is_empty());
    }

    #[test]
    fn a_peek_fades_out_towards_the_outer_side() {
        let mut strip = Pixmap::new(10, 2).unwrap();
        strip.fill(tiny_skia::Color::WHITE);
        fade(&mut strip, false);
        let alpha = |p: &Pixmap, x: u32| p.pixel(x, 0).unwrap().alpha();
        assert!(alpha(&strip, 0) > 230 && alpha(&strip, 9) < 20);
        let mut strip = Pixmap::new(10, 2).unwrap();
        strip.fill(tiny_skia::Color::WHITE);
        fade(&mut strip, true);
        assert!(alpha(&strip, 0) < 20 && alpha(&strip, 9) > 230);
    }

    #[test]
    fn a_screen_reader_hears_the_shown_workspace() {
        assert_eq!(label("0;1,2*,3,4"), "Workspaces: 2 shown, 2 of 4");
        assert_eq!(label(""), "Workspaces");
    }
}
