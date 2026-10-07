//! The window list (M5.2h): a button for each window on a screen, in the
//! order the windows opened, each its app's icon, when the app has one,
//! then its title on one line, cut short with an ellipsis. The focused window's button is lit and marked by a short
//! accent line along its foot; another window's by a dot; a minimized
//! window's has no mark and dimmer text. Buttons share one width, at most
//! 180 logical pixels, and together take at most 45% of the panel, so
//! they never jump as titles change; when even their narrowest do not
//! fit, the last one stands for the windows left over, with their count,
//! and brings the first of them forward. A click on the focused window
//! minimizes it, and on any other brings it forward, back if minimized,
//! over wlr-foreign-toplevel-management (`crate::toplevels`). The icon is
//! found as the apps widget finds it: the installed app the window
//! belongs to, else its app id as an icon name.

use accesskit::Role;
use tiny_skia::{FilterQuality, PixmapPaint, Rect, Transform};

use edel::tokens::Colour;

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{fill, mix, paint_of};

pub const WIDGET: Widget = Widget {
    name: "windows",
    needs: None,
    shows,
    width,
    draw,
    input,
    role: Role::Group,
    label,
};

/// The windows' titles in order, each with its state: "Windows: Files
/// (focused), Mail (minimized)".
fn label(shown: &str) -> String {
    let titles: Vec<String> = shown
        .lines()
        .map(|line| {
            let mut chars = line.chars();
            let state = match chars.next() {
                Some('*') => " (focused)",
                Some('-') => " (minimized)",
                _ => "",
            };
            let rest = chars.as_str();
            let title = rest.split_once('\t').map_or(rest, |(_, title)| title);
            format!("{title}{state}")
        })
        .collect();
    format!("Windows: {}", titles.join(", "))
}

/// The widest a button is.
const WIDEST: f32 = 180.0;
/// The narrowest, when many windows share the room.
const NARROWEST: f32 = 32.0;
/// How much of the panel the buttons take at most.
const SHARE: f32 = 0.45;
/// Space at each end and between buttons.
const EDGE: f32 = 4.0;
const GAP: f32 = 4.0;
/// A button's height; its corners are the tokens' controls'.
const HEIGHT: f32 = 30.0;
/// The title's inset from each side of its button.
const PAD: f32 = 10.0;
/// The app's icon, and the room between it and the title.
const ICON: f32 = 16.0;
const ICON_GAP: f32 = 7.0;
/// The marks along a button's foot: the focused window's line and other
/// windows' dot, 2 px high, 2 px above the foot.
const LINE: f32 = 14.0;
const DOT: f32 = 4.0;

/// What it shows: a line for each window, its first character `*` for
/// the focused one, `-` for a minimized one, else a space, then its
/// app's icon name, a tab and its title. Empty without windows.
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
            let one_line = |text: &str| -> String {
                text.chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect()
            };
            let icon = live
                .installed
                .iter()
                .find(|p| super::apps::belongs(&task.app_id, &p.id))
                .map_or(task.app_id.as_str(), |p| p.icon.as_str());
            format!("{mark}{}\t{}", one_line(icon), one_line(&task.title))
        })
        .collect();
    lines.join("\n")
}

/// `shows`' text read back: each window's mark, icon name and title.
fn read(shown: &str) -> Vec<(char, &str, &str)> {
    if shown.is_empty() {
        return Vec::new();
    }
    shown
        .split('\n')
        .map(|line| {
            let mut chars = line.chars();
            let mark = chars.next().unwrap_or(' ');
            let rest = chars.as_str();
            let (icon, title) = rest.split_once('\t').unwrap_or(("", rest));
            (mark, icon, title)
        })
        .collect()
}

/// The most buttons a panel `panel` logical pixels wide holds.
fn fits(panel: f32) -> usize {
    (((panel * SHARE - 2.0 * EDGE + GAP) / (NARROWEST + GAP)).floor() as usize).max(1)
}

/// How many buttons `count` windows get: one each while they fit.
pub fn buttons(count: usize, panel: f32) -> usize {
    count.min(fits(panel))
}

/// A button's width for `count` windows on a panel `panel` logical pixels
/// wide.
pub fn button_width(count: usize, panel: f32) -> f32 {
    if count > fits(panel) {
        // Left over windows: the narrowest, so a click finds its button.
        return NARROWEST;
    }
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
    logical_width(buttons(count, panel), button_width(count, panel)) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let windows = read(shown);
    let panel = canvas.pixmap.width() as f32 / s;
    let button = button_width(windows.len(), panel);
    let drawn = buttons(windows.len(), panel);
    let more = windows.len() - drawn;
    let height = HEIGHT * s;
    let y = canvas.top + ((canvas.height - height) / 2.0).round();
    let lit = Colour {
        a: 0.08,
        ..tokens.panel_text
    };
    let size = (tokens.panel_text_size as f32 * 0.96).round() * s;
    for (i, (mark, icon, title)) in windows.into_iter().take(drawn).enumerate() {
        let bx = (x + (EDGE + i as f32 * (button + GAP)) * s).round();
        let bw = (button * s).round();
        // The last button, when windows are left over, counts them.
        if more > 0 && i + 1 == drawn {
            if let Some(text) = canvas.text.as_deref_mut() {
                let ink = mix(tokens.panel_text, tokens.panel, 0.25);
                let mut line = text.fit(&format!("+{}", more + 1), size, bw);
                let ty = y + (height - size * 1.25) / 2.0;
                let lx = bx + (bw - line.width) / 2.0;
                text.draw(canvas.pixmap, &mut line, lx, ty, ink);
            }
            continue;
        }
        if mark == '*' {
            let r = tokens.radius_control as f32 * s;
            fill(canvas.pixmap, bx, y, bw, height, r, lit);
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
        // The app's icon, when it has one and the button has room for
        // it; a button too narrow for a title shows the icon alone,
        // centred.
        let px = (ICON * s).round();
        let alone = bw < (2.0 * PAD + ICON + ICON_GAP + 24.0) * s;
        let ix = if alone {
            (bx + (bw - px) / 2.0).round()
        } else {
            (bx + PAD * s).round()
        };
        let iy = (y + (height - px) / 2.0 - s).round();
        let shown_icon = !icon.is_empty()
            && bw >= px
            && canvas
                .icons
                .as_deref_mut()
                .and_then(|icons| icons.get(icon, px as u32))
                .map(|picture| {
                    let paint = PixmapPaint {
                        quality: FilterQuality::Nearest,
                        ..PixmapPaint::default()
                    };
                    canvas.pixmap.draw_pixmap(
                        ix as i32,
                        iy as i32,
                        picture.as_ref(),
                        &paint,
                        Transform::identity(),
                        None,
                    );
                })
                .is_some();
        if shown_icon && alone {
            continue;
        }
        let tx = if shown_icon {
            ix + px + ICON_GAP * s
        } else {
            bx + PAD * s
        };
        if let Some(text) = canvas.text.as_deref_mut() {
            let mut line = text.fit(title, size, bx + bw - PAD * s - tx);
            let ty = y + (height - size * 1.25) / 2.0;
            text.draw(canvas.pixmap, &mut line, tx, ty, ink);
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
    // The buttons as drawn, from the list's own width: one for each
    // window unless that would make them narrower than the narrowest.
    let each = |k: usize| (width - 2.0 * EDGE - (k as f32 - 1.0) * GAP) / k as f32;
    let k = if each(n) < NARROWEST - 0.5 {
        (((width - 2.0 * EDGE + GAP) / (NARROWEST + GAP)).round() as usize).clamp(1, n)
    } else {
        n
    };
    let button = each(k);
    let along = at - EDGE;
    if along < 0.0 {
        return None;
    }
    let i = (along / (button + GAP)) as usize;
    if i >= k || along - i as f32 * (button + GAP) >= button {
        return None;
    }
    // The last button, when it counts the windows left over, brings the
    // first of them forward.
    if k < n && i + 1 == k {
        return Some(Action::Activate(i));
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
            app_id: "foot".into(),
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
        assert_eq!(shown, " foot\tfoot\n*foot\ttwo lines\n-foot\taway");
        assert_eq!(
            read(&shown),
            [
                (' ', "foot", "foot"),
                ('*', "foot", "two lines"),
                ('-', "foot", "away")
            ]
        );
        assert_eq!(shows(&live(Vec::new())), "");
        assert!(read("").is_empty());
    }

    #[test]
    fn a_window_shows_its_installed_apps_icon_else_its_app_id() {
        let mut files = task("Home", true, false);
        files.app_id = "org.gnome.Nautilus".into();
        let live = Live {
            windows: vec![files, task("term", false, false)],
            installed: vec![crate::widgets::Pin {
                id: "org.gnome.Nautilus".into(),
                name: "Files".into(),
                icon: "system-file-manager".into(),
            }],
            ..Live::default()
        };
        let shown = shows(&live);
        assert_eq!(
            read(&shown),
            [('*', "system-file-manager", "Home"), (' ', "foot", "term")]
        );
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
        // Forty never pass the share: fifteen buttons, the last for the
        // rest.
        assert_eq!(buttons(40, 1280.0), 15);
        assert!(logical_width(15, button_width(40, 1280.0)) <= 1280.0 * SHARE);
        assert_eq!(buttons(3, 1280.0), 3);
    }

    #[test]
    fn the_last_button_brings_the_windows_left_over_forward() {
        let shown: Vec<String> = (0..20).map(|i| format!(" w{i}")).collect();
        let shown = shown.join("\n");
        let width = logical_width(15, NARROWEST);
        let middle = |i: f32| EDGE + i * (NARROWEST + GAP) + 16.0;
        let click = |at| input(&shown, Input::Click(at, width));
        assert_eq!(click(middle(0.0)), Some(Action::Activate(0)));
        assert_eq!(click(middle(13.0)), Some(Action::Activate(13)));
        assert_eq!(click(middle(14.0)), Some(Action::Activate(14)));
        assert_eq!(click(middle(15.0)), None, "past the list");
    }

    #[test]
    fn a_click_minimizes_the_focused_window_and_brings_others_forward() {
        let shown = " foot\tfoot\n*\tone\n-\taway";
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

    #[test]
    fn a_screen_reader_hears_each_title_and_its_state() {
        assert_eq!(
            label("*files\tFiles\n-mail\tMail\n \tNotes"),
            "Windows: Files (focused), Mail (minimized), Notes"
        );
    }
}
