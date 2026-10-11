//! The lock screen's edit mode (M5.10c6b), plain data and drawing, tested
//! without a display: the lock screen as it stands, live over the desktop,
//! each part outlined as a tile; the one part being dragged and where it
//! would stand, snapped to the guides; the drawer of the parts not shown,
//! which takes a part dragged onto it and gives one dragged from it. The
//! drawer is `editor.rs`'s, shared with the panel editor, so a change to
//! its look is one change for both (ADR-010). `lock_editor_card.rs` owns the
//! surface, the pointer and the keyboard, and writes what Done says. The
//! keyboard's focus and its keys are here too (M5.10c6c), as are the words a
//! screen reader reads for each part.

use accesskit::Role;
use edel::i18n::{tr, trf};
use edel::lock_screen::{self, Anchor, Guide, Part, Placed, SNAP};
use edel::tokens::{Colour, Tokens};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

use crate::a11y::Item;
use crate::editor::{self, Act, DRAG_START, Tile};
use crate::lock;
use crate::paint::{Text, fill, outline};
use crate::player;
use crate::popup::{self, Rect};
use crate::trayview::Key;

/// The drawer's room above the screen's bottom edge, logical pixels: the
/// drawer is centred across the screen, this far up from its bottom.
pub const DRAWER_GAP: f32 = 24.0;

/// A message as it is added from the drawer, logical pixels: its words wrap
/// to one line of 300 px, 50 px high, the size of a line of its words.
const MESSAGE_NEW: (f32, f32) = (300.0, 50.0);
/// The sizes of the other parts dragged in from the drawer, logical pixels,
/// each close to what `lock::layout` gives it at its default size (the
/// clock's digital face at the token's size, the date's line, the count's
/// pill with one app, the battery's icon and percent). They only size the
/// rectangle that follows the pointer until the part is dropped, which the
/// layout then sizes for real.
const CLOCK_NEW: (f32, f32) = (240.0, 120.0);
const DATE_NEW: (f32, f32) = (260.0, 24.0);
const COUNT_NEW: (f32, f32) = (200.0, 36.0);
const BATTERY_NEW: (f32, f32) = (80.0, 22.0);

/// How far a placed part's outline is drawn in, and the line's width, logical
/// pixels, and the accent's line for a hovered or dragged part.
const PART_LINE: f32 = 1.5;
const HOVER_LINE: f32 = 2.0;
/// The guides' width, logical pixels.
const GUIDE_LINE: f32 = 1.0;
/// How much of `lock_glass` a dragged part's new place is veiled with.
const VEIL: f32 = 0.35;

/// What the edit mode shows (M5.10c6b): the parts as they stand, the one
/// being dragged and where, the guides it snaps to, the part under the
/// pointer, and the drawer.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The lock screen's parts, in the list's order: what Done writes.
    pub parts: Vec<Placed>,
    /// The screen's size in logical pixels.
    pub screen: (f32, f32),
    /// The press or the drag in progress, if any.
    pub drag: Option<Drag>,
    /// The lines the dragged part snaps to, across the screen.
    pub guides: Vec<Guide>,
    /// The part under the pointer, by its index in `parts`.
    pub hover: Option<usize>,
    /// The keyboard's focus, if any (M5.10c6c). `drawer.focus` is the same
    /// focus when it is in the drawer; `set_focus` keeps the two alike.
    pub focus: Option<Focus>,
    /// The drawer of the parts not shown (kind `LockScreen`).
    pub drawer: editor::View,
}

/// The keyboard's focus in the edit mode (M5.10c6c): a placed part by its
/// index in `parts`, or a part of the drawer (a tile, Undo or Done).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Part(usize),
    Drawer(editor::Part),
}

/// What a key does in the edit mode (M5.10c6c): move the focus, move or take
/// away the focused part, add a drawer tile's part, act on Undo or Done, or
/// nothing. Escape closes, and Return with no focus is Done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Focus(Option<Focus>),
    Nudge(usize, i32, i32),
    Remove(usize),
    Add(Part),
    Undo,
    Done,
    Close,
    Nothing,
}

/// Where a drag began: a part on the screen, by its index in `parts`, or a
/// tile of the drawer, by the part it stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum From {
    Part(usize),
    Drawer(Part),
}

/// A press that may become a drag (M5.10c6b): where it began, where the
/// pointer is now, both in screen logical pixels, whether it has moved far
/// enough to be a drag (`editor::DRAG_START`), and whether it is over the
/// drawer, which takes a part away.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub from: From,
    pub start: (f32, f32),
    pub now: (f32, f32),
    pub moving: bool,
    pub over_drawer: bool,
}

/// The title of a part as people read it (M5.10c6b), translated.
pub fn part_title(part: Part) -> String {
    match part {
        Part::Clock => tr("Clock").into(),
        Part::Date => tr("Date").into(),
        Part::Count => tr("Notifications").into(),
        Part::Player => tr("Now playing").into(),
        Part::Person => tr("You (the person)").into(),
        Part::Message => tr("Message").into(),
        Part::Picture => tr("Picture").into(),
        Part::Battery => tr("Battery").into(),
    }
}

/// The part a drawer tile is named for (`Part::name`), if the release has it.
pub fn part_named(name: &str) -> Option<Part> {
    [
        Part::Clock,
        Part::Date,
        Part::Count,
        Part::Player,
        Part::Person,
        Part::Message,
        Part::Picture,
        Part::Battery,
    ]
    .into_iter()
    .find(|part| part.name() == name)
}

/// The size a part has when it is dragged in from the drawer, logical pixels
/// (see the consts above).
fn new_size(part: Part) -> (f32, f32) {
    match part {
        Part::Message => MESSAGE_NEW,
        Part::Clock => CLOCK_NEW,
        Part::Date => DATE_NEW,
        Part::Count => COUNT_NEW,
        Part::Player => player::wide_size(1),
        Part::Battery => BATTERY_NEW,
        Part::Person | Part::Picture => MESSAGE_NEW,
    }
}

/// The drawer's tiles (M5.10c6b): one for each part `lock_screen::missing`
/// offers, in its order, with no picture (a picture needs a file, which the
/// edit mode does not choose yet, M5.10c6c), and none of them placed.
pub fn drawer_tiles(parts: &[Placed], battery: bool) -> Vec<Tile> {
    lock_screen::missing(parts, battery)
        .into_iter()
        .filter(|part| *part != Part::Picture)
        .map(|part| Tile {
            name: part.name(),
            title: part_title(part),
            shown: String::new(),
            placed: false,
        })
        .collect()
}

/// The drawer's rectangle on a screen of `screen` logical pixels: centred
/// across it, its bottom `DRAWER_GAP` above the screen's bottom edge.
pub fn drawer_rect(screen: (f32, f32), drawer: &editor::Layout) -> Rect {
    let (w, h) = (drawer.size.0 as f32, drawer.size.1 as f32);
    Rect::new(
        ((screen.0 - w) / 2.0).round(),
        screen.1 - DRAWER_GAP - h,
        w,
        h,
    )
}

/// The room kept round the lock screen's picture in the edit mode, logical
/// pixels: from the screen's top and sides, and between it and the drawer.
pub const PREVIEW_GAP: f32 = 24.0;

/// Where the edit mode shows the lock screen (M5.10c6b): the whole screen's
/// lock drawn smaller, `k` of its size, its top left corner at `x`, `y` on
/// the screen, in the room above the drawer, so the drawer never covers a
/// part (as the phones show a lock screen being edited, live in the middle
/// with the controls below it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preview {
    pub k: f32,
    pub x: f32,
    pub y: f32,
}

impl Preview {
    /// A point on the screen as a point on the lock screen being edited.
    pub fn to_lock(self, (x, y): (f32, f32)) -> (f32, f32) {
        ((x - self.x) / self.k, (y - self.y) / self.k)
    }

    /// A rectangle on the lock screen as it shows on the screen.
    pub fn to_screen(self, r: Rect) -> Rect {
        Rect::new(
            self.x + r.x * self.k,
            self.y + r.y * self.k,
            r.w * self.k,
            r.h * self.k,
        )
    }
}

/// The lock screen's place in the edit mode on a screen of `screen` logical
/// pixels with `drawer` laid out: as large as fits between the screen's top
/// and the drawer, and inside the sides, `PREVIEW_GAP` from each, centred
/// across; never larger than the screen itself.
pub fn preview(screen: (f32, f32), drawer: &editor::Layout) -> Preview {
    let place = drawer_rect(screen, drawer);
    let room_h = (place.y - 2.0 * PREVIEW_GAP).max(1.0);
    let room_w = (screen.0 - 2.0 * PREVIEW_GAP).max(1.0);
    let k = (room_h / screen.1).min(room_w / screen.0).min(1.0);
    Preview {
        k,
        x: ((screen.0 - screen.0 * k) / 2.0).round(),
        y: PREVIEW_GAP,
    }
}

/// Lays the lock screen's picture `lock` (the whole screen at `s`) into
/// `target`, the edit mode's surface at `s`: the backdrop's deep colour all
/// round, then the picture at `preview`'s place and size, smoothly scaled.
pub fn compose(target: &mut Pixmap, lock: &Pixmap, preview: Preview, tokens: &Tokens, s: f32) {
    let deep = tokens.lock_backdrop_deep;
    if let Some(colour) = tiny_skia::Color::from_rgba(deep.r, deep.g, deep.b, 1.0) {
        target.fill(colour);
    }
    let smooth = PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    target.draw_pixmap(
        0,
        0,
        lock.as_ref(),
        &smooth,
        Transform::from_row(preview.k, 0.0, 0.0, preview.k, preview.x * s, preview.y * s),
        None,
    );
}

/// A rectangle of the edit model's, as the lock screen's maths reads it.
fn to_lock(r: Rect) -> lock_screen::Rect {
    lock_screen::Rect {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
    }
}

/// The edit model's rectangle, from the lock screen's maths.
fn from_lock(r: lock_screen::Rect) -> Rect {
    Rect::new(r.x, r.y, r.w, r.h)
}

/// Where the dragged part would stand, and the guides it snaps to (pure:
/// nothing is changed). A part from the screen moves by the pointer's way
/// from where it began; a drawer part is centred on the pointer at its
/// `new_size`. Both are snapped against the other parts with `SNAP` and the
/// lock's margin (`lock_screen::snap`). Over the drawer nothing snaps. None
/// with no drag, or a part from the screen that is not placed.
fn target(view: &View, at: &lock::Layout) -> Option<(Rect, Vec<Guide>)> {
    let drag = view.drag?;
    let (raw, own) = match drag.from {
        From::Part(i) => {
            let base = (*at.placed.get(i)?)?;
            let dx = drag.now.0 - drag.start.0;
            let dy = drag.now.1 - drag.start.1;
            (Rect::new(base.x + dx, base.y + dy, base.w, base.h), Some(i))
        }
        From::Drawer(part) => {
            let (w, h) = new_size(part);
            (
                Rect::new(drag.now.0 - w / 2.0, drag.now.1 - h / 2.0, w, h),
                None,
            )
        }
    };
    if drag.over_drawer {
        return Some((raw, Vec::new()));
    }
    let others: Vec<lock_screen::Rect> = at
        .placed
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != own)
        .filter_map(|(_, r)| r.map(to_lock))
        .collect();
    let (rect, guides) = lock_screen::snap(to_lock(raw), &others, view.screen, lock::MARGIN, SNAP);
    Some((from_lock(rect), guides))
}

/// The rectangle the dragged part is at now, snapped, and the guides it lies
/// on, which are written into `view.guides` for the drawing (M5.10c6b). None
/// with no drag.
pub fn dragged_rect(view: &mut View, at: &lock::Layout) -> Option<Rect> {
    let (rect, guides) = target(view, at)?;
    view.guides = guides;
    Some(rect)
}

/// What a drag that ends now does (M5.10c6b): the new parts and the words the
/// log says, or None when nothing changes. A press that did not move
/// (`DRAG_START`) is a click, not a drop. A part dropped on the screen is
/// moved to its snapped place (`moved clock to top_right`); a part dropped on
/// the drawer is taken away when `lock_screen::removable` (`took away clock`),
/// and the person is refused (None; the caller says so); a drawer part dropped
/// on the screen is added where it lands (`added message`, with the
/// placeholder words), when `lock_screen::with_added` takes it. A drawer
/// tile's battery is offered only on a machine that has one, so the tile
/// existing means the battery is there (`battery` true).
pub fn drop(view: &View, at: &lock::Layout) -> Option<(Vec<Placed>, String)> {
    let drag = view.drag?;
    let far = (drag.now.0 - drag.start.0).hypot(drag.now.1 - drag.start.1) >= DRAG_START;
    if !drag.moving || !far {
        return None;
    }
    let (rect, _) = target(view, at)?;
    let margin = lock::MARGIN;
    match drag.from {
        From::Part(i) if drag.over_drawer => {
            let name = view.parts.get(i)?.part.name();
            if !lock_screen::removable(&view.parts, i) {
                return None;
            }
            Some((
                lock_screen::without(&view.parts, i),
                format!("took away {name}"),
            ))
        }
        From::Part(i) => {
            let moved = lock_screen::moved(view.parts.get(i)?, to_lock(rect), view.screen, margin);
            let words = format!("moved {} to {}", moved.part.name(), moved.at.name());
            let mut parts = view.parts.clone();
            parts[i] = moved;
            Some((parts, words))
        }
        From::Drawer(_) if drag.over_drawer => None,
        From::Drawer(part) => adding(view, part, rect),
    }
}

/// What adding `part` at `rect` does (M5.10c6c): the parts with it, and the
/// words the log says (`added message`), or None when the model refuses it.
/// A drop on the screen and Enter on a drawer tile both come here, so the
/// two add a part the same way.
fn adding(view: &View, part: Part, rect: Rect) -> Option<(Vec<Placed>, String)> {
    let parts = lock_screen::with_added(
        &view.parts,
        part,
        to_lock(rect),
        view.screen,
        lock::MARGIN,
        &lock_screen::message_placeholder(),
        true,
    );
    if parts == view.parts {
        return None;
    }
    Some((parts, format!("added {}", part.name())))
}

/// The part added from the drawer by the keyboard (M5.10c6c): centred on the
/// screen's middle, as a drop there would put it, through `adding`.
pub fn added_at_middle(view: &View, part: Part) -> Option<(Vec<Placed>, String)> {
    let (w, h) = new_size(part);
    let (cx, cy) = (view.screen.0 / 2.0, view.screen.1 / 2.0);
    adding(view, part, Rect::new(cx - w / 2.0, cy - h / 2.0, w, h))
}

/// Moves the keyboard's focus to `focus` (M5.10c6c), the drawer's own too.
pub fn set_focus(view: &mut View, focus: Option<Focus>) {
    view.focus = focus;
    view.drawer.focus = match focus {
        Some(Focus::Drawer(part)) => Some(part),
        _ => None,
    };
}

/// The keyboard's ring in the edit mode (M5.10c6c): each placed part in the
/// list's order, then the drawer's ring (its tiles, Undo and Done).
pub fn ring(view: &View) -> Vec<Focus> {
    (0..view.parts.len())
        .map(Focus::Part)
        .chain(editor::ring(&view.drawer).into_iter().map(Focus::Drawer))
        .collect()
}

/// The next focus of the ring after `view`'s, forwards or backwards (Tab,
/// Shift+Tab), wrapping; from no focus, the first or the last (M5.10c6c).
fn step_ring(view: &View, backwards: bool) -> Option<Focus> {
    let ring = ring(view);
    let n = ring.len();
    if n == 0 {
        return None;
    }
    let at = view.focus.and_then(|f| ring.iter().position(|r| *r == f));
    let next = match (at, backwards) {
        (None, false) => 0,
        (None, true) => n - 1,
        (Some(i), false) => (i + 1) % n,
        (Some(i), true) => (i + n - 1) % n,
    };
    ring.get(next).copied()
}

/// What a key does with the focus in the edit mode (M5.10c6c). Tab and
/// Shift+Tab (`shift`) step the ring; Escape closes; with no focus Return is
/// Done and the other keys do nothing. On a placed part the arrows move it
/// one step of `lock_screen::NUDGE` (ten with Shift), and Delete or Backspace
/// takes it away when `lock_screen::removable`. On a drawer tile Return adds
/// its part; on Undo and Done it acts; the arrows move between the drawer's
/// parts as `editor::key` says.
pub fn key(view: &View, key: Key, shift: bool) -> Step {
    match key {
        Key::Escape => return Step::Close,
        Key::Tab => return Step::Focus(step_ring(view, shift)),
        _ => {}
    }
    match view.focus {
        None if key == Key::Activate => Step::Done,
        None => Step::Nothing,
        Some(Focus::Part(i)) => part_key(view, i, key, shift),
        Some(Focus::Drawer(part)) => drawer_key(view, part, key),
    }
}

/// A key on a placed part (M5.10c6c): see `key`.
fn part_key(view: &View, i: usize, key: Key, shift: bool) -> Step {
    let step = if shift { 10 } else { 1 };
    match key {
        Key::Left => Step::Nudge(i, -step, 0),
        Key::Right => Step::Nudge(i, step, 0),
        Key::Up => Step::Nudge(i, 0, -step),
        Key::Down => Step::Nudge(i, 0, step),
        Key::Delete if lock_screen::removable(&view.parts, i) => Step::Remove(i),
        _ => Step::Nothing,
    }
}

/// A key on the drawer's focus (M5.10c6c): see `key`. The arrows and Escape
/// are `editor::key`'s, which the drawer shares with the panel editor.
fn drawer_key(view: &View, part: editor::Part, key: Key) -> Step {
    if key == Key::Activate {
        return match part {
            editor::Part::Tile(i) => view
                .drawer
                .tiles
                .get(i)
                .and_then(|tile| part_named(tile.name))
                .map_or(Step::Nothing, Step::Add),
            editor::Part::Undo => Step::Undo,
            editor::Part::Done => Step::Done,
        };
    }
    match editor::key(&view.drawer, Some(part), key) {
        editor::Step::Focus(p) => Step::Focus(Some(Focus::Drawer(p))),
        editor::Step::Act(Act::Undo) => Step::Undo,
        editor::Step::Act(Act::Done) => Step::Done,
        editor::Step::Act(Act::Close) => Step::Close,
        editor::Step::Nothing => Step::Nothing,
    }
}

/// The words a screen reader says for a part's anchor (M5.10c6c): where it
/// stands across the screen and down it, as "top left", "top centre",
/// "middle", "bottom right" and so on.
pub fn where_words(at: Anchor) -> &'static str {
    match at {
        Anchor::TopLeft => tr("top left"),
        Anchor::Top => tr("top centre"),
        Anchor::TopRight => tr("top right"),
        Anchor::Left => tr("middle left"),
        Anchor::Centre => tr("middle"),
        Anchor::Right => tr("middle right"),
        Anchor::BottomLeft => tr("bottom left"),
        Anchor::Bottom => tr("bottom centre"),
        Anchor::BottomRight => tr("bottom right"),
    }
}

/// The name the log says for the focus (M5.10c6c): the part's name, `undo`,
/// `done`, or `tile NAME` for a drawer tile.
pub fn focus_name(view: &View, focus: Focus) -> String {
    match focus {
        Focus::Part(i) => view.parts.get(i).map_or("part", |p| p.part.name()).into(),
        Focus::Drawer(editor::Part::Undo) => "undo".into(),
        Focus::Drawer(editor::Part::Done) => "done".into(),
        Focus::Drawer(editor::Part::Tile(j)) => format!(
            "tile {}",
            view.drawer.tiles.get(j).map_or("tile", |t| t.name)
        ),
    }
}

/// A rectangle on the screen, as the screen reader's bounds.
fn bounds(r: Rect) -> accesskit::Rect {
    accesskit::Rect::new(
        f64::from(r.x),
        f64::from(r.y),
        f64::from(r.x + r.w),
        f64::from(r.y + r.h),
    )
}

/// What a screen reader finds on the edit mode (M5.10c6c): each placed part
/// as a button named `"{title}, {where}"`, then the drawer's tiles, Undo and
/// Done (`editor::nodes`), moved to the drawer's place on the screen. The
/// placed parts' rectangles are on the screen through `preview`.
pub fn nodes(
    view: &View,
    at: &lock::Layout,
    preview: Preview,
    drawer_at: Rect,
    drawer: &editor::Layout,
) -> Vec<Item> {
    let mut out: Vec<Item> = view
        .parts
        .iter()
        .zip(&at.placed)
        .filter_map(|(part, rect)| {
            let rect = (*rect)?;
            let title = part_title(part.part);
            Some(Item {
                role: Role::Button,
                label: trf(
                    "{title}, {where}",
                    &[("title", title.as_str()), ("where", where_words(part.at))],
                ),
                bounds: bounds(preview.to_screen(rect)),
                children: Vec::new(),
                toggled: None,
                value: None,
            })
        })
        .collect();
    let (dx, dy) = (f64::from(drawer_at.x), f64::from(drawer_at.y));
    out.extend(
        editor::nodes(&view.drawer, drawer)
            .into_iter()
            .map(|mut n| {
                let b = n.bounds;
                n.bounds = accesskit::Rect::new(b.x0 + dx, b.y0 + dy, b.x1 + dx, b.y1 + dy);
                n
            }),
    );
    out
}

/// The screen reader's item for the keyboard's focus, in `nodes`' order
/// (M5.10c6c): a placed part's place among those drawn, else the drawer's
/// item after them. None with no focus, or a part that is not drawn.
pub fn focus_node(view: &View, at: &lock::Layout) -> Option<usize> {
    let drawn = at.placed.iter().filter(|r| r.is_some()).count();
    let tiles = view.drawer.tiles.len();
    match view.focus? {
        Focus::Part(i) => {
            if at.placed.get(i)?.is_none() {
                return None;
            }
            Some(at.placed[..i].iter().filter(|r| r.is_some()).count())
        }
        Focus::Drawer(editor::Part::Tile(j)) => Some(drawn + j),
        Focus::Drawer(editor::Part::Undo) => Some(drawn + tiles),
        Focus::Drawer(editor::Part::Done) => Some(drawn + tiles + 1),
    }
}

/// Where the parts lie, as the log line CI reads (M5.10c6b), in screen
/// logical pixels: `screen WxH, partN NAME X+Y+WxH, ..., drawer X+Y+WxH,
/// tileN NAME X+Y+WxH, ..., undo X+Y+WxH, done X+Y+WxH`. `partN` is each
/// placed part by its index in the list; the tiles, Undo and Done are the
/// drawer's, offset by its place.
pub fn places(
    view: &View,
    at: &lock::Layout,
    preview: Preview,
    drawer_at: Rect,
    drawer: &editor::Layout,
) -> String {
    let at_rect = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let moved = |r: Rect| Rect::new(r.x + drawer_at.x, r.y + drawer_at.y, r.w, r.h);
    let mut out = format!("screen {:.0}x{:.0}", view.screen.0, view.screen.1);
    for (i, (part, rect)) in view.parts.iter().zip(&at.placed).enumerate() {
        if let Some(rect) = rect {
            out.push_str(&format!(
                ", part{i} {} {}",
                part.part.name(),
                at_rect(preview.to_screen(*rect))
            ));
        }
    }
    out.push_str(&format!(", drawer {}", at_rect(drawer_at)));
    for (i, (tile, rect)) in view.drawer.tiles.iter().zip(&drawer.tiles).enumerate() {
        out.push_str(&format!(
            ", tile{i} {} {}",
            tile.name,
            at_rect(moved(*rect))
        ));
    }
    out.push_str(&format!(
        ", undo {}, done {}",
        at_rect(moved(drawer.undo)),
        at_rect(moved(drawer.done))
    ));
    out
}

/// Draws the edit mode at `s` over what `lock::paint` drew (M5.10c6b): each
/// placed part outlined in `lock_glass_edge` (2 px accent for the one under
/// the pointer or being dragged); a dragged part's new place veiled with
/// `lock_glass` and outlined in accent; the guides as 1 px accent lines
/// across the whole screen; a player's place drawn as the glass with "Now
/// playing" in it, since the edit mode has no card to show. The drawer is
/// `paint_drawer`'s, over the composed picture.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    at: &lock::Layout,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let radius = tokens.radius_control as f32 * s;
    let dragged = view.drag.filter(|d| d.moving);
    for (i, (part, rect)) in view.parts.iter().zip(&at.placed).enumerate() {
        let Some(rect) = rect else {
            continue;
        };
        let device = rect.device(s);
        if part.part == Part::Player {
            lock::glass(pixmap, device, radius, s, tokens);
            let size = tokens.panel_text_size as f32 * s;
            let mut line = text.line(tr("Now playing"), size);
            let x = device.0 + ((device.2 - line.width) / 2.0).round();
            let y = crate::popup::middle(rect.y, rect.h, size, s);
            text.draw(pixmap, &mut line, x, y, tokens.backdrop_text);
        }
        let marked = view.hover == Some(i) || dragged.is_some_and(|d| d.from == From::Part(i));
        if marked {
            outline(pixmap, device, radius, HOVER_LINE * s, tokens.accent);
        } else {
            outline(
                pixmap,
                device,
                radius,
                PART_LINE * s,
                tokens.lock_glass_edge,
            );
        }
    }
    // The dragged part at the place it would take, veiled, with its outline.
    if let Some((rect, _)) = dragged
        .filter(|d| !d.over_drawer)
        .and_then(|_| target(view, at))
    {
        let device = rect.device(s);
        let veil = Colour {
            a: VEIL,
            ..tokens.lock_glass
        };
        fill(pixmap, device.0, device.1, device.2, device.3, radius, veil);
        outline(pixmap, device, radius, HOVER_LINE * s, tokens.accent);
    }
    // The keyboard's focus on a placed part: the ring round it (M5.10c6c).
    if let Some(Focus::Part(i)) = view.focus {
        if let Some(Some(rect)) = at.placed.get(i) {
            popup::focus_ring(pixmap, *rect, tokens.radius_control as f32, s, tokens);
        }
    }
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    for guide in &view.guides {
        if guide.vertical {
            fill(
                pixmap,
                (guide.at * s).round(),
                0.0,
                GUIDE_LINE * s,
                ph,
                0.0,
                tokens.accent,
            );
        } else {
            fill(
                pixmap,
                0.0,
                (guide.at * s).round(),
                pw,
                GUIDE_LINE * s,
                0.0,
                tokens.accent,
            );
        }
    }
}

/// Draws the drawer through `editor::paint` at its place on the edit mode's
/// surface, `pixmap` at `s`, over the lock screen's picture.
pub fn paint_drawer(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, text: &mut Text, s: f32) {
    let layout = editor::layout(&view.drawer);
    let place = drawer_rect(view.screen, &layout);
    if let Some(mut layer) = Pixmap::new(
        (layout.size.0 as f32 * s).ceil() as u32,
        (layout.size.1 as f32 * s).ceil() as u32,
    ) {
        editor::paint(
            &mut layer,
            &view.drawer,
            &layout,
            tokens,
            None,
            Some(text),
            s,
        );
        pixmap.draw_pixmap(
            (place.x * s).round() as i32,
            (place.y * s).round() as i32,
            layer.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corner;
    use crate::editor::Kind;
    use edel::lock_screen::{Anchor, Background};
    use edel::tokens::Tokens;

    /// The classic arrangement on a 1280 by 800 screen, laid out as the lock
    /// lays it out with the corner's buttons kept clear and the battery
    /// offered (`battery` true, so the layout has none of its own).
    fn at_classic(parts: &[Placed]) -> lock::Layout {
        let mut text = Text::load(&Tokens::built_in().font);
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
        let look = lock::Look {
            parts,
            player: Some(player::wide_size(1)),
            people: false,
            keep_out: lock::keep_out(&corner_at),
            battery: false,
            background: Background::Backdrop,
        };
        lock::layout(1280.0, 800.0, &Tokens::built_in(), true, &look, &mut text)
    }

    /// The edit mode's view of `parts` on a 1280 by 800 screen, with the
    /// drawer laid out for them.
    fn view_of(parts: Vec<Placed>) -> View {
        let drawer = editor::View {
            kind: Kind::LockScreen,
            tiles: drawer_tiles(&parts, true),
            compact: false,
            width: 1280,
            hover: None,
            focus: None,
            lifted: None,
            taking: false,
        };
        View {
            parts,
            screen: (1280.0, 800.0),
            drag: None,
            guides: Vec::new(),
            hover: None,
            focus: None,
            drawer,
        }
    }

    /// The index of the part called `name` in `parts`.
    fn index_of(parts: &[Placed], name: &str) -> usize {
        parts.iter().position(|p| p.part.name() == name).unwrap()
    }

    /// The centre of a placed rectangle.
    fn centre(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// A drag that is moving, from `from` (started at `start`) to `now`.
    fn drag(from: From, start: (f32, f32), now: (f32, f32), over_drawer: bool) -> Drag {
        Drag {
            from,
            start,
            now,
            moving: true,
            over_drawer,
        }
    }

    #[test]
    fn the_drawer_of_classic_offers_the_message_and_the_battery_but_not_the_person() {
        let classic = lock_screen::classic(false);
        let names: Vec<&str> = drawer_tiles(&classic, true)
            .iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["message", "battery"]);
        assert!(!names.contains(&"person"));
        let no_battery: Vec<&str> = drawer_tiles(&classic, false)
            .iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(no_battery, ["message"], "a machine without one offers none");
    }

    #[test]
    fn a_clock_dragged_to_the_top_right_stands_there_and_says_so() {
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let i = index_of(&classic, "clock");
        let r = at.placed[i].expect("the clock is drawn");
        // Its right edge on the margin and its top on the top margin: the
        // pointer moves the clock's centre to where that puts it.
        let target_x = 1280.0 - lock::MARGIN - r.w / 2.0;
        let target_y = lock::MARGIN + r.h / 2.0;
        let start = centre(r);
        let mut view = view_of(classic.clone());
        view.drag = Some(drag(From::Part(i), start, (target_x, target_y), false));
        dragged_rect(&mut view, &at);
        let (parts, words) = drop(&view, &at).expect("a moved clock");
        assert_eq!(words, "moved clock to top_right");
        assert_eq!(parts[i].at, Anchor::TopRight);
        assert_eq!(parts[i].part, Part::Clock);
        assert_eq!(parts.len(), classic.len());
    }

    #[test]
    fn a_clock_dragged_onto_the_drawer_is_taken_away() {
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let i = index_of(&classic, "clock");
        let mut view = view_of(classic.clone());
        view.drag = Some(drag(From::Part(i), (300.0, 60.0), (640.0, 700.0), true));
        let (parts, words) = drop(&view, &at).expect("a taken clock");
        assert_eq!(words, "took away clock");
        assert!(parts.iter().all(|p| p.part != Part::Clock));
        assert_eq!(parts.len(), classic.len() - 1);
    }

    #[test]
    fn the_person_dragged_onto_the_drawer_is_refused() {
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let i = index_of(&classic, "person");
        let mut view = view_of(classic.clone());
        view.drag = Some(drag(From::Part(i), (640.0, 400.0), (640.0, 700.0), true));
        assert_eq!(drop(&view, &at), None, "nobody is ever taken away");
    }

    #[test]
    fn a_message_dragged_to_the_bottom_middle_is_added_there_with_the_placeholder() {
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let mut view = view_of(classic.clone());
        view.drag = Some(drag(
            From::Drawer(Part::Message),
            (120.0, 500.0),
            (640.0, 650.0),
            false,
        ));
        let (parts, words) = drop(&view, &at).expect("an added message");
        assert_eq!(words, "added message");
        let message = parts.last().expect("the message is last");
        assert_eq!(message.part, Part::Message);
        assert_eq!(message.at, Anchor::Bottom);
        assert_eq!(
            message.text.as_deref(),
            Some(lock_screen::message_placeholder().as_str())
        );
        assert!(lock_screen::check(&parts).is_ok());
    }

    #[test]
    fn a_drag_of_three_pixels_is_no_drop() {
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let i = index_of(&classic, "clock");
        let mut view = view_of(classic.clone());
        view.drag = Some(Drag {
            from: From::Part(i),
            start: (300.0, 60.0),
            now: (303.0, 60.0),
            moving: false,
            over_drawer: false,
        });
        assert_eq!(drop(&view, &at), None);
        view.drag = Some(drag(From::Part(i), (300.0, 60.0), (303.0, 60.0), false));
        assert_eq!(drop(&view, &at), None, "under DRAG_START it is a click");
    }

    #[test]
    fn places_name_every_placed_part_and_each_tile() {
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let view = view_of(classic.clone());
        let drawer = editor::layout(&view.drawer);
        let drawer_at = drawer_rect(view.screen, &drawer);
        let line = places(
            &view,
            &at,
            preview(view.screen, &drawer),
            drawer_at,
            &drawer,
        );
        assert!(line.starts_with("screen 1280x800, part0 date "), "{line}");
        for name in ["clock", "count", "person", "player"] {
            assert!(line.contains(&format!(" {name} ")), "{name}: {line}");
        }
        assert!(line.contains(", drawer "), "{line}");
        assert!(line.contains(", tile0 message "), "{line}");
        assert!(line.contains(", tile1 battery "), "{line}");
        assert!(
            line.contains(", undo ") && line.contains(", done "),
            "{line}"
        );
    }

    #[test]
    fn a_part_the_layout_left_out_is_not_named() {
        let classic = lock_screen::classic(false);
        let mut at = at_classic(&classic);
        at.placed[index_of(&classic, "count")] = None;
        let view = view_of(classic.clone());
        let drawer = editor::layout(&view.drawer);
        let line = places(
            &view,
            &at,
            preview(view.screen, &drawer),
            drawer_rect(view.screen, &drawer),
            &drawer,
        );
        assert!(!line.contains(" count "), "{line}");
    }

    #[test]
    fn the_lock_stands_smaller_above_the_drawer_and_points_map_back() {
        let view = view_of(lock_screen::classic(false));
        let drawer = editor::layout(&view.drawer);
        let place = drawer_rect(view.screen, &drawer);
        let shown = preview(view.screen, &drawer);
        assert!(shown.k > 0.3 && shown.k < 1.0, "{shown:?}");
        assert!(shown.y + 800.0 * shown.k <= place.y);
        let r = shown.to_screen(Rect::new(100.0, 200.0, 50.0, 40.0));
        let back = shown.to_lock((r.x, r.y));
        assert!((back.0 - 100.0).abs() < 0.01 && (back.1 - 200.0).abs() < 0.01);
    }

    #[test]
    fn the_drawer_sits_centred_with_its_gap_above_the_bottom() {
        let view = view_of(lock_screen::classic(false));
        let layout = editor::layout(&view.drawer);
        let rect = drawer_rect(view.screen, &layout);
        assert_eq!(rect.y + rect.h, 800.0 - DRAWER_GAP);
        assert!(((rect.x + rect.w / 2.0) - 640.0).abs() <= 1.0);
    }

    /// Writes the lock screen's edit mode with the clock being dragged to the
    /// top right, its guides and the drawer, into `EDEL_LOCK_PNG` as
    /// `lock-editor.png`.
    #[test]
    fn the_edit_mode_draws_the_lock_screen_its_guides_and_the_drawer() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let i = index_of(&classic, "clock");
        let r = at.placed[i].expect("the clock is drawn");
        let mut view = view_of(classic.clone());
        view.drag = Some(drag(
            From::Part(i),
            centre(r),
            (
                1280.0 - lock::MARGIN - r.w / 2.0 - 40.0,
                lock::MARGIN + r.h / 2.0 + 30.0,
            ),
            false,
        ));
        dragged_rect(&mut view, &at);
        assert!(!view.guides.is_empty(), "the clock snaps near the margin");
        let lock_view = lock::View::new("14:06".into(), "Saturday 3 October".into(), "Ali".into());
        let mut pixmap = Pixmap::new(1280, 800).unwrap();
        lock::paint(
            &mut pixmap,
            &lock_view,
            &at,
            &tokens,
            &mut text,
            None,
            &mut lock::Pictures::default(),
            &mut lock::Backdrops::default(),
            &edel::wallpapers::Wallpaper::Background,
            1.0,
        );
        paint(&mut pixmap, &view, &at, &tokens, &mut text, 1.0);
        let drawer = editor::layout(&view.drawer);
        let place = drawer_rect(view.screen, &drawer);
        // The lock smaller above the drawer, and the drawer over it.
        let shown = preview(view.screen, &drawer);
        let lock = pixmap;
        let mut pixmap = Pixmap::new(1280, 800).unwrap();
        compose(&mut pixmap, &lock, shown, &tokens, 1.0);
        paint_drawer(&mut pixmap, &view, &tokens, &mut text, 1.0);
        // Nothing of the lock lies under the drawer.
        assert!(shown.y + 800.0 * shown.k <= place.y - PREVIEW_GAP + 0.5);
        let (dx, dy) = (
            (place.x + place.w / 2.0) as u32,
            (place.y + place.h / 2.0) as u32,
        );
        let inside = pixmap.pixel(dx, dy).expect("inside the pixmap");
        assert_ne!(inside.alpha(), 0, "the drawer's card is drawn");
        if let Some(dir) = std::env::var_os("EDEL_LOCK_PNG") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            pixmap.save_png(dir.join("lock-editor.png")).unwrap();
        }
    }

    #[test]
    fn tab_from_nowhere_focuses_the_first_part_and_shift_tab_the_last_part_of_the_drawer() {
        let view = view_of(lock_screen::classic(false));
        assert_eq!(
            key(&view, Key::Tab, false),
            Step::Focus(Some(Focus::Part(0)))
        );
        assert_eq!(
            key(&view, Key::Tab, true),
            Step::Focus(Some(Focus::Drawer(editor::Part::Done)))
        );
    }

    #[test]
    fn the_ring_wraps_from_done_to_the_first_part_and_back() {
        let classic = lock_screen::classic(false);
        let mut view = view_of(classic.clone());
        assert_eq!(
            ring(&view).len(),
            classic.len() + view.drawer.tiles.len() + 2
        );
        set_focus(&mut view, Some(Focus::Drawer(editor::Part::Done)));
        assert_eq!(
            key(&view, Key::Tab, false),
            Step::Focus(Some(Focus::Part(0)))
        );
        set_focus(&mut view, Some(Focus::Part(0)));
        assert_eq!(
            key(&view, Key::Tab, true),
            Step::Focus(Some(Focus::Drawer(editor::Part::Done)))
        );
    }

    #[test]
    fn arrows_nudge_a_focused_part_one_step_and_ten_with_shift() {
        let classic = lock_screen::classic(false);
        let mut view = view_of(classic.clone());
        let i = index_of(&classic, "clock");
        set_focus(&mut view, Some(Focus::Part(i)));
        assert_eq!(key(&view, Key::Right, false), Step::Nudge(i, 1, 0));
        assert_eq!(key(&view, Key::Left, false), Step::Nudge(i, -1, 0));
        assert_eq!(key(&view, Key::Up, true), Step::Nudge(i, 0, -10));
        assert_eq!(key(&view, Key::Down, false), Step::Nudge(i, 0, 1));
    }

    #[test]
    fn delete_takes_away_the_clock_but_never_the_person() {
        let classic = lock_screen::classic(false);
        let mut view = view_of(classic.clone());
        let clock = index_of(&classic, "clock");
        let person = index_of(&classic, "person");
        set_focus(&mut view, Some(Focus::Part(clock)));
        assert_eq!(key(&view, Key::Delete, false), Step::Remove(clock));
        set_focus(&mut view, Some(Focus::Part(person)));
        assert_eq!(key(&view, Key::Delete, false), Step::Nothing);
    }

    #[test]
    fn return_adds_a_drawer_tile_acts_on_undo_and_done_and_done_with_no_focus() {
        let mut view = view_of(lock_screen::classic(false));
        set_focus(&mut view, Some(Focus::Drawer(editor::Part::Tile(0))));
        assert_eq!(key(&view, Key::Activate, false), Step::Add(Part::Message));
        set_focus(&mut view, Some(Focus::Drawer(editor::Part::Done)));
        assert_eq!(key(&view, Key::Activate, false), Step::Done);
        set_focus(&mut view, Some(Focus::Drawer(editor::Part::Undo)));
        assert_eq!(key(&view, Key::Activate, false), Step::Undo);
        set_focus(&mut view, None);
        assert_eq!(key(&view, Key::Activate, false), Step::Done);
        assert_eq!(key(&view, Key::Escape, false), Step::Close);
    }

    #[test]
    fn every_anchor_has_its_words_for_a_screen_reader() {
        let words: Vec<&str> = [
            Anchor::TopLeft,
            Anchor::Top,
            Anchor::TopRight,
            Anchor::Left,
            Anchor::Centre,
            Anchor::Right,
            Anchor::BottomLeft,
            Anchor::Bottom,
            Anchor::BottomRight,
        ]
        .into_iter()
        .map(where_words)
        .collect();
        assert_eq!(
            words,
            [
                "top left",
                "top centre",
                "top right",
                "middle left",
                "middle",
                "middle right",
                "bottom left",
                "bottom centre",
                "bottom right"
            ]
        );
    }

    #[test]
    fn the_log_names_the_part_or_the_drawer_item_the_focus_is_on() {
        let classic = lock_screen::classic(false);
        let view = view_of(classic.clone());
        let clock = index_of(&classic, "clock");
        assert_eq!(focus_name(&view, Focus::Part(clock)), "clock");
        assert_eq!(
            focus_name(&view, Focus::Drawer(editor::Part::Tile(0))),
            "tile message"
        );
        assert_eq!(focus_name(&view, Focus::Drawer(editor::Part::Undo)), "undo");
        assert_eq!(focus_name(&view, Focus::Drawer(editor::Part::Done)), "done");
    }

    #[test]
    fn the_reader_names_each_part_then_the_drawer_and_follows_the_focus() {
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let mut view = view_of(classic.clone());
        let drawer = editor::layout(&view.drawer);
        let place = drawer_rect(view.screen, &drawer);
        let shown = preview(view.screen, &drawer);
        let items = nodes(&view, &at, shown, place, &drawer);
        let placed = at.placed.iter().filter(|r| r.is_some()).count();
        assert_eq!(items.len(), placed + view.drawer.tiles.len() + 2);
        assert_eq!(
            items[0].label,
            format!(
                "{}, {}",
                part_title(classic[0].part),
                where_words(classic[0].at)
            )
        );
        assert_eq!(items[items.len() - 1].label, "Done");
        assert_eq!(focus_node(&view, &at), None);
        set_focus(&mut view, Some(Focus::Drawer(editor::Part::Done)));
        assert_eq!(focus_node(&view, &at), Some(items.len() - 1));
        let clock = index_of(&classic, "clock");
        set_focus(&mut view, Some(Focus::Part(clock)));
        let among_placed = at.placed[..clock].iter().filter(|r| r.is_some()).count();
        assert_eq!(focus_node(&view, &at), Some(among_placed));
    }

    #[test]
    fn a_focused_part_has_the_accent_ring_just_outside_it() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let i = index_of(&classic, "clock");
        let r = at.placed[i].expect("the clock is drawn");
        let mut view = view_of(classic.clone());
        let accent =
            [tokens.accent.r, tokens.accent.g, tokens.accent.b].map(|c| (c * 255.0).round() as i32);
        let mut ringed = |view: &View| -> bool {
            let mut pixmap = Pixmap::new(1280, 800).unwrap();
            paint(&mut pixmap, view, &at, &tokens, &mut text, 1.0);
            // The ring is 3 px outside the part's left edge, 2 px wide.
            let y = (r.y + r.h / 2.0) as u32;
            (0..6).any(|k| {
                let x = (r.x - 6.0 + k as f32) as u32;
                pixmap.pixel(x, y).is_some_and(|p| {
                    let c = p.demultiply();
                    p.alpha() > 200
                        && [c.red(), c.green(), c.blue()]
                            .iter()
                            .zip(accent)
                            .all(|(got, want)| (i32::from(*got) - want).abs() <= 12)
                })
            })
        };
        assert!(!ringed(&view), "with no focus there is no ring");
        set_focus(&mut view, Some(Focus::Part(i)));
        assert!(ringed(&view), "the focused part is ringed in the accent");
    }
}
