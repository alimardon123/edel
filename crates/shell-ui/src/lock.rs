//! The lock screen's look and its password check (M5.10), plain data and
//! drawing, tested without a display. As `docs/mockups/shell/lock.jpg`
//! draws it (M5.10c), on every screen: the date over a large, light clock
//! in tabular figures, a frosted count of the notifications that came,
//! with their apps' icons and never their words, then the person's
//! initial on an accent disc, their name and a frosted password field
//! with a round go button, on the backdrop (`backdrop`, the one function
//! that draws what lies behind, so a wallpaper or a song's cover can take
//! its place later). The field shows a dot per letter typed, never the
//! letters, a Caps Lock mark while Caps Lock is on, and under it what
//! happened ("Wrong password", "Checking"); a wrong password shakes it
//! (`shake_at`), unless `appearance.animations` asks for less motion.
//! The password is checked by the system's own helper,
//! `edel::places::PASSWORD_CHECKERS` (Linux-PAM's `unix_chkpwd`), which a
//! person's own program may ask about that person's password alone; a
//! wrong one waits two seconds before the next try, so guessing is slow.
//! With `View::people` (M5.10c) the row of people from `people.rs` takes the
//! disc and the name's place, and the hint to choose with the arrows shows
//! under the field while it is ready.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use edel::app_icons::Icons;
use edel::i18n::{tr, trf};
use edel::lock_screen::{self, Anchor, Item, Part, Placed};
use edel::tokens::{Colour, Tokens};
use tiny_skia::{
    FillRule, GradientStop, Paint, PathBuilder, Pixmap, PixmapPaint, Point, RadialGradient,
    SpreadMode, Transform,
};

use crate::corner;
use crate::messages;
use crate::paint::{self, Face, Text, fill, fill_vertical, mix, outline};
use crate::popup::{self, Rect};

/// What the field says under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Nothing typed, or typing.
    Ready,
    /// The helper is checking the password.
    Checking,
    /// The last password was wrong.
    Wrong,
    /// The system has no password helper: nothing can unlock.
    NoChecker,
    /// The greeter's login was refused: the user name or the password
    /// was wrong (M5.10b).
    Refused,
    /// The greeter could not log in for another reason.
    Failed,
}

/// What a screen shows.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// `14:06` and `Saturday 3 October`.
    pub time: String,
    pub date: String,
    pub name: String,
    /// How many letters are typed.
    pub dots: usize,
    pub field: Field,
    /// The greeter's first step (M5.10b): the field takes the user name,
    /// shown as typed, and `name` is what is typed so far.
    pub asking_name: bool,
    /// Whether Caps Lock is on (M5.10c).
    pub caps: bool,
    /// How many notifications came that nobody has read yet, and their
    /// apps, newest first, at most `MOST_APPS`: each its name and its
    /// icon's name (M5.10c).
    pub notices: usize,
    pub apps: Vec<(String, String)>,
    /// How far the field is pushed sideways now, logical pixels: a wrong
    /// password shakes it (`shake_at`).
    pub shake: i32,
    /// Whether the pointer is over the go button.
    pub go_hover: bool,
    /// The row of people (M5.10c) that takes the disc and the name's place
    /// when there is one; None on the lock screen.
    pub people: Option<crate::people::Row>,
}

impl View {
    /// A view with the time, the date and the person, and nothing else yet.
    pub fn new(time: String, date: String, name: String) -> View {
        View {
            time,
            date,
            name,
            dots: 0,
            field: Field::Ready,
            asking_name: false,
            caps: false,
            notices: 0,
            apps: Vec::new(),
            shake: 0,
            go_hover: false,
            people: None,
        }
    }
}

/// Where a screen's parts lie, logical pixels from its top left corner
/// (M5.10c5). Each part is its rectangle, or None when `lock_screen::arrange`
/// found no clear place for it, and then it is not drawn. The disc, the
/// name, the field, the go button and the note stand inside `person`, as
/// they always did, one under the other.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Whether the screen is Compact (narrower than `COMPACT_BELOW`).
    pub compact: bool,
    /// The date and the clock: each drawn centred across its rectangle.
    pub date: Option<Rect>,
    pub clock: Option<Rect>,
    /// The clock's font size.
    pub clock_size: f32,
    /// The notifications' count, drawn centred in its rectangle; None
    /// while nothing is unread.
    pub count: Option<Rect>,
    /// The person's block: the disc, the name and the field.
    pub person: Rect,
    /// The disc's centre and radius.
    pub avatar: (f32, f32, f32),
    pub name: f32,
    /// The field and its round go button at its right end, before a shake.
    pub field: Rect,
    pub go: Rect,
    pub note: f32,
    /// The player card, when one shows.
    pub player: Option<Rect>,
    /// Each message: its rectangle, its wrapped lines and its anchor, which
    /// says how the lines are aligned.
    pub messages: Vec<(Rect, Vec<String>, Anchor)>,
    /// Each picture: its rectangle and its path.
    pub pictures: Vec<(Rect, String)>,
}

/// What the layout needs besides the screen: the parts a list says, the
/// player card's size when one shows, whether the greeter's row of people
/// stands in place of the disc and name, and the places no part may cover
/// (the corner's buttons, and its menu while it is open).
pub struct Look<'a> {
    /// The parts, in the order the list gives them (`lock_screen::applying`).
    pub parts: &'a [Placed],
    /// The player card's size (`player::wide_size` with its pages, its
    /// width less the margins on a Compact screen), when one shows.
    pub player: Option<(f32, f32)>,
    /// Whether the greeter's row of people shows: the person's block is then
    /// as wide as the screen less the margins.
    pub people: bool,
    /// Places no part may cover, with the gap kept round each.
    pub keep_out: Vec<lock_screen::Rect>,
}

/// The screen's width below which it is Compact (M5.6c's size classes).
pub(crate) const COMPACT_BELOW: f32 = 600.0;
/// The field's size: as wide as `FIELD_WIDE`, or the screen less 2 x
/// `MARGIN`, and touch-sized (44 px) high; the go button 4 px inside its end.
const FIELD_WIDE: f32 = 300.0;
const FIELD_HIGH: f32 = 44.0;
const GO_INSET: f32 = 4.0;
const AVATAR: f32 = 44.0;
const DATE: f32 = 20.0;
const NAME: f32 = 18.0;
const NOTE: f32 = 13.0;
/// The room kept from the screen's edges for every part, and the room kept
/// between two parts and round the corner's buttons (`lock_screen::arrange`).
pub const MARGIN: f32 = 24.0;
pub const GAP: f32 = 16.0;
/// A message's widest line, its words at most `MESSAGE_LINES` lines and
/// their size.
const MESSAGE_WIDE: f32 = 360.0;
const MESSAGE_LINES: usize = 3;
const MESSAGE_TEXT: f32 = 17.0;
/// A picture's side in logical pixels: it is cut to a square.
const PICTURE: f32 = 112.0;
/// The widest date and the widest time the date and the clock are sized
/// for. Tabular figures make every time as wide as "88:88", and the widest
/// English date is a Wednesday in September; a translated date that is
/// wider is drawn in the same room, cut where it must be.
const DATE_WIDEST: &str = "Wednesday 28 September";
const CLOCK_WIDEST: &str = "88:88";
/// The widest count the count is sized for, in its words' size.
const COUNT_WIDEST: &str = "99 notifications";
/// The notifications' count: its height, its apps' tiles, how much each
/// tile overlaps the one before and its words' size.
const COUNT_HIGH: f32 = 36.0;
const COUNT_TILE: f32 = 24.0;
const COUNT_OVERLAP: f32 = 8.0;
const COUNT_TEXT: f32 = 13.0;
/// An app's tile in the count: a rounded square, as apps' icons are.
const TILE_RADIUS: f32 = 7.0;
pub const MOST_APPS: usize = 3;

/// The one function that places everything on a screen `w` by `h` (M5.10c5):
/// each part of `look.parts` at its anchor and offset, sized as it is drawn
/// and moved clear of the others and of `look.keep_out` (`arrange`). A
/// list whose person cannot stand clear falls back to the classic
/// arrangement, so the field always shows (Reliable, the first principle).
pub fn layout(
    w: f32,
    h: f32,
    tokens: &Tokens,
    notices: bool,
    look: &Look,
    text: &mut Text,
) -> Layout {
    let (at, clear) = place_all(w, h, tokens, notices, look, look.parts, text);
    if clear {
        return at;
    }
    let classic = lock_screen::classic(w < COMPACT_BELOW);
    place_all(w, h, tokens, notices, look, &classic, text).0
}

/// The layout of one screen from the list the person or the machine chose
/// (`lock_screen::applying`, Compact by `w`), with `player` and `people`
/// as `Look` says and the corner's places kept clear. What `layout` and
/// both screens use.
#[allow(clippy::too_many_arguments)]
pub fn screen_layout(
    w: f32,
    h: f32,
    tokens: &Tokens,
    notices: bool,
    list: Option<&[Placed]>,
    player: Option<(f32, f32)>,
    people: bool,
    corner_at: &corner::Layout,
    text: &mut Text,
) -> Layout {
    let parts = lock_screen::applying(list, w < COMPACT_BELOW);
    let look = Look {
        parts: &parts,
        player,
        people,
        keep_out: keep_out(corner_at),
    };
    layout(w, h, tokens, notices, &look, text)
}

/// The corner's buttons and, while it is open, its menu, as places no part
/// may cover.
pub fn keep_out(corner_at: &corner::Layout) -> Vec<lock_screen::Rect> {
    let edge = |r: Rect| lock_screen::Rect {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
    };
    let mut out: Vec<lock_screen::Rect> = corner_at.buttons.iter().map(|(_, r)| edge(*r)).collect();
    out.extend(corner_at.menu.map(edge));
    out
}

/// What an item of the layout stands for, until `arrange` says where it goes.
enum Slot {
    Date,
    Clock,
    Count,
    Person,
    Player,
    Message(Vec<String>, Anchor),
    Picture(String),
}

/// One arrangement of `parts` on a screen: the layout, and whether the
/// person stands clear (the layout places it itself only then; otherwise
/// it is centred, unarranged, so the field still shows).
fn place_all(
    w: f32,
    h: f32,
    tokens: &Tokens,
    notices: bool,
    look: &Look,
    parts: &[Placed],
    text: &mut Text,
) -> (Layout, bool) {
    let compact = w < COMPACT_BELOW;
    let mut clock_size = (tokens.lock_clock as f32).min(w * 0.24).max(40.0);
    let field_w = FIELD_WIDE.min(w - 2.0 * MARGIN).max(120.0);
    let person_w = if look.people {
        w - 2.0 * MARGIN
    } else {
        field_w
    };
    // The person's block: the disc, the name, the field and the note under
    // it, as tall as the note's last line (the note is part of the block, so
    // a part at the bottom keeps the note clear of the corner too).
    let person_h = 2.0 * AVATAR + 12.0 + NAME * 1.25 + 14.0 + FIELD_HIGH + 12.0 + NOTE * 1.25;
    let clock_face = Face {
        weight: tokens.lock_clock_weight.clamp(100, 900) as u16,
        tabular: true,
    };
    // The clock is never wider than the screen's room: a wider font (a
    // language's own, or the one a machine without the tokens' font falls
    // back to) shrinks it rather than leaving the time out.
    let room = w - 2.0 * MARGIN;
    let wide = text.line_in(CLOCK_WIDEST, clock_size, clock_face).width;
    if wide > room {
        clock_size *= room / wide;
    }
    let mut slots: Vec<(Slot, Item)> = Vec::new();
    for p in parts {
        let item = |w: f32, h: f32| Item {
            at: p.at,
            x: p.x as f32,
            y: p.y as f32,
            w,
            h,
        };
        match p.part {
            Part::Date => {
                let width = text.line_in(DATE_WIDEST, DATE, Face::REGULAR).width;
                slots.push((Slot::Date, item(width, DATE * 1.25)));
            }
            Part::Clock => {
                let width = text.line_in(CLOCK_WIDEST, clock_size, clock_face).width;
                slots.push((Slot::Clock, item(width, clock_size * 1.2)));
            }
            Part::Count if notices => {
                let words = text.line_in(COUNT_WIDEST, COUNT_TEXT, Face::SEMIBOLD).width;
                let width = count_width(MOST_APPS, words);
                slots.push((Slot::Count, item(width, COUNT_HIGH)));
            }
            Part::Person => slots.push((Slot::Person, item(person_w, person_h))),
            Part::Player => {
                if let Some((pw, ph)) = look.player {
                    slots.push((Slot::Player, item(pw, ph)));
                }
            }
            Part::Message => {
                let (lines, width) = message_lines(text, p.text.as_deref().unwrap_or_default(), w);
                let high = lines.len() as f32 * MESSAGE_TEXT * 1.25;
                slots.push((Slot::Message(lines, p.at), item(width, high)));
            }
            Part::Picture => {
                let path = p.path.clone().unwrap_or_default();
                slots.push((Slot::Picture(path), item(PICTURE, PICTURE)));
            }
            // Not yet: the battery's charge joins in M5.10c5c, and until then
            // it is not drawn. Count with no unread notification is skipped
            // too.
            Part::Count | Part::Battery => {}
        }
    }
    let items: Vec<Item> = slots.iter().map(|(_, item)| *item).collect();
    let mut rects = lock_screen::arrange((w, h), &items, &look.keep_out, GAP, MARGIN);
    // The person and the player card must show (Reliable). When the list's
    // order leaves either no place, both are placed first and the other
    // parts arrange round them, moving or not drawn. The classic arrangement
    // on a desktop of 1024 by 768 with notifications needs this.
    let n = items.len();
    let must = |i: usize| matches!(slots[i].0, Slot::Person | Slot::Player);
    let standing = (0..n).all(|i| !must(i) || rects[i].is_some());
    let retried = !standing;
    if retried {
        let order: Vec<usize> = (0..n)
            .filter(|&i| matches!(slots[i].0, Slot::Player))
            .chain((0..n).filter(|&i| matches!(slots[i].0, Slot::Person)))
            .chain((0..n).filter(|&i| !must(i)))
            .collect();
        let ordered: Vec<Item> = order.iter().map(|&i| items[i]).collect();
        let placed = lock_screen::arrange((w, h), &ordered, &look.keep_out, GAP, MARGIN);
        rects = vec![None; n];
        for (&i, rect) in order.iter().zip(placed) {
            rects[i] = rect;
        }
    }
    let mut date = None;
    let mut clock = None;
    let mut count = None;
    let mut person = None;
    let mut player = None;
    let mut messages = Vec::new();
    let mut pictures = Vec::new();
    for ((slot, _), rect) in slots.into_iter().zip(rects) {
        let Some(rect) = rect.map(|r| Rect::new(r.x, r.y, r.w, r.h)) else {
            continue;
        };
        match slot {
            Slot::Date => date = Some(rect),
            Slot::Clock => clock = Some(rect),
            Slot::Count => count = Some(rect),
            Slot::Person => person = Some(rect),
            Slot::Player => player = Some(rect),
            Slot::Message(lines, anchor) => messages.push((rect, lines, anchor)),
            Slot::Picture(path) => pictures.push((rect, path)),
        }
    }
    // Where the list fits only by moving the parts round the person, a count
    // that would stand above the clock is left out, as it belongs under the
    // time; a list's own order is never changed this way.
    if retried {
        if let (Some(c), Some(k)) = (count, clock) {
            if c.y < k.y + k.h {
                count = None;
            }
        }
    }
    let clear = person.is_some();
    // Unarranged, the person is centred on the screen's middle, which is
    // where the classic arrangement puts it at any size.
    let person = person.unwrap_or_else(|| {
        Rect::new(
            (w - person_w) / 2.0,
            (h - person_h) / 2.0,
            person_w,
            person_h,
        )
    });
    let centre = person.x + person.w / 2.0;
    let avatar_y = person.y + AVATAR;
    let name = avatar_y + AVATAR + 12.0;
    let field_y = name + NAME * 1.25 + 14.0;
    let field = Rect::new(centre - field_w / 2.0, field_y, field_w, FIELD_HIGH);
    let side = FIELD_HIGH - 2.0 * GO_INSET;
    let layout = Layout {
        compact,
        date,
        clock,
        clock_size,
        count,
        person,
        avatar: (centre, avatar_y, AVATAR),
        name,
        field,
        go: Rect::new(
            field.right() - GO_INSET - side,
            field_y + GO_INSET,
            side,
            side,
        ),
        note: field_y + FIELD_HIGH + 12.0,
        player,
        messages,
        pictures,
    };
    (layout, clear)
}

/// The count's pill as wide as `apps` tiles and `words` of text need, with
/// the room a pill keeps round them.
fn count_width(apps: usize, words: f32) -> f32 {
    let pad = (COUNT_HIGH - COUNT_TILE) / 2.0;
    let tiles = if apps == 0 {
        0.0
    } else {
        COUNT_TILE + (apps - 1) as f32 * (COUNT_TILE - COUNT_OVERLAP) + 8.0
    };
    pad + tiles + words + 14.0
}

/// A message's words wrapped into at most `MESSAGE_LINES` lines of
/// `MESSAGE_TEXT`, and the width of its widest line, at most the room
/// `MESSAGE_WIDE` or the screen less the margins gives. Words that do not
/// fit the lines are cut by the drawing (`Text::fit_in`).
fn message_lines(text: &mut Text, words: &str, w: f32) -> (Vec<String>, f32) {
    let room = MESSAGE_WIDE.min(w - 2.0 * MARGIN).max(0.0);
    let mut lines = popup::wrap(words, MESSAGE_LINES, room, |s| {
        text.line_in(s, MESSAGE_TEXT, Face::REGULAR).width
    });
    if lines.is_empty() {
        lines.push(String::new());
    }
    let width = lines
        .iter()
        .map(|line| text.line_in(line, MESSAGE_TEXT, Face::REGULAR).width)
        .fold(0.0, f32::max)
        .min(room);
    (lines, width)
}

/// How far a shake pushes the field `ms` after a wrong password, logical
/// pixels: three swings that die away within `SHAKE_MS`.
pub fn shake_at(ms: u32) -> i32 {
    if ms >= SHAKE_MS {
        return 0;
    }
    let t = ms as f32 / SHAKE_MS as f32;
    let swing = (t * 3.0 * std::f32::consts::TAU).sin();
    (10.0 * (1.0 - t) * swing).round() as i32
}

/// The apps of the `unread` newest notifications in `items` (newest
/// first), each once, at most `MOST_APPS`: its name and its icon's name,
/// for the count's tiles.
pub fn unread_apps(items: &[crate::notify::Notification], unread: usize) -> Vec<(String, String)> {
    let mut apps: Vec<(String, String)> = Vec::new();
    for n in items.iter().take(unread) {
        if apps.len() == MOST_APPS {
            break;
        }
        if !apps.iter().any(|(app, _)| *app == n.app) {
            apps.push((n.app.clone(), n.icon.clone()));
        }
    }
    apps
}

/// How long a shake lasts.
pub const SHAKE_MS: u32 = 420;

/// The log line CI reads to click the field's parts: `field X+Y+WxH, go
/// X+Y+WxH`, logical pixels, with `count Y` while it shows, then the date,
/// the clock and each message and picture as `X+Y+WxH` (M5.10c5).
pub fn places(at: &Layout) -> String {
    let r = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut line = format!("field {}, go {}", r(at.field), r(at.go));
    if let Some(count) = at.count {
        line.push_str(&format!(", count {:.0}", count.y));
    }
    if let Some(date) = at.date {
        line.push_str(&format!(", date {}", r(date)));
    }
    if let Some(clock) = at.clock {
        line.push_str(&format!(", clock {}", r(clock)));
    }
    for (i, (rect, _, _)) in at.messages.iter().enumerate() {
        line.push_str(&format!(", message{i} {}", r(*rect)));
    }
    for (i, (rect, _)) in at.pictures.iter().enumerate() {
        line.push_str(&format!(", picture{i} {}", r(*rect)));
    }
    line
}

/// The parts the lock and login screens show, as the settings files say
/// them (M5.10c5): the person's `lock_screen.parts` over the machine's, None
/// when neither has a list (the classic arrangement).
pub fn parts_of(machine: Option<&str>, person: Option<&str>) -> Option<Vec<Placed>> {
    let read = |text: Option<&str>| {
        text.and_then(|t| edel::settings::read(t).ok())
            .and_then(|read| read.file.lock_screen.parts)
    };
    read(person).or_else(|| read(machine))
}

/// The pictures of the lock screen (M5.10c5), each read once for each
/// device size it is drawn at and kept: a picture cut to a square cover.
#[derive(Default)]
pub struct Pictures {
    made: HashMap<(String, u32), Option<Pixmap>>,
}

impl Pictures {
    /// The picture at `path` as a square of `side` device pixels, covering
    /// it. None when it cannot be read, which is logged once, for that path
    /// and size, and the lock screen leaves the picture out.
    pub fn get(&mut self, path: &str, side: u32) -> Option<&Pixmap> {
        self.made
            .entry((path.to_string(), side))
            .or_insert_with(|| match load_cover(path, side) {
                Ok(picture) => Some(picture),
                Err(why) => {
                    eprintln!("edel-shell-ui: {}", messages::picture_not_read(path, why));
                    None
                }
            })
            .as_ref()
    }
}

/// Reads the PNG at `path` and cuts it to a square of `side` pixels, its
/// shorter side fitted and the rest cut from both ends. JPEG pictures wait
/// for `edel::pictures`, which this release does not have.
fn load_cover(path: &str, side: u32) -> Result<Pixmap, String> {
    if !path.ends_with(".png") {
        return Err(messages::PICTURE_NOT_PNG.to_string());
    }
    let source = Pixmap::load_png(path).map_err(|e| e.to_string())?;
    let mut out = Pixmap::new(side, side).ok_or_else(|| messages::PICTURE_NO_MEMORY.to_string())?;
    let (sw, sh) = (source.width() as f32, source.height() as f32);
    let k = side as f32 / sw.min(sh);
    let smooth = PixmapPaint {
        quality: tiny_skia::FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    let at = Transform::from_row(
        k,
        0.0,
        0.0,
        k,
        (side as f32 - sw * k) / 2.0,
        (side as f32 - sh * k) / 2.0,
    );
    out.draw_pixmap(0, 0, source.as_ref(), &smooth, at, None);
    Ok(out)
}

/// Whether a point lies on the go button, the shake left aside.
pub fn on_go(at: &Layout, x: f32, y: f32) -> bool {
    at.go.contains(x, y)
}

/// What lies behind everything: the backdrop's gradient, settling on
/// `lock_backdrop_deep` at 85 percent of the height (which the compositor's
/// cover also shows before this is drawn), lit by two soft glows above,
/// `lock_glow` at the upper left and `lock_glow_second` at the right, as
/// the mockups draw it. The one function a wallpaper or a cover would
/// replace.
pub fn backdrop(pixmap: &mut Pixmap, tokens: &Tokens) {
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let (sky, ground) = (tokens.lock_backdrop, tokens.lock_backdrop_deep);
    // A blurred scene: the sky holds its colour down to a third of the
    // height, a hazy horizon falls off below it, and the ground settles
    // on the deep colour by `SETTLED` of the height.
    let c = |colour: Colour| tiny_skia::Color::from_rgba(colour.r, colour.g, colour.b, 1.0);
    let stops = [
        (0.0, sky),
        (0.3, mix(sky, ground, 0.08)),
        (0.42, mix(sky, ground, 0.45)),
        (0.55, mix(sky, ground, 0.85)),
        (SETTLED, ground),
    ];
    let stops: Vec<GradientStop> = stops
        .iter()
        .filter_map(|&(at, colour)| c(colour).map(|col| GradientStop::new(at, col)))
        .collect();
    if let (Some(shader), Some(rect)) = (
        tiny_skia::LinearGradient::new(
            Point::from_xy(0.0, 0.0),
            Point::from_xy(0.0, ph),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        ),
        tiny_skia::Rect::from_xywh(0.0, 0.0, pw, ph),
    ) {
        let paint = Paint {
            shader,
            ..Paint::default()
        };
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }
    let settled = (ph * SETTLED).round();
    // Two soft lights in the sky, kept above the ground.
    let glows = [
        (0.2, 0.14, 0.4, tokens.lock_glow, 0.9),
        (0.84, 0.27, 0.32, tokens.lock_glow_second, 0.6),
    ];
    let reach = pw.max(ph);
    for (x, y, r, colour, a) in glows {
        glow(pixmap, (x * pw, y * ph), r * reach, colour, a, settled);
    }
}

/// Where the backdrop has settled on the deep colour, a share of the
/// height, which the compositor's cover also shows before this is drawn.
const SETTLED: f32 = 0.85;

/// A soft round light of `colour` at `centre`, `radius` across, fading to
/// nothing, kept above `floor` so the bottom stays the deep colour.
fn glow(pixmap: &mut Pixmap, centre: (f32, f32), radius: f32, colour: Colour, a: f32, floor: f32) {
    let c = |alpha: f32| {
        tiny_skia::Color::from_rgba(colour.r, colour.g, colour.b, alpha.clamp(0.0, 1.0))
            .unwrap_or(tiny_skia::Color::TRANSPARENT)
    };
    let Some(shader) = RadialGradient::new(
        Point::from_xy(centre.0, centre.1),
        0.0,
        Point::from_xy(centre.0, centre.1),
        radius,
        vec![
            GradientStop::new(0.0, c(a)),
            GradientStop::new(0.5, c(a * 0.35)),
            GradientStop::new(1.0, c(0.0)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) else {
        return;
    };
    let bottom = floor.min(centre.1 + radius);
    let Some(rect) = tiny_skia::Rect::from_xywh(0.0, 0.0, pixmap.width() as f32, bottom) else {
        return;
    };
    let path = PathBuilder::from_rect(rect);
    let paint = Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    };
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// A frosted card that holds words (M5.10c): a solid base of the
/// backdrop's own colours at `CARD_SOLID`, so text reads over any
/// backdrop, then the glass over it. The corner's power menu and the
/// player's card are drawn with it, so both change here.
pub(crate) fn frosted(
    pixmap: &mut Pixmap,
    (x, y, w, h): (f32, f32, f32, f32),
    r: f32,
    s: f32,
    tokens: &Tokens,
) {
    let solid = Colour {
        a: CARD_SOLID,
        ..mix(tokens.lock_backdrop_deep, tokens.lock_backdrop, 0.35)
    };
    fill(pixmap, x, y, w, h, r, solid);
    glass(pixmap, (x, y, w, h), r, s, tokens);
}

/// How solid a frosted card's base is under its glass.
const CARD_SOLID: f32 = 0.85;

/// Frosted glass: the tokens' tint over what lies behind, edged with
/// their hairline, its corners `r` (device pixels).
pub(crate) fn glass(
    pixmap: &mut Pixmap,
    (x, y, w, h): (f32, f32, f32, f32),
    r: f32,
    s: f32,
    tokens: &Tokens,
) {
    fill(pixmap, x, y, w, h, r, tokens.lock_glass);
    outline(pixmap, (x, y, w, h), r, s, tokens.lock_glass_edge);
}

/// Draws `view` at `scale` into `pixmap`, the screen's size times `scale`,
/// with the parts `at` places (from `layout`); the apps' icons come from
/// `icons` when given, and the pictures from `pictures`.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    at: &Layout,
    tokens: &Tokens,
    text: &mut Text,
    icons: Option<&mut Icons>,
    pictures: &mut Pictures,
    scale: f32,
) {
    let s = scale;
    let w = pixmap.width() as f32 / s;
    backdrop(pixmap, tokens);
    let ink = tokens.backdrop_text;
    let quiet = mix(ink, tokens.lock_backdrop_deep, 0.3);
    let date_ink = mix(ink, tokens.lock_backdrop_deep, 0.15);
    if let Some(rect) = at.date {
        let centre = rect.x + rect.w / 2.0;
        centred(
            pixmap,
            text,
            s,
            w,
            &view.date,
            DATE,
            Face::REGULAR,
            (centre, rect.y),
            date_ink,
        );
    }
    if let Some(rect) = at.clock {
        let centre = rect.x + rect.w / 2.0;
        let weight = tokens.lock_clock_weight.clamp(100, 900) as u16;
        centred(
            pixmap,
            text,
            s,
            w,
            &view.time,
            at.clock_size,
            Face {
                weight,
                tabular: true,
            },
            (centre, rect.y),
            ink,
        );
    }
    for (rect, lines, anchor) in &at.messages {
        message(pixmap, text, s, *rect, lines, *anchor, ink);
    }
    let side = (PICTURE * s).round() as u32;
    for (rect, path) in &at.pictures {
        if let Some(picture) = pictures.get(path, side) {
            let device = rect.device(s);
            paint::rounded_picture(
                pixmap,
                picture,
                (device.0, device.1, side as f32, side as f32),
                tokens.radius_menu as f32 * s,
            );
        }
    }
    if view.asking_name {
        return paint_name_field(pixmap, view, tokens, text, s, w, at);
    }
    if let Some(rect) = at.count {
        count(pixmap, view, tokens, text, icons, s, w, rect);
    }
    match &view.people {
        Some(row) => {
            let spots = crate::people::spots(row, w, at.avatar.1);
            crate::people::paint(pixmap, row, &spots, tokens, text, s);
        }
        None => {
            avatar(pixmap, view, tokens, text, s, at);
            centred(
                pixmap,
                text,
                s,
                w,
                &view.name,
                NAME,
                Face::SEMIBOLD,
                (at.avatar.0, at.name),
                ink,
            );
        }
    }
    // The field: frosted glass, a dot per letter, else the placeholder,
    // the Caps Lock mark and the go button at its end; all of it moved
    // by the shake.
    let dx = view.shake as f32;
    let f = Rect::new(at.field.x + dx, at.field.y, at.field.w, at.field.h);
    let go = Rect::new(at.go.x + dx, at.go.y, at.go.w, at.go.h);
    lift(pixmap, f, f.h / 2.0, s, tokens);
    let (fx, fy, fw, fh) = f.device(s);
    // The backdrop's own colour under the glass first, so the shadow does
    // not darken it from behind.
    fill(
        pixmap,
        fx,
        fy,
        fw,
        fh,
        fh / 2.0,
        mix(tokens.lock_backdrop, tokens.lock_backdrop_deep, 0.55),
    );
    glass(pixmap, (fx, fy, fw, fh), fh / 2.0, s, tokens);
    if view.field == Field::Wrong {
        outline(
            pixmap,
            (fx, fy, fw, fh),
            fh / 2.0,
            1.5 * s,
            mix(tokens.title_close_hover, ink, 0.2),
        );
    }
    let mut room = go.x - 10.0 - (f.x + 20.0);
    if view.caps {
        let side = 14.0;
        let x = go.x - 10.0 - side;
        paint::icon(
            pixmap,
            "caps-lock",
            side * s,
            x * s,
            (f.y + (f.h - side) / 2.0) * s,
            quiet,
        );
        room -= side + 8.0;
    }
    if view.dots == 0 {
        let mut line = text.fit(tr("Password"), 15.0 * s, room.max(0.0) * s);
        text.draw(
            pixmap,
            &mut line,
            (f.x + 20.0) * s,
            (f.y + (f.h - 15.0 * 1.25) / 2.0) * s,
            quiet,
        );
    } else {
        // At most as many dots as fit, 14 px apart.
        let most = (room / 14.0).max(0.0) as usize;
        for i in 0..view.dots.min(most) {
            let d = 8.0;
            let x = f.x + 20.0 + i as f32 * 14.0;
            fill(
                pixmap,
                x * s,
                (f.y + (f.h - d) / 2.0) * s,
                d * s,
                d * s,
                d / 2.0 * s,
                ink,
            );
        }
    }
    go_button(pixmap, view, tokens, s, go);
    // The close button's red, lifted towards the text so it reads on the
    // dark backdrop.
    let warn = mix(tokens.title_close_hover, ink, 0.3);
    let colour = match view.field {
        Field::Wrong | Field::NoChecker | Field::Refused | Field::Failed => warn,
        Field::Ready | Field::Checking => quiet,
    };
    if let Some(words) = note_text(view) {
        centred(
            pixmap,
            text,
            s,
            w,
            words,
            NOTE,
            Face::MEDIUM,
            (at.avatar.0, at.note),
            colour,
        );
    }
}

/// The words under the field, if any (M5.10c): Caps Lock's mark while it
/// is on; with a row of people, the hint to choose with the arrows while
/// the field is ready; else what happened to the last try.
fn note_text(view: &View) -> Option<&'static str> {
    match view.field {
        Field::Ready if view.caps => Some(tr("Caps Lock is on")),
        Field::Ready if view.people.is_some() => Some(tr("Left and right choose a person")),
        Field::Ready => None,
        Field::Checking => Some(tr("Checking")),
        Field::Wrong => Some(tr("Wrong password, try again")),
        Field::NoChecker => Some(tr("This system cannot check passwords")),
        Field::Refused => Some(tr("Wrong user name or password")),
        Field::Failed => Some(tr("Could not log in, try again")),
    }
}

/// The person: their initial in white on a disc of the accent, lighter
/// at its top as if lit from above, ringed with the glass's edge and
/// lifted by a soft shadow.
fn avatar(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, text: &mut Text, s: f32, at: &Layout) {
    let initial: String = view
        .name
        .chars()
        .take(1)
        .flat_map(char::to_uppercase)
        .collect();
    disc(pixmap, &initial, at.avatar, 36.0, false, tokens, text, s);
}

/// A person's disc (M5.10c): their `initial` in white, `size` high, on an
/// accent disc at `(cx, cy)` of radius `r`, lighter at its top as if lit
/// from above, ringed with the glass's edge and lifted by a soft shadow.
/// `quiet` mixes its colours 30 percent towards the deep backdrop, for a
/// person of the row who is not chosen (`people.rs`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn disc(
    pixmap: &mut Pixmap,
    initial: &str,
    (cx, cy, r): (f32, f32, f32),
    size: f32,
    quiet: bool,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let ring = 3.0;
    lift(
        pixmap,
        Rect::new(
            cx - r - ring,
            cy - r - ring,
            2.0 * (r + ring),
            2.0 * (r + ring),
        ),
        r + ring,
        s,
        tokens,
    );
    fill(
        pixmap,
        (cx - r - ring) * s,
        (cy - r - ring) * s,
        2.0 * (r + ring) * s,
        2.0 * (r + ring) * s,
        (r + ring) * s,
        mix(tokens.lock_backdrop, tokens.backdrop_text, 0.16),
    );
    let (mut top, mut bottom) = (
        mix(tokens.accent, tokens.backdrop_text, 0.22),
        mix(tokens.accent, tokens.lock_backdrop_deep, 0.12),
    );
    if quiet {
        top = mix(top, tokens.lock_backdrop_deep, 0.3);
        bottom = mix(bottom, tokens.lock_backdrop_deep, 0.3);
    }
    fill_vertical(
        pixmap,
        ((cx - r) * s, (cy - r) * s, 2.0 * r * s, 2.0 * r * s),
        r * s,
        top,
        bottom,
    );
    let mut line = text.line_in(initial, size * s, Face::SEMIBOLD);
    let x = cx * s - line.width / 2.0;
    text.draw(
        pixmap,
        &mut line,
        x,
        (cy - size * 0.62) * s,
        tokens.backdrop_text,
    );
}

/// A soft shadow under a part `r` (logical pixels) with corners `radius`,
/// so it seems to float over the backdrop: the tokens' shadow, half as
/// deep, as the lock screen's parts sit on a dark backdrop already; drawn
/// only round the part.
pub(crate) fn lift(pixmap: &mut Pixmap, r: Rect, radius: f32, s: f32, tokens: &Tokens) {
    let room = paint::shadow_room(tokens, false);
    let (w, h) = ((r.w * s).round() as u32, (r.h * s).round() as u32);
    let Some(shadow) = paint::shadow(w, h, room, radius * s, tokens, s) else {
        return;
    };
    let paint = tiny_skia::PixmapPaint {
        opacity: 0.6,
        ..tiny_skia::PixmapPaint::default()
    };
    // Only round the part, never under it: the part is glass, and a shadow
    // under it would grey it (CSS's box-shadow is drawn the same way).
    let (x, y) = ((r.x * s).round(), (r.y * s).round());
    let mut outside = tiny_skia::Mask::new(pixmap.width(), pixmap.height());
    if let (Some(mask), Some(path)) = (
        outside.as_mut(),
        paint::rounded(x, y, w as f32, h as f32, radius * s),
    ) {
        mask.fill_path(
            &path,
            tiny_skia::FillRule::Winding,
            true,
            Transform::identity(),
        );
        mask.invert();
    }
    pixmap.draw_pixmap(
        x as i32 - room as i32,
        y as i32 - room as i32,
        shadow.as_ref(),
        &paint,
        Transform::identity(),
        outside.as_ref(),
    );
}

/// The round go button: the accent with an arrow, lighter under the
/// pointer, faint until a letter is typed.
fn go_button(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, s: f32, go: Rect) {
    let (x, y, w, h) = go.device(s);
    let colour = if view.dots == 0 {
        Colour {
            a: 0.45,
            ..tokens.accent
        }
    } else if view.go_hover {
        mix(tokens.accent, tokens.backdrop_text, 0.15)
    } else {
        tokens.accent
    };
    fill(pixmap, x, y, w, h, w / 2.0, colour);
    let side = 16.0;
    paint::icon(
        pixmap,
        "arrow-right",
        side * s,
        x + (w - side * s) / 2.0,
        y + (h - side * s) / 2.0,
        tokens.backdrop_text,
    );
}

/// The notifications' count: a frosted pill with the apps' icons, each
/// overlapping the one before, and how many came, centred at `top`.
#[allow(clippy::too_many_arguments)]
fn count(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    text: &mut Text,
    mut icons: Option<&mut Icons>,
    s: f32,
    w: f32,
    rect: Rect,
) {
    let top = rect.y;
    let words = if view.notices == 1 {
        tr("1 notification").to_string()
    } else {
        trf(
            "{count} notifications",
            &[("count", &view.notices.to_string())],
        )
    };
    let mut line = text.fit_in(&words, COUNT_TEXT * s, (w - 120.0) * s, Face::SEMIBOLD);
    let apps = &view.apps[..view.apps.len().min(MOST_APPS)];
    let pad = (COUNT_HIGH - COUNT_TILE) / 2.0;
    let tiles = count_width(apps.len(), 0.0) - pad - 14.0;
    let width = count_width(apps.len(), line.width / s);
    // Centred in its rectangle, which the layout centres on the screen.
    let x = rect.x + (rect.w - width) / 2.0;
    glass(
        pixmap,
        (x * s, top * s, width * s, COUNT_HIGH * s),
        COUNT_HIGH / 2.0 * s,
        s,
        tokens,
    );
    let side = (COUNT_TILE * s).round();
    for (i, (app, icon)) in apps.iter().enumerate() {
        let tx = x + pad + i as f32 * (COUNT_TILE - COUNT_OVERLAP);
        let ty = top + pad;
        // A ring of the backdrop sets each tile off the one under it.
        fill(
            pixmap,
            (tx - 1.5) * s,
            (ty - 1.5) * s,
            (COUNT_TILE + 3.0) * s,
            (COUNT_TILE + 3.0) * s,
            (TILE_RADIUS + 1.5) * s,
            mix(tokens.lock_backdrop_deep, tokens.lock_backdrop, 0.4),
        );
        let found = icons.as_deref_mut().and_then(|icons| {
            [icon.clone(), app.to_lowercase().replace(' ', "-")]
                .into_iter()
                .filter(|n| !n.is_empty())
                .find_map(|n| icons.get(&n, side as u32).cloned())
        });
        match found {
            Some(picture) => paint::rounded_picture(
                pixmap,
                &picture,
                (tx * s, ty * s, side, side),
                TILE_RADIUS * s,
            ),
            None => {
                fill(
                    pixmap,
                    tx * s,
                    ty * s,
                    side,
                    side,
                    side / 2.0,
                    tokens.accent,
                );
                let letter: String = app.chars().take(1).flat_map(char::to_uppercase).collect();
                let mut l = text.line_in(&letter, 11.0 * s, Face::SEMIBOLD);
                let lx = tx * s + (side - l.width) / 2.0;
                text.draw(
                    pixmap,
                    &mut l,
                    lx,
                    ty * s + (side - 11.0 * s * 1.25) / 2.0,
                    tokens.backdrop_text,
                );
            }
        }
    }
    let lx = (x + pad + tiles) * s;
    text.draw(
        pixmap,
        &mut line,
        lx,
        (top + (COUNT_HIGH - COUNT_TEXT * 1.25) / 2.0) * s,
        tokens.backdrop_text,
    );
}

/// The greeter's first step: the field shows the user name as typed, with
/// a caret in the accent, or "User name" when empty, and the note under
/// it says why the last try failed.
fn paint_name_field(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
    w: f32,
    at: &Layout,
) {
    let ink = tokens.backdrop_text;
    let quiet = mix(ink, tokens.lock_backdrop_deep, 0.3);
    // With the machine's people in a row (M5.10c), Other stands chosen in
    // it, named under its disc; without anyone to show, the question.
    match view.people.as_ref().filter(|row| !row.people.is_empty()) {
        Some(row) => {
            let spots = crate::people::spots(row, w, at.avatar.1);
            crate::people::paint(pixmap, row, &spots, tokens, text, s);
        }
        None => centred(
            pixmap,
            text,
            s,
            w,
            tr("Who is logging in?"),
            NAME,
            Face::SEMIBOLD,
            (at.avatar.0, at.name),
            ink,
        ),
    }
    let (fx, fy, fw, fh) = at.field.device(s);
    glass(pixmap, (fx, fy, fw, fh), fh / 2.0, s, tokens);
    outline(pixmap, (fx, fy, fw, fh), fh / 2.0, s, tokens.accent);
    let f = at.field;
    let top = (f.y + (f.h - 15.0 * 1.25) / 2.0) * s;
    let x = (f.x + 20.0) * s;
    if view.name.is_empty() {
        let mut line = text.fit(tr("User name"), 15.0 * s, (f.w - 40.0) * s);
        text.draw(pixmap, &mut line, x, top, quiet);
    }
    let mut line = text.fit(&view.name, 15.0 * s, (f.w - 44.0) * s);
    let end = x + line.width;
    text.draw(pixmap, &mut line, x, top, ink);
    // The caret, where the next letter goes.
    let caret = if view.name.is_empty() {
        x - 2.0 * s
    } else {
        end + 2.0 * s
    };
    fill(
        pixmap,
        caret,
        (f.y + 12.0) * s,
        2.0 * s,
        (f.h - 24.0) * s,
        0.0,
        tokens.accent,
    );
    let warn = mix(tokens.title_close_hover, ink, 0.3);
    let note = match view.field {
        Field::Refused => Some(tr("Wrong user name or password")),
        Field::Failed => Some(tr("Could not log in, try again")),
        _ => None,
    };
    if let Some(words) = note {
        centred(
            pixmap,
            text,
            s,
            w,
            words,
            NOTE,
            Face::MEDIUM,
            (at.avatar.0, at.note),
            warn,
        );
    }
}

/// Draws a message's `lines` in `colour` inside its rectangle `rect` (M5.10c5),
/// each line one `MESSAGE_TEXT` lower than the last. The lines are centred
/// for the top, centre and bottom anchors, left-aligned for the left ones
/// and right-aligned for the right ones.
fn message(
    pixmap: &mut Pixmap,
    text: &mut Text,
    s: f32,
    rect: Rect,
    lines: &[String],
    anchor: Anchor,
    colour: Colour,
) {
    let across = match anchor {
        Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => 0.0,
        Anchor::TopRight | Anchor::Right | Anchor::BottomRight => 1.0,
        Anchor::Top | Anchor::Centre | Anchor::Bottom => 0.5,
    };
    let room = rect.w * s;
    for (i, words) in lines.iter().enumerate() {
        let mut line = text.fit_in(words, MESSAGE_TEXT * s, room, Face::REGULAR);
        let left = rect.x * s + (room - line.width) * across;
        let top = (rect.y + i as f32 * MESSAGE_TEXT * 1.25) * s;
        text.draw(pixmap, &mut line, left, top, colour);
    }
}

/// Draws `words` centred on `x`, its top at `y`, logical pixels on a
/// screen `w` wide drawn at scale `s`.
#[allow(clippy::too_many_arguments)]
fn centred(
    pixmap: &mut Pixmap,
    text: &mut Text,
    s: f32,
    w: f32,
    words: &str,
    size: f32,
    face: Face,
    (x, y): (f32, f32),
    colour: edel::tokens::Colour,
) {
    let mut line = text.fit_in(words, size * s, (w - 32.0) * s, face);
    let left = x * s - line.width / 2.0;
    text.draw(pixmap, &mut line, left, y * s, colour);
}

/// A password being typed: its bytes, wiped when it is dropped or cleared.
#[derive(Default)]
pub struct Password(Vec<u8>);

impl Password {
    pub fn push(&mut self, text: &str) {
        // Long enough for any passphrase, short enough to bound memory.
        if self.0.len() + text.len() <= 1024 {
            self.0.extend_from_slice(text.as_bytes());
        }
    }

    /// Takes the last letter back.
    pub fn pop(&mut self) {
        let Ok(text) = std::str::from_utf8(&self.0) else {
            return self.clear();
        };
        let keep = text.char_indices().last().map_or(0, |(i, _)| i);
        self.0[keep..].fill(0);
        self.0.truncate(keep);
    }

    pub fn clear(&mut self) {
        self.0.fill(0);
        self.0.clear();
    }

    /// How many letters it holds.
    pub fn letters(&self) -> usize {
        std::str::from_utf8(&self.0).map_or(0, |t| t.chars().count())
    }

    /// The password as text, for greetd's message (M5.10b).
    pub fn text(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// Takes the bytes out, leaving it empty.
    pub fn take(&mut self) -> Password {
        Password(std::mem::take(&mut self.0))
    }
}

impl Drop for Password {
    fn drop(&mut self) {
        self.clear();
    }
}

/// What the helper said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    Right,
    Wrong,
    /// No helper could run: why.
    NoChecker(String),
}

/// The password helper, the first of `PASSWORD_CHECKERS` that exists.
pub fn checker() -> Option<PathBuf> {
    edel::places::PASSWORD_CHECKERS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// How long a wrong password waits before the next try.
pub const WRONG_WAIT: Duration = Duration::from_secs(2);

/// Asks `helper` whether `password` is `user`'s: it reads the password,
/// ended by a NUL byte, on its standard input and exits 0 when it is
/// right. Runs on a thread of its own, never the drawing one; a wrong
/// answer comes back after `WRONG_WAIT`.
pub fn check(helper: &std::path::Path, user: &str, password: Password) -> Checked {
    let child = Command::new(helper)
        .arg(user)
        .arg("nonull")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return Checked::NoChecker(format!("{}: {e}", helper.display())),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&password.0);
        let _ = stdin.write_all(&[0]);
    }
    drop(password);
    match child.wait() {
        Ok(status) if status.success() => Checked::Right,
        Ok(_) => {
            std::thread::sleep(WRONG_WAIT);
            Checked::Wrong
        }
        Err(e) => Checked::NoChecker(format!("{}: {e}", helper.display())),
    }
}

/// The person's login name: `USER`, else `LOGNAME`, as greetd and every
/// login set them.
pub fn user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_default()
}

/// The time and the date as the lock screen shows them.
pub fn time_and_date(now: &jiff::Zoned) -> (String, String) {
    (
        now.strftime("%H:%M").to_string(),
        now.strftime("%A %-d %B").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classic arrangement on a screen `w` by `h`, with the corner's
    /// buttons kept clear as a lock with two layouts shows them.
    fn classic_at(w: f32, h: f32, notices: bool, text: &mut Text) -> Layout {
        let corner_at = corner::layout(
            w,
            h,
            &corner::View {
                layout: Some("EN".into()),
                menu: false,
                restart_waiting: false,
                hover: None,
                session: None,
                chip_w: 0.0,
                sessions: None,
            },
        );
        // The card as `lock_card` sizes it: wide, or the width less the margins
        // on a Compact screen.
        let card = if w < COMPACT_BELOW {
            (w - 2.0 * corner::EDGE, 100.0)
        } else {
            (400.0, 100.0)
        };
        screen_layout(
            w,
            h,
            &Tokens::built_in(),
            notices,
            None,
            Some(card),
            false,
            &corner_at,
            text,
        )
    }

    /// Whether two rectangles overlap; touching edges do not.
    fn overlap(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.y + b.h && b.y < a.y + a.h
    }

    /// Every rectangle of a layout, by name, for the overlap checks. The
    /// field lies inside the person's block, so it is not listed.
    fn rects_of(at: &Layout) -> Vec<(String, Rect)> {
        let mut out = vec![("person".to_string(), at.person)];
        if let Some(r) = at.date {
            out.push(("date".into(), r));
        }
        if let Some(r) = at.clock {
            out.push(("clock".into(), r));
        }
        if let Some(r) = at.count {
            out.push(("count".into(), r));
        }
        if let Some(r) = at.player {
            out.push(("player".into(), r));
        }
        for (i, (r, _, _)) in at.messages.iter().enumerate() {
            out.push((format!("message{i}"), *r));
        }
        for (i, (r, _)) in at.pictures.iter().enumerate() {
            out.push((format!("picture{i}"), *r));
        }
        out
    }

    #[test]
    fn the_classic_parts_stand_in_order_down_the_middle_and_fit_a_phone() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        for (w, h) in [(1280.0, 800.0), (1024.0, 768.0), (360.0, 740.0)] {
            for notices in [false, true] {
                let at = classic_at(w, h, notices, &mut text);
                let date = at.date.expect("the date stands");
                let clock = at.clock.expect("the clock stands");
                assert!(date.y < clock.y, "{w}x{h}: {date:?} {clock:?}");
                assert!(clock.y + clock.h < at.person.y, "{w}x{h} {notices}");
                assert!(at.person.y < at.field.y);
                assert!(at.name < at.field.y);
                assert!(at.field.y + at.field.h < at.note);
                assert!(at.note + 20.0 < h, "{w}x{h}");
                assert!(at.field.h >= 44.0, "touch-sized");
                // The go button lies inside the field's right end.
                assert!(at.go.x > at.field.x && at.go.right() < at.field.right());
                assert!(at.go.y > at.field.y && at.go.y + at.go.h < at.field.y + at.field.h);
                // On a screen 768 high the count is left out when the person and
                // the card need the room (see the test below), so it is only
                // checked on the taller ones.
                if h > 768.0 {
                    assert_eq!(at.count.is_some(), notices);
                }
            }
        }
        // The clock is sized by the classic arrangement at a desktop's size.
        assert_eq!(
            classic_at(1280.0, 800.0, false, &mut text).clock_size,
            tokens.lock_clock as f32
        );
        assert!(
            classic_at(360.0, 740.0, false, &mut text).clock_size
                < classic_at(1280.0, 800.0, false, &mut text).clock_size
        );
    }

    #[test]
    fn classic_puts_the_date_above_the_clock_above_a_centred_field_and_the_player_bottom_left() {
        let mut text = Text::load(&Tokens::built_in().font);
        let at = classic_at(1280.0, 800.0, false, &mut text);
        let (date, clock) = (at.date.unwrap(), at.clock.unwrap());
        assert!(date.y + date.h <= clock.y, "{date:?} {clock:?}");
        assert!(clock.y + clock.h <= at.field.y, "{clock:?} {:?}", at.field);
        // Centred across: the field's middle is the screen's.
        assert!((at.field.x + at.field.w / 2.0 - 640.0).abs() < 0.01);
        assert!((date.x + date.w / 2.0 - 640.0).abs() < 0.01);
        assert!((clock.x + clock.w / 2.0 - 640.0).abs() < 0.01);
        // The player lies in the bottom left, MARGIN from its edges.
        let player = at.player.expect("the player stands");
        assert_eq!(player.x, MARGIN);
        assert!(
            (player.y + player.h - (800.0 - MARGIN)).abs() < 0.01,
            "{player:?}"
        );
    }

    #[test]
    fn on_a_phone_the_player_lies_under_the_clock() {
        let mut text = Text::load(&Tokens::built_in().font);
        for notices in [false, true] {
            let at = classic_at(360.0, 740.0, notices, &mut text);
            let clock = at.clock.unwrap();
            let player = at.player.expect("the player stands");
            assert!(
                player.y >= clock.y + clock.h,
                "{player:?} is over {clock:?}"
            );
            assert!(
                player.y + player.h <= at.person.y,
                "{player:?} meets the person"
            );
        }
    }

    #[test]
    fn a_list_puts_the_clock_top_left_and_a_message_at_the_bottom_clear_of_the_field() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let parts = vec![
            Placed {
                part: Part::Person,
                at: Anchor::Centre,
                x: 0.0,
                y: 0.21,
                text: None,
                path: None,
            },
            Placed {
                part: Part::Clock,
                at: Anchor::TopLeft,
                x: 0.0,
                y: 0.0,
                text: None,
                path: None,
            },
            Placed {
                part: Part::Message,
                at: Anchor::Bottom,
                x: 0.0,
                y: 0.0,
                text: Some("If found, call 555 0100".into()),
                path: None,
            },
        ];
        let corner_at = corner::layout(
            1280.0,
            800.0,
            &corner::View {
                layout: Some("EN".into()),
                menu: false,
                restart_waiting: false,
                hover: None,
                session: None,
                chip_w: 0.0,
                sessions: None,
            },
        );
        let at = screen_layout(
            1280.0,
            800.0,
            &tokens,
            false,
            Some(&parts),
            None,
            false,
            &corner_at,
            &mut text,
        );
        let clock = at.clock.expect("the clock stands");
        assert!(clock.x < 100.0, "{clock:?}");
        let (rect, lines, anchor) = &at.messages[0];
        assert_eq!(*anchor, Anchor::Bottom);
        assert_eq!(lines.join(" "), "If found, call 555 0100");
        assert!(rect.y + rect.h <= 800.0 - MARGIN, "{rect:?}");
        assert!(!overlap(*rect, at.field), "{rect:?} meets the field");
    }

    #[test]
    fn no_two_parts_overlap_on_any_screen_checked_classic_or_listed() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let list = vec![
            Placed {
                part: Part::Person,
                at: Anchor::Centre,
                x: 0.0,
                y: 0.21,
                text: None,
                path: None,
            },
            Placed {
                part: Part::Clock,
                at: Anchor::TopLeft,
                x: 0.0,
                y: 0.0,
                text: None,
                path: None,
            },
            Placed {
                part: Part::Message,
                at: Anchor::Bottom,
                x: 0.0,
                y: 0.0,
                text: Some("If found, call 555 0100".into()),
                path: None,
            },
        ];
        for (w, h) in [(1280.0, 800.0), (360.0, 740.0), (3840.0, 2160.0)] {
            let corner_at = corner::layout(
                w,
                h,
                &corner::View {
                    layout: Some("EN".into()),
                    menu: false,
                    restart_waiting: false,
                    hover: None,
                    session: None,
                    chip_w: 0.0,
                    sessions: None,
                },
            );
            let classic = classic_at(w, h, true, &mut text);
            let listed = screen_layout(
                w,
                h,
                &tokens,
                true,
                Some(&list),
                Some((400.0, 100.0)),
                false,
                &corner_at,
                &mut text,
            );
            for at in [classic, listed] {
                let rects = rects_of(&at);
                for (i, (name, a)) in rects.iter().enumerate() {
                    assert!(a.x >= 0.0 && a.y >= 0.0 && a.right() <= w && a.y + a.h <= h);
                    for (other, b) in &rects[i + 1..] {
                        assert!(
                            !overlap(*a, *b),
                            "{name} {a:?} meets {other} {b:?} on {w}x{h}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn on_a_768_high_screen_the_person_and_the_card_stand_and_a_count_never_rises_above_the_clock()
    {
        let mut text = Text::load(&Tokens::built_in().font);
        for (w, h) in [(1024.0, 768.0), (1366.0, 768.0)] {
            let at = classic_at(w, h, true, &mut text);
            let clock = at.clock.expect("the clock stands");
            let card = at.player.expect("the card stands");
            assert!(
                !overlap(card, at.person),
                "{card:?} meets the person on {w}x{h}"
            );
            assert!(
                at.person.y >= clock.y + clock.h,
                "{:?} rises into the clock on {w}x{h}",
                at.person
            );
            if let Some(count) = at.count {
                assert!(
                    count.y >= clock.y + clock.h,
                    "{count:?} stands above the clock"
                );
            }
        }
    }

    #[test]
    fn a_person_that_cannot_stand_clear_falls_back_to_classic() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        // A list with no person: the classic arrangement is used instead,
        // so the field still shows where classic puts it.
        let at = classic_at(360.0, 740.0, false, &mut text);
        let corner_at = corner::layout(
            360.0,
            740.0,
            &corner::View {
                layout: Some("EN".into()),
                menu: false,
                restart_waiting: false,
                hover: None,
                session: None,
                chip_w: 0.0,
                sessions: None,
            },
        );
        let broken = Look {
            parts: &[],
            player: None,
            people: false,
            keep_out: keep_out(&corner_at),
        };
        let fallback = layout(360.0, 740.0, &tokens, false, &broken, &mut text);
        assert_eq!(fallback.field, at.field);
        assert_eq!(fallback.person, at.person);
    }

    #[test]
    fn a_shake_swings_and_dies_away() {
        assert_eq!(shake_at(0), 0);
        assert!((1..SHAKE_MS).any(|ms| shake_at(ms) > 4));
        assert!((1..SHAKE_MS).any(|ms| shake_at(ms) < -4));
        assert!((0..SHAKE_MS).all(|ms| shake_at(ms).abs() <= 10));
        assert_eq!(shake_at(SHAKE_MS), 0);
        assert_eq!(shake_at(SHAKE_MS + 500), 0);
    }

    #[test]
    fn the_places_line_names_the_field_the_go_button_and_the_count() {
        let mut text = Text::load(&Tokens::built_in().font);
        let at = classic_at(1280.0, 800.0, true, &mut text);
        let line = places(&at);
        assert!(line.starts_with("field 490+"), "{line}");
        assert!(line.contains(", go "), "{line}");
        assert!(line.contains(", count "), "{line}");
        assert!(line.contains(", date "), "{line}");
        assert!(line.contains(", clock "), "{line}");
        assert!(on_go(&at, at.go.x + at.go.w / 2.0, at.go.y + at.go.h / 2.0));
        assert!(!on_go(&at, at.field.x + 10.0, at.field.y + 10.0));
        assert!(!places(&classic_at(1280.0, 800.0, false, &mut text)).contains("count"));
    }

    #[test]
    fn a_password_counts_letters_takes_one_back_and_wipes_itself() {
        let mut p = Password::default();
        p.push("pä");
        p.push("ss");
        assert_eq!(p.letters(), 4);
        p.pop();
        assert_eq!(p.letters(), 3);
        assert_eq!(p.0, "päs".as_bytes());
        let taken = p.take();
        assert_eq!(p.letters(), 0);
        assert_eq!(taken.letters(), 3);
        p.push("x");
        p.clear();
        assert!(p.0.is_empty());
    }

    #[test]
    fn a_missing_helper_is_said_and_never_unlocks() {
        let got = check(
            std::path::Path::new("/nonexistent/unix_chkpwd"),
            "ci",
            Password::default(),
        );
        assert!(matches!(got, Checked::NoChecker(_)), "{got:?}");
    }

    #[test]
    fn the_helper_answers_right_by_its_exit_and_reads_the_password() {
        let dir = std::env::temp_dir().join(format!("edel-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("chk");
        // Right when the bytes before the NUL are "open".
        std::fs::write(
            &helper,
            "#!/bin/sh\n[ \"$1\" = ci ] && [ \"$2\" = nonull ] || exit 9\n[ \"$(tr -d '\\000')\" = open ]\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut right = Password::default();
        right.push("open");
        assert_eq!(check(&helper, "ci", right), Checked::Right);
        let mut wrong = Password::default();
        wrong.push("shut");
        let started = std::time::Instant::now();
        assert_eq!(check(&helper, "ci", wrong), Checked::Wrong);
        assert!(started.elapsed() >= WRONG_WAIT);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_screen_draws_with_its_field_and_note() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let mut view = View::new("14:06".into(), "Saturday 3 October".into(), "Ali".into());
        view.dots = 6;
        view.caps = true;
        view.notices = 3;
        view.apps = vec![
            ("Mail".into(), String::new()),
            ("Chat".into(), String::new()),
        ];
        let mut pictures = Pictures::default();
        let at = classic_at(1280.0, 800.0, true, &mut text);
        let mut pixmap = Pixmap::new(1280, 800).unwrap();
        paint(
            &mut pixmap,
            &view,
            &at,
            &tokens,
            &mut text,
            None,
            &mut pictures,
            1.0,
        );
        // The bottom is the backdrop's deep colour all the way across, the
        // go button the accent, and the disc a blue of the accent.
        let px = |x: f32, y: f32| pixmap.pixel(x as u32, y as u32).unwrap();
        for x in [4.0, 640.0, 1276.0] {
            let deep = px(x, 796.0);
            assert!((deep.red() as f32 / 255.0 - tokens.lock_backdrop_deep.r).abs() < 0.02);
            assert!((deep.blue() as f32 / 255.0 - tokens.lock_backdrop_deep.b).abs() < 0.02);
        }
        let go = px(at.go.x + 3.0, at.go.y + at.go.h / 2.0);
        assert!(
            (go.blue() as f32 / 255.0 - tokens.accent.b).abs() < 0.03,
            "{go:?}"
        );
        let disc = px(at.avatar.0 - at.avatar.2 + 6.0, at.avatar.1);
        assert!(disc.blue() > disc.red(), "{disc:?}");
        // The field's glass is lighter than the backdrop beside it.
        let inside = px(at.field.x + 6.0, at.field.y + at.field.h / 2.0);
        let beside = px(at.field.x - 6.0, at.field.y + at.field.h / 2.0);
        assert!(inside.green() > beside.green(), "{inside:?} {beside:?}");
        if let Ok(dir) = std::env::var("EDEL_LOCK_PNG") {
            pixmap.save_png(format!("{dir}/lock.png")).unwrap();
            let mut phone = Pixmap::new(720, 1480).unwrap();
            let phone_at = classic_at(360.0, 740.0, true, &mut text);
            paint(
                &mut phone,
                &view,
                &phone_at,
                &tokens,
                &mut text,
                None,
                &mut pictures,
                2.0,
            );
            phone.save_png(format!("{dir}/lock-phone.png")).unwrap();
            let mut wrong = view.clone();
            wrong.field = Field::Wrong;
            wrong.caps = false;
            wrong.notices = 0;
            wrong.shake = shake_at(60);
            let mut pixmap = Pixmap::new(1280, 800).unwrap();
            let wrong_at = classic_at(1280.0, 800.0, false, &mut text);
            paint(
                &mut pixmap,
                &wrong,
                &wrong_at,
                &tokens,
                &mut text,
                None,
                &mut pictures,
                1.0,
            );
            pixmap.save_png(format!("{dir}/lock-wrong.png")).unwrap();
            let mut name = View::new("14:06".into(), "Saturday 3 October".into(), "c".into());
            name.asking_name = true;
            let mut pixmap = Pixmap::new(1280, 800).unwrap();
            paint(
                &mut pixmap,
                &name,
                &at,
                &tokens,
                &mut text,
                None,
                &mut pictures,
                1.0,
            );
            pixmap.save_png(format!("{dir}/greeter-name.png")).unwrap();
            // The parts a list says, as CI's lock parts test sets them.
            let parts = vec![
                Placed {
                    part: Part::Person,
                    at: Anchor::Centre,
                    x: 0.0,
                    y: 0.21,
                    text: None,
                    path: None,
                },
                Placed {
                    part: Part::Clock,
                    at: Anchor::TopLeft,
                    x: 0.0,
                    y: 0.0,
                    text: None,
                    path: None,
                },
                Placed {
                    part: Part::Message,
                    at: Anchor::Bottom,
                    x: 0.0,
                    y: 0.0,
                    text: Some("If found, call 555 0100".into()),
                    path: None,
                },
            ];
            let corner_at = corner::layout(
                1280.0,
                800.0,
                &corner::View {
                    layout: Some("EN".into()),
                    menu: false,
                    restart_waiting: false,
                    hover: None,
                    session: None,
                    chip_w: 0.0,
                    sessions: None,
                },
            );
            let listed = screen_layout(
                1280.0,
                800.0,
                &tokens,
                true,
                Some(&parts),
                Some((400.0, 100.0)),
                false,
                &corner_at,
                &mut text,
            );
            let mut pixmap = Pixmap::new(1280, 800).unwrap();
            paint(
                &mut pixmap,
                &view,
                &listed,
                &tokens,
                &mut text,
                None,
                &mut pictures,
                1.0,
            );
            pixmap.save_png(format!("{dir}/lock-parts.png")).unwrap();
        }
    }
    #[test]
    fn a_row_of_people_takes_the_disc_and_the_hint_shows_only_with_people() {
        use crate::people::{Person, Row};
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let mut view = View::new("14:06".into(), "Saturday 3 October".into(), "ali".into());
        let person = |login: &str, shown: &str| Person {
            login: login.into(),
            shown: shown.into(),
        };
        // The chosen name lands where the lock screen's name is, so the row
        // and the single person line up.
        let at = classic_at(1280.0, 800.0, false, &mut text);
        assert_eq!(
            at.name,
            at.avatar.1 + crate::people::CHOSEN / 2.0 + crate::people::CHOSEN_NAME_GAP
        );
        assert_eq!(note_text(&view), None);
        view.people = Some(Row {
            people: vec![person("ali", "Ali"), person("mira", "Mira")],
            chosen: 0,
        });
        assert_eq!(
            note_text(&view),
            Some("Left and right choose a person"),
            "the hint shows with people while the field is ready"
        );
        view.caps = true;
        assert_eq!(note_text(&view), Some("Caps Lock is on"));
        view.caps = false;
        view.field = Field::Wrong;
        assert_eq!(note_text(&view), Some("Wrong password, try again"));
        view.field = Field::Ready;
        view.people = None;
        assert_eq!(note_text(&view), None, "no hint on the lock screen");
        view.people = Some(Row {
            people: vec![person("ali", "Ali"), person("mira", "Mira")],
            chosen: 2,
        });
        let mut pixmap = Pixmap::new(1280, 800).unwrap();
        paint(
            &mut pixmap,
            &view,
            &at,
            &tokens,
            &mut text,
            None,
            &mut Pictures::default(),
            1.0,
        );
        if let Ok(dir) = std::env::var("EDEL_LOCK_PNG") {
            pixmap.save_png(format!("{dir}/lock-people.png")).unwrap();
        }
    }
}
