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
use edel::lock_screen::{self, Anchor, Background, DatePart, DateStyle, Guide, Part, Placed, SNAP};
use edel::settings::{LockScreen, OwnLook};
use edel::tokens::{Colour, Tokens};
use edel::wallpapers::Wallpaper;
use edel::widgets::Surface;
use tiny_skia::{FillRule, PathBuilder, Pixmap, PixmapPaint, Transform};

use crate::a11y::Item;
use crate::clock_face;
use crate::edit_kit;
use crate::editor::{self, Act, DRAG_START, Tile};
use crate::lock;
use crate::paint::{self as paint_kit, Face, Text, fill, paint_of};
use crate::part_card;
use crate::player;
use crate::popup::{self, Rect};
use crate::trayview::Key;

/// The screen's margin round everything in the edit mode, logical pixels (the
/// lock's own, `lock::MARGIN`): the bar, the panels, the strip and the canvas.
pub const MARGIN: f32 = lock::MARGIN;
/// The bar's height on a desktop, logical pixels (M5.10c7f: the mockup's 32 px
/// pills). A Compact screen uses the touch height instead (`bar_height`).
pub const BAR_H: f32 = 32.0;
/// The bar's words: 13 px (regular for Cancel and Preview, semibold for the
/// title), the title 13.5 px, its subtitle 11 px at 60 percent white.
const BAR_WORDS: f32 = 13.0;
const TITLE_SIZE: f32 = 13.5;
const SUBTITLE_SHARE: f32 = 0.60;
/// Cancel's fill (white at 10 percent) and Preview's outline (white at 28
/// percent, 1 px).
const CANCEL_FILL: f32 = 0.10;
const PREVIEW_LINE: f32 = 0.28;
const BAR_LINE: f32 = 1.0;
/// The room between the bar and the canvas, logical pixels.
const BAR_GAP: f32 = 16.0;
/// The bar's buttons: Cancel and Done are `BAR_BUTTON_W` wide, Preview
/// `BAR_PREVIEW_W` (M5.10c7f), with `BAR_BUTTON_GAP` between Preview and Done.
/// The title lies between Cancel and Preview, `TITLE_GAP` from each.
const BAR_BUTTON_W: f32 = 88.0;
const BAR_PREVIEW_W: f32 = 112.0;
const BAR_BUTTON_GAP: f32 = 8.0;
const TITLE_GAP: f32 = 16.0;
/// The side panels: their width, the room between each and the canvas, and
/// the screens at least `WIDE` logical pixels wide that get them.
pub const PANEL_W: f32 = 208.0;
pub const PANEL_GAP: f32 = 24.0;
pub const WIDE: f32 = 960.0;
/// The room between the canvas and the strip's tabs, logical pixels.
pub const STRIP_GAP: f32 = 16.0;
/// The wallpaper panel's height on the left, logical pixels: what its heading
/// (a line of 16), its picture, its title (a line of 18) and its caption (a
/// line of 14) take with the padding and the gaps between them (M5.10c7f).
/// The right panel is as high as the canvas, and the card in it takes what it
/// needs.
pub const WALLPAPER_PANEL_H: f32 = 2.0 * PANEL_PAD_Y
    + 16.0
    + PANEL_STACK_GAP
    + (PANEL_W - 2.0 * PANEL_PAD) * WALLPAPER_SHARE
    + PANEL_STACK_GAP
    + NAME_LINE
    + NOTE_LINE
    + DIM_GAP
    + DIM_WORD_LINE
    + DIM_SLIDER_GAP
    + edit_kit::KNOB;
/// The lines the wallpaper panel's name (18) and its note (14) take, and the
/// Dim block under them (M5.10c7g): the word "Dim" (12 px, white at 75
/// percent, a line of 14) after `DIM_GAP`, and the slider `DIM_SLIDER_GAP`
/// under the word.
const NAME_LINE: f32 = 18.0;
const NOTE_LINE: f32 = 14.0;
const DIM_GAP: f32 = 12.0;
const DIM_WORD: f32 = 12.0;
const DIM_WORD_LINE: f32 = 14.0;
const DIM_WORD_SHARE: f32 = 0.75;
const DIM_SLIDER_GAP: f32 = 8.0;
/// How far above and below the slider a press still counts as on it, logical
/// pixels: the knob is 16 px, a finger needs more.
const DIM_HIT: f32 = 12.0;
/// The step the Left and Right keys move the Dim by, in percent (M5.10c7g).
pub const DIM_STEP: i32 = 5;
/// The wallpaper panel's inner padding, logical pixels: 14 at the sides and
/// 16 at the top and bottom (M5.10c7f), and the gap between its heading, its
/// picture and its title.
const PANEL_PAD: f32 = 14.0;
const PANEL_PAD_Y: f32 = 16.0;
const PANEL_STACK_GAP: f32 = 8.0;
/// The panel's heading (10.5 px semibold, white at 50 percent), its name
/// (13.5 px semibold) and its note (11 px, white at 55 percent), and the
/// thumbnail's corners and hairline (10 px, white at 16 percent).
const HEADING_SIZE: f32 = 10.5;
const HEADING_SHARE: f32 = 0.50;
const NAME_SIZE: f32 = 13.5;
const NOTE_SHARE: f32 = 0.55;
const THUMB_RADIUS: f32 = 10.0;
const THUMB_EDGE: f32 = 0.16;
/// The wallpaper's picture is the screen's 16 to 10 (`THUMB`).
const WALLPAPER_SHARE: f32 = 10.0 / 16.0;
/// The preview's pill (M5.10c7f): 16 px from the top, at most `PREVIEW_W` wide,
/// `PREVIEW_H` high, with the white Back button `BACK_W` wide inside it.
const PREVIEW_TOP: f32 = 16.0;
const PREVIEW_W: f32 = 420.0;
const PREVIEW_H: f32 = 44.0;
const BACK_W: f32 = 140.0;
/// The preview's dot, logical pixels across.
const PREVIEW_DOT: f32 = 8.0;
/// The size of a strip tile's live preview, logical pixels, and the scale it
/// is drawn at (2, so it stays sharp on a 2x screen too; `editor::thumbnail`
/// scales it to the tile's place).
pub const PREVIEW_BOX: (f32, f32) = (98.0, 41.0);
pub const PREVIEW_SCALE: f32 = 2.0;
/// The date's words (white at 85 percent) in a strip tile's preview (M5.10c7f).
const PREVIEW_DATE_SHARE: f32 = 0.85;
/// The battery's charge a strip tile's preview draws its outline at (M5.10c7g).
const PREVIEW_CHARGE: u32 = 82;
/// The music player's preview (M5.10c7g), logical pixels: a 40 by 22 pill of
/// white at 12 percent, 3 px in from its left and right ends the cover (16
/// px, `PREVIEW_COVER`) and the play glyph (9 px, `PREVIEW_PLAY`).
const PREVIEW_PLAYER: (f32, f32) = (40.0, 22.0);
const PREVIEW_PLAYER_SHARE: f32 = 0.12;
const PREVIEW_PLAYER_PAD: f32 = 3.0;
const PREVIEW_COVER: f32 = 16.0;
const PREVIEW_PLAY: f32 = 9.0;
/// The message's icon, 14 px, at 85 percent, and the picture's glyph, 12 px.
const PREVIEW_MESSAGE_SHARE: f32 = 0.85;
const PREVIEW_PICTURE: f32 = 12.0;

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
    /// The look the edit is inside (M5.10c7a): the one that applies when the
    /// edit opens, or the one chosen in the Looks tab. Done saves into it, as
    /// a new look of the person's when it is a built-in one.
    pub editing: String,
    /// Whether anything changed since the edit opened or a look was chosen:
    /// a part moved, added, taken away or changed, or a background chosen.
    /// Done saves a look only then (M5.10c7a).
    pub changed: bool,
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
    /// The preview (M5.10c7f): the lock as it will be, over the whole screen,
    /// with the pill that says so; only Escape and Return go back.
    pub previewing: bool,
    /// How much the background is darkened, in percent (M5.10c7g): the Dim
    /// slider's value, which the lock picture uses and Done saves.
    pub dim: u32,
    /// The surface being edited (M8.21 part 21j2): the lock screen, or the
    /// desktop, whose strip is the clock and the date, with no person,
    /// wallpaper panel, Dim, tabs or looks.
    pub on: Surface,
}

/// The words the log says the edit is called by (M8.21 part 21j2): the lock's
/// lines keep "lock editor", the desktop's say "desktop editor".
pub fn log_name(on: Surface) -> &'static str {
    match on {
        Surface::Desktop => "desktop editor",
        _ => "lock editor",
    }
}

/// The keyboard's focus in the edit mode (M5.10c6c): a placed part by its
/// index in `parts`, or a part of the drawer (a tile, Undo or Done).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Part(usize),
    Drawer(editor::Part),
    /// One of the bar's buttons (M5.10c7e), when the drawer is bare.
    Bar(BarButton),
    /// The Dim slider of the wallpaper panel (M5.10c7g), when the panel shows.
    Dim,
}

/// The bar's buttons (M5.10c7e, M5.10c7f): Cancel closes without writing,
/// Preview shows the lock as it will be, Done writes the edit. Undo is Ctrl+Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarButton {
    Cancel,
    Preview,
    Done,
}

/// The bar's buttons from left to right: the order the arrows step them.
const BAR_ORDER: [BarButton; 3] = [BarButton::Cancel, BarButton::Preview, BarButton::Done];

/// The bar across the top of the edit mode (M5.10c7e), in the screen's logical
/// pixels: Cancel at the left, Undo right of it, Done at the right edge, and
/// the title between Undo and Done. It has no background of its own: it lies
/// on the edit mode's stage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub cancel: Rect,
    pub preview: Rect,
    pub done: Rect,
    pub title: Rect,
}

/// The bar's height on a screen of `screen` logical pixels: `BAR_H`, or the
/// touch height on a Compact screen.
pub fn bar_height(screen: (f32, f32)) -> f32 {
    if screen.0 < lock::COMPACT_BELOW {
        edit_kit::BUTTON_TOUCH
    } else {
        BAR_H
    }
}

/// The bar on a screen of `screen` logical pixels (M5.10c7e), at the margin.
pub fn bar(screen: (f32, f32)) -> Bar {
    let h = bar_height(screen);
    let cancel = Rect::new(MARGIN, MARGIN, BAR_BUTTON_W, h);
    let done = Rect::new(screen.0 - MARGIN - BAR_BUTTON_W, MARGIN, BAR_BUTTON_W, h);
    let preview = Rect::new(
        done.x - BAR_BUTTON_GAP - BAR_PREVIEW_W,
        MARGIN,
        BAR_PREVIEW_W,
        h,
    );
    let left = cancel.right() + TITLE_GAP;
    let title = Rect::new(left, MARGIN, (preview.x - TITLE_GAP - left).max(0.0), h);
    Bar {
        cancel,
        preview,
        done,
        title,
    }
}

/// Which of the bar's buttons a point (screen logical pixels) is on, if any.
pub fn bar_hit(bar: &Bar, x: f32, y: f32) -> Option<BarButton> {
    if bar.cancel.contains(x, y) {
        Some(BarButton::Cancel)
    } else if bar.preview.contains(x, y) {
        Some(BarButton::Preview)
    } else if bar.done.contains(x, y) {
        Some(BarButton::Done)
    } else {
        None
    }
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
    /// Chooses a background of the Wallpaper tab (M5.10c6c3).
    Background(usize),
    /// Opens the part's card, for a part that has one (M5.10c6c2).
    Open(usize),
    /// Shows the preview, or goes back from it (M5.10c7f).
    Preview,
    Back,
    /// Moves the Dim by this many percent (M5.10c7g), clamped by the caller.
    Dim(i32),
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
    tr(part.widget().title).into()
}

/// The part a drawer tile is named for (`Part::name`), if the release has it.
pub fn part_named(name: &str) -> Option<Part> {
    Part::named(name)
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
/// caller keeps the result, so each look is drawn once. `playing` is the
/// music player the lock shows over the look (M5.10c7c, the look pictures
/// of Settings); the editor's tiles draw none.
#[allow(clippy::too_many_arguments)]
pub fn thumbnail(
    view: &lock::View,
    parts: &[Placed],
    background: &Background,
    dim: u32,
    battery: bool,
    tokens: &Tokens,
    text: &mut Text,
    icons: Option<&mut edel::app_icons::Icons>,
    pictures: &mut lock::Pictures,
    backdrops: &mut lock::Backdrops,
    wallpaper: &Wallpaper,
    size: (u32, u32),
    playing: Option<&player::PlayerView>,
) -> Option<Pixmap> {
    // No parts: the background alone, as the wallpaper panel shows it (M5.10c7f).
    // The lock's own layout would fall back to the classic parts.
    if parts.is_empty() {
        let mut pixmap = Pixmap::new(size.0, size.1)?;
        match backdrops.get(background, dim, wallpaper, size, tokens) {
            Some(made) => pixmap.draw_pixmap(
                0,
                0,
                made.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            ),
            None => lock::backdrop(&mut pixmap, tokens),
        }
        return Some(pixmap);
    }
    let look = lock::Look {
        parts,
        player: Some(player::wide_size(1)),
        people: false,
        keep_out: Vec::new(),
        battery,
        background: background.clone(),
        dim,
    };
    let at = lock::layout(size.0 as f32, size.1 as f32, tokens, true, &look, text);
    let mut pixmap = Pixmap::new(size.0, size.1)?;
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
    if let (Some(sample), Some(rect)) = (playing, at.player) {
        draw_player(&mut pixmap, rect, false, tokens, text, 1.0, sample);
    }
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

/// The widgets the strip shows, in its order (M5.10c7f): the clock, the date,
/// the now playing bar, the battery, the message and the picture. The person
/// and the notifications' count are not offered.
pub const STRIP: [Part; 6] = [
    Part::Clock,
    Part::Date,
    Part::Player,
    Part::Battery,
    Part::Message,
    Part::Picture,
];

/// Whether `part` stands as many times as the look allows (M5.10c7f): `missing`
/// does not offer it. A battery the machine does not have counts as full, as
/// it can never be added there.
pub fn full(parts: &[Placed], battery: bool, part: Part, on: Surface) -> bool {
    !lock_screen::missing_on(parts, battery, on).contains(&part)
}

/// The strip's tiles (M5.10c6b, M5.10c7f, M5.10c7g): one for each `STRIP` part,
/// in its order, its name the part's. Every tile is at full strength, `placed`
/// is never set, so a kind the look already holds as many of is still drawn
/// whole: a press or Return on it chooses the look's part of that kind
/// (`full` says which). Their live previews come from `preview_picture`,
/// which the caller attaches.
pub fn drawer_tiles(on: Surface) -> Vec<Tile> {
    STRIP
        .into_iter()
        .filter(|part| part.widget().fits(on))
        .map(|part| Tile {
            name: part.name().into(),
            icon: "",
            title: part_title(part),
            shown: String::new(),
            placed: false,
            chosen: false,
            picture: None,
        })
        .collect()
}

/// The drawer's rectangle on a screen of `screen` logical pixels: centred
/// across it, `STRIP_GAP` under the canvas, so the strip stays with the
/// lock it adds to (as the canvas board draws it).
pub fn drawer_rect(screen: (f32, f32), drawer: &editor::Layout) -> Rect {
    let (w, h) = (drawer.size.0 as f32, drawer.size.1 as f32);
    let shown = preview(screen, drawer);
    let y = (shown.y + screen.1 * shown.k + STRIP_GAP).round();
    Rect::new(((screen.0 - w) / 2.0).round(), y, w, h)
}

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
/// pixels with `drawer` laid out (M5.10c7e): as large as fits between the bar
/// and the strip, inside the margin, and inside the side panels' room on a
/// wide screen; centred across, and never larger than the screen itself.
pub fn preview(screen: (f32, f32), drawer: &editor::Layout) -> Preview {
    let strip_h = drawer.size.1 as f32;
    let top = MARGIN + bar_height(screen) + BAR_GAP;
    let bottom = screen.1 - MARGIN;
    let room_h = (bottom - strip_h - STRIP_GAP - top).max(1.0);
    let side = MARGIN
        + if screen.0 >= WIDE {
            PANEL_W + PANEL_GAP
        } else {
            0.0
        };
    let room_w = (screen.0 - 2.0 * side).max(1.0);
    let k = (room_w / screen.0).min(room_h / screen.1).min(1.0);
    // The canvas and the strip under it stand together, centred between the
    // bar and the screen's margin, so no empty band opens between them when
    // the width decides the canvas's size.
    let group = screen.1 * k + STRIP_GAP + strip_h;
    Preview {
        k,
        x: ((screen.0 - screen.0 * k) / 2.0).round(),
        y: (top + ((bottom - top - group) / 2.0).max(0.0)).round(),
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

/// The side panels of the edit mode (M5.10c7e) on a screen at least `WIDE`
/// wide: the wallpaper's on the left, `WALLPAPER_PANEL_H` high, and the chosen
/// widget's on the right, as high as the canvas. Each is `PANEL_GAP` from the
/// canvas and at its top. None on a narrower screen, where a card floats by
/// its part.
pub fn panels(screen: (f32, f32), preview: Preview) -> Option<(Rect, Rect)> {
    if screen.0 < WIDE {
        return None;
    }
    let canvas = picture_rect(screen, preview);
    let left = Rect::new(
        canvas.x - PANEL_GAP - PANEL_W,
        canvas.y,
        PANEL_W,
        WALLPAPER_PANEL_H,
    );
    let right = Rect::new(canvas.right() + PANEL_GAP, canvas.y, PANEL_W, canvas.h);
    Some((left, right))
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
    let parts = lock_screen::with_added_on(
        &view.parts,
        part,
        to_lock(rect),
        view.screen,
        lock::MARGIN,
        &lock_screen::message_placeholder(),
        true,
        view.on,
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
    // The placed parts first, then the strip's tabs and tiles, then the bar's
    // buttons last (M5.10c7f), so Tab from nothing focuses a part.
    let bar: Vec<Focus> = if view.drawer.bare {
        BAR_ORDER.iter().map(|b| Focus::Bar(*b)).collect()
    } else {
        Vec::new()
    };
    // The wallpaper panel's Dim comes after the parts, where its panel lies
    // beside the canvas, and only on a screen wide enough for the panel.
    let dim = has_panel(view).then_some(Focus::Dim);
    (0..view.parts.len())
        .map(Focus::Part)
        .chain(dim)
        .chain(editor::ring(&view.drawer).into_iter().map(Focus::Drawer))
        .chain(bar)
        .collect()
}

/// Whether the edit mode draws the wallpaper panel, and with it the Dim
/// slider: on the lock screen, on a screen at least `WIDE` wide (`panels`).
/// The desktop has none (M8.21 part 21j2).
pub fn has_panel(view: &View) -> bool {
    view.on == Surface::LockScreen && view.screen.0 >= WIDE
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
    // The preview takes only Escape and Return, which go back to the edit.
    if view.previewing {
        return match key {
            Key::Escape | Key::Activate => Step::Back,
            _ => Step::Nothing,
        };
    }
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
        Some(Focus::Bar(button)) => bar_key(button, key),
        Some(Focus::Dim) => dim_key(key),
    }
}

/// A key on the Dim slider (M5.10c7g): the Left and Right keys step it by
/// `DIM_STEP`, the others do nothing (Tab and Escape are the edit's).
fn dim_key(key: Key) -> Step {
    match key {
        Key::Left => Step::Dim(-DIM_STEP),
        Key::Right => Step::Dim(DIM_STEP),
        _ => Step::Nothing,
    }
}

/// A key on one of the bar's buttons (M5.10c7e): Return and Space act on it,
/// the arrows step to the next button along the bar (no wrap).
fn bar_key(button: BarButton, key: Key) -> Step {
    let at = BAR_ORDER.iter().position(|b| *b == button).unwrap_or(0);
    match key {
        Key::Activate => match button {
            BarButton::Cancel => Step::Close,
            BarButton::Preview => Step::Preview,
            BarButton::Done => Step::Done,
        },
        Key::Left => Step::Focus(Some(Focus::Bar(BAR_ORDER[at.saturating_sub(1)]))),
        Key::Right => Step::Focus(Some(Focus::Bar(
            BAR_ORDER[(at + 1).min(BAR_ORDER.len() - 1)],
        ))),
        _ => Step::Nothing,
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
                1 => Step::Background(i),
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
        // Return on a tile is this drawer's own, above; Delete takes no part
        // from the drawer.
        editor::Step::Act(Act::Add(_) | Act::Remove(_)) | editor::Step::Nothing => Step::Nothing,
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
        Focus::Bar(BarButton::Cancel) => "cancel".into(),
        Focus::Bar(BarButton::Preview) => "preview".into(),
        Focus::Bar(BarButton::Done) => "done".into(),
        Focus::Dim => "dim".into(),
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

/// The bar's buttons as a screen reader finds them (M5.10c7e): last in the
/// reader's list, after the parts and the drawer, when the drawer is bare, none
/// otherwise.
fn bar_nodes(view: &View) -> Vec<Item> {
    if !view.drawer.bare {
        return Vec::new();
    }
    let bar = bar(view.screen);
    [
        (bar.cancel, tr("Cancel")),
        (bar.preview, tr("Preview")),
        (bar.done, tr("Done")),
    ]
    .into_iter()
    .map(|(rect, label)| Item {
        role: Role::Button,
        label: label.into(),
        bounds: bounds(rect),
        children: Vec::new(),
        toggled: None,
        value: None,
        value_text: None,
        disabled: false,
    })
    .collect()
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
    let mut out: Vec<Item> = Vec::new();
    out.extend(
        view.parts
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
                    value_text: None,
                    disabled: false,
                })
            })
            .collect::<Vec<Item>>(),
    );
    if let Some(rect) = dim_on_screen(view, preview) {
        out.push(Item {
            role: Role::Slider,
            label: tr("Dim").to_string(),
            bounds: bounds(rect),
            children: Vec::new(),
            toggled: None,
            value: Some((f64::from(view.dim), 0.0, f64::from(lock_screen::MOST_DIM))),
            value_text: None,
            disabled: false,
        });
    }
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
    // The bar's buttons come last, as the ring's (M5.10c7f).
    out.extend(bar_nodes(view));
    out
}

/// The screen reader's item for the keyboard's focus, in `nodes`' order
/// (M5.10c6c): a placed part's place among those drawn, else the drawer's
/// item after them. None with no focus, or a part that is not drawn.
pub fn focus_node(view: &View, at: &lock::Layout) -> Option<usize> {
    // The parts come first, then the drawer's tabs, tiles, Undo and Done, then
    // the bar's buttons, as `nodes` lists them (M5.10c7f).
    let drawn = at.placed.iter().filter(|r| r.is_some()).count();
    let tiles = view.drawer.tiles.len();
    let tabs = view.drawer.tabs.len();
    // The Dim slider is one node after the parts when the panel shows it.
    let dim = usize::from(has_panel(view));
    match view.focus? {
        Focus::Bar(button) => BAR_ORDER
            .iter()
            .position(|b| *b == button)
            .map(|p| drawn + dim + tabs + tiles + p),
        Focus::Part(i) => {
            if at.placed.get(i)?.is_none() {
                return None;
            }
            Some(at.placed[..i].iter().filter(|r| r.is_some()).count())
        }
        Focus::Dim => has_panel(view).then_some(drawn),
        // The drawer's nodes are its tabs, then its tiles, then Undo and Done.
        Focus::Drawer(editor::Part::Tab(j)) => Some(drawn + dim + j),
        Focus::Drawer(editor::Part::Tile(j)) => Some(drawn + dim + tabs + j),
        Focus::Drawer(editor::Part::Undo) => Some(drawn + dim + tabs + tiles),
        Focus::Drawer(editor::Part::Done) => Some(drawn + dim + tabs + tiles + 1),
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
    if let Some(dim) = dim_on_screen(view, preview) {
        out.push_str(&format!(", dim {}", at_rect(dim)));
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
    if view.drawer.bare {
        let bar = bar(view.screen);
        out.push_str(&format!(
            ", cancel {}, preview {}, done {}",
            at_rect(bar.cancel),
            at_rect(bar.preview),
            at_rect(bar.done)
        ));
    } else {
        out.push_str(&format!(
            ", undo {}, done {}",
            at_rect(moved(drawer.undo)),
            at_rect(moved(drawer.done))
        ));
    }
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
    // The parts' marks: none at rest, a faint outline under the pointer, the
    // accent ring on the one a card is open for or the keyboard holds
    // (M5.10c7f). The dragged part shows no mark at its old place.
    for (i, rect) in at.placed.iter().enumerate() {
        let Some(rect) = rect else {
            continue;
        };
        let screen = preview.to_screen(*rect);
        let carded = view.card.as_ref().is_some_and(|c| c.index == i);
        let state = if moved == Some(i) {
            edit_kit::State::Rest
        } else if carded || view.focus == Some(Focus::Part(i)) {
            edit_kit::State::Focus
        } else if view.hover == Some(i) {
            edit_kit::State::Hover
        } else {
            edit_kit::State::Rest
        };
        edit_kit::mark_part(pixmap, screen, state, tokens, style, s);
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

/// The bar across the top (M5.10c7e), drawn over the stage: Cancel and Undo
/// as the quiet buttons, Done as the accent one, Undo dimmed when `undo_on` is
/// false (nothing to undo), and the look's title with its subtitle under it,
/// centred in the room between Undo and Done. The subtitle is the drawer's,
/// or the hint its tab gives when the drawer has none.
pub fn paint_bar(
    pixmap: &mut Pixmap,
    view: &View,
    bar: &Bar,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let style = edit_kit::Style::GLASS;
    let ink = edit_kit::inks(style, tokens);
    let state = |button: BarButton| {
        if view.focus == Some(Focus::Bar(button)) {
            edit_kit::State::Focus
        } else {
            edit_kit::State::Rest
        }
    };
    // Cancel is a pill filled with white at 10 percent, no outline, its words
    // regular (the mockup's); the keyboard's ring lies round it when focused.
    let size = BAR_WORDS * s;
    let (x, y, w, h) = bar.cancel.device(s);
    fill(
        pixmap,
        x,
        y,
        w,
        h,
        h / 2.0,
        Colour {
            a: CANCEL_FILL,
            ..ink.text
        },
    );
    let mut cancel = text.line_in(tr("Cancel"), size, Face::REGULAR);
    let x = bar.cancel.x * s + ((bar.cancel.w * s - cancel.width) / 2.0).round();
    let y = popup::middle(bar.cancel.y, bar.cancel.h, size, s);
    text.draw(pixmap, &mut cancel, x, y, ink.text);
    if state(BarButton::Cancel) == edit_kit::State::Focus {
        popup::focus_ring(pixmap, bar.cancel, bar.cancel.h / 2.0, s, tokens);
    }
    // Preview is a pill with no fill and a 1 px outline of white at 28 percent,
    // a play triangle before its regular word.
    let (x, y, w, h) = bar.preview.device(s);
    edit_kit::stroke(
        pixmap,
        (x, y, w, h),
        h / 2.0,
        BAR_LINE * s,
        Colour {
            a: PREVIEW_LINE,
            ..ink.text
        },
        None,
    );
    if state(BarButton::Preview) == edit_kit::State::Focus {
        popup::focus_ring(pixmap, bar.preview, bar.preview.h / 2.0, s, tokens);
    }
    let mut word = text.line_in(tr("Preview"), size, Face::REGULAR);
    const TRIANGLE: (f32, f32) = (8.0, 10.0);
    const TRIANGLE_GAP: f32 = 6.0;
    let total = TRIANGLE.0 + TRIANGLE_GAP + word.width / s;
    let left = bar.preview.x + (bar.preview.w - total) / 2.0;
    let mid = bar.preview.y + bar.preview.h / 2.0;
    let mut path = PathBuilder::new();
    path.move_to(left * s, (mid - TRIANGLE.1 / 2.0) * s);
    path.line_to(left * s, (mid + TRIANGLE.1 / 2.0) * s);
    path.line_to((left + TRIANGLE.0) * s, mid * s);
    path.close();
    if let Some(path) = path.finish() {
        pixmap.fill_path(
            &path,
            &paint_of(ink.text),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    let y = popup::middle(bar.preview.y, bar.preview.h, size, s);
    text.draw(
        pixmap,
        &mut word,
        (left + TRIANGLE.0 + TRIANGLE_GAP) * s,
        y,
        ink.text,
    );
    edit_kit::button(
        pixmap,
        bar.done,
        tr("Done"),
        edit_kit::ButtonKind::Light,
        state(BarButton::Done),
        tokens,
        Some(&mut *text),
        style,
        s,
    );
    let room = bar.title.w * s;
    if room <= 0.0 {
        return;
    }
    let title = view
        .drawer
        .heading
        .clone()
        .unwrap_or_else(|| tr("Edit lock screen").into());
    let size = TITLE_SIZE * s;
    let mut line = text.fit_in(&title, size, room, Face::SEMIBOLD);
    let x = bar.title.x * s + ((room - line.width) / 2.0).round();
    let y = popup::middle(bar.title.y, bar.title.h / 2.0, size, s);
    text.draw(pixmap, &mut line, x, y, ink.text);
    let subtitle = match view.drawer.subtitle {
        Some(words) => words.to_string(),
        None => editor::hint_words(&view.drawer).to_string(),
    };
    // The subtitle is 11 px, white at 60 percent (the mockup's).
    let small = tokens.panel_text_small_size as f32 * s;
    let mut words = text.fit(&subtitle, small, room);
    let x = bar.title.x * s + ((room - words.width) / 2.0).round();
    let y = popup::middle(bar.title.y + bar.title.h / 2.0, bar.title.h / 2.0, small, s);
    let dim = Colour {
        a: SUBTITLE_SHARE,
        ..ink.text
    };
    text.draw(pixmap, &mut words, x, y, dim);
}

/// The Dim block of the wallpaper panel `panel` (M5.10c7g): the top of the
/// word "Dim" and the slider's rectangle under it, logical pixels, from the
/// panel's padding and the lines above it (the heading, the picture, the name
/// and the note).
fn dim_block(panel: Rect) -> (f32, Rect) {
    let inner_w = panel.w - 2.0 * PANEL_PAD;
    let top = panel.y
        + PANEL_PAD_Y
        + HEADING_SIZE
        + PANEL_STACK_GAP
        + inner_w * WALLPAPER_SHARE
        + PANEL_STACK_GAP;
    let word_y = top + NAME_LINE + NOTE_LINE + DIM_GAP;
    let slider_y = word_y + DIM_WORD_LINE + DIM_SLIDER_GAP;
    (
        word_y,
        Rect::new(panel.x + PANEL_PAD, slider_y, inner_w, edit_kit::KNOB),
    )
}

/// The Dim slider's rectangle on the screen (M5.10c7g), when the edit mode
/// draws the wallpaper panel (`has_panel`), else None.
pub fn dim_on_screen(view: &View, preview: Preview) -> Option<Rect> {
    if !has_panel(view) {
        return None;
    }
    let (left, _) = panels(view.screen, preview)?;
    Some(dim_block(left).1)
}

/// The Dim a press or a drag at `x` gives on the slider `slider` (M5.10c7g):
/// the knob's centre travels from half a knob in from the slider's left to
/// half a knob in from its right, so the value is where the knob would stand.
pub fn dim_at(slider: Rect, x: f32) -> u32 {
    let travel = (slider.w - edit_kit::KNOB).max(1.0);
    let share = ((x - slider.x - edit_kit::KNOB / 2.0) / travel).clamp(0.0, 1.0);
    (share * lock_screen::MOST_DIM as f32).round() as u32
}

/// Whether a point lies on the Dim slider, with `DIM_HIT` room above and below
/// it for a finger (M5.10c7g).
pub fn dim_hit(slider: Rect, x: f32, y: f32) -> bool {
    x >= slider.x
        && x <= slider.right()
        && y >= slider.y - DIM_HIT
        && y <= slider.y + slider.h + DIM_HIT
}

/// The wallpaper panel on the left of the edit mode (M5.10c7e): a quiet glass
/// card with the heading "Wallpaper", the chosen background's picture (its
/// thumbnail, drawn 16 to 10 with the tile's corners; a grey space for the
/// person's own picture, which has none) and its title under it. Nothing in
/// it is pressable.
pub fn paint_wallpaper_panel(
    pixmap: &mut Pixmap,
    rect: Rect,
    view: &View,
    picture: Option<&Pixmap>,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let style = edit_kit::Style::GLASS;
    edit_kit::side_panel(pixmap, rect, tokens, s);
    let ink = edit_kit::inks(style, tokens);
    let inner = Rect::new(
        rect.x + PANEL_PAD,
        rect.y + PANEL_PAD_Y,
        rect.w - 2.0 * PANEL_PAD,
        rect.h - 2.0 * PANEL_PAD_Y,
    );
    // The heading is WALLPAPER in capitals, 10.5 px semibold at half white.
    let small_words = HEADING_SIZE;
    let small = small_words * s;
    let mut heading = text.line_in(&tr("Wallpaper").to_uppercase(), small, Face::SEMIBOLD);
    let y = popup::middle(inner.y, small_words, small, s);
    let heading_ink = Colour {
        a: HEADING_SHARE,
        ..ink.text
    };
    text.draw(pixmap, &mut heading, inner.x * s, y, heading_ink);
    let picture_at = Rect::new(
        inner.x,
        inner.y + small_words + PANEL_STACK_GAP,
        inner.w,
        inner.w * WALLPAPER_SHARE,
    );
    let (x, y, w, h) = picture_at.device(s);
    match picture {
        Some(picture) => {
            paint_kit::rounded_picture(pixmap, picture, (x, y, w, h), THUMB_RADIUS * s);
        }
        None => fill(pixmap, x, y, w, h, THUMB_RADIUS * s, ink.raised),
    }
    let edge = Colour {
        a: THUMB_EDGE,
        ..ink.text
    };
    edit_kit::stroke(
        pixmap,
        (x, y, w, h),
        THUMB_RADIUS * s,
        BAR_LINE * s,
        edge,
        None,
    );
    let title_words = NAME_SIZE;
    let title_size = title_words * s;
    let mut title = text.fit_in(
        &wallpaper_title(&view.background),
        title_size,
        inner.w * s,
        Face::SEMIBOLD,
    );
    let top = picture_at.y + picture_at.h + PANEL_STACK_GAP;
    let y = popup::middle(top, title_words, title_size, s);
    text.draw(pixmap, &mut title, inner.x * s, y, ink.text);
    // Under the name, dim: "Classic's own" when the look brings this background.
    if let Some(own) = own_words(view) {
        let caption = tokens.panel_text_small_size as f32 * s;
        let mut words = text.fit(&own, caption, inner.w * s);
        let caption_top = top + title_words + 2.0;
        let y = popup::middle(caption_top, 14.0, caption, s);
        let note = Colour {
            a: NOTE_SHARE,
            ..ink.text
        };
        text.draw(pixmap, &mut words, inner.x * s, y, note);
    }
    // The Dim (M5.10c7g): the word, then the slider under it, its knob at the
    // dim, and the keyboard's ring when it has the focus.
    let (word_y, slider) = dim_block(rect);
    let mut word = text.line_in(tr("Dim"), DIM_WORD * s, Face::REGULAR);
    let y = popup::middle(word_y, DIM_WORD_LINE, DIM_WORD * s, s);
    let word_ink = Colour {
        a: DIM_WORD_SHARE,
        ..ink.text
    };
    text.draw(pixmap, &mut word, inner.x * s, y, word_ink);
    let share = view.dim as f32 / lock_screen::MOST_DIM as f32;
    edit_kit::slider(pixmap, slider, share, tokens, s);
    if view.focus == Some(Focus::Dim) {
        edit_kit::mark_part(pixmap, slider, edit_kit::State::Focus, tokens, style, s);
    }
}

/// The wallpaper's name, as the panel shows it (M5.10c7f): Blurred wallpaper,
/// Sharp wallpaper, Navy for the colour, and Picture for the person's own.
pub fn wallpaper_title(background: &Background) -> String {
    match background {
        Background::Blurred => tr("Blurred wallpaper").into(),
        Background::Sharp => tr("Sharp wallpaper").into(),
        Background::Backdrop => tr("Navy").into(),
        Background::Picture(_) => tr("Picture").into(),
        Background::Colour(_) => tr("Colour").into(),
    }
}

/// "Classic's own" (M5.10c7f): the look's name with `'s own`, when the look
/// brings the background in use; None otherwise.
fn own_words(view: &View) -> Option<String> {
    (lock_screen::look_background(&view.editing) == view.background).then(|| {
        trf(
            "{look}'s own",
            &[("look", lock_screen::look_title(&view.editing).as_str())],
        )
    })
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
    for (part, rect) in view.parts.iter().zip(&at.placed) {
        if let (Part::Player, Some(rect)) = (part.part, rect) {
            draw_player(pixmap, *rect, compact, tokens, text, s, &sample);
        }
    }
}

/// The player's card at `rect` (logical pixels), with `sample` in it:
/// `draw_placeholders`' one part, and the look pictures' playing player.
fn draw_player(
    pixmap: &mut Pixmap,
    rect: Rect,
    compact: bool,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
    sample: &player::PlayerView,
) {
    let radius = rect.h / 2.0;
    lock::lift(pixmap, rect, radius, s, tokens);
    lock::glass(pixmap, rect.device(s), radius * s, s, tokens);
    let parts = player::place_wide(rect, compact, 1, 0);
    let marks = player::Marks {
        hover: None,
        focus: None,
        page: 0,
    };
    player::paint(pixmap, sample, &parts, marks, tokens, Some(text), s);
}

/// Draws the drawer through `editor::paint` at its place on the edit mode's
/// surface, `pixmap` at `s`, over the lock screen's picture.
pub fn paint_drawer(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, text: &mut Text, s: f32) {
    let layout = editor::layout(&view.drawer);
    let place = drawer_rect(view.screen, &layout);
    // The sheet's shadow lies outside the sheet, so it is cast here on the
    // whole screen: the layer below has no room round the sheet. A bare strip
    // has no sheet, so it has no shadow either.
    if !view.drawer.bare {
        lock::lift(pixmap, place, edit_kit::SHEET_RADIUS, s, tokens);
    }
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

/// A part was moved, added, taken away or changed: the edit now saves a look
/// when Done is pressed (M5.10c7a).
pub fn parts_moved(view: &mut View) {
    view.changed = true;
}

/// A background was chosen in the Background tab (M5.10c7a): the edit saves a
/// look when Done is pressed, as a part change does.
pub fn background_chosen(view: &mut View, background: Background) {
    // A dim the person has not moved follows the background's default (M5.10c7g).
    if view.dim == lock_screen::default_dim(&view.background) {
        view.dim = lock_screen::default_dim(&background);
    }
    view.background = background;
    view.changed = true;
}

/// The words the drawer's heading and hint say (M5.10c7a): the look's title,
/// and on the Parts and Wallpaper tabs what Done does to this look, a new
/// look for a built-in one. The Looks tab (tab 2) keeps its own hint (None).
pub fn drawer_words(view: &View) -> (String, Option<&'static str>) {
    if view.on == Surface::Desktop {
        return (
            tr("Edit desktop").to_string(),
            Some(tr("Yours · Done saves it")),
        );
    }
    let title = lock_screen::look_title(&view.editing);
    let subtitle = if lock_screen::is_built_in(&view.editing) {
        tr("Built in · Done saves a new look")
    } else {
        tr("Yours · Done saves it")
    };
    (title, Some(subtitle))
}

/// The preview's pill (M5.10c7f): centred, `PREVIEW_TOP` from the top, inside
/// the margins on a narrow screen.
pub fn preview_pill(screen: (f32, f32)) -> Rect {
    let w = PREVIEW_W.min(screen.0 - 2.0 * MARGIN).max(1.0);
    Rect::new(((screen.0 - w) / 2.0).round(), PREVIEW_TOP, w, PREVIEW_H)
}

/// The Back to editing button, at the right inside the preview's pill.
pub fn preview_back(screen: (f32, f32)) -> Rect {
    let pill = preview_pill(screen);
    Rect::new(
        pill.right() - 8.0 - BACK_W,
        pill.y + (pill.h - edit_kit::BUTTON) / 2.0,
        BACK_W,
        edit_kit::BUTTON,
    )
}

/// The preview as the screen reader finds it (M5.10c7f): its Back button.
pub fn preview_nodes(screen: (f32, f32)) -> Vec<Item> {
    vec![Item {
        role: Role::Button,
        label: tr("Back to editing").into(),
        bounds: bounds(preview_back(screen)),
        children: Vec::new(),
        toggled: None,
        value: None,
        value_text: None,
        disabled: false,
    }]
}

/// Draws the preview's pill over the lock as it will be (M5.10c7f): a glass
/// pill with a small dot, "Preview" in semibold, the dim "Not locked · Esc to
/// go back", and the white Back button. The lock itself is drawn by the caller
/// at the full screen, with no marks.
pub fn paint_preview(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, text: &mut Text, s: f32) {
    let style = edit_kit::Style::GLASS;
    let ink = edit_kit::inks(style, tokens);
    let pill = preview_pill(view.screen);
    lock::lift(pixmap, pill, pill.h / 2.0, s, tokens);
    lock::glass(pixmap, pill.device(s), pill.h / 2.0 * s, s, tokens);
    let mid = pill.y + pill.h / 2.0;
    fill(
        pixmap,
        (pill.x + 16.0) * s,
        (mid - PREVIEW_DOT / 2.0) * s,
        PREVIEW_DOT * s,
        PREVIEW_DOT * s,
        PREVIEW_DOT * s / 2.0,
        tokens.accent,
    );
    let size = tokens.panel_text_size as f32 * s;
    let mut word = text.line_in(tr("Preview"), size, Face::SEMIBOLD);
    let x = pill.x + 16.0 + PREVIEW_DOT + 8.0;
    let y = popup::middle(pill.y, pill.h, size, s);
    text.draw(pixmap, &mut word, x * s, y, ink.text);
    let small = tokens.panel_text_small_size as f32;
    let words = match view.on {
        Surface::Desktop => tr("Esc to go back"),
        _ => tr("Not locked · Esc to go back"),
    };
    let mut dim = text.fit(
        words,
        small * s,
        ((pill.right() - BACK_W - 24.0) - (x + word.width / s + 8.0)).max(1.0) * s,
    );
    let x = x + word.width / s + 8.0;
    let y = popup::middle(pill.y, pill.h, small * s, s);
    text.draw(pixmap, &mut dim, x * s, y, ink.dim);
    let back = preview_back(view.screen);
    let state = if view.focus == Some(Focus::Bar(BarButton::Done)) {
        edit_kit::State::Focus
    } else {
        edit_kit::State::Rest
    };
    edit_kit::button(
        pixmap,
        back,
        tr("Back to editing"),
        edit_kit::ButtonKind::Light,
        state,
        tokens,
        Some(&mut *text),
        style,
        s,
    );
}

/// The words a strip tile's preview shows for `part` (M5.10c7f): the clock's
/// `H:MM`, the short date, the battery's charge (`82%`, or none for the icon),
/// the sample message in quotes. Empty for the others.
pub fn preview_words(part: Part, when: &lock::When, charge: Option<u8>) -> String {
    match part {
        Part::Clock => format!("{}:{:02}", when.hour, when.minute),
        Part::Date => clock_face::date_text(
            when.weekday,
            when.day,
            when.month,
            when.year,
            &[DatePart::Weekday, DatePart::Day, DatePart::Month],
            DateStyle::Short,
        ),
        Part::Battery => charge.map_or_else(String::new, |c| format!("{c}%")),
        Part::Message => format!("\"{}\"", tr("Hello")),
        _ => String::new(),
    }
}

/// A strip tile's live preview (M5.10c7f, recognisable pictures in M5.10c7g),
/// drawn once for its words at `PREVIEW_SCALE`: the clock's time in 22 px
/// light, the date in 12 px, the battery's outline icon at 82 percent with its
/// charge in 14 px, the message's icon with the words in 12 px, a small music
/// player (a 40 by 22 pill with a cover and the play glyph) and the picture's
/// tile of 26 by 20 with a two-colour gradient of the accent and its glyph.
/// None when the pixmap cannot be made.
pub fn preview_picture(
    part: Part,
    words: &str,
    tokens: &Tokens,
    text: &mut Text,
) -> Option<Pixmap> {
    let s = PREVIEW_SCALE;
    let (bw, bh) = PREVIEW_BOX;
    let mut pixmap = Pixmap::new((bw * s).round() as u32, (bh * s).round() as u32)?;
    let ink = edit_kit::inks(edit_kit::Style::GLASS, tokens);
    let centre_text = |pixmap: &mut Pixmap, text: &mut Text, words: &str, face: Face, size: f32| {
        let mut line = text.line_in(words, size * s, face);
        let x = ((bw * s - line.width) / 2.0).round();
        let y = popup::middle(0.0, bh, size * s, s);
        text.draw(pixmap, &mut line, x, y, ink.text);
    };
    // The mockup's sizes (M5.10c7f): the clock 22 px at weight 250, the date
    // and the message 12 px, the charge 17 px, the bar 40 by 12 and the
    // picture's tile 26 by 20.
    let light = Face {
        weight: 250,
        tabular: false,
    };
    let date_ink = Colour {
        a: PREVIEW_DATE_SHARE,
        ..ink.text
    };
    match part {
        Part::Clock => centre_text(&mut pixmap, text, words, light, 22.0),
        Part::Date => {
            let mut line = text.line_in(words, 12.0 * s, Face::REGULAR);
            let x = ((bw * s - line.width) / 2.0).round();
            let y = popup::middle(0.0, bh, 12.0 * s, s);
            text.draw(&mut pixmap, &mut line, x, y, date_ink);
        }
        Part::Battery => {
            // The outline at 16 px as the status area draws it, then the charge.
            let px = 16.0 * s;
            let mut line = text.line_in(words, 14.0 * s, Face::REGULAR);
            let gap = if words.is_empty() { 0.0 } else { 6.0 * s };
            let total = px + gap + line.width;
            let x = ((bw * s - total) / 2.0).round();
            let y = (bh * s - px) / 2.0;
            let halo = s.round().max(1.0) as i32;
            paint_kit::battery(
                &mut pixmap,
                px,
                (x, y),
                (PREVIEW_CHARGE, false),
                ink.text,
                halo,
            );
            if !words.is_empty() {
                let top = popup::middle(0.0, bh, 14.0 * s, s);
                text.draw(&mut pixmap, &mut line, x + px + gap, top, ink.text);
            }
        }
        Part::Message => {
            // The message's icon at 14 px, at 85 percent, then the words.
            let px = 14.0 * s;
            let gap = 5.0 * s;
            let mut line = text.line_in(words, 12.0 * s, Face::REGULAR);
            let total = px + gap + line.width;
            let x = ((bw * s - total) / 2.0).round();
            let y = (bh * s - px) / 2.0;
            let glyph = Colour {
                a: PREVIEW_MESSAGE_SHARE,
                ..ink.text
            };
            paint_kit::icon(&mut pixmap, "message", px, x, y, glyph);
            let top = popup::middle(0.0, bh, 12.0 * s, s);
            text.draw(&mut pixmap, &mut line, x + px + gap, top, ink.text);
        }
        Part::Player => {
            // A small player: the pill, the cover with the music glyph in it,
            // and the play glyph at its right.
            let (w, h) = (PREVIEW_PLAYER.0 * s, PREVIEW_PLAYER.1 * s);
            let (x, y) = ((bw * s - w) / 2.0, (bh * s - h) / 2.0);
            let pill = Colour {
                a: PREVIEW_PLAYER_SHARE,
                ..ink.text
            };
            fill(&mut pixmap, x, y, w, h, h / 2.0, pill);
            let cover = PREVIEW_COVER * s;
            let cover_y = y + (h - cover) / 2.0;
            let cover_x = x + PREVIEW_PLAYER_PAD * s;
            fill(
                &mut pixmap,
                cover_x,
                cover_y,
                cover,
                cover,
                4.0 * s,
                tokens.accent,
            );
            let note = 10.0 * s;
            paint_kit::icon(
                &mut pixmap,
                "music",
                note,
                cover_x + (cover - note) / 2.0,
                cover_y + (cover - note) / 2.0,
                ink.text,
            );
            let play = PREVIEW_PLAY * s;
            let play_x = x + w - PREVIEW_PLAYER_PAD * s - play;
            let play_y = y + (h - play) / 2.0;
            paint_kit::icon(&mut pixmap, "media-play", play, play_x, play_y, ink.text);
        }
        Part::Picture => {
            let (w, h) = (26.0 * s, 20.0 * s);
            let (x, y) = ((bw * s - w) / 2.0, (bh * s - h) / 2.0);
            let bottom = paint_kit::mix(tokens.accent, tokens.backdrop_text, 0.5);
            paint_kit::fill_vertical(&mut pixmap, (x, y, w, h), 6.0 * s, tokens.accent, bottom);
            // The picture glyph, white, centred on the tile.
            let glyph = PREVIEW_PICTURE * s;
            let (gx, gy) = (x + (w - glyph) / 2.0, y + (h - glyph) / 2.0);
            paint_kit::icon(&mut pixmap, "picture", glyph, gx, gy, ink.text);
        }
        _ => {}
    }
    Some(pixmap)
}

/// The lines Done writes (M5.10c6c3, M5.10c7a). `looks` and `look` are each
/// None to leave the person's line as it is, `Some(None)` to take it out and
/// `Some(Some(value))` to write it. `parts` and `background` are never written:
/// Done saves them into a look, so their lines are taken out (None).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lines {
    pub looks: Option<Option<String>>,
    pub look: Option<Option<String>>,
    /// The person's `lock_screen.widgets` and `wallpaper` lines, changes over
    /// the look from the command line: None leaves them, `Some(None)` takes
    /// them out, as a look Done saves or switches to brings its own.
    pub parts: Option<Option<String>>,
    pub background: Option<Option<String>>,
    /// The person's `lock_screen.dim` line, taken out as the background's is
    /// when a look saves it (M5.10c7g).
    pub dim: Option<Option<String>>,
}

/// The `lock_screen.look` line for the look `name` (classic is none): None
/// when the lock already applies it, so the line is left as it is.
fn look_line(name: &str, lock: &LockScreen) -> Option<Option<String>> {
    let target = (name != lock_screen::CLASSIC).then(|| name.to_string());
    (target.as_deref() != lock.look.as_deref()).then_some(target)
}

/// What Done writes for the edit `view` (M5.10c6c3, M5.10c7a), with `lock` the
/// person's lock settings over the machine's. Nothing changed writes only the
/// look chosen in the Looks tab. Something changed saves a look: a built-in
/// one never changes, so the edit makes a new look of the person's, named
/// from it (`classic-2`, `magazine-2`); one of the person's own is rewritten.
/// Its background is kept only when it is not the blurred wallpaper, which a
/// look without one gets. Fails with the message when the looks do not write
/// as TOML.
pub fn written(view: &View, lock: &LockScreen) -> Result<Lines, String> {
    if !view.changed {
        // Nothing changed: the same look keeps the changes over it, and
        // another look chosen brings its own.
        let look = look_line(&view.editing, lock);
        let over = look.is_some().then_some(None);
        return Ok(Lines {
            looks: None,
            look,
            parts: over.clone(),
            background: over.clone(),
            dim: over,
        });
    }
    let own = lock.looks.as_ref();
    let name = if lock_screen::is_built_in(&view.editing) {
        lock_screen::new_look_name(&view.editing, |n| {
            own.is_some_and(|own| own.contains_key(n))
        })
    } else {
        view.editing.clone()
    };
    let background = (view.background != Background::Blurred)
        .then(|| lock_screen::background_value(&view.background));
    // The dim is kept only when it is not the background's default (M5.10c7g).
    let dim = (view.dim != lock_screen::default_dim(&view.background)).then_some(view.dim);
    let look = OwnLook {
        parts: view.parts.clone(),
        background,
        dim,
    };
    let looks = lock_screen::looks_value(own, &name, Some(look))
        .ok_or_else(|| tr("the looks do not write as TOML").to_string())?;
    Ok(Lines {
        looks: Some(Some(looks)),
        look: look_line(&name, lock),
        parts: Some(None),
        background: Some(None),
        dim: Some(None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corner;
    use crate::editor::Kind;
    use edel::lock_screen::{Anchor, Background};
    use edel::settings;
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
            dim: lock_screen::DIM,
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
            tiles: drawer_tiles(Surface::LockScreen),
            compact: false,
            width: 1280,
            hover: None,
            focus: None,
            lifted: None,
            taking: false,
            heading: None,
            subtitle: None,
            bare: false,
        };
        View {
            parts,
            editing: lock_screen::CLASSIC.into(),
            changed: false,
            background: Background::Backdrop,
            dim: lock_screen::DIM,
            screen: (1280.0, 800.0),
            card: None,
            previewing: false,
            drag: None,
            guides: Vec::new(),
            hover: None,
            focus: None,
            drawer,
            on: Surface::LockScreen,
        }
    }

    /// The desktop's edit mode over `parts` (M8.21 part 21j2): the usable area
    /// of a 1280 by 800 screen with a 36 px panel at the foot, so 1280 by 764,
    /// no wallpaper panel, no Dim, no tabs, the strip the clock and the date.
    fn desktop_view_of(parts: Vec<Placed>) -> View {
        let mut view = view_of(parts);
        view.on = Surface::Desktop;
        view.screen = (1280.0, 764.0);
        view.background = Background::Sharp;
        view.dim = 0;
        view.drawer.tabs = Vec::new();
        view.drawer.tiles = drawer_tiles(Surface::Desktop);
        view
    }

    /// The desktop's layout of `parts` as the desktop editor draws it.
    fn at_desktop(parts: &[Placed]) -> lock::Layout {
        let mut text = Text::load(&Tokens::built_in().font);
        lock::widgets_layout(1280.0, 764.0, &Tokens::built_in(), parts, &mut text)
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
    fn the_strip_shows_every_widget_kind_in_order_at_full_strength() {
        let classic = lock_screen::classic(false);
        let tiles = drawer_tiles(Surface::LockScreen);
        let names: Vec<&str> = tiles.iter().map(|t| t.name.as_str()).collect();
        // The six kinds in the mockup's order; the person and the count are not offered.
        assert_eq!(
            names,
            ["clock", "date", "player", "battery", "message", "picture"]
        );
        assert!(!names.contains(&"person"));
        // Every tile is at full strength, the kinds the look already holds too
        // (Classic's date, now playing bar and clock): a press chooses them.
        assert!(tiles.iter().all(|t| !t.placed));
        assert!(full(&classic, true, Part::Date, Surface::LockScreen));
        assert!(!full(&classic, true, Part::Message, Surface::LockScreen));
        // A battery the machine lacks counts as full, though its tile is whole.
        assert!(full(&classic, false, Part::Battery, Surface::LockScreen));
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
        view.drawer.tabs = vec!["Widgets", "Wallpaper"];
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
        assert!(line.contains(", tab1 wallpaper "), "{line}");
        assert!(line.contains(", tile0 clock "), "{line}");
        assert!(line.contains(", tile4 message "), "{line}");
        assert!(line.contains(", tile3 battery "), "{line}");
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
        assert_eq!(rect.y + rect.h, 800.0 - MARGIN);
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
        assert!(shown.y + 800.0 * shown.k <= place.y - STRIP_GAP + 0.5);
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
            dim: lock_screen::DIM,
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
            classic.len() + view.drawer.tiles.len() + 2 + usize::from(has_panel(&view))
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
        assert_eq!(key(&view, Key::Activate, false), Step::Add(Part::Clock));
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
            "tile clock"
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
        assert_eq!(
            items.len(),
            placed + view.drawer.tiles.len() + 2 + usize::from(has_panel(&view))
        );
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
        // The screen is the lock's own, so the part's place is its place.
        let same = Preview {
            k: 1.0,
            x: 0.0,
            y: 0.0,
        };
        // The ring is 1.5 px of white at 90 percent on the part's own left
        // edge (M5.10c7f); the accent halo lies just outside it, 4 px wide.
        let mut shot = |view: &View| -> Pixmap {
            let mut pixmap = Pixmap::new(1280, 800).unwrap();
            paint(&mut pixmap, view, &at, same, &tokens, &mut text, 1.0);
            pixmap
        };
        let y = (r.y + r.h / 2.0) as u32;
        let white_at = |pixmap: &Pixmap, x: u32| {
            pixmap.pixel(x, y).is_some_and(|p| {
                let c = p.demultiply();
                p.alpha() > 200 && c.red() > 180 && c.green() > 180 && c.blue() > 180
            })
        };
        let unfocused = shot(&view);
        let x0 = r.x as u32;
        assert!(
            !(x0 - 1..x0 + 2).any(|x| white_at(&unfocused, x)),
            "with no focus there is no ring"
        );
        set_focus(&mut view, Some(Focus::Part(i)));
        let focused = shot(&view);
        assert!(
            (x0 - 1..x0 + 2).any(|x| white_at(&focused, x)),
            "the focused part has the white ring on its edge"
        );
        let halo = (x0 - 5..x0 - 1).any(|x| {
            let (a, b) = (unfocused.pixel(x, y), focused.pixel(x, y));
            a != b
        });
        assert!(halo, "the accent halo lies just outside the ring");
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
        written(view, &LockScreen::default()).unwrap()
    }

    /// A person's lock settings with `looks` as their own and `look` in use.
    fn lock_with(looks: &[(&str, OwnLook)], look: Option<&str>) -> LockScreen {
        LockScreen {
            look: look.map(str::to_string),
            looks: Some(
                looks
                    .iter()
                    .map(|(name, own)| (name.to_string(), own.clone()))
                    .collect(),
            ),
            ..LockScreen::default()
        }
    }

    /// The clock of `view` moved to the top right, as a drag would leave it.
    fn clock_top_right(view: &mut View) {
        let i = index_of(&view.parts, "clock");
        view.parts[i] = Placed {
            at: Anchor::TopRight,
            x: 0.0,
            y: 0.0,
            ..view.parts[i].clone()
        };
    }

    #[test]
    fn nothing_changed_writes_nothing_for_classic() {
        let view = view_of(lock_screen::classic(false));
        assert_eq!(
            written_for(&view),
            Lines {
                looks: None,
                look: None,
                parts: None,
                background: None,
                dim: None,
            }
        );
    }

    #[test]
    fn nothing_changed_but_a_look_chosen_writes_only_the_look() {
        // A look the edit is inside, unchanged: Done writes only its line.
        let mut view = view_of(lock_screen::classic(false));
        view.editing = "magazine".into();
        assert_eq!(
            written_for(&view),
            Lines {
                looks: None,
                look: Some(Some("magazine".into())),
                parts: Some(None),
                background: Some(None),
                dim: Some(None),
            }
        );
        let lock = LockScreen {
            look: Some("magazine".into()),
            ..LockScreen::default()
        };
        assert_eq!(
            written(&view, &lock).unwrap().look,
            None,
            "a look the lock already applies is left as it is"
        );
    }

    #[test]
    fn a_moved_part_on_classic_saves_a_look_of_its_own_as_classic_2() {
        let mut view = view_of(lock_screen::classic(false));
        clock_top_right(&mut view);
        parts_moved(&mut view);
        let lines = written_for(&view);
        assert_eq!(lines.look, Some(Some("classic-2".into())));
        assert_eq!(lines.parts, Some(None), "the parts are the look's now");
        assert_eq!(lines.background, Some(None));
        let text = lines.looks.flatten().expect("the look is written");
        assert!(text.contains("classic-2"), "{text}");
        assert!(
            text.contains(r#"wallpaper = "backdrop""#),
            "classic's navy backdrop is kept, as only blurred is left out: {text}"
        );
        assert!(text.contains(r#"widget = "clock""#), "{text}");
    }

    #[test]
    fn a_look_saved_by_done_reads_back_in_the_settings_file() {
        let mut view = view_of(lock_screen::classic(false));
        clock_top_right(&mut view);
        parts_moved(&mut view);
        let text = written_for(&view).looks.flatten().unwrap();
        let file = settings::set("format = 1\n", "lock_screen.looks", &text).unwrap();
        assert!(
            file.contains(r#"at = "top_right", widget = "clock""#),
            "{file}"
        );
        assert!(settings::check(&file).unwrap().is_empty(), "{file}");
    }

    #[test]
    fn a_moved_part_on_an_own_look_rewrites_it_and_writes_no_look_when_it_applies() {
        let home = OwnLook {
            parts: lock_screen::classic(false),
            background: None,
            dim: None,
        };
        let lock = lock_with(&[("home", home)], Some("home"));
        let mut view = view_of(lock_screen::classic(false));
        view.editing = "home".into();
        view.background = Background::Blurred;
        parts_moved(&mut view);
        let lines = written(&view, &lock).unwrap();
        assert_eq!(lines.look, None, "home already applies");
        let text = lines.looks.flatten().expect("home is rewritten");
        assert!(text.contains("home"), "{text}");
        assert!(!text.contains("classic-2"), "no copy is made: {text}");
    }

    #[test]
    fn a_built_in_look_changed_saves_as_the_next_free_name() {
        let own = OwnLook {
            parts: lock_screen::classic(false),
            background: None,
            dim: None,
        };
        let lock = lock_with(&[("classic-2", own)], None);
        let mut view = view_of(lock_screen::classic(false));
        parts_moved(&mut view);
        let lines = written(&view, &lock).unwrap();
        assert_eq!(
            lines.look,
            Some(Some("classic-3".into())),
            "classic-2 is taken"
        );
        let text = lines.looks.flatten().unwrap();
        assert!(text.contains("classic-3"), "{text}");
    }

    #[test]
    fn a_chosen_background_marks_the_edit_changed_and_is_saved() {
        let mut view = view_of(lock_screen::classic(false));
        background_chosen(&mut view, Background::Sharp);
        assert!(view.changed);
        let text = written_for(&view).looks.flatten().unwrap();
        assert!(text.contains(r#"wallpaper = "sharp""#), "{text}");
    }

    /// The edit mode's view of `parts` as the lock editor shows it (M5.10c7e):
    /// a bare strip with the lock's three tabs, its tiles under them.
    fn bare_view_of(parts: Vec<Placed>) -> View {
        let mut view = view_of(parts);
        view.drawer.bare = true;
        view.drawer.tabs = vec!["Widgets", "Wallpaper"];
        view
    }

    /// The bar's place on a 1280 by 800 screen: Cancel at the margin, 88 wide,
    /// Preview 112 wide before Done with 8 px between, Done at the right margin,
    /// and the title between Cancel and Preview with 16 px to each (M5.10c7f).
    #[test]
    fn the_bar_sits_at_the_margin_and_the_title_between_cancel_and_preview() {
        let b = bar((1280.0, 800.0));
        assert_eq!(b.cancel, Rect::new(24.0, 24.0, 88.0, 32.0));
        assert_eq!(b.preview.x, 1048.0, "1280 - 24 - 88 - 8 - 112");
        assert_eq!(b.done.right(), 1280.0 - 24.0);
        assert_eq!(b.title.y, 24.0);
        assert_eq!(b.title.h, 32.0);
        assert!(b.title.x >= b.cancel.right() + 16.0 - 0.01);
        assert!(b.title.right() <= b.preview.x - 16.0 + 0.01);
        // A bar's buttons are hit where they lie and nowhere else.
        assert_eq!(bar_hit(&b, 30.0, 30.0), Some(BarButton::Cancel));
        assert_eq!(
            bar_hit(&b, b.preview.x + 1.0, 30.0),
            Some(BarButton::Preview)
        );
        assert_eq!(bar_hit(&b, b.done.x + 1.0, 30.0), Some(BarButton::Done));
        assert_eq!(
            bar_hit(&b, 300.0, 30.0),
            None,
            "the title lies on no button"
        );
        // A Compact screen gives the bar touch-sized buttons.
        assert_eq!(bar((390.0, 844.0)).cancel.h, edit_kit::BUTTON_TOUCH);
    }

    /// On 1280 by 800 the canvas is at least 740 wide and keeps 16 to 10, and
    /// it lies clear of the bar, the two panels and the strip's tabs; the
    /// panels lie inside the screen, clear of the canvas and the bar.
    #[test]
    fn on_1280_by_800_the_canvas_is_large_and_clear_of_the_bar_the_panels_and_the_strip() {
        let screen = (1280.0, 800.0);
        let view = bare_view_of(lock_screen::classic(false));
        let drawer = editor::layout(&view.drawer);
        let shown = preview(screen, &drawer);
        let canvas = picture_rect(screen, shown);
        let b = bar(screen);
        let strip = drawer_rect(screen, &drawer);
        assert!(canvas.w >= 740.0, "{canvas:?}");
        assert!((canvas.w / canvas.h - 1.6).abs() < 0.01, "{canvas:?}");
        assert!(canvas.y >= b.cancel.y + b.cancel.h, "clear of the bar");
        assert!(canvas.y + canvas.h <= strip.y, "clear of the strip");
        let (left, right) = panels(screen, shown).expect("a wide screen has panels");
        assert!(
            left.right() <= canvas.x && right.x >= canvas.right(),
            "beside the canvas"
        );
        assert!(
            left.x >= 0.0 && right.right() <= screen.0,
            "inside the screen"
        );
        assert!(left.y >= b.cancel.y + b.cancel.h && right.y >= b.cancel.y + b.cancel.h);
        assert!(left.y + left.h <= strip.y && right.y + right.h <= strip.y);
        assert_eq!(left.h, WALLPAPER_PANEL_H);
        assert_eq!(
            right.h, canvas.h,
            "the widget's panel is as high as the canvas"
        );
    }

    /// On a portrait screen and on a phone there are no panels, and nothing
    /// overlaps: the bar, the canvas and the strip stack down the screen.
    #[test]
    fn on_a_portrait_screen_and_a_phone_there_are_no_panels_and_nothing_overlaps() {
        for (screen, compact) in [((800.0, 1280.0), false), ((390.0, 844.0), true)] {
            let mut view = bare_view_of(lock_screen::classic(compact));
            view.screen = screen;
            view.drawer.compact = compact;
            view.drawer.width = screen.0 as u32;
            let drawer = editor::layout(&view.drawer);
            let shown = preview(screen, &drawer);
            assert_eq!(panels(screen, shown), None, "{screen:?}");
            let canvas = picture_rect(screen, shown);
            let b = bar(screen);
            let strip = drawer_rect(screen, &drawer);
            assert!(
                canvas.y >= b.cancel.y + b.cancel.h,
                "{screen:?}: below the bar"
            );
            assert!(
                canvas.y + canvas.h <= strip.y,
                "{screen:?}: above the strip"
            );
            assert!(canvas.x >= 0.0 && canvas.right() <= screen.0, "{screen:?}");
        }
    }

    /// The strip sits centred across the screen, `STRIP_GAP` under the
    /// canvas, and inside the margin at the bottom.
    #[test]
    fn the_strip_lies_under_the_canvas_inside_the_margin() {
        for screen in [(1280.0, 800.0), (1920.0, 1080.0), (800.0, 1280.0)] {
            let view = bare_view_of(lock_screen::classic(false));
            let drawer = editor::layout(&view.drawer);
            let strip = drawer_rect(screen, &drawer);
            let canvas = picture_rect(screen, preview(screen, &drawer));
            assert!(
                (strip.y - (canvas.y + canvas.h + STRIP_GAP)).abs() <= 1.0,
                "{screen:?}"
            );
            assert!(strip.y + strip.h <= screen.1 - MARGIN + 0.5, "{screen:?}");
        }
        let screen = (1280.0, 800.0);
        let view = bare_view_of(lock_screen::classic(false));
        let strip = drawer_rect(screen, &editor::layout(&view.drawer));
        assert!(strip.x >= MARGIN - 0.5 && strip.right() <= 1280.0 - MARGIN + 0.5);
    }

    /// A bare strip lays its tabs across the top from y 0, its tiles in one row
    /// under them, and no footer below.
    #[test]
    fn a_bare_strip_has_its_tabs_on_top_and_its_tiles_in_one_row_and_no_footer() {
        let view = bare_view_of(lock_screen::classic(false));
        let l = editor::layout(&view.drawer);
        assert_eq!(l.tabs.len(), 2, "Widgets and Wallpaper");
        assert_eq!(l.tabs_row.y, 0.0);
        assert!(l.tabs.iter().all(|t| t.w == editor::TAB_W), "{:?}", l.tabs);
        let row_y = l.tiles[0].y;
        assert!(l.tiles.iter().all(|t| t.y == row_y), "one row");
        assert_eq!(
            row_y,
            l.tabs_row.y + l.tabs_row.h + 12.0,
            "under the tabs, SPACE lower"
        );
        assert_eq!(l.card.h, row_y + editor::BARE_TILE.1, "no padding below");
        assert_eq!(
            (l.undo.w, l.done.w),
            (0.0, 0.0),
            "the bar holds Undo and Done"
        );
    }

    /// The ring of a bare strip starts with the bar's three buttons and has no
    /// drawer Undo or Done; Tab steps the bar first.
    #[test]
    fn the_ring_ends_with_the_bar_and_the_bare_strip_has_no_undo_or_done() {
        let classic = lock_screen::classic(false);
        let mut view = bare_view_of(classic.clone());
        let ring = ring(&view);
        assert_eq!(
            ring[ring.len() - 3..],
            [
                Focus::Bar(BarButton::Cancel),
                Focus::Bar(BarButton::Preview),
                Focus::Bar(BarButton::Done),
            ]
        );
        assert_eq!(
            ring.len(),
            3 + classic.len()
                + view.drawer.tiles.len()
                + view.drawer.tabs.len()
                + usize::from(has_panel(&view)),
            "parts, dim, tabs, tiles and bar"
        );
        assert!(!ring.contains(&Focus::Drawer(editor::Part::Undo)));
        // Tab from nothing focuses the first placed part, not the bar.
        assert_eq!(
            key(&view, Key::Tab, false),
            Step::Focus(Some(Focus::Part(0)))
        );
        set_focus(&mut view, Some(Focus::Bar(BarButton::Done)));
        assert_eq!(
            key(&view, Key::Tab, false),
            Step::Focus(Some(Focus::Part(0))),
            "Done, the last, wraps to the first part"
        );
    }

    /// Return and Space act on the bar's buttons, and the arrows step along
    /// the bar without wrapping.
    #[test]
    fn keys_act_on_the_bar_and_the_arrows_step_along_it() {
        let mut view = bare_view_of(lock_screen::classic(false));
        set_focus(&mut view, Some(Focus::Bar(BarButton::Cancel)));
        assert_eq!(key(&view, Key::Activate, false), Step::Close);
        assert_eq!(
            key(&view, Key::Right, false),
            Step::Focus(Some(Focus::Bar(BarButton::Preview)))
        );
        assert_eq!(
            key(&view, Key::Left, false),
            Step::Focus(Some(Focus::Bar(BarButton::Cancel))),
            "no wrap at the left"
        );
        set_focus(&mut view, Some(Focus::Bar(BarButton::Preview)));
        assert_eq!(key(&view, Key::Activate, false), Step::Preview);
        set_focus(&mut view, Some(Focus::Bar(BarButton::Done)));
        assert_eq!(key(&view, Key::Activate, false), Step::Done);
        assert_eq!(
            key(&view, Key::Right, false),
            Step::Focus(Some(Focus::Bar(BarButton::Done))),
            "no wrap at the right"
        );
        assert_eq!(focus_name(&view, Focus::Bar(BarButton::Cancel)), "cancel");
        assert_eq!(focus_name(&view, Focus::Bar(BarButton::Preview)), "preview");
        assert_eq!(focus_name(&view, Focus::Bar(BarButton::Done)), "done");
    }

    /// The reader finds the bar's three buttons first, named and placed, then
    /// the parts and the drawer; the focus on a bar button is its item.
    #[test]
    fn the_reader_finds_the_bar_first_and_follows_its_focus() {
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let mut view = bare_view_of(classic.clone());
        let drawer = editor::layout(&view.drawer);
        let place = drawer_rect(view.screen, &drawer);
        let shown = preview(view.screen, &drawer);
        let items = nodes(&view, &at, shown, place, &drawer);
        let placed = at.placed.iter().filter(|r| r.is_some()).count();
        assert_eq!(
            items.len(),
            3 + placed
                + view.drawer.tabs.len()
                + view.drawer.tiles.len()
                + usize::from(has_panel(&view))
        );
        // The bar's three come last, as the ring's do (M5.10c7f).
        let n = items.len();
        let labels: Vec<&str> = items[n - 3..].iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Cancel", "Preview", "Done"]);
        assert_eq!(
            items[n - 2].bounds.x0,
            f64::from(bar(view.screen).preview.x)
        );
        set_focus(&mut view, Some(Focus::Bar(BarButton::Done)));
        assert_eq!(focus_node(&view, &at), Some(n - 1));
        set_focus(&mut view, Some(Focus::Bar(BarButton::Cancel)));
        assert_eq!(focus_node(&view, &at), Some(n - 3));
        let clock = index_of(&classic, "clock");
        set_focus(&mut view, Some(Focus::Part(clock)));
        let among = at.placed[..clock].iter().filter(|r| r.is_some()).count();
        assert_eq!(focus_node(&view, &at), Some(among));
    }

    /// The bar's own places are in the log line CI reads, where the drawer's
    /// Undo and Done were.
    #[test]
    fn the_places_line_names_the_bars_buttons_when_bare() {
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let view = bare_view_of(classic.clone());
        let drawer = editor::layout(&view.drawer);
        let line = places(
            &view,
            &at,
            preview(view.screen, &drawer),
            drawer_rect(view.screen, &drawer),
            &drawer,
        );
        assert!(
            line.contains(", cancel 24+24+88x32, preview 1048+24+112x32, done 1168+24+88x32"),
            "{line}"
        );
        assert!(line.ends_with("x32"), "Done's place ends the line: {line}");
        assert!(line.contains(", tab1 wallpaper "), "{line}");
    }

    /// The new screen (M5.10c7e): the lock with a clock's card open, its
    /// wallpaper panel on the left and the card in the right panel, the bar
    /// and the strip. Written as `lock-stage.png` into `EDEL_LOCK_PNG`.
    /// The strip's previews for `view` (M5.10c7f): each tile of the Widgets
    /// tab drawn as the shell draws it, from the words `when` and a battery at
    /// 82 percent give.
    fn attach_previews(view: &mut View, when: &lock::When, tokens: &Tokens, text: &mut Text) {
        for tile in view.drawer.tiles.iter_mut() {
            let Some(part) = part_named(&tile.name) else {
                continue;
            };
            let words = preview_words(part, when, Some(82));
            let pixmap = preview_picture(part, &words, tokens, text).expect("a preview");
            tile.picture = Some(editor::Picture(std::rc::Rc::new(pixmap)));
        }
    }

    /// The edit mode's whole screen as the shell draws it at 1280 by 800
    /// (M5.10c7f): the lock with its parts, the stage round its picture, the
    /// marks, the bar, the wallpaper panel, the card docked in the right
    /// panel, the strip with its previews; or, previewing, the lock alone with
    /// the preview's pill.
    fn stage_picture(view: &View, when: &lock::When, tokens: &Tokens, text: &mut Text) -> Pixmap {
        let screen = view.screen;
        let drawer = editor::layout(&view.drawer);
        let shown = preview(screen, &drawer);
        let wallpaper = edel::wallpapers::Wallpaper::Background;
        // The corner as the live lock draws it, with its Power and layout
        // buttons at the bottom right (M5.10c7g), kept clear of the parts.
        let corner_view = corner::View {
            layout: Some("EN".into()),
            menu: false,
            restart_waiting: false,
            hover: None,
            session: None,
            chip_w: 0.0,
            sessions: None,
        };
        let corner_at = corner::layout(screen.0, screen.1, &corner_view);
        // The desktop's editor has no corner: its layout is the desktop's.
        let at = match view.on {
            Surface::Desktop => lock::widgets_layout(screen.0, screen.1, tokens, &view.parts, text),
            _ => {
                let look = lock::Look {
                    parts: &view.parts,
                    player: Some(player::wide_size(1)),
                    people: false,
                    keep_out: lock::keep_out(&corner_at),
                    battery: false,
                    background: view.background.clone(),
                    dim: lock_screen::DIM,
                };
                lock::layout(screen.0, screen.1, tokens, true, &look, text)
            }
        };
        let mut lock_view =
            lock::View::new("9:41".into(), "Sunday 11 October".into(), "Ali".into());
        lock_view.when = Some(*when);
        lock_view.zoned = vec![("Asia/Tokyo".to_string(), *when)];
        // Two notifications from two apps, so the count shows, as `render` does.
        lock_view.notices = 2;
        lock_view.apps = vec![
            ("Mail".into(), "mail".into()),
            ("Calendar".into(), "calendar".into()),
        ];
        let (w, h) = (screen.0 as u32, screen.1 as u32);
        let mut lock_pixmap = Pixmap::new(w, h).unwrap();
        match view.on {
            Surface::Desktop => lock::paint_desktop(
                &mut lock_pixmap,
                &lock_view,
                &at,
                tokens,
                text,
                &mut lock::Backdrops::default(),
                &wallpaper,
                1.0,
            ),
            _ => {
                lock::paint(
                    &mut lock_pixmap,
                    &lock_view,
                    &at,
                    tokens,
                    text,
                    None,
                    &mut lock::Pictures::default(),
                    &mut lock::Backdrops::default(),
                    &wallpaper,
                    1.0,
                );
                corner::paint(
                    &mut lock_pixmap,
                    &corner_view,
                    &corner_at,
                    tokens,
                    text,
                    1.0,
                );
            }
        }
        draw_placeholders(&mut lock_pixmap, view, &at, tokens, text, 1.0);
        if view.previewing {
            paint_preview(&mut lock_pixmap, view, tokens, text, 1.0);
            return lock_pixmap;
        }
        let mut pixmap = Pixmap::new(w, h).unwrap();
        let picture = picture_rect(screen, shown);
        edit_kit::stage(
            &mut pixmap,
            Rect::new(0.0, 0.0, screen.0, screen.1),
            picture,
            tokens,
            edit_kit::Style::GLASS,
            1.0,
        );
        edit_kit::stage_frame(
            &mut pixmap,
            &lock_pixmap,
            picture,
            tokens,
            edit_kit::Style::GLASS,
            1.0,
        );
        paint(&mut pixmap, view, &at, shown, tokens, text, 1.0);
        paint_bar(&mut pixmap, view, &bar(screen), tokens, text, 1.0);
        if let Some((left, right)) = panels(screen, shown) {
            if has_panel(view) {
                let thumb = thumbnail(
                    &lock_view,
                    &[],
                    &view.background,
                    lock_screen::DIM,
                    false,
                    tokens,
                    text,
                    None,
                    &mut lock::Pictures::default(),
                    &mut lock::Backdrops::default(),
                    &wallpaper,
                    THUMB,
                    None,
                );
                paint_wallpaper_panel(&mut pixmap, left, view, thumb.as_ref(), tokens, text, 1.0);
            }
            if let Some(card) = view.card.as_ref() {
                let near = at.placed[card.index].expect("the card's part is drawn");
                let l = part_card::layout(card, shown.to_screen(near), screen, tokens, Some(right));
                part_card::paint(&mut pixmap, card, &l, tokens, text, 1.0);
            }
        }
        paint_drawer(&mut pixmap, view, tokens, text, 1.0);
        pixmap
    }

    /// The edit mode drawn as the mockup's frames (M5.10c7e, M5.10c7f): the
    /// stage with a second clock chosen and its card docked (`lock-stage.png`),
    /// the Wallpaper tab (`lock-stage-wallpaper.png`) and the preview
    /// (`lock-preview.png`), written into `EDEL_LOCK_PNG`. Each keeps the bar,
    /// the wallpaper panel and the strip with its live previews.
    #[test]
    fn the_edit_mode_draws_the_mockup_and_writes_its_pictures() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let when = lock::When {
            hour: 9,
            minute: 41,
            weekday: 7,
            day: 11,
            month: 10,
            year: 2026,
        };
        // The look: classic, and a second clock at the top right, analog, small
        // and set to Tokyo, as the mockup shows it.
        let classic = lock_screen::classic(false);
        let mut parts = classic.clone();
        parts.push(Placed {
            part: Part::Clock,
            at: Anchor::TopRight,
            x: 0.0,
            y: 0.0,
            text: None,
            path: None,
            face: Some(lock_screen::Face::Analog),
            hours: None,
            size: Some(lock_screen::Size::Small),
            weight: None,
            show: None,
            style: None,
            zone: Some("Asia/Tokyo".into()),
            label: None,
            colour: None,
        });
        let second = parts.len() - 1;
        let mut view = bare_view_of(parts.clone());
        view.drawer.heading = Some("Classic".into());
        view.drawer.subtitle = Some("Built in · Done saves a new look");
        view.drawer.tiles = drawer_tiles(Surface::LockScreen);
        attach_previews(&mut view, &when, &tokens, &mut text);
        let mut card = part_card::open(second, parts[second].clone());
        card.docked = true;
        card.number = Some(2);
        // A card opened by a click has no keyboard focus, so no ring shows.
        view.card = Some(card);
        let stage = stage_picture(&view, &when, &tokens, &mut text);
        // The bar's Done is white and the strip has its six tiles drawn.
        let done = bar((1280.0, 800.0)).done;
        let px = stage
            .pixel((done.x + 4.0) as u32, (done.y + done.h / 2.0) as u32)
            .expect("inside");
        assert_ne!(px.alpha(), 0, "Done is drawn");
        if let Some(dir) = std::env::var_os("EDEL_LOCK_PNG") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            stage.save_png(dir.join("lock-stage.png")).unwrap();
        }

        // The Wallpaper tab: the backgrounds, each drawn over the classic lock.
        view.card = None;
        view.drawer.tab = 1;
        view.drawer.subtitle = Some("Built in · Done saves a new look");
        view.drawer.tiles = background_tiles(&view.background);
        let mut lock_pictures = lock::Pictures::default();
        let mut backdrops = lock::Backdrops::default();
        let wallpaper = edel::wallpapers::Wallpaper::Background;
        let lock_view = lock::View::new("9:41".into(), "Sunday 11 October".into(), "Ali".into());
        for tile in view.drawer.tiles.iter_mut() {
            let Some((_, _, Some(background))) = background_choices()
                .into_iter()
                .find(|(name, _, _)| *name == tile.name.as_str())
            else {
                continue;
            };
            let thumb = thumbnail(
                &lock_view,
                &classic,
                &background,
                lock_screen::DIM,
                false,
                &tokens,
                &mut text,
                None,
                &mut lock_pictures,
                &mut backdrops,
                &wallpaper,
                THUMB,
                None,
            )
            .expect("a thumbnail");
            tile.picture = Some(editor::Picture(std::rc::Rc::new(thumb)));
        }
        let wallpaper_pixmap = stage_picture(&view, &when, &tokens, &mut text);
        if let Some(dir) = std::env::var_os("EDEL_LOCK_PNG") {
            let dir = std::path::PathBuf::from(dir);
            wallpaper_pixmap
                .save_png(dir.join("lock-stage-wallpaper.png"))
                .unwrap();
        }

        // The preview: the lock over the whole screen with its pill.
        view.drawer.tab = 0;
        view.drawer.tiles = drawer_tiles(Surface::LockScreen);
        attach_previews(&mut view, &when, &tokens, &mut text);
        view.previewing = true;
        let preview_pixmap = stage_picture(&view, &when, &tokens, &mut text);
        let pill = preview_pill((1280.0, 800.0));
        let centre = preview_pixmap
            .pixel((pill.x + pill.w / 2.0) as u32, (pill.y + 4.0) as u32)
            .expect("inside");
        assert_ne!(centre.alpha(), 0, "the pill is drawn at the top");
        if let Some(dir) = std::env::var_os("EDEL_LOCK_PNG") {
            let dir = std::path::PathBuf::from(dir);
            preview_pixmap
                .save_png(dir.join("lock-preview.png"))
                .unwrap();
        }
    }
    /// The Dim slider (M5.10c7g): it lies in the wallpaper panel under the
    /// note, its knob's travel maps a pointer to the dim and back, and the
    /// press's hit area covers it with room above and below.
    #[test]
    fn the_dim_slider_maps_the_pointer_to_the_dim_and_back() {
        let classic = lock_screen::classic(false);
        let mut view = view_of(classic);
        let drawer = editor::layout(&view.drawer);
        let shown = preview(view.screen, &drawer);
        let slider = dim_on_screen(&view, shown).expect("the panel shows the slider");
        let left = panels(view.screen, shown).expect("wide screen").0;
        assert!(
            slider.x > left.x && slider.right() < left.right(),
            "{slider:?}"
        );
        assert!(
            slider.y > left.y && slider.y < left.y + left.h,
            "{slider:?}"
        );
        let knob = edit_kit::KNOB;
        assert_eq!(dim_at(slider, slider.x), 0);
        assert_eq!(dim_at(slider, slider.x + knob / 2.0), 0);
        assert_eq!(
            dim_at(slider, slider.right() - knob / 2.0),
            lock_screen::MOST_DIM
        );
        assert_eq!(dim_at(slider, slider.right()), lock_screen::MOST_DIM);
        let middle = dim_at(slider, slider.x + slider.w / 2.0);
        assert!((38..=42).contains(&middle), "{middle}");
        assert!(dim_hit(slider, slider.x + 4.0, slider.y + slider.h / 2.0));
        assert!(dim_hit(slider, slider.x + 4.0, slider.y - DIM_HIT + 1.0));
        assert!(!dim_hit(slider, slider.x - 1.0, slider.y));
        assert!(!dim_hit(slider, slider.x + 4.0, slider.y - DIM_HIT - 1.0));
        // On a screen with no wallpaper panel there is no slider.
        view.screen = (360.0, 740.0);
        assert_eq!(dim_on_screen(&view, shown), None);
    }

    /// The keyboard reaches the Dim after the parts and steps it by five
    /// percent; a dim the person changes makes the edit changed.
    #[test]
    fn the_dim_takes_the_keys_left_and_right_by_five() {
        let classic = lock_screen::classic(false);
        let mut view = view_of(classic.clone());
        let ring = ring(&view);
        let at = ring
            .iter()
            .position(|f| *f == Focus::Dim)
            .expect("in the ring");
        assert_eq!(at, classic.len(), "after the parts");
        set_focus(&mut view, Some(Focus::Dim));
        assert_eq!(key(&view, Key::Right, false), Step::Dim(DIM_STEP));
        assert_eq!(key(&view, Key::Left, false), Step::Dim(-DIM_STEP));
        assert_eq!(key(&view, Key::Up, false), Step::Nothing);
        assert_eq!(focus_name(&view, Focus::Dim), "dim");
        // Tab steps on from it, to the strip's first tile (its tabs are not in
        // the ring of a bare strip).
        assert_eq!(
            key(&view, Key::Tab, false),
            Step::Focus(Some(Focus::Drawer(editor::Part::Tile(0))))
        );
    }

    /// The reader's node for the slider is a slider named Dim with its value,
    /// placed after the parts.
    #[test]
    fn the_reader_names_the_dim_a_slider_with_its_value() {
        let classic = lock_screen::classic(false);
        let at = at_classic(&classic);
        let mut view = view_of(classic.clone());
        view.dim = 55;
        let drawer = editor::layout(&view.drawer);
        let place = drawer_rect(view.screen, &drawer);
        let shown = preview(view.screen, &drawer);
        let items = nodes(&view, &at, shown, place, &drawer);
        let placed = at.placed.iter().filter(|r| r.is_some()).count();
        let dim = &items[placed];
        assert_eq!(dim.label, "Dim");
        assert_eq!(dim.role, Role::Slider);
        assert_eq!(
            dim.value,
            Some((55.0, 0.0, f64::from(lock_screen::MOST_DIM)))
        );
        set_focus(&mut view, Some(Focus::Dim));
        assert_eq!(focus_node(&view, &at), Some(placed));
        // The log names the slider's place once it is drawn.
        assert!(places(&view, &at, shown, place, &drawer).contains(", dim "));
    }

    /// A dim the person sets is saved in the look like the background is,
    /// unless it is the background's default; a new background moves a dim
    /// the person has not set.
    #[test]
    fn a_dim_is_saved_in_the_look_and_follows_the_background_until_set() {
        let mut view = view_of(lock_screen::classic(false));
        view.dim = 70;
        parts_moved(&mut view);
        let lines = written_for(&view);
        assert_eq!(lines.dim, Some(None), "the person's line goes");
        let text = lines.looks.flatten().expect("a look is saved");
        assert!(text.contains("dim = 70"), "{text}");
        // The default for the background is not written.
        view.dim = lock_screen::DIM;
        view.background = Background::Blurred;
        let lines = written_for(&view);
        assert!(!lines.looks.flatten().unwrap().contains("dim ="));
        // A new background moves the dim while it is the default; a set one stays.
        background_chosen(&mut view, Background::Sharp);
        assert_eq!(view.dim, lock_screen::DIM_SHARP);
        view.dim = 20;
        background_chosen(
            &mut view,
            Background::Colour(edel::tokens::Colour::parse("#336699").unwrap()),
        );
        assert_eq!(view.dim, 20);
    }

    /// Preview (M5.10c7f): the pill sits 16 px from the top and centred, its
    /// Back button inside it, and only Escape and Return go back to the edit;
    /// the other keys do nothing while the lock is shown as it will be.
    #[test]
    fn preview_takes_only_escape_and_return_and_its_pill_sits_at_the_top() {
        let mut view = bare_view_of(lock_screen::classic(false));
        view.previewing = true;
        assert_eq!(key(&view, Key::Escape, false), Step::Back);
        assert_eq!(key(&view, Key::Activate, false), Step::Back);
        assert_eq!(key(&view, Key::Tab, false), Step::Nothing);
        assert_eq!(key(&view, Key::Left, false), Step::Nothing);
        let screen = (1280.0, 800.0);
        let pill = preview_pill(screen);
        assert_eq!(pill.y, 16.0);
        assert_eq!(pill.x + pill.w / 2.0, 640.0, "centred");
        let back = preview_back(screen);
        assert!(pill.contains(back.x + 1.0, back.y + 1.0));
        assert!(pill.contains(back.right() - 1.0, back.y + back.h - 1.0));
        assert_eq!(
            preview_nodes(screen)[0].label,
            "Back to editing",
            "the screen reader finds the Back button"
        );
    }

    /// The bar's Preview and the strip's six kinds (M5.10c7f): a press on a
    /// kind the look holds as many of is refused by the strip, and the Preview
    /// button is the bar's second, between Cancel and Done.
    #[test]
    fn the_bar_has_cancel_preview_and_done_in_that_order() {
        let view = bare_view_of(lock_screen::classic(false));
        let ring = ring(&view);
        assert_eq!(
            ring[ring.len() - 3..],
            [
                Focus::Bar(BarButton::Cancel),
                Focus::Bar(BarButton::Preview),
                Focus::Bar(BarButton::Done),
            ]
        );
        let b = bar((1280.0, 800.0));
        assert!(b.cancel.right() <= b.title.x && b.title.right() <= b.preview.x);
        assert!(b.preview.right() <= b.done.x);
    }

    #[test]
    fn the_desktop_strip_is_the_clock_and_the_date_and_its_edit_has_no_panel() {
        assert_eq!(
            drawer_tiles(Surface::Desktop)
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>(),
            ["clock", "date"]
        );
        assert_eq!(log_name(Surface::Desktop), "desktop editor");
        assert_eq!(log_name(Surface::LockScreen), "lock editor");
        let parts = lock_screen::desktop_default();
        let view = desktop_view_of(parts.clone());
        // No Dim in the ring, the slider or the places line.
        assert!(!ring(&view).contains(&Focus::Dim));
        let at = at_desktop(&parts);
        let drawer = editor::layout(&view.drawer);
        let line = places(
            &view,
            &at,
            preview(view.screen, &drawer),
            drawer_rect(view.screen, &drawer),
            &drawer,
        );
        assert!(line.starts_with("screen 1280x764, part0 clock "), "{line}");
        assert!(
            line.contains(", tile0 clock ") && line.contains(", tile1 date "),
            "{line}"
        );
        assert!(!line.contains("tile2") && !line.contains("dim "), "{line}");
        assert!(!has_panel(&view));
        assert_eq!(drawer_words(&view).1, Some("Yours · Done saves it"));
    }

    #[test]
    fn a_desktop_clock_dragged_to_the_top_left_stands_there_and_says_so() {
        let parts = lock_screen::desktop_default();
        let at = at_desktop(&parts);
        let i = index_of(&parts, "clock");
        let start = centre(at.placed[i].expect("the clock is drawn"));
        let mut view = desktop_view_of(parts.clone());
        view.drag = Some(drag(
            From::Part(i),
            start,
            (start.0 - 1100.0, start.1 - 50.0),
            false,
        ));
        dragged_rect(&mut view, &at);
        let (moved, words) = drop(&view, &at).expect("a moved clock");
        assert_eq!(words, "moved clock to top_left");
        assert_eq!(moved[i].at, Anchor::TopLeft);
        assert!(lock_screen::check_desktop(&moved).is_ok());
    }

    #[test]
    fn the_desktop_edit_mode_draws_and_writes_its_picture() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let when = lock::When {
            hour: 9,
            minute: 41,
            weekday: 7,
            day: 11,
            month: 10,
            year: 2026,
        };
        // The clock moved to the top left, with its card docked on the right.
        let mut parts = lock_screen::desktop_default();
        parts[0].at = Anchor::TopLeft;
        let mut view = desktop_view_of(parts.clone());
        view.drawer.heading = Some("Edit desktop".into());
        view.drawer.subtitle = Some("Yours · Done saves it");
        attach_previews(&mut view, &when, &tokens, &mut text);
        let mut card = part_card::open(0, parts[0].clone());
        card.docked = true;
        view.card = Some(card);
        let stage = stage_picture(&view, &when, &tokens, &mut text);
        let done = bar(view.screen).done;
        let inside = stage
            .pixel(
                (done.x + done.w / 2.0) as u32,
                (done.y + done.h / 2.0) as u32,
            )
            .expect("inside the picture");
        assert_ne!(inside.alpha(), 0, "Done is drawn");
        view.previewing = true;
        let preview_pixmap = stage_picture(&view, &when, &tokens, &mut text);
        if let Some(dir) = std::env::var_os("EDEL_LOCK_PNG") {
            let dir = std::path::PathBuf::from(dir);
            stage.save_png(dir.join("desktop-editor.png")).unwrap();
            preview_pixmap
                .save_png(dir.join("desktop-editor-preview.png"))
                .unwrap();
        }
    }
}
