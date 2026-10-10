//! The window list (M5.2h): a button for each window on a screen, in the
//! order the windows opened, each its app's icon (a generic one when the
//! app has none) and its title on one line, cut short with an ellipsis,
//! drawn as the mockups draw it (M5.29). A button is as wide as its
//! content: the title's own width between the paddings, at most 200
//! logical pixels, so a short title never leaves a button empty. Buttons
//! together take at most 45% of the panel; when their natural widths do
//! not fit, the widest give way first, so every title keeps as much as
//! possible, and when even icons alone do not fit, the last button stands
//! for the windows left over, with their count, and brings the first of
//! them forward. The focused window's button is lit and marked by a short
//! accent line along its foot; another window's by a short neutral one; a
//! minimized window's has no mark, dimmer text and a fainter icon. A
//! click on the focused window minimizes it, and on any other brings it
//! forward, back if minimized, over wlr-foreign-toplevel-management
//! (`crate::toplevels`). The icon is found as the apps widget finds it:
//! the installed app the window belongs to, else its app id as an icon
//! name. Sizes come from the tokens (`size.panel_control`,
//! `size.panel_icon`, `size.panel_text`, `size.radius_control`).

use accesskit::Role;
use edel::i18n::{n_, trf};
use tiny_skia::{FilterQuality, PixmapPaint, Transform};

use edel::tokens::Colour;

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{Face, fill, mix};

pub const WIDGET: Widget = Widget {
    name: "windows",
    title: n_("Windows"),
    needs: None,
    shows,
    width,
    draw,
    input,
    parts: super::no_parts,
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
            let mark = chars.next();
            let rest = chars.as_str();
            let title = rest.split_once('\t').map_or(rest, |(_, title)| title);
            match mark {
                Some('*') => trf("{title} (focused)", &[("title", title)]),
                Some('-') => trf("{title} (minimized)", &[("title", title)]),
                _ => title.to_string(),
            }
        })
        .collect();
    trf("Windows: {titles}", &[("titles", &titles.join(", "))])
}

/// The widest a button is, as the mockups' `max-width`.
const WIDEST: f32 = 200.0;
/// How much of the panel the buttons take at most.
const SHARE: f32 = 0.45;
/// Space at each end and between buttons.
const EDGE: f32 = 2.0;
const GAP: f32 = 2.0;
/// A button's padding, at the icon's side and the title's, and the room
/// between the icon and the title.
const PAD_ICON: f32 = 7.0;
const PAD_TITLE: f32 = 10.0;
const ICON_GAP: f32 = 8.0;
/// The room a title needs for a button to show it beside the icon; with
/// less the icon stands alone, centred.
const LEAST_TITLE: f32 = 24.0;
/// The marks along a button's foot: the focused window's line and other
/// windows' short one, 2 px high, 2 px above the foot.
const LINE: f32 = 14.0;
const DOT: f32 = 4.0;
/// A minimized window's icon is this opaque.
const FADED: f32 = 0.5;

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

/// The narrowest a button is: the icon between its paddings, with no
/// title.
fn narrowest(icon: f32) -> f32 {
    2.0 * PAD_ICON + icon
}

/// The most buttons a panel `panel` logical pixels wide holds, each the
/// narrowest.
fn fits(panel: f32, icon: f32) -> usize {
    (((panel * SHARE - 2.0 * EDGE + GAP) / (narrowest(icon) + GAP)).floor() as usize).max(1)
}

/// How many buttons `count` windows get: one each while they fit.
pub fn buttons(count: usize, panel: f32, icon: f32) -> usize {
    count.min(fits(panel, icon))
}

/// How the buttons lie: each one's width in logical pixels, and how many
/// windows the last button stands for besides its own (none unless they
/// do not fit).
#[derive(Debug, PartialEq)]
struct Plan {
    widths: Vec<f32>,
    more: usize,
}

/// The widths of the buttons for windows whose titles are `titles`,
/// the titles' natural widths in logical pixels, on a panel `panel`
/// logical pixels wide, icons `icon` big. A button is its natural width
/// (the content and paddings, at most the widest); when they do not
/// fit, the widest are cut back to one common width, as wide as the room
/// allows, so short titles keep theirs.
fn plan(titles: &[f32], panel: f32, icon: f32) -> Plan {
    let count = titles.len();
    let least = narrowest(icon);
    if count == 0 {
        return Plan {
            widths: Vec::new(),
            more: 0,
        };
    }
    let drawn = buttons(count, panel, icon);
    if drawn < count {
        // Left over windows: the narrowest, so a click finds its button.
        return Plan {
            widths: vec![least; drawn],
            more: count - drawn,
        };
    }
    let room = panel * SHARE - 2.0 * EDGE - (count as f32 - 1.0) * GAP;
    let natural: Vec<f32> = titles
        .iter()
        .map(|t| {
            (PAD_ICON + icon + ICON_GAP + t + PAD_TITLE)
                .ceil()
                .min(WIDEST)
        })
        .collect();
    // The widest common limit that fits, whole pixels from the widest
    // down.
    let total = |limit: f32| natural.iter().map(|n| n.min(limit)).sum::<f32>();
    let mut limit = WIDEST;
    while limit > least && total(limit) > room {
        limit -= 1.0;
    }
    Plan {
        widths: natural.iter().map(|n| n.min(limit).max(least)).collect(),
        more: 0,
    }
}

/// The whole list's width for buttons `widths` wide, in logical pixels.
pub fn logical_width(widths: &[f32]) -> f32 {
    if widths.is_empty() {
        return 0.0;
    }
    2.0 * EDGE + widths.iter().sum::<f32>() + (widths.len() as f32 - 1.0) * GAP
}

/// The text size in the pixmap's pixels.
fn size(canvas: &Canvas) -> f32 {
    canvas.tokens.panel_text_size as f32 * canvas.scale
}

/// How the buttons for what is `shown` lie on this canvas's panel.
fn plan_for(canvas: &mut Canvas, windows: &[(char, &str, &str)]) -> Plan {
    let panel = canvas.pixmap.width() as f32 / canvas.scale;
    let icon = canvas.tokens.panel_icon as f32;
    let (size, s) = (size(canvas), canvas.scale);
    let titles: Vec<f32> = windows
        .iter()
        .map(|(_, _, title)| {
            canvas.text.as_deref_mut().map_or(0.0, |text| {
                text.line_in(title, size, Face::MEDIUM).width / s
            })
        })
        .collect();
    plan(&titles, panel, icon)
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    let windows = read(shown);
    let plan = plan_for(canvas, &windows);
    logical_width(&plan.widths) * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let windows = read(shown);
    let Plan { widths, more } = plan_for(canvas, &windows);
    let height = (tokens.panel_control as f32 * s).round();
    let y = canvas.top + ((canvas.height - height) / 2.0).round();
    let lit = Colour {
        a: 0.06,
        ..tokens.panel_text
    };
    let size = size(canvas);
    let px = (tokens.panel_icon as f32 * s).round();
    let mut along = EDGE;
    for (i, ((mark, icon, title), button)) in windows.into_iter().zip(&widths).enumerate() {
        let bx = (x + along * s).round();
        let bw = (button * s).round();
        along += button + GAP;
        // The last button, when windows are left over, counts them.
        if more > 0 && i + 1 == widths.len() {
            if let Some(text) = canvas.text.as_deref_mut() {
                let ink = mix(tokens.panel_text, tokens.panel, 0.3);
                let mut line = text.fit_in(&format!("+{}", more + 1), size, bw, Face::MEDIUM);
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
        // Text, then the mark, a little dimmer as a window matters less.
        let (ink, foot) = match mark {
            '*' => (tokens.panel_text, Some((LINE, tokens.accent))),
            '-' => (mix(tokens.panel_text, tokens.panel, 0.55), None),
            _ => (
                mix(tokens.panel_text, tokens.panel, 0.3),
                Some((DOT, mix(tokens.panel_text, tokens.panel, 0.55))),
            ),
        };
        if let Some((w, colour)) = foot {
            let (fw, fh) = ((w * s).round(), (2.0 * s).round());
            let fx = (bx + (bw - fw) / 2.0).round();
            let fy = y + height - 2.0 * fh;
            fill(canvas.pixmap, fx, fy, fw, fh, fh / 2.0, colour);
        }
        // The app's icon, else the generic one; a button too narrow for
        // a title shows the icon alone, centred.
        let room = bw - (PAD_ICON + PAD_TITLE + ICON_GAP) * s - px;
        let alone = room < LEAST_TITLE * s;
        let ix = if alone {
            (bx + (bw - px) / 2.0).round()
        } else {
            (bx + PAD_ICON * s).round()
        };
        let iy = (y + (height - px) / 2.0).round();
        let paint = PixmapPaint {
            quality: FilterQuality::Nearest,
            opacity: if mark == '-' { FADED } else { 1.0 },
            ..PixmapPaint::default()
        };
        let found = !icon.is_empty()
            && canvas
                .icons
                .as_deref_mut()
                .and_then(|icons| icons.get(icon, px as u32))
                .map(|picture| {
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
        if !found {
            let generic = Colour {
                a: if mark == '-' { FADED } else { 1.0 },
                ..mix(tokens.panel_text, tokens.panel, 0.35)
            };
            crate::paint::icon(canvas.pixmap, "app-generic", px, ix, iy, generic);
        }
        if alone {
            continue;
        }
        let tx = ix + px + ICON_GAP * s;
        if let Some(text) = canvas.text.as_deref_mut() {
            let mut line = text.fit_in(title, size, bx + bw - PAD_TITLE * s - tx, Face::MEDIUM);
            let ty = y + (height - size * 1.25) / 2.0;
            text.draw(canvas.pixmap, &mut line, tx, ty, ink);
        }
    }
}

/// A click on the focused window's button minimizes it; on another's,
/// brings that window forward.
fn input(canvas: &mut Canvas, shown: &str, input: Input) -> Option<Action> {
    let Input::Click(at, _) = input else {
        return None;
    };
    let windows = read(shown);
    if windows.is_empty() {
        return None;
    }
    // The buttons as drawn: the same plan, from the same panel.
    let Plan { widths, more } = plan_for(canvas, &windows);
    let mut left = EDGE;
    for (i, button) in widths.iter().enumerate() {
        if (left..left + button).contains(&at) {
            // The last button, when it counts the windows left over,
            // brings the first of them forward.
            if more > 0 && i + 1 == widths.len() {
                return Some(Action::Activate(i));
            }
            return Some(match windows[i].0 {
                '*' => Action::Minimize(i),
                _ => Action::Activate(i),
            });
        }
        left += button + GAP;
    }
    None
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

    /// A button's natural width for a title `text` px wide, icons 20.
    fn natural(text: f32) -> f32 {
        (PAD_ICON + 20.0 + ICON_GAP + text + PAD_TITLE).ceil()
    }

    #[test]
    fn buttons_hug_their_titles_and_never_pass_the_widest() {
        // On a 1280 px panel three short titles each keep their own width.
        let plan = plan(&[40.0, 90.0, 55.5], 1280.0, 20.0);
        assert_eq!(plan.widths, [natural(40.0), natural(90.0), natural(55.5)]);
        assert_eq!(plan.more, 0);
        assert!(plan.widths[0] < plan.widths[1], "a short title is narrower");
        // A very long title stops at 200.
        let plan = plan_long();
        assert_eq!(plan, [WIDEST]);
        assert_eq!(logical_width(&[]), 0.0);
        assert_eq!(logical_width(&[100.0, 50.0]), 4.0 + 150.0 + GAP);
    }

    fn plan_long() -> Vec<f32> {
        plan(&[900.0], 1280.0, 20.0).widths
    }

    #[test]
    fn the_widest_buttons_give_way_first_when_they_do_not_fit() {
        // 1280 px: 576 px for buttons. Eight with long titles do not fit
        // at their widest; they share one width, short ones keep theirs.
        let mut titles = vec![300.0; 7];
        titles.push(10.0);
        let plan = plan(&titles, 1280.0, 20.0);
        assert_eq!(plan.more, 0);
        let short = natural(10.0);
        assert_eq!(*plan.widths.last().unwrap(), short);
        let long = plan.widths[0];
        assert!(long < WIDEST && long > short, "{long}");
        assert!(plan.widths[..7].iter().all(|w| *w == long));
        assert!(logical_width(&plan.widths) <= 1280.0 * SHARE);
        // One pixel wider for the long ones would not have fitted.
        let wider: f32 = plan.widths[..7].iter().map(|w| w + 1.0).sum::<f32>() + short;
        assert!(wider + 2.0 * EDGE + 7.0 * GAP > 1280.0 * SHARE);
    }

    #[test]
    fn forty_windows_leave_icons_with_a_count_on_the_last() {
        let titles = vec![50.0; 40];
        let plan = plan(&titles, 1280.0, 20.0);
        assert_eq!(plan.widths.len(), buttons(40, 1280.0, 20.0));
        assert!(plan.widths.iter().all(|w| *w == narrowest(20.0)));
        assert_eq!(plan.more, 40 - plan.widths.len());
        assert!(logical_width(&plan.widths) <= 1280.0 * SHARE);
        // Three fit a phone's panel at the narrowest; the rest are counted.
        let phone = super::plan(&[50.0; 6], 360.0, 20.0);
        assert!(phone.more > 0);
        assert!(logical_width(&phone.widths) <= 360.0 * SHARE);
    }

    /// A canvas for hit tests: a one-pixel-high strip as wide as a panel.
    fn with_canvas<R>(panel: u32, run: impl FnOnce(&mut Canvas) -> R) -> R {
        let tokens = edel::tokens::Tokens::built_in();
        let mut pixmap = tiny_skia::Pixmap::new(panel, 1).unwrap();
        let mut canvas = Canvas {
            pixmap: &mut pixmap,
            tokens: &tokens,
            text: None,
            icons: None,
            scale: 1.0,
            top: 0.0,
            height: 1.0,
            dock: false,
            along_top: false,
        };
        run(&mut canvas)
    }

    #[test]
    fn the_last_button_brings_the_windows_left_over_forward() {
        let shown: Vec<String> = (0..20).map(|i| format!(" \tw{i}")).collect();
        let shown = shown.join("\n");
        with_canvas(1280, |canvas| {
            // Without fonts a title has no width: 38 px buttons, as many
            // as the share holds.
            let drawn = buttons(20, 1280.0, 20.0);
            assert!(drawn < 20);
            let at = |i: usize| EDGE + i as f32 * (narrowest(20.0) + GAP) + 10.0;
            let mut click = |at| input(canvas, &shown, Input::Click(at, 0.0));
            assert_eq!(click(at(0)), Some(Action::Activate(0)));
            assert_eq!(click(at(drawn - 2)), Some(Action::Activate(drawn - 2)));
            assert_eq!(click(at(drawn - 1)), Some(Action::Activate(drawn - 1)));
            assert_eq!(click(at(drawn)), None, "past the list");
        });
    }

    #[test]
    fn a_click_minimizes_the_focused_window_and_brings_others_forward() {
        let shown = " foot\tfoot\n*\tone\n-\taway";
        with_canvas(1280, |canvas| {
            // Three buttons of 37 px, without fonts to widen them.
            let each = natural(0.0);
            let middle = |i: f32| EDGE + i * (each + GAP) + each / 2.0;
            let width = logical_width(&[each; 3]);
            let mut click = |at| input(canvas, shown, Input::Click(at, width));
            assert_eq!(click(middle(0.0)), Some(Action::Activate(0)));
            assert_eq!(click(middle(1.0)), Some(Action::Minimize(1)));
            assert_eq!(click(middle(2.0)), Some(Action::Activate(2)));
            assert_eq!(click(1.0), None, "the edge is no button");
            assert_eq!(click(EDGE + each + 1.0), None, "nor the gap");
            assert_eq!(click(width + 1.0), None);
            assert_eq!(input(canvas, "", Input::Click(10.0, 0.0)), None);
            assert_eq!(input(canvas, shown, Input::Scroll(1)), None);
        });
    }

    #[test]
    fn a_screen_reader_hears_each_title_and_its_state() {
        assert_eq!(
            label("*files\tFiles\n-mail\tMail\n \tNotes"),
            "Windows: Files (focused), Mail (minimized), Notes"
        );
    }
}
