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
use edel::lock_screen::{self, Anchor, Background, Guide, Part, Placed, SNAP};
use edel::settings::LockScreen;
use edel::tokens::Tokens;
use edel::wallpapers::Wallpaper;
use tiny_skia::{Pixmap, PixmapPaint, Transform};

use crate::a11y::Item;
use crate::edit_kit;
use crate::editor::{self, Act, DRAG_START, Tile};
use crate::lock;
use crate::paint::Text;
use crate::part_card;
use crate::player;
use crate::popup::Rect;
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

/// What the edit mode shows (M5.10c6b): the parts as they stand, the one
/// being dragged and where, the guides it snaps to, the part under the
/// pointer, and the drawer.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The lock screen's parts, in the list's order: what Done writes.
    pub parts: Vec<Placed>,
    /// The look the parts came from, while no part has been moved since
    /// (M5.10c6c3); None once one has, as the parts are then the person's.
    pub look: Option<String>,
    /// What lies behind the parts (M5.10c6c3): what Done writes, and the
    /// backgrounds' tiles show which one is in use.
    pub background: Background,
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
    /// The part card open beside a part, if any (M5.10c6c2): the keys go to
    /// it first, and a press away from it closes it.
    pub card: Option<part_card::View>,
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
    /// Shows a drawer tab (M5.10c6c3), its focus with it.
    Tab(usize),
    /// Chooses a look or a background of the drawer's tabs (M5.10c6c3).
    Look(usize),
    Background(usize),
    /// Opens the part's card, for a part that has one (M5.10c6c2).
    Open(usize),
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

/// The icon a part shows on its tile in the drawer and on the screen's
/// outline (M5.10c6c3): the clock's dial, the calendar, the bell, the music,
/// the person, the message, the picture and the battery, from `design/icons/`.
pub fn part_icon(part: Part) -> &'static str {
    match part {
        Part::Clock => "clock",
        Part::Date => "calendar",
        Part::Count => "bell",
        Part::Player => "music",
        Part::Person => "person",
        Part::Message => "message",
        Part::Picture => "picture",
        Part::Battery => "battery",
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

/// A look's or a background's thumbnail, logical pixels (M5.10c6c3): a
/// 16 to 10 screen, drawn once and kept, then scaled into each tile.
pub const THUMB: (u32, u32) = (960, 600);

/// The backgrounds the Background tab offers, in order: each one's name, its
/// title as people read it, and what it is (`None` for the picture, which
/// opens Settings' lock screen page and changes nothing here).
pub fn background_choices() -> Vec<(&'static str, String, Option<Background>)> {
    vec![
        ("blurred", tr("Blurred").into(), Some(Background::Blurred)),
        ("sharp", tr("Sharp").into(), Some(Background::Sharp)),
        ("backdrop", tr("Colour").into(), Some(Background::Backdrop)),
        ("picture", tr("Picture").into(), None),
    ]
}

/// The title of a built-in look as people read it, or a person's own look's
/// name as it is.
fn look_title(name: &str) -> String {
    match name {
        "classic" => tr("Classic").into(),
        "magazine" => tr("Magazine").into(),
        "minimal" => tr("Minimal").into(),
        own => own.into(),
    }
}

/// The Looks tab's tiles (M5.10c6c3): one for each look, the built-in ones
/// and then the person's own, each with its name, and the one `chosen` is
/// the look in use. Their pictures come from `thumbnail`.
pub fn look_tiles(looks: &[(String, Vec<Placed>)], chosen: Option<&str>) -> Vec<Tile> {
    looks
        .iter()
        .map(|(name, _)| Tile {
            name: name.clone(),
            icon: "",
            title: look_title(name),
            shown: String::new(),
            placed: false,
            chosen: chosen == Some(name.as_str()),
            picture: None,
        })
        .collect()
}

/// The Background tab's tiles (M5.10c6c3), one for each choice of
/// `background_choices`; the one `current` is is the one in use.
pub fn background_tiles(current: &Background) -> Vec<Tile> {
    background_choices()
        .into_iter()
        .map(|(name, title, background)| Tile {
            name: name.into(),
            icon: if background.is_none() { "picture" } else { "" },
            title,
            shown: String::new(),
            placed: false,
            chosen: match &background {
                Some(background) => background == current,
                None => matches!(current, Background::Picture(_)),
            },
            picture: None,
        })
        .collect()
}

/// A lock screen drawn small (M5.10c6c3): `parts` over `background` at
/// `THUMB`, laid out and painted as the lock draws it, with the lock's
/// `view` (the time and date) and the battery when the machine has one. The
/// caller keeps the result, so each look is drawn once.
#[allow(clippy::too_many_arguments)]
pub fn thumbnail(
    view: &lock::View,
    parts: &[Placed],
    background: &Background,
    battery: bool,
    tokens: &Tokens,
    text: &mut Text,
    icons: Option<&mut edel::app_icons::Icons>,
    pictures: &mut lock::Pictures,
    backdrops: &mut lock::Backdrops,
    wallpaper: &Wallpaper,
) -> Option<Pixmap> {
    let look = lock::Look {
        parts,
        player: Some(player::wide_size(1)),
        people: false,
        keep_out: Vec::new(),
        battery,
        background: background.clone(),
    };
    let at = lock::layout(THUMB.0 as f32, THUMB.1 as f32, tokens, true, &look, text);
    let mut pixmap = Pixmap::new(THUMB.0, THUMB.1)?;
    lock::paint(
        &mut pixmap,
        view,
        &at,
        tokens,
        text,
        icons,
        pictures,
        backdrops,
        wallpaper,
        1.0,
    );
    Some(pixmap)
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

/// The drawer's tiles of the Parts tab (M5.10c6b): one for each part `lock_screen::missing`
/// offers, in its order, with no picture (a picture needs a file, which the
/// edit mode does not choose yet, M5.10c6c), and none of them placed.
pub fn drawer_tiles(parts: &[Placed], battery: bool) -> Vec<Tile> {
    lock_screen::missing(parts, battery)
        .into_iter()
        .filter(|part| *part != Part::Picture)
        .map(|part| Tile {
            name: part.name().into(),
            icon: part_icon(part),
            title: part_title(part),
            shown: String::new(),
            placed: false,
            chosen: false,
            picture: None,
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

/// The lock screen's picture as it stands on the screen in the edit mode
/// (logical pixels): the whole screen at the preview's scale, at its place.
pub fn picture_rect(screen: (f32, f32), preview: Preview) -> Rect {
    Rect::new(
        preview.x,
        preview.y,
        screen.0 * preview.k,
        screen.1 * preview.k,
    )
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
        Key::Activate => match view.parts.get(i) {
            Some(placed) if part_card::card_for(placed.part) => Step::Open(i),
            _ => Step::Nothing,
        },
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
            editor::Part::Tile(i) => match view.drawer.tab {
                1 => Step::Look(i),
                2 => Step::Background(i),
                _ => view
                    .drawer
                    .tiles
                    .get(i)
                    .and_then(|tile| part_named(&tile.name))
                    .map_or(Step::Nothing, Step::Add),
            },
            editor::Part::Tab(i) => Step::Tab(i),
            editor::Part::Undo => Step::Undo,
            editor::Part::Done => Step::Done,
        };
    }
    match editor::key(&view.drawer, Some(part), key) {
        editor::Step::Focus(p) => Step::Focus(Some(Focus::Drawer(p))),
        editor::Step::Tab(i) => Step::Tab(i),
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
        Focus::Drawer(editor::Part::Tab(j)) => {
            format!("tab {}", view.drawer.tabs.get(j).map_or("tab", |t| t))
        }
        Focus::Drawer(editor::Part::Tile(j)) => format!(
            "tile {}",
            view.drawer.tiles.get(j).map_or("tile", |t| t.name.as_str())
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
    let tabs = view.drawer.tabs.len();
    match view.focus? {
        Focus::Part(i) => {
            if at.placed.get(i)?.is_none() {
                return None;
            }
            Some(at.placed[..i].iter().filter(|r| r.is_some()).count())
        }
        // The drawer's nodes are its tabs, then its tiles, then Undo and Done.
        Focus::Drawer(editor::Part::Tab(j)) => Some(drawn + j),
        Focus::Drawer(editor::Part::Tile(j)) => Some(drawn + tabs + j),
        Focus::Drawer(editor::Part::Undo) => Some(drawn + tabs + tiles),
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
    for (i, (word, rect)) in view.drawer.tabs.iter().zip(&drawer.tabs).enumerate() {
        out.push_str(&format!(
            ", tab{i} {} {}",
            word.to_lowercase(),
            at_rect(moved(*rect))
        ));
    }
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

/// Draws the edit mode's overlay at `s` (M5.10c6c3) over the composed
/// picture, in the screen's logical pixels: each placed part's outline
/// (`edit_kit::outline_part`, at rest, under the pointer, focused, or lifted)
/// with its name's chip when it is not at rest. A dragged part is cut from
/// the picture at its old place, which is dimmed (`edit_kit::vacate`), and
/// drawn at the place it would take, lifted with the accent outline and its
/// chip; the player is drawn again as its card. A dragged drawer part, and a
/// part dragged over the drawer, show the outline alone. The guides are
/// dashed accent lines across the picture only. The drawer is `paint_drawer`'s.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    at: &lock::Layout,
    preview: Preview,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let style = edit_kit::Style::GLASS;
    let dragged = view.drag.filter(|d| d.moving);
    let over = dragged.is_some_and(|d| d.over_drawer);
    let moved = match dragged.map(|d| d.from) {
        Some(From::Part(i)) => Some(i),
        _ => None,
    };
    // The dragged part's new place on the screen, and the picture cut from
    // its old place, which moves with the pointer.
    let target_now = target(view, at).map(|(r, _)| preview.to_screen(r));
    let mut piece = None;
    if let Some(old) = moved.and_then(|i| at.placed.get(i).copied().flatten()) {
        let old = preview.to_screen(old);
        let (x, y, w, h) = old.device(s);
        if let Some(mut cut) = Pixmap::new(w.round().max(1.0) as u32, h.round().max(1.0) as u32) {
            cut.draw_pixmap(
                -x.round() as i32,
                -y.round() as i32,
                pixmap.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            piece = Some(cut);
        }
        edit_kit::vacate(pixmap, old, tokens, s);
    }
    // The parts at rest, under the pointer or the keyboard. The dragged one
    // keeps its resting outline at its old place.
    for (i, (part, rect)) in view.parts.iter().zip(&at.placed).enumerate() {
        let Some(rect) = rect else {
            continue;
        };
        let screen = preview.to_screen(*rect);
        let state = if moved == Some(i) {
            edit_kit::State::Rest
        } else if view.focus == Some(Focus::Part(i)) {
            edit_kit::State::Focus
        } else if view.hover == Some(i) {
            edit_kit::State::Hover
        } else {
            edit_kit::State::Rest
        };
        edit_kit::outline_part(pixmap, screen, state, tokens, style, s);
        if state != edit_kit::State::Rest {
            edit_kit::chip(
                pixmap,
                &part_title(part.part),
                edit_kit::part_area(screen),
                tokens,
                Some(&mut *text),
                style,
                s,
            );
        }
    }
    if let (Some(drag), Some(new)) = (dragged, target_now) {
        if !over {
            match drag.from {
                From::Part(_) => {
                    if let Some(piece) = &piece {
                        let (x, y, _, _) = new.device(s);
                        pixmap.draw_pixmap(
                            x.round() as i32,
                            y.round() as i32,
                            piece.as_ref(),
                            &PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                    }
                }
                From::Drawer(_) => {}
            }
        }
        edit_kit::outline_part(pixmap, new, edit_kit::State::Lifted, tokens, style, s);
        let name = match drag.from {
            From::Part(i) => view
                .parts
                .get(i)
                .map_or(String::new(), |p| part_title(p.part)),
            From::Drawer(part) => part_title(part),
        };
        edit_kit::chip(
            pixmap,
            &name,
            edit_kit::part_area(new),
            tokens,
            Some(&mut *text),
            style,
            s,
        );
    }
    let picture = picture_rect(view.screen, preview);
    let guides: Vec<Guide> = view
        .guides
        .iter()
        .map(|g| Guide {
            vertical: g.vertical,
            at: if g.vertical {
                preview.x + g.at * preview.k
            } else {
                preview.y + g.at * preview.k
            },
        })
        .collect();
    edit_kit::guides(pixmap, &guides, picture, tokens, s);
}

/// The edit mode's own placeholders, drawn into the lock picture at its full
/// size (M5.10c6c3), so they scale with it: the player's card as it shows
/// (the glass pill and the player's drawing with the sample words "Now
/// playing" and an empty cover). Called before the picture is laid smaller.
pub fn draw_placeholders(
    pixmap: &mut Pixmap,
    view: &View,
    at: &lock::Layout,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let compact = view.screen.0 < lock::COMPACT_BELOW;
    for (part, rect) in view.parts.iter().zip(&at.placed) {
        if let (Part::Player, Some(rect)) = (part.part, rect) {
            draw_player(pixmap, *rect, compact, tokens, text, s);
        }
    }
}

/// The player's card at `rect` (logical pixels), `draw_placeholders`' one part.
fn draw_player(
    pixmap: &mut Pixmap,
    rect: Rect,
    compact: bool,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let radius = rect.h / 2.0;
    lock::lift(pixmap, rect, radius, s, tokens);
    lock::glass(pixmap, rect.device(s), radius * s, s, tokens);
    let parts = player::place_wide(rect, compact, 1, 0);
    let sample = player::PlayerView {
        title: tr("Now playing").into(),
        artist: String::new(),
        identity: tr("Music").into(),
        playing: false,
        can_previous: true,
        can_next: true,
        cover: None,
        app_icon: None,
        position: None,
        length: None,
    };
    let marks = player::Marks {
        hover: None,
        focus: None,
        page: 0,
    };
    player::paint(pixmap, &sample, &parts, marks, tokens, Some(text), s);
}

/// Draws the drawer through `editor::paint` at its place on the edit mode's
/// surface, `pixmap` at `s`, over the lock screen's picture.
pub fn paint_drawer(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, text: &mut Text, s: f32) {
    let layout = editor::layout(&view.drawer);
    let place = drawer_rect(view.screen, &layout);
    // The sheet's shadow lies outside the sheet, so it is cast here on the
    // whole screen: the layer below has no room round the sheet.
    lock::lift(pixmap, place, edit_kit::SHEET_RADIUS, s, tokens);
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

/// Chooses the look at `i` of `looks` (M5.10c6c3): the edit's parts become
/// its parts and its background its background, and the look is in use until
/// a part moves. Returns its name, or None for no such look.
pub fn choose_look(view: &mut View, looks: &[(String, Vec<Placed>)], i: usize) -> Option<String> {
    let (name, parts) = looks.get(i)?.clone();
    view.background = lock_screen::look_background(&name);
    view.parts = parts;
    view.look = Some(name.clone());
    Some(name)
}

/// A part was moved, added or taken away: the parts are the person's own, so
/// no look is in use any more (M5.10c6c3).
pub fn parts_moved(view: &mut View) {
    view.look = None;
}

/// The three lines Done writes (M5.10c6c3): each one's value, None taking
/// the person's line out, as a default is never written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lines {
    pub look: Option<String>,
    pub parts: Option<String>,
    pub background: Option<String>,
}

/// What Done writes for the edit `view` (M5.10c6c3), with `lock` the person's
/// lock settings over the machine's. The look is written while it is in use
/// and is not the release's. The parts are written unless a look is in use
/// (it brings its own) or what applies without a parts line, under that look,
/// is what the edit shows. The background is written only when it differs
/// from what the look gives (the release's for none). Fails with the parts'
/// own message when they do not write as a list.
pub fn written(view: &View, lock: &LockScreen, compact: bool) -> Result<Lines, String> {
    let look = view
        .look
        .clone()
        .filter(|name| name != lock_screen::CLASSIC);
    let base = LockScreen {
        parts: None,
        look: look.clone(),
        ..lock.clone()
    };
    let without_parts = lock_screen::applied(&base, compact).parts;
    let parts = if view.look.is_some() || view.parts == without_parts {
        None
    } else {
        Some(lock_screen::parts_value(&view.parts).map_err(|e| format!("{e:#}"))?)
    };
    let gives = lock_screen::look_background(look.as_deref().unwrap_or(lock_screen::CLASSIC));
    let background =
        (view.background != gives).then(|| lock_screen::background_value(&view.background));
    Ok(Lines {
        look,
        parts,
        background,
    })
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
            tabs: Vec::new(),
            tab: 0,
            tallest: 0,
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
            look: None,
            background: Background::Backdrop,
            screen: (1280.0, 800.0),
            card: None,
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
        let tiles = drawer_tiles(&classic, true);
        let names: Vec<&str> = tiles.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["message", "battery"]);
        assert!(!names.contains(&"person"));
        let tiles = drawer_tiles(&classic, false);
        let no_battery: Vec<&str> = tiles.iter().map(|t| t.name.as_str()).collect();
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
        let mut view = view_of(classic.clone());
        view.drawer.tabs = vec!["Parts", "Looks", "Background"];
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
        assert!(line.contains(", tab1 looks "), "{line}");
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
        let mut lock_view =
            lock::View::new("14:06".into(), "Saturday 3 October".into(), "Ali".into());
        lock_view.notices = 2;
        lock_view.apps = vec![
            ("Mail".into(), "mail".into()),
            ("Calendar".into(), "calendar".into()),
        ];
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
        let drawer = editor::layout(&view.drawer);
        let place = drawer_rect(view.screen, &drawer);
        // The lock smaller above the drawer, and the drawer over it.
        let shown = preview(view.screen, &drawer);
        let lock = pixmap;
        let mut pixmap = Pixmap::new(1280, 800).unwrap();
        let picture = picture_rect(view.screen, shown);
        edit_kit::stage(
            &mut pixmap,
            Rect::new(0.0, 0.0, 1280.0, 800.0),
            picture,
            &tokens,
            edit_kit::Style::GLASS,
            1.0,
        );
        let mut lock = lock;
        draw_placeholders(&mut lock, &view, &at, &tokens, &mut text, 1.0);
        edit_kit::stage_frame(
            &mut pixmap,
            &lock,
            picture,
            &tokens,
            edit_kit::Style::GLASS,
            1.0,
        );
        paint(&mut pixmap, &view, &at, shown, &tokens, &mut text, 1.0);
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
            pixmap.save_png(dir.join("lock-editor-drag.png")).unwrap();
        }
    }

    /// The edit mode as the surface draws it for `view`: the lock with its
    /// placeholders, laid smaller on the stage, the marks over it and the drawer.
    fn render(view: &View, at: &lock::Layout, tokens: &Tokens, text: &mut Text) -> Pixmap {
        let mut lock_view =
            lock::View::new("14:06".into(), "Saturday 3 October".into(), "Ali".into());
        lock_view.notices = 2;
        lock_view.apps = vec![
            ("Mail".into(), "mail".into()),
            ("Calendar".into(), "calendar".into()),
        ];
        let mut lock = Pixmap::new(view.screen.0 as u32, view.screen.1 as u32).unwrap();
        lock::paint(
            &mut lock,
            &lock_view,
            at,
            tokens,
            text,
            None,
            &mut lock::Pictures::default(),
            &mut lock::Backdrops::default(),
            &edel::wallpapers::Wallpaper::Background,
            1.0,
        );
        draw_placeholders(&mut lock, view, at, tokens, text, 1.0);
        let drawer = editor::layout(&view.drawer);
        let shown = preview(view.screen, &drawer);
        let picture = picture_rect(view.screen, shown);
        let mut pixmap = Pixmap::new(view.screen.0 as u32, view.screen.1 as u32).unwrap();
        edit_kit::stage(
            &mut pixmap,
            Rect::new(0.0, 0.0, view.screen.0, view.screen.1),
            picture,
            tokens,
            edit_kit::Style::GLASS,
            1.0,
        );
        edit_kit::stage_frame(
            &mut pixmap,
            &lock,
            picture,
            tokens,
            edit_kit::Style::GLASS,
            1.0,
        );
        paint(&mut pixmap, view, at, shown, tokens, text, 1.0);
        paint_drawer(&mut pixmap, view, tokens, text, 1.0);
        pixmap
    }

    /// Writes the edit mode at rest (`lock-editor.png`) and with the date
    /// focused (`lock-editor-focus.png`) into `EDEL_LOCK_PNG`, and checks that
    /// the focus ring and the chip show.
    #[test]
    fn the_edit_mode_at_rest_and_with_the_date_focused() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let mut view = view_of(classic.clone());
        let rest = render(&view, &at, &tokens, &mut text);
        let date = index_of(&classic, "date");
        set_focus(&mut view, Some(Focus::Part(date)));
        let focused = render(&view, &at, &tokens, &mut text);
        assert_ne!(rest.data(), focused.data(), "the focus shows");
        if let Some(dir) = std::env::var_os("EDEL_LOCK_PNG") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            rest.save_png(dir.join("lock-editor.png")).unwrap();
            focused.save_png(dir.join("lock-editor-focus.png")).unwrap();
        }
    }

    /// The edit mode on a phone (360 by 740 logical pixels, a Compact screen):
    /// the drawer is the screen's width with touch-sized buttons, the lock
    /// laid smaller above it and every part in the screen. Writes
    /// `lock-editor-phone.png` into `EDEL_LOCK_PNG`.
    #[test]
    fn the_edit_mode_fits_a_phone_screen() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let classic = lock_screen::classic(false);
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
        let look = lock::Look {
            parts: &classic,
            player: Some(player::wide_size(1)),
            people: false,
            keep_out: lock::keep_out(&corner_at),
            battery: false,
            background: Background::Backdrop,
        };
        let at = lock::layout(360.0, 740.0, &tokens, true, &look, &mut text);
        let mut view = view_of(classic.clone());
        view.screen = (360.0, 740.0);
        view.drawer.compact = true;
        view.drawer.width = 360;
        let drawer = editor::layout(&view.drawer);
        let place = drawer_rect(view.screen, &drawer);
        assert!(place.x >= 0.0 && place.x + place.w <= 360.0, "{place:?}");
        assert!(place.y >= 0.0 && place.y + place.h <= 740.0, "{place:?}");
        let shown = preview(view.screen, &drawer);
        assert!(shown.k > 0.2 && shown.k <= 1.0, "{shown:?}");
        assert!(
            shown.y + 740.0 * shown.k <= place.y,
            "the lock is over the drawer"
        );
        let pixmap = render(&view, &at, &tokens, &mut text);
        if let Some(dir) = std::env::var_os("EDEL_LOCK_PNG") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            pixmap.save_png(dir.join("lock-editor-phone.png")).unwrap();
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
        // The screen is the lock's own, so the part's place is its place.
        let same = Preview {
            k: 1.0,
            x: 0.0,
            y: 0.0,
        };
        let mut ringed = |view: &View| -> bool {
            let mut pixmap = Pixmap::new(1280, 800).unwrap();
            paint(&mut pixmap, view, &at, same, &tokens, &mut text, 1.0);
            // The ring lies 3 px outside the outline, which is 8 px outside
            // the part: 11 px out from the part's left edge, 2 px wide.
            let y = (r.y + r.h / 2.0) as u32;
            (0..8).any(|k| {
                let x = (r.x - 12.0 + k as f32) as u32;
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

    /// The Looks tab's list: the built-in looks, classic first.
    fn looks_of() -> Vec<(String, Vec<Placed>)> {
        lock_screen::LOOKS
            .iter()
            .filter_map(|name| {
                lock_screen::look_parts(name, false).map(|parts| (name.to_string(), parts))
            })
            .collect()
    }

    #[test]
    fn choosing_a_look_sets_its_parts_and_background_and_a_moved_part_clears_it() {
        let mut view = view_of(lock_screen::classic(false));
        let looks = looks_of();
        assert_eq!(choose_look(&mut view, &looks, 1), Some("magazine".into()));
        assert_eq!(view.look.as_deref(), Some("magazine"));
        assert_eq!(view.parts, looks[1].1);
        assert_eq!(view.background, Background::Blurred);
        assert_eq!(choose_look(&mut view, &looks, 9), None, "no such look");
        parts_moved(&mut view);
        assert_eq!(view.look, None, "the parts are the person's own now");
    }

    #[test]
    fn the_look_tiles_name_each_look_and_the_one_in_use_is_chosen() {
        let tiles = look_tiles(&looks_of(), Some("magazine"));
        let names: Vec<&str> = tiles.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["classic", "magazine", "minimal"]);
        assert_eq!(
            tiles.iter().map(|t| t.chosen).collect::<Vec<_>>(),
            [false, true, false]
        );
        assert_eq!(tiles[1].title, "Magazine");
    }

    #[test]
    fn the_background_tab_offers_blurred_sharp_colour_and_the_picture() {
        let tiles = background_tiles(&Background::Sharp);
        let names: Vec<&str> = tiles.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["blurred", "sharp", "backdrop", "picture"]);
        assert_eq!(
            tiles.iter().map(|t| t.chosen).collect::<Vec<_>>(),
            [false, true, false, false]
        );
        assert_eq!(tiles[3].title, "Picture");
        let picture = background_tiles(&Background::Picture("/x/a.png".into()));
        assert!(
            picture[3].chosen,
            "a picture of the person's is the picture tile in use"
        );
    }

    /// The lines Done writes for `view` with the person's settings empty.
    fn written_for(view: &View) -> Lines {
        written(view, &LockScreen::default(), false).unwrap()
    }

    #[test]
    fn done_writes_nothing_for_the_releases_own_look() {
        let view = view_of(lock_screen::classic(false));
        assert_eq!(
            written_for(&view),
            Lines {
                look: None,
                parts: None,
                background: None
            }
        );
    }

    #[test]
    fn done_writes_magazine_as_the_look_and_takes_its_parts_out() {
        let mut view = view_of(lock_screen::look_parts("magazine", false).unwrap());
        view.look = Some("magazine".into());
        view.background = Background::Blurred;
        assert_eq!(
            written_for(&view),
            Lines {
                look: Some("magazine".into()),
                parts: None,
                background: None
            },
            "the look brings its parts, and its own background"
        );
    }

    #[test]
    fn done_after_a_moved_part_takes_the_look_out_and_writes_the_parts() {
        let mut view = view_of(lock_screen::look_parts("magazine", false).unwrap());
        view.look = Some("magazine".into());
        view.background = Background::Blurred;
        parts_moved(&mut view);
        let i = index_of(&view.parts, "date");
        view.parts[i] = lock_screen::nudged(&view.parts[i], 3, 0);
        let lines = written_for(&view);
        assert_eq!(lines.look, None);
        assert!(lines.parts.is_some(), "the moved list is written");
        assert_eq!(
            lines.background.as_deref(),
            Some("blurred"),
            "classic gives the navy scene"
        );
    }

    #[test]
    fn a_background_the_look_does_not_give_is_written_and_the_looks_own_is_not() {
        let mut view = view_of(lock_screen::classic(false));
        view.background = Background::Sharp;
        assert_eq!(written_for(&view).background.as_deref(), Some("sharp"));
        view.look = Some("magazine".into());
        view.background = Background::Blurred;
        assert_eq!(
            written_for(&view).background,
            None,
            "magazine gives the blurred wallpaper"
        );
    }

    /// The Looks and the Background tabs (M5.10c6c3) with their tiles' own
    /// pictures: each look drawn small as the lock draws it, the one in use
    /// ringed, and the backgrounds, each over the classic lock. The tiles
    /// with pictures differ from the same drawer without them. Writes
    /// `lock-editor-looks.png` and `lock-editor-background.png` into
    /// `EDEL_LOCK_PNG`.
    #[test]
    fn the_looks_and_the_backgrounds_tabs_draw_their_pictures() {
        use crate::editor::Picture;
        use std::rc::Rc;
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let wallpaper = lock::wallpaper_of(None, None, false);
        let mut pictures = lock::Pictures::default();
        let mut backdrops = lock::Backdrops::default();
        let mut lock_view =
            lock::View::new("14:06".into(), "Saturday 3 October".into(), "Ali".into());
        lock_view.notices = 2;
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let looks = looks_of();
        let mut view = view_of(classic.clone());
        view.look = Some("magazine".into());
        view.drawer.tabs = vec!["Parts", "Looks", "Background"];
        view.drawer.tab = 1;
        view.drawer.tallest = 3;
        view.drawer.tiles = look_tiles(&looks, Some("magazine"));
        for (tile, (_, parts)) in view.drawer.tiles.iter_mut().zip(&looks) {
            let background = lock_screen::look_background(&tile.name);
            let pixmap = thumbnail(
                &lock_view,
                parts,
                &background,
                false,
                &tokens,
                &mut text,
                None,
                &mut pictures,
                &mut backdrops,
                &wallpaper,
            )
            .expect("a look's thumbnail draws");
            tile.picture = Some(Picture(Rc::new(pixmap)));
        }
        let looks_png = render(&view, &at, &tokens, &mut text);
        if let (Some(dir), Some(Picture(thumb))) = (
            std::env::var_os("EDEL_LOCK_PNG"),
            view.drawer.tiles[1].picture.clone(),
        ) {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            thumb.save_png(dir.join("lock-look-magazine.png")).unwrap();
        }
        let mut bare = view.clone();
        for tile in &mut bare.drawer.tiles {
            tile.picture = None;
        }
        assert_ne!(
            render(&bare, &at, &tokens, &mut text).data(),
            looks_png.data(),
            "the looks' pictures show in their tiles"
        );

        view.drawer.tab = 2;
        view.drawer.tiles = background_tiles(&Background::Blurred);
        for tile in &mut view.drawer.tiles {
            let Some((_, _, Some(background))) = background_choices()
                .into_iter()
                .find(|(name, _, _)| *name == tile.name)
            else {
                continue;
            };
            let pixmap = thumbnail(
                &lock_view,
                &classic,
                &background,
                false,
                &tokens,
                &mut text,
                None,
                &mut pictures,
                &mut backdrops,
                &wallpaper,
            )
            .expect("a background's thumbnail draws");
            tile.picture = Some(Picture(Rc::new(pixmap)));
        }
        let background_png = render(&view, &at, &tokens, &mut text);
        assert_ne!(
            looks_png.data(),
            background_png.data(),
            "the tabs show their own tiles"
        );
        if let Some(dir) = std::env::var_os("EDEL_LOCK_PNG") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            looks_png
                .save_png(dir.join("lock-editor-looks.png"))
                .unwrap();
            background_png
                .save_png(dir.join("lock-editor-background.png"))
                .unwrap();
        }
    }
}
