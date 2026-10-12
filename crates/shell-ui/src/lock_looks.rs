//! The looks side by side on the lock (M5.10c7h, frame 2 of the approved
//! "Lock looks" board): the look in use large in the middle, its neighbours
//! cut by the screen's edges, the look's title, Customize and Use this look,
//! one dot per look and a plus, and a line of hints. Plain data, layout, hit
//! testing, keys, places, the reader's nodes and drawing; tested without a
//! display, with PNGs when `EDEL_LOCK_PNG` is set. `lock_looks_card.rs` owns
//! the surface, the input and the writes.

use accesskit::Role;
use edel::i18n::{tr, trf};
use edel::lock_screen;
use edel::settings::LockScreen;
use edel::tokens::{Colour, Tokens};
use edel::wallpapers::Wallpaper;
use tiny_skia::{FillRule, PathBuilder, Pixmap, Transform};

use crate::a11y::Item;
use crate::edit_kit::{self, ButtonKind, State, Style};
use crate::lock;
use crate::lock_editor;
use crate::paint::{self as paint_kit, Face, Text, fill, paint_of};
use crate::popup::{self, Rect};

/// The middle look's width, a share of the screen's, at most `MIDDLE_MOST`
/// logical pixels; its height is 16:10 of that (the board's frame 2).
pub const MIDDLE_SHARE: f32 = 0.62;
pub const MIDDLE_MOST: f32 = 800.0;
/// A neighbour's size, a share of the middle look's, and its gap from it.
pub const SIDE_SHARE: f32 = 0.55;
pub const SIDE_GAP: f32 = 28.0;
/// How much a neighbour shows: its picture at this opacity.
pub const SIDE_OPACITY: f32 = 0.7;
/// The picture's corners, and the white ring round the middle one: 2 px
/// wide, 3 px outside it, at 90 percent.
pub const RADIUS: f32 = 14.0;
pub const RING: f32 = 2.0;
pub const RING_GAP: f32 = 3.0;
pub const RING_ALPHA: f32 = 0.9;
/// The look's title, 16 px under the picture, and the buttons 12 px under it.
pub const TITLE_GAP: f32 = 16.0;
pub const TITLE_HEIGHT: f32 = 18.0;
pub const TITLE_BUTTONS: f32 = 12.0;
pub const TITLE_SIZE: f32 = 15.0;
/// The two pills: 150 px wide, 32 px high (44 on Compact), 10 px apart.
pub const BUTTON_WIDTH: f32 = 150.0;
pub const BUTTON_HEIGHT: f32 = 32.0;
pub const BUTTON_TOUCH: f32 = 44.0;
pub const BUTTON_GAP: f32 = 10.0;
/// The dots: 6 px each, 8 px apart; the plus is a 22 px round, 12 px after
/// the last dot, and the row lies 18 px under the buttons.
pub const DOT: f32 = 6.0;
pub const DOT_GAP: f32 = 8.0;
pub const PLUS: f32 = 22.0;
pub const PLUS_AFTER: f32 = 12.0;
pub const ROW_BELOW: f32 = 18.0;
/// The hint, 14 px under the row, 11.5 px and half white (a second line on
/// Compact).
pub const HINT_BELOW: f32 = 14.0;
pub const HINT_SIZE: f32 = 11.5;
pub const HINT_LINE: f32 = 16.0;
/// The side margin of a Compact screen's middle look, and the gutter the
/// hint keeps from each side (16 px, as the phone layouts keep).
pub const COMPACT_MARGIN: f32 = 24.0;
const HINT_GUTTER: f32 = 16.0;
/// The part of a dot's row a press may land on, round the dot (logical px).
const DOT_HIT: f32 = 7.0;

/// The parts a look's switcher has that a person may point at or focus.
/// `Left` and `Right` are the neighbours: a click steps to them, and they
/// are not in the keyboard's ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Customize,
    Use,
    Plus,
    Dot(usize),
    Left,
    Right,
}

/// The keyboard's ring, in Tab's order.
pub const TAB_ORDER: [Part; 3] = [Part::Customize, Part::Use, Part::Plus];

/// What the switcher shows: the looks in order, the one shown in the middle,
/// the one in use, the keyboard's focus and the pointer's part.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub screen: (f32, f32),
    pub compact: bool,
    pub names: Vec<String>,
    pub shown: usize,
    pub in_use: usize,
    pub focus: Option<Part>,
    pub hover: Option<Part>,
}

impl View {
    /// Whether the middle look is the one in use.
    pub fn using_shown(&self) -> bool {
        self.shown == self.in_use
    }
}

/// Where each part lies, in logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub middle: Rect,
    pub left: Option<Rect>,
    pub right: Option<Rect>,
    pub title: Rect,
    pub customize: Rect,
    pub using: Rect,
    pub plus: Rect,
    /// Each dot's place (its centre's box) and its hit box, one per look.
    pub dots: Vec<(Rect, Rect)>,
    pub hint: Rect,
    /// The buttons' height (44 on Compact).
    pub button_height: f32,
}

/// Lays everything out for `view`: the middle look centred, the title and
/// buttons under it, the dots and the plus, then the hint; the whole stack is
/// centred on the screen, with neighbours on each side.
pub fn layout(view: &View) -> Layout {
    let (w, h) = view.screen;
    let mw = if view.compact {
        (w - 2.0 * COMPACT_MARGIN).max(1.0)
    } else {
        (w * MIDDLE_SHARE).min(MIDDLE_MOST)
    };
    let mh = mw * 10.0 / 16.0;
    let button_height = if view.compact {
        BUTTON_TOUCH
    } else {
        BUTTON_HEIGHT
    };
    let hint_height = if view.compact {
        2.0 * HINT_LINE
    } else {
        HINT_LINE
    };
    let stack = mh
        + TITLE_GAP
        + TITLE_HEIGHT
        + TITLE_BUTTONS
        + button_height
        + ROW_BELOW
        + PLUS
        + HINT_BELOW
        + hint_height;
    let top = ((h - stack) / 2.0).max(16.0);
    let mx = (w - mw) / 2.0;
    let middle = Rect::new(mx, top, mw, mh);
    let title = Rect::new(mx, top + mh + TITLE_GAP, mw, TITLE_HEIGHT);
    let buttons_y = title.y + TITLE_HEIGHT + TITLE_BUTTONS;
    let buttons_w = 2.0 * BUTTON_WIDTH + BUTTON_GAP;
    let bx = (w - buttons_w) / 2.0;
    let customize = Rect::new(bx, buttons_y, BUTTON_WIDTH, button_height);
    let using = Rect::new(
        bx + BUTTON_WIDTH + BUTTON_GAP,
        buttons_y,
        BUTTON_WIDTH,
        button_height,
    );
    let row_y = buttons_y + button_height + ROW_BELOW;
    let count = view.names.len();
    let dots_w = if count == 0 {
        0.0
    } else {
        count as f32 * DOT + (count - 1) as f32 * DOT_GAP
    };
    let row_x = (w - (dots_w + PLUS_AFTER + PLUS)) / 2.0;
    let dots = (0..count)
        .map(|i| {
            let x = row_x + i as f32 * (DOT + DOT_GAP);
            let cy = row_y + PLUS / 2.0;
            let place = Rect::new(x, cy - DOT / 2.0, DOT, DOT);
            let hit = Rect::new(
                x + DOT / 2.0 - DOT_HIT,
                cy - DOT_HIT,
                2.0 * DOT_HIT,
                2.0 * DOT_HIT,
            );
            (place, hit)
        })
        .collect();
    let plus = Rect::new(row_x + dots_w + PLUS_AFTER, row_y, PLUS, PLUS);
    let hint = Rect::new(0.0, row_y + PLUS + HINT_BELOW, w, hint_height);
    let side_w = mw * SIDE_SHARE;
    let side_h = mh * SIDE_SHARE;
    let side_y = top + (mh - side_h) / 2.0;
    let left = (view.shown > 0).then(|| Rect::new(mx - SIDE_GAP - side_w, side_y, side_w, side_h));
    let right =
        (view.shown + 1 < count).then(|| Rect::new(mx + mw + SIDE_GAP, side_y, side_w, side_h));
    Layout {
        middle,
        left,
        right,
        title,
        customize,
        using,
        plus,
        dots,
        hint,
        button_height,
    }
}

fn contains(r: Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// The part under a point, if any: a button, the plus, a dot's hit box or a
/// neighbour. The middle look itself is no part.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Part> {
    if contains(l.customize, x, y) {
        return Some(Part::Customize);
    }
    if contains(l.using, x, y) {
        return Some(Part::Use);
    }
    if contains(l.plus, x, y) {
        return Some(Part::Plus);
    }
    if let Some(i) = l.dots.iter().position(|(_, hit)| contains(*hit, x, y)) {
        return Some(Part::Dot(i));
    }
    if l.left.is_some_and(|r| contains(r, x, y)) {
        return Some(Part::Left);
    }
    if l.right.is_some_and(|r| contains(r, x, y)) {
        return Some(Part::Right);
    }
    None
}

/// The look a step moves to: one left or right, never past either end and
/// never round (Left and Right do not wrap).
pub fn step(shown: usize, count: usize, forward: bool) -> usize {
    if forward {
        (shown + 1).min(count.saturating_sub(1))
    } else {
        shown.saturating_sub(1)
    }
}

/// The keyboard's next focus from `current` (Tab, or Shift+Tab when `back`):
/// the ring's first part from nothing, and nothing past its ends.
pub fn next_focus(current: Option<Part>, back: bool) -> Option<Part> {
    let ring = TAB_ORDER;
    let at = current.and_then(|part| ring.iter().position(|p| *p == part));
    match (at, back) {
        (None, false) => Some(ring[0]),
        (None, true) => Some(ring[ring.len() - 1]),
        (Some(i), false) => ring.get(i + 1).copied().or(Some(ring[i])),
        (Some(i), true) => Some(ring[i.saturating_sub(1)]),
    }
}

/// `X+Y+WxH` of a rectangle, whole logical pixels, as CI reads it.
fn at(r: Rect) -> String {
    format!(
        "{}+{}+{}x{}",
        r.x.round() as i64,
        r.y.round() as i64,
        r.w.round() as i64,
        r.h.round() as i64
    )
}

/// The places line the log shows once per change (M5.10c7h): the screen,
/// the middle look, the buttons, the plus and each dot with its look's name.
pub fn places(view: &View, l: &Layout) -> String {
    let mut words = vec![
        format!("screen {}x{}", view.screen.0.round(), view.screen.1.round()),
        format!("middle {}", at(l.middle)),
        format!("customize {}", at(l.customize)),
        format!("use {}", at(l.using)),
        format!("plus {}", at(l.plus)),
    ];
    for (i, (name, (place, _))) in view.names.iter().zip(&l.dots).enumerate() {
        words.push(format!("dot{i} {name} {}", at(*place)));
    }
    words.join(", ")
}

/// The title of the look `name` as people read it.
pub fn title_of(name: &str) -> String {
    lock_screen::look_title(name)
}

/// A rectangle as the reader's tree holds it.
fn bounds(r: Rect) -> accesskit::Rect {
    accesskit::Rect::new(
        f64::from(r.x),
        f64::from(r.y),
        f64::from(r.x + r.w),
        f64::from(r.y + r.h),
    )
}

/// The reader's items with the part each one is (for the focus), in Tab's
/// order first, then the rest.
fn items(view: &View, l: &Layout) -> Vec<(Option<Part>, Item)> {
    let title = view
        .names
        .get(view.shown)
        .map(|n| title_of(n))
        .unwrap_or_default();
    let button = |role, label: String, rect: Rect| Item {
        role,
        label,
        bounds: bounds(rect),
        children: Vec::new(),
        toggled: None,
        value: None,
    };
    let mut out = vec![
        (
            Some(Part::Customize),
            button(Role::Button, tr("Customize").to_string(), l.customize),
        ),
        (
            Some(Part::Use),
            button(
                Role::Button,
                if view.using_shown() {
                    tr("In use").to_string()
                } else {
                    tr("Use this look").to_string()
                },
                l.using,
            ),
        ),
        (
            Some(Part::Plus),
            button(
                Role::Button,
                tr("New look from this one").to_string(),
                l.plus,
            ),
        ),
    ];
    for (i, name) in view.names.iter().enumerate() {
        let label = trf("Look {title}", &[("title", &title_of(name))]);
        out.push((Some(Part::Dot(i)), button(Role::Button, label, l.dots[i].1)));
    }
    if let Some(rect) = l.left {
        let label = tr("Previous look").to_string();
        out.push((Some(Part::Left), button(Role::Button, label, rect)));
    }
    if let Some(rect) = l.right {
        let label = tr("Next look").to_string();
        out.push((Some(Part::Right), button(Role::Button, label, rect)));
    }
    out.insert(0, (None, button(Role::Label, title, l.middle)));
    out
}

/// What a screen reader finds on the switcher, in order.
pub fn nodes(view: &View, l: &Layout) -> Vec<Item> {
    items(view, l).into_iter().map(|(_, item)| item).collect()
}

/// The index in `nodes` of the focused part, for the reader's focus.
pub fn focus_index(view: &View, l: &Layout) -> Option<usize> {
    let focus = view.focus?;
    items(view, l)
        .iter()
        .position(|(part, _)| *part == Some(focus))
}

/// What a look's picture is drawn with: the colours, the fonts, the icons and
/// the caches the lock's own drawing keeps.
pub struct Kit<'a> {
    pub tokens: &'a Tokens,
    pub text: &'a mut Text,
    pub icons: &'a mut edel::app_icons::Icons,
    pub pictures: &'a mut lock::Pictures,
    pub backdrops: &'a mut lock::Backdrops,
    pub wallpaper: &'a Wallpaper,
}

/// The size a look's picture is laid out at: the sample screen of
/// `look_picture.rs`, 1280 by 800, since at `lock_editor::THUMB` (960 by 600)
/// the classic layout leaves its clock out (M5.10c7h; the pictures are scaled
/// down to the cards, so nothing is cut).
pub const PICTURE: (u32, u32) = (1280, 800);

/// A look's picture as the lock draws it, at `PICTURE` (the switcher's
/// neighbours and middle are drawn from it): the look alone, as Settings'
/// cards apply it (`lock_screen::applied` of that look). `view` is the lock's
/// view of the time and the person, with no notices.
pub fn look_picture(
    kit: Kit,
    name: &str,
    compact: bool,
    view: &lock::View,
    battery: bool,
) -> Option<Pixmap> {
    let mut lock = lock_screen::applied(
        &LockScreen {
            look: Some(name.to_string()),
            ..LockScreen::default()
        },
        compact,
    );
    // The switcher's pictures show no notifications (the board's frame 2): the
    // count part is left out, as `lock_screen.notifications = "none"` leaves it.
    lock.parts
        .retain(|placed| placed.part != lock_screen::Part::Count);
    let now = jiff::Zoned::now();
    let mut view = view.clone();
    // Each clock shows the time in its own zone, as the lock does (M5.10c7f).
    view.zoned = lock::zoned_of_parts(&lock.parts, &now, &std::cell::RefCell::new(Vec::new()));
    lock_editor::thumbnail(
        &view,
        &lock.parts,
        &lock.background,
        lock.dim,
        battery,
        kit.tokens,
        kit.text,
        Some(kit.icons),
        kit.pictures,
        kit.backdrops,
        kit.wallpaper,
        PICTURE,
        None,
    )
}

/// Draws the switcher: the glass stage, the neighbours at 70 percent, the
/// middle look with its ring, the title, the two buttons, the dots and the
/// plus, and the hint. `pictures` are the looks before the shown one, the
/// shown one and the one after it, as drawn at `lock_editor::THUMB`. `s` is
/// the device scale.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    l: &Layout,
    pictures: [Option<&Pixmap>; 3],
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let (w, h) = view.screen;
    let style = Style::GLASS;
    edit_kit::stage(
        pixmap,
        Rect::new(0.0, 0.0, w, h),
        l.middle,
        tokens,
        style,
        s,
    );
    let white = |a: f32| Colour {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a,
    };
    // The neighbours: the look before and after, dimmed to 70 percent by
    // laying the stage's colour over them, with no ring.
    for (rect, picture) in [(l.left, pictures[0]), (l.right, pictures[2])] {
        if let (Some(rect), Some(picture)) = (rect, picture) {
            let (x, y, dw, dh) = rect.device(s);
            paint_kit::rounded_picture(pixmap, picture, (x, y, dw, dh), RADIUS * s);
            fill(
                pixmap,
                x,
                y,
                dw,
                dh,
                RADIUS * s,
                Colour {
                    a: 1.0 - SIDE_OPACITY,
                    ..tokens.lock_edit_deep
                },
            );
        }
    }
    if let Some(picture) = pictures[1] {
        let (x, y, dw, dh) = l.middle.device(s);
        paint_kit::rounded_picture(pixmap, picture, (x, y, dw, dh), RADIUS * s);
    }
    let grow = (RING_GAP + RING / 2.0) * s;
    let (x, y, dw, dh) = l.middle.device(s);
    edit_kit::stroke(
        pixmap,
        (x - grow, y - grow, dw + 2.0 * grow, dh + 2.0 * grow),
        (RADIUS + RING_GAP + RING / 2.0) * s,
        RING * s,
        white(RING_ALPHA),
        None,
    );
    // The title, 15 px semibold white.
    if let Some(name) = view.names.get(view.shown) {
        let size = TITLE_SIZE * s;
        let mut line = text.line_in(&title_of(name), size, Face::SEMIBOLD);
        let (x, _, dw, _) = l.title.device(s);
        let at = x + ((dw - line.width) / 2.0).round();
        let baseline = popup::middle(l.title.y, l.title.h, size, s);
        text.draw(pixmap, &mut line, at, baseline, white(1.0));
    }
    // Customize is the light pill; Use this look is the quiet outline, and
    // reads In use, dimmed, while the look shown is the one in use.
    let state = |part: Part| {
        if view.focus == Some(part) {
            State::Focus
        } else if view.hover == Some(part) {
            State::Hover
        } else {
            State::Rest
        }
    };
    edit_kit::button(
        pixmap,
        l.customize,
        tr("Customize"),
        ButtonKind::Light,
        state(Part::Customize),
        tokens,
        Some(text),
        style,
        s,
    );
    let using_label = if view.using_shown() {
        tr("In use")
    } else {
        tr("Use this look")
    };
    edit_kit::button(
        pixmap,
        l.using,
        using_label,
        ButtonKind::Quiet,
        state(Part::Use),
        tokens,
        Some(text),
        style,
        s,
    );
    if view.using_shown() {
        let (x, y, dw, dh) = l.using.device(s);
        fill(
            pixmap,
            x,
            y,
            dw,
            dh,
            dh / 2.0,
            Colour {
                a: 0.35,
                ..tokens.lock_edit_deep
            },
        );
    }
    // The dots: the shown one white, the rest white at 35 percent.
    for (i, (place, _)) in l.dots.iter().enumerate() {
        let shown = i == view.shown;
        let colour = white(if shown { 1.0 } else { 0.35 });
        let (cx, cy) = ((place.x + place.w / 2.0) * s, (place.y + place.h / 2.0) * s);
        fill_circle(pixmap, cx, cy, DOT / 2.0 * s, colour);
    }
    // The plus: a round at 12 percent white with a white glyph.
    let (x, y, dw, dh) = l.plus.device(s);
    fill(pixmap, x, y, dw, dh, dw / 2.0, white(0.12));
    let (cx, cy) = (x + dw / 2.0, y + dh / 2.0);
    let arm = 5.0 * s;
    let thick = 0.8 * s;
    fill(
        pixmap,
        cx - arm,
        cy - thick,
        2.0 * arm,
        2.0 * thick,
        0.0,
        white(1.0),
    );
    fill(
        pixmap,
        cx - thick,
        cy - arm,
        2.0 * thick,
        2.0 * arm,
        0.0,
        white(1.0),
    );
    if view.focus == Some(Part::Plus) {
        popup::focus_ring(pixmap, l.plus, l.plus.w / 2.0, s, tokens);
    }
    // The hint, 11.5 px and half white; two lines on Compact.
    let hint =
        [
            tr("Left and right, or a swipe, move between looks. Return uses it, Escape leaves.")
                .to_string(),
        ];
    // Kept 16 px from each side of the screen, as the phone's gutter is.
    let room = (l.hint.w - 2.0 * HINT_GUTTER).max(1.0) * s;
    let size = HINT_SIZE * s;
    let lines = if view.compact {
        popup::wrap(&hint[0], 2, room, |t| text.line(t, size).width)
    } else {
        hint.to_vec()
    };
    for (n, words) in lines.iter().enumerate() {
        let mut line = text.line(words, size);
        let x = ((room - line.width) / 2.0).round();
        let top = l.hint.y + n as f32 * HINT_LINE;
        let baseline = popup::middle(top, HINT_LINE, size, s);
        text.draw(pixmap, &mut line, x, baseline, white(0.5));
    }
}

/// A round of `colour` centred at (`cx`, `cy`), device pixels.
fn fill_circle(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, colour: Colour) {
    if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
        pixmap.fill_path(
            &path,
            &paint_of(colour),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(screen: (f32, f32), names: &[&str], shown: usize) -> View {
        View {
            screen,
            compact: screen.0 < lock::COMPACT_BELOW,
            names: names.iter().map(|n| n.to_string()).collect(),
            shown,
            in_use: 0,
            focus: None,
            hover: None,
        }
    }

    fn inside(r: Rect, (w, h): (f32, f32)) -> bool {
        r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= w && r.y + r.h <= h
    }

    #[test]
    fn the_middle_look_is_62_percent_of_a_wide_screen_at_16_to_10() {
        let v = view((1280.0, 800.0), &["classic", "magazine", "minimal"], 0);
        let l = layout(&v);
        assert!((l.middle.w - 794.0).abs() < 1.0, "{:?}", l.middle);
        assert!((l.middle.h / l.middle.w - 10.0 / 16.0).abs() < 0.001);
        assert!((l.middle.x + l.middle.w / 2.0 - 640.0).abs() < 0.5);
    }

    #[test]
    fn the_middle_look_is_at_most_800_px_wide() {
        let v = view((2560.0, 1440.0), &["classic"], 0);
        assert_eq!(layout(&v).middle.w, MIDDLE_MOST);
    }

    #[test]
    fn the_stack_lies_on_the_screen_at_desktop_and_phone_sizes() {
        for screen in [(1280.0, 800.0), (390.0, 844.0), (1920.0, 1080.0)] {
            let v = view(screen, &["classic", "magazine", "minimal", "work"], 1);
            let l = layout(&v);
            assert!(inside(l.middle, screen), "{screen:?} {:?}", l.middle);
            assert!(inside(l.customize, screen));
            assert!(inside(l.using, screen));
            assert!(inside(l.plus, screen));
            assert!(inside(l.title, screen));
            assert!(l.dots.iter().all(|(p, _)| inside(*p, screen)));
        }
    }

    #[test]
    fn neighbours_are_55_percent_and_28_px_from_the_middle_and_none_at_the_ends() {
        let v = view((1280.0, 800.0), &["classic", "magazine", "minimal"], 1);
        let l = layout(&v);
        let (left, right) = (l.left.unwrap(), l.right.unwrap());
        assert!((left.w - l.middle.w * SIDE_SHARE).abs() < 0.01);
        assert!((l.middle.x - (left.x + left.w) - SIDE_GAP).abs() < 0.01);
        assert!((right.x - (l.middle.x + l.middle.w) - SIDE_GAP).abs() < 0.01);
        assert!((left.y + left.h / 2.0 - (l.middle.y + l.middle.h / 2.0)).abs() < 0.01);
        let first = layout(&view((1280.0, 800.0), &["a", "b"], 0));
        assert!(first.left.is_none() && first.right.is_some());
        let last = layout(&view((1280.0, 800.0), &["a", "b"], 1));
        assert!(last.left.is_some() && last.right.is_none());
    }

    #[test]
    fn a_press_finds_the_part_under_it() {
        let v = view((1280.0, 800.0), &["classic", "magazine", "minimal"], 0);
        let l = layout(&v);
        let centre = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (x, y) = centre(l.customize);
        assert_eq!(hit(&l, x, y), Some(Part::Customize));
        let (x, y) = centre(l.using);
        assert_eq!(hit(&l, x, y), Some(Part::Use));
        let (x, y) = centre(l.plus);
        assert_eq!(hit(&l, x, y), Some(Part::Plus));
        let (x, y) = centre(l.dots[2].0);
        assert_eq!(hit(&l, x, y), Some(Part::Dot(2)));
        let (x, y) = centre(l.right.unwrap());
        assert_eq!(hit(&l, x, y), Some(Part::Right));
        let (x, y) = centre(l.middle);
        assert_eq!(hit(&l, x, y), None);
    }

    #[test]
    fn steps_stop_at_the_ends() {
        assert_eq!(step(0, 3, false), 0);
        assert_eq!(step(0, 3, true), 1);
        assert_eq!(step(2, 3, true), 2);
        assert_eq!(step(2, 3, false), 1);
        assert_eq!(step(0, 0, true), 0);
    }

    #[test]
    fn tab_walks_customize_use_and_plus_and_stops_at_the_ends() {
        assert_eq!(next_focus(None, false), Some(Part::Customize));
        assert_eq!(next_focus(Some(Part::Customize), false), Some(Part::Use));
        assert_eq!(next_focus(Some(Part::Use), false), Some(Part::Plus));
        assert_eq!(next_focus(Some(Part::Plus), false), Some(Part::Plus));
        assert_eq!(next_focus(Some(Part::Use), true), Some(Part::Customize));
        assert_eq!(next_focus(None, true), Some(Part::Plus));
    }

    #[test]
    fn the_places_line_names_each_part_and_look() {
        let v = view((1280.0, 800.0), &["classic", "magazine", "minimal"], 0);
        let line = places(&v, &layout(&v));
        assert!(line.starts_with("screen 1280x800, middle "), "{line}");
        for word in [
            "customize ",
            "use ",
            "plus ",
            "dot0 classic ",
            "dot2 minimal ",
        ] {
            assert!(line.contains(word), "{word} in {line}");
        }
    }

    #[test]
    fn the_nodes_read_the_title_then_the_buttons_and_focus_follows() {
        let mut v = view((1280.0, 800.0), &["classic", "magazine"], 1);
        v.focus = Some(Part::Use);
        let l = layout(&v);
        let nodes = nodes(&v, &l);
        assert_eq!(nodes[0].label, "Magazine");
        assert_eq!(focus_index(&v, &l), Some(2));
        v.focus = None;
        assert_eq!(focus_index(&v, &l), None);
    }

    #[test]
    fn compact_screens_stack_the_buttons_at_touch_size() {
        let v = view((390.0, 844.0), &["classic"], 0);
        let l = layout(&v);
        assert_eq!(l.button_height, BUTTON_TOUCH);
        assert!(l.customize.h >= 44.0 && l.using.h >= 44.0);
    }

    /// Writes the switcher at 1280 by 800 and on a phone when `EDEL_LOCK_PNG`
    /// names a folder: `lock-looks.png` and `lock-looks-phone.png`.
    #[test]
    fn the_looks_side_by_side_draw_with_their_pictures() {
        let Some(dir) = std::env::var_os("EDEL_LOCK_PNG") else {
            return;
        };
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let mut icons = edel::app_icons::Icons::new(edel::apps::data_dirs());
        let mut pictures = lock::Pictures::default();
        let mut backdrops = lock::Backdrops::default();
        let wallpaper = lock::wallpaper_of(None, None, true);
        let now = jiff::Zoned::now();
        let (time, date) = lock::time_and_date(&now);
        let mut lock_view = lock::View::new(time, date, "Ali".to_string());
        lock_view.when = Some(lock::When::of(&now));
        let names = ["classic", "magazine", "minimal"];
        for (name, screen) in [
            ("lock-looks.png", (1280.0, 800.0)),
            ("lock-looks-phone.png", (390.0, 844.0)),
        ] {
            let compact = screen.0 < lock::COMPACT_BELOW;
            let shown = 0;
            let pics: Vec<Option<Pixmap>> = names
                .iter()
                .map(|n| {
                    let kit = Kit {
                        tokens: &tokens,
                        text: &mut text,
                        icons: &mut icons,
                        pictures: &mut pictures,
                        backdrops: &mut backdrops,
                        wallpaper: &wallpaper,
                    };
                    look_picture(kit, n, compact, &lock_view, false)
                })
                .collect();
            let v = View {
                screen,
                compact,
                names: names.iter().map(|n| n.to_string()).collect(),
                shown,
                in_use: 0,
                focus: None,
                hover: None,
            };
            let l = layout(&v);
            let mut canvas = Pixmap::new(screen.0 as u32, screen.1 as u32).unwrap();
            let pick = |i: isize| -> Option<&Pixmap> {
                usize::try_from(i)
                    .ok()
                    .filter(|i| *i < names.len())
                    .and_then(|i| pics[i].as_ref())
            };
            paint(
                &mut canvas,
                &v,
                &l,
                [
                    pick(shown as isize - 1),
                    pick(shown as isize),
                    pick(shown as isize + 1),
                ],
                &tokens,
                &mut text,
                1.0,
            );
            let out = std::path::Path::new(&dir).join(name);
            crate::look_picture::write_png(&canvas, &out).unwrap();
        }
    }
}
