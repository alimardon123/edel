//! Edit panels (M5.31b): the drawer a panel's menu opens. It is a card of
//! every widget the release has, each by its title and a small picture of
//! it as it is on a panel, the ones already on a panel dimmed, then a
//! hairline and Undo and Done. While it is open the panels show their
//! widgets as tiles (`paint::Look::editing`). Plain data and drawing,
//! tested without a display; `editor_card.rs` owns the surface and the
//! panels. The sizes are the drawer's own: the mockups have none for it.

use std::rc::Rc;

use accesskit::Role;
use edel::i18n::{tr, trf};
use edel::panel_edit::Group;
use edel::tokens::{Colour, Tokens};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use crate::a11y::Item as Node;
use crate::edit_kit;
use crate::lock_editor;
use crate::paint::{EDIT_PAD, Face, LIFTED, Text, fill, outline};
use crate::popup::{self, Card, Rect, wrap};
use crate::trayview::Key;
use crate::widgets::{Canvas, Widget};

/// How far the pointer moves, pressed, before a press becomes a drag, in
/// logical pixels (M5.31b).
pub const DRAG_START: f32 = 6.0;

/// The margin round the drawer's content, logical pixels.
pub const PAD: f32 = 16.0;
/// The drawer's widest on a desktop.
pub const WIDTH: f32 = 560.0;
/// The room the drawer keeps at each side of a screen narrower than it.
const SIDE: f32 = 32.0;
/// A tile's size on a desktop; a compact drawer shrinks its width to fit.
pub const TILE: (f32, f32) = (104.0, 84.0);
/// A tile of a bare strip (the lock screen's, M5.10c7f): 112 by 92 logical
/// pixels, the mockup's, with its live preview and its title stacked in it.
pub const BARE_TILE: (f32, f32) = (112.0, 92.0);
/// A tab's width in a bare drawer (the lock screen's strip), logical pixels
/// (M5.10c7e): each tab's segment is this wide, the tabs centred under the bar.
pub const TAB_W: f32 = 120.0;
/// The bare strip's tabs (M5.10c7f): each segment 32 px high, the track round
/// them `BARE_TAB_PAD` (4 px) wide, its corners `BARE_TRACK_RADIUS`.
const BARE_TAB_H: f32 = 32.0;
const BARE_TAB_PAD: f32 = 4.0;
const BARE_TRACK_RADIUS: f32 = 14.0;
const BARE_SEGMENT_RADIUS: f32 = 10.0;
/// The bare strip's tile (M5.10c7f): its corners, its fill (white at 6 percent,
/// 10 under the pointer), its hairline (white at 10 percent), the gap between
/// its preview and its title, and the title's size.
const BARE_RADIUS: f32 = 16.0;
const BARE_FILL: f32 = 0.06;
const BARE_HOVER: f32 = 0.10;
const BARE_EDGE: f32 = 0.10;
const BARE_GAP: f32 = 6.0;
const BARE_TITLE: f32 = 11.5;
const BARE_TITLE_LINE: f32 = 14.0;
/// The tab words' shares: the others white at 80 percent (the chosen is dark).
const BARE_OTHER_SHARE: f32 = 0.80;
/// The tab track's fill, white at 8 percent, and its hairline at 10.
const BARE_TRACK_FILL: f32 = 0.08;
const BARE_TRACK_EDGE: f32 = 0.10;
const BARE_LINE: f32 = 1.0;
/// The opacity of a tile a panel already holds (its picture and words alike).
const CHOSEN_OPACITY: f32 = 0.5;
/// The room between tiles, rows and columns alike, and between the buttons.
const TILE_GAP: f32 = 10.0;
const GAP: f32 = 8.0;
/// The heading's line, and the hint under it: two lines at most.
const HEADING: f32 = 22.0;
const HINT_GAP: f32 = 4.0;
const HINT_LINE: f32 = 15.0;
const HINT_LINES: usize = 2;
/// A card at least this wide holds the hint on one line.
const ONE_LINE_HINT: f32 = 480.0;

/// How many lines the hint takes in a card of `view`'s width: one on a
/// desktop's card, two on a narrow one.
fn hint_lines(view: &View) -> usize {
    if card_width(view) >= ONE_LINE_HINT {
        1
    } else {
        HINT_LINES
    }
}
/// The room between the hint and the tiles, and between the tiles and the
/// hairline.
const SPACE: f32 = 12.0;
/// The buttons' width, and their height on a desktop and on a touch screen
/// (Compact), logical pixels (`edit_kit`'s).
const BUTTON_WIDTH: f32 = 88.0;
const BUTTON: f32 = edit_kit::BUTTON;
const BUTTON_TOUCH: f32 = edit_kit::BUTTON_TOUCH;
/// Three tiles across on a Compact screen.
const COLUMNS_COMPACT: usize = 3;
/// Where a tile's picture starts from the tile's top, logical pixels.
const PICTURE_TOP: f32 = 10.0;
/// The share of a tile's height its icon or picture takes, the rest going to
/// its title (`edit_kit::tile`'s `ICON_SHARE`).
const PICTURE_SHARE: f32 = 0.6;

/// A picture drawn once and kept (M5.10c6c3), such as a look's thumbnail. It
/// is shared, so two views holding the same one are equal and a change to
/// the view is only what really changed.
#[derive(Clone, Debug)]
pub struct Picture(pub Rc<Pixmap>);

impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// One widget in the drawer, or (M5.10c6c3) a look or a background.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    /// Its name: a widget's in the widget table, a part's, or a look's.
    pub name: String,
    /// The icon its tile shows in place of a picture (the lock screen's
    /// parts), empty for a widget's picture.
    pub icon: &'static str,
    /// Its title as people read it.
    pub title: String,
    /// What it shows now, as its picture shows it.
    pub shown: String,
    /// Whether a panel holds it already, so it is drawn dimmed.
    pub placed: bool,
    /// Whether it is the one in use: a look or a background the lock screen
    /// has now, drawn with the accent's ring (M5.10c6c3).
    pub chosen: bool,
    /// The picture drawn in the tile's top part, in place of a widget's
    /// (a look's thumbnail).
    pub picture: Option<Picture>,
}

/// Which editor the drawer serves (M5.10c6): the panels' (its tiles are the
/// widgets, each drawn as it is on a panel) or the lock screen's (its tiles
/// are the parts, each named by its title alone). The words differ, and the
/// rest of the drawer is the same code.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Panels,
    LockScreen,
}

/// What the drawer shows and where the pointer and the keyboard are.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    /// Which editor the drawer serves (M5.10c6); `Panels` by default.
    pub kind: Kind,
    pub tiles: Vec<Tile>,
    /// The tabs' words, a segmented row under the heading (M5.10c6c3); none
    /// for the panels' drawer.
    pub tabs: Vec<&'static str>,
    /// The tab shown, an index into `tabs`.
    pub tab: usize,
    /// The most tiles any tab holds, so the drawer keeps one height as the
    /// tabs change; 0 for a drawer with no tabs.
    pub tallest: usize,
    /// A Compact screen: touch sizes, three tiles across.
    pub compact: bool,
    /// The screen's width in logical pixels, 0 when unknown.
    pub width: u32,
    pub hover: Option<Part>,
    pub focus: Option<Part>,
    /// A drawer tile being dragged, by its index: drawn at 35 percent.
    pub lifted: Option<usize>,
    /// A widget of a panel is dragged over the drawer: it gets an outline
    /// and the hint says it will be taken off the panel.
    pub taking: bool,
    /// The heading's words when they are not the kind's (M5.10c7a: the look's
    /// title in the lock screen's edit mode); None for the kind's own.
    pub heading: Option<String>,
    /// The hint's words in place of the kind's, when set and no tile is being
    /// dragged over the drawer (M5.10c7a: what Done does to the look).
    pub subtitle: Option<&'static str>,
    /// A bare strip (M5.10c7e, the lock screen's edit mode): no sheet, no
    /// heading, no hint and no footer, as the edit's bar holds those; the tabs
    /// row first at the top, then the tiles, and Undo and Done are not reached.
    pub bare: bool,
}

/// Where a widget dragged to a panel would land there (M5.31b): the group,
/// the widget it lands before (`None`: the group's end), and the caret's
/// place in the panel's logical x.
#[derive(Debug, Clone, PartialEq)]
pub struct Landing {
    pub group: Group,
    pub before: Option<&'static str>,
    pub caret: f32,
}

/// Where a dragged widget would land on a panel (M5.31b). `row` is each
/// widget the panel shows, in order, with its group and its place (left and
/// width, logical pixels from the panel's left); `empty` is each group's
/// place while the panel is edited; `x` is the pointer along the panel;
/// `lifted` is the widget being dragged off it, which is not a place to
/// land by. Inside an empty group's place the widget lands in that group's
/// end. Otherwise the nearest widget decides: before its middle it lands
/// before it, after its middle before the next widget of its group (the
/// group's end when none). `None` when nothing is there to land by.
pub fn landing(
    row: &[(&'static str, Group, f32, f32)],
    empty: [Option<(f32, f32)>; 3],
    x: f32,
    lifted: Option<&str>,
) -> Option<Landing> {
    for (k, place) in empty.iter().enumerate() {
        if let Some((left, width)) = *place {
            if x >= left && x <= left + width {
                return Some(Landing {
                    group: Group::ALL[k],
                    before: None,
                    caret: left + width / 2.0,
                });
            }
        }
    }
    let distance = |left: f32, width: f32| {
        if x < left {
            left - x
        } else if x > left + width {
            x - left - width
        } else {
            0.0
        }
    };
    let (index, _) = row
        .iter()
        .enumerate()
        .filter(|(_, (name, ..))| Some(*name) != lifted)
        .map(|(i, &(_, _, left, width))| (i, distance(left, width)))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    let (name, group, left, width) = row[index];
    if x < left + width / 2.0 {
        return Some(Landing {
            group,
            before: Some(name),
            caret: left,
        });
    }
    let after = row[index + 1..]
        .iter()
        .find(|(other, g, ..)| *g == group && Some(*other) != lifted);
    Some(Landing {
        group,
        before: after.map(|(other, ..)| *other),
        caret: left + width,
    })
}

/// The parts a press or the keyboard reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Tile(usize),
    /// A tab of the drawer, by its index (M5.10c6c3).
    Tab(usize),
    Undo,
    Done,
}

/// The gap inside a tabs' row at each end, logical pixels: the segmented
/// control's inset (`popup`'s).
const TAB_INNER: f32 = 2.0;

/// Where everything lies, logical pixels from the card's top left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub size: (u32, u32),
    pub card: Rect,
    pub tiles: Vec<Rect>,
    pub undo: Rect,
    pub done: Rect,
    /// The tabs' row and each tab's segment (M5.10c6c3); empty for a drawer
    /// with no tabs.
    pub tabs_row: Rect,
    pub tabs: Vec<Rect>,
}

/// What a key asks for: Undo or Done as if pressed, or closing the drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Undo,
    Done,
    Close,
}

/// What a key does: move the focus, act, or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Focus(Part),
    /// Shows tab `i` of the drawer (Left or Right on a focused tab).
    Tab(usize),
    Act(Act),
    Nothing,
}

/// The widget called `name` in the table, if the release has it.
fn widget_of(name: &str) -> Option<&'static Widget> {
    crate::widgets::TABLE.iter().find(|w| w.name == name)
}

/// The drawer's width: the screen's on a Compact screen, else `WIDTH`, or
/// the screen less `SIDE` when that is narrower.
fn card_width(view: &View) -> f32 {
    let screen = view.width as f32;
    if view.bare && screen > 0.0 {
        return screen - 2.0 * crate::lock::MARGIN;
    }
    match (view.compact, screen > 0.0) {
        (true, true) => screen,
        (_, true) => WIDTH.min(screen - SIDE),
        _ => WIDTH,
    }
}

/// How many tiles lie across.
fn columns(view: &View) -> usize {
    if view.compact {
        return COLUMNS_COMPACT;
    }
    let inner = card_width(view) - 2.0 * PAD;
    let tile = if view.bare { BARE_TILE.0 } else { TILE.0 };
    ((inner + GAP) / (tile + GAP)).floor().max(1.0) as usize
}

/// The drawer's size and where each part lies: the one function that
/// places everything. Tiles fill rows from the top, centred across the
/// card; the footer holds Undo and Done at the right.
pub fn layout(view: &View) -> Layout {
    if view.bare {
        return layout_bare(view);
    }
    let width = card_width(view);
    let columns = columns(view);
    let tile_w = tile_width(view, width, columns);
    // The drawer keeps the height of its tallest tab, so changing tabs never
    // makes it jump (M5.10c6c3).
    let counted = view.tiles.len().max(view.tallest);
    let rows = counted.max(1).div_ceil(columns);
    let hint_bottom = PAD + HEADING + HINT_GAP + hint_lines(view) as f32 * HINT_LINE;
    let button = if view.compact { BUTTON_TOUCH } else { BUTTON };
    // The tabs' row sits under the hint, one segment each across the card.
    let inner = width - 2.0 * PAD;
    let (tabs_row, tabs) = if view.tabs.is_empty() {
        (Rect::new(0.0, 0.0, 0.0, 0.0), Vec::new())
    } else {
        let row = Rect::new(PAD, hint_bottom + SPACE, inner, button);
        let segment = (inner - 2.0 * TAB_INNER) / view.tabs.len() as f32;
        popup::segments_at(row, PAD + inner, view.tabs.len(), segment, button)
    };
    let top = if view.tabs.is_empty() {
        hint_bottom + SPACE
    } else {
        tabs_row.y + button + SPACE
    };
    let tiles = place_tiles(view, width, columns, tile_w, top, counted, TILE.1);
    let tiles_bottom = top + rows as f32 * TILE.1 + (rows - 1) as f32 * TILE_GAP;
    // The footer holds the buttons, with the card's padding under them.
    let footer_top = tiles_bottom + SPACE;
    let height = footer_top + button + PAD;
    let y = footer_top;
    let done = Rect::new(width - PAD - BUTTON_WIDTH, y, BUTTON_WIDTH, button);
    let undo = Rect::new(done.x - GAP - BUTTON_WIDTH, y, BUTTON_WIDTH, button);
    Layout {
        size: (width.ceil() as u32, height.ceil() as u32),
        card: Rect::new(0.0, 0.0, width, height),
        tiles,
        undo,
        done,
        tabs_row,
        tabs,
    }
}

/// A tile's width: the desktop's `TILE` width, or on a Compact screen the
/// card's width shared out across the columns.
fn tile_width(view: &View, width: f32, columns: usize) -> f32 {
    if view.compact {
        (width - 2.0 * PAD - (columns - 1) as f32 * TILE_GAP) / columns as f32
    } else if view.bare {
        BARE_TILE.0
    } else {
        TILE.0
    }
}

/// The tiles' places from `top`, `columns` across: each row's tiles centred
/// across the card on their own, so a short row (the last one) sits in the
/// middle too. `counted` is the most tiles any tab holds, which keeps the
/// card's height (`tallest`).
fn place_tiles(
    view: &View,
    width: f32,
    columns: usize,
    tile_w: f32,
    top: f32,
    counted: usize,
    tile_h: f32,
) -> Vec<Rect> {
    let row_left = |row: usize| {
        let n = (counted - row * columns).min(columns) as f32;
        let used = n * tile_w + (n - 1.0) * TILE_GAP;
        ((width - used) / 2.0).round()
    };
    (0..view.tiles.len())
        .map(|i| {
            let (row, column) = (i / columns, i % columns);
            Rect::new(
                row_left(row) + column as f32 * (tile_w + TILE_GAP),
                top + row as f32 * (tile_h + TILE_GAP),
                tile_w,
                tile_h,
            )
        })
        .collect()
}

/// A bare strip's size and places (M5.10c7e): the tabs' row at the top, the
/// tiles under it, then no padding below. Undo and Done have no place here
/// (the edit's bar holds them), so their rectangles are empty at the corner.
fn layout_bare(view: &View) -> Layout {
    let width = card_width(view);
    let columns = columns(view);
    let tile_w = tile_width(view, width, columns);
    let counted = view.tiles.len().max(view.tallest);
    let rows = counted.max(1).div_ceil(columns);
    let (tabs_row, tabs) = if view.tabs.is_empty() {
        (Rect::new(0.0, 0.0, 0.0, 0.0), Vec::new())
    } else {
        let n = view.tabs.len() as f32;
        let w = n * TAB_W + 2.0 * BARE_TAB_PAD;
        let h = BARE_TAB_H + 2.0 * BARE_TAB_PAD;
        let row = Rect::new(((width - w) / 2.0).round(), 0.0, w, h);
        let tabs = (0..view.tabs.len())
            .map(|i| {
                Rect::new(
                    row.x + BARE_TAB_PAD + i as f32 * TAB_W,
                    row.y + BARE_TAB_PAD,
                    TAB_W,
                    BARE_TAB_H,
                )
            })
            .collect();
        (row, tabs)
    };
    let top = if view.tabs.is_empty() {
        0.0
    } else {
        tabs_row.y + tabs_row.h + SPACE
    };
    let tiles = place_tiles(view, width, columns, tile_w, top, counted, BARE_TILE.1);
    let height = top + rows as f32 * BARE_TILE.1 + (rows - 1) as f32 * TILE_GAP;
    let none = Rect::new(0.0, 0.0, 0.0, 0.0);
    Layout {
        size: (width.ceil() as u32, height.ceil() as u32),
        card: Rect::new(0.0, 0.0, width, height),
        tiles,
        undo: none,
        done: none,
        tabs_row,
        tabs,
    }
}

/// The card, with the sheet's corners (`edit_kit::SHEET_RADIUS`), so a
/// popup's shadow follows them.
pub fn cards(l: &Layout, _tokens: &Tokens) -> Vec<Card> {
    vec![Card {
        rect: l.card,
        radius: edit_kit::SHEET_RADIUS,
    }]
}

/// The look an editor is dressed in: the lock screen's glass for its
/// drawer, the desktop's colours for the panels' (`edit_kit::Style`).
pub fn style(kind: Kind) -> edit_kit::Style {
    match kind {
        Kind::LockScreen => edit_kit::Style::GLASS,
        Kind::Panels => edit_kit::Style::DESKTOP,
    }
}

/// The part under `x`, `y` (logical pixels from the card's corner), if
/// any. The padding, the hint and the hairline do nothing.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Part> {
    if l.undo.contains(x, y) {
        return Some(Part::Undo);
    }
    if l.done.contains(x, y) {
        return Some(Part::Done);
    }
    if let Some(i) = l.tabs.iter().position(|t| t.contains(x, y)) {
        return Some(Part::Tab(i));
    }
    l.tiles
        .iter()
        .position(|t| t.contains(x, y))
        .map(Part::Tile)
}

/// The parts in the keyboard's order: the tabs, the tiles, then Undo and Done.
pub fn ring(view: &View) -> Vec<Part> {
    let mut out: Vec<Part> = (0..view.tabs.len()).map(Part::Tab).collect();
    out.extend((0..view.tiles.len()).map(Part::Tile));
    if !view.bare {
        out.push(Part::Undo);
        out.push(Part::Done);
    }
    out
}

/// What a key does with `focus` as the focused part: arrows move between
/// tiles (Up and Down by a row), Undo and Done sit below the last row, Tab
/// cycles the ring, Return acts on Undo or Done, and Escape closes. Return
/// on a tile does nothing yet: dragging a widget comes with the editor's
/// drag (M5.31b).
pub fn key(view: &View, focus: Option<Part>, key: Key) -> Step {
    if key == Key::Escape {
        return Step::Act(Act::Close);
    }
    if key == Key::Delete {
        return Step::Nothing;
    }
    let ring = ring(view);
    if key == Key::Tab {
        let next = match focus.and_then(|f| ring.iter().position(|p| *p == f)) {
            Some(i) => (i + 1) % ring.len(),
            None => 0,
        };
        return Step::Focus(ring[next]);
    }
    let n = view.tiles.len();
    let columns = columns(view);
    let tabs = view.tabs.len();
    match (key, focus) {
        (Key::Activate, Some(Part::Undo)) => Step::Act(Act::Undo),
        (Key::Activate, Some(Part::Done)) => Step::Act(Act::Done),
        (_, None) => ring.first().map_or(Step::Nothing, |p| Step::Focus(*p)),
        (Key::Left, Some(Part::Tab(i))) => Step::Tab(i.saturating_sub(1)),
        (Key::Right, Some(Part::Tab(i))) => Step::Tab((i + 1).min(tabs.saturating_sub(1))),
        (Key::Down, Some(Part::Tab(_))) if n > 0 => Step::Focus(Part::Tile(0)),
        (Key::Left, Some(Part::Tile(i))) => Step::Focus(Part::Tile(i.saturating_sub(1))),
        (Key::Right, Some(Part::Tile(i))) => {
            Step::Focus(Part::Tile((i + 1).min(n.saturating_sub(1))))
        }
        (Key::Up, Some(Part::Tile(i))) => Step::Focus(Part::Tile(i.saturating_sub(columns))),
        (Key::Down, Some(Part::Tile(i))) if i + columns < n => Step::Focus(Part::Tile(i + columns)),
        (Key::Down, Some(Part::Tile(_))) if !view.bare => Step::Focus(Part::Undo),
        (Key::Left | Key::Up, Some(Part::Done)) => Step::Focus(Part::Undo),
        (Key::Right | Key::Down, Some(Part::Undo)) => Step::Focus(Part::Done),
        (Key::Left | Key::Up, Some(Part::Undo)) if n > 0 => Step::Focus(Part::Tile(n - 1)),
        _ => Step::Nothing,
    }
}

/// Where the parts lie, as one log line CI reads:
/// `card WxH, tile NAME X+Y+WxH, ..., undo X+Y+WxH, done X+Y+WxH`, logical
/// pixels from the card's corner.
pub fn places(l: &Layout, view: &View) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut out = format!("card {}x{}", l.size.0, l.size.1);
    for (i, (tab, rect)) in view.tabs.iter().zip(&l.tabs).enumerate() {
        out.push_str(&format!(", tab{i} {} {}", tab.to_lowercase(), at(*rect)));
    }
    for (tile, rect) in view.tiles.iter().zip(&l.tiles) {
        out.push_str(&format!(", tile {} {}", tile.name, at(*rect)));
    }
    if !view.bare {
        out.push_str(&format!(", undo {}, done {}", at(l.undo), at(l.done)));
    }
    out
}

/// A tile's picture (M5.31b): the widget drawn whole into a pixmap its
/// natural width by `panel_control` high, then laid into the tile's `layer`
/// centred in its top part, scaled down to fit the tile less `EDIT_PAD` at
/// each side when it is wider (never up), so nothing spills out of a tile.
fn picture(
    layer: &mut Pixmap,
    widget: &Widget,
    shown: &str,
    tokens: &Tokens,
    text: Option<&mut Text>,
    icons: Option<&mut edel::app_icons::Icons>,
    s: f32,
) {
    let (mut text, mut icons) = (text, icons);
    let height = tokens.panel_control as f32 * s;
    let top = PICTURE_TOP * s;
    let natural = {
        let mut canvas = Canvas {
            pixmap: layer,
            tokens,
            text: text.as_deref_mut(),
            icons: icons.as_deref_mut(),
            scale: s,
            top,
            height,
            dock: false,
            along_top: false,
        };
        (widget.width)(&mut canvas, shown)
    };
    let Some(mut piece) = Pixmap::new(natural.ceil().max(1.0) as u32, height.ceil() as u32) else {
        return;
    };
    {
        let mut canvas = Canvas {
            pixmap: &mut piece,
            tokens,
            text,
            icons,
            scale: s,
            top: 0.0,
            height,
            dock: false,
            along_top: false,
        };
        (widget.draw)(&mut canvas, shown, 0.0);
    }
    let lw = layer.width() as f32;
    let room = lw - 2.0 * EDIT_PAD * s;
    let fit = (room / natural.max(1.0)).min(1.0);
    let (w, h) = (natural * fit, height * fit);
    let x = ((lw - w) / 2.0).round();
    let y = top + ((height - h) / 2.0).round();
    let paint = PixmapPaint {
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    layer.draw_pixmap(
        0,
        0,
        piece.as_ref(),
        &paint,
        Transform::from_row(fit, 0.0, 0.0, fit, x, y),
        None,
    );
}

/// A look's picture (M5.10c6c3) laid into a tile's picture area: scaled down
/// to fit it, centred, never up, so the tile never spills. `layer` is the
/// tile's own pixmap and `w`, `h` its size in logical pixels.
fn thumbnail(layer: &mut Pixmap, picture: &Pixmap, w: f32, h: f32, s: f32) {
    let top = PICTURE_TOP * s;
    let room_w = w * s - 2.0 * EDIT_PAD * s;
    let room_h = h * PICTURE_SHARE * s - top - 4.0 * s;
    let fit = (room_w / picture.width() as f32).min(room_h / picture.height() as f32);
    if fit <= 0.0 {
        return;
    }
    let x = ((w * s - picture.width() as f32 * fit) / 2.0).round();
    let y = top + ((room_h - picture.height() as f32 * fit) / 2.0).round();
    let paint = PixmapPaint {
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    layer.draw_pixmap(
        0,
        0,
        picture.as_ref(),
        &paint,
        Transform::from_row(fit, 0.0, 0.0, fit, x, y),
        None,
    );
}

/// The words a drawer's hint says (`paint`): the panels' and the lock
/// screen's, and on the lock screen's tabs what their tiles do.
pub(crate) fn hint_words(view: &View) -> &'static str {
    if let (Some(words), false) = (view.subtitle, view.taking) {
        return words;
    }
    match (view.kind, view.taking) {
        (Kind::Panels, true) => tr("Drop here to take it off the panel"),
        (Kind::Panels, false) => tr(
            "Drag a widget along the panels or onto another, from here to add it, or here to take it away",
        ),
        (Kind::LockScreen, true) => tr("Drop here to take it away"),
        (Kind::LockScreen, false) if view.tab == 2 => {
            tr("Choose a look, which brings its widgets and its wallpaper")
        }
        (Kind::LockScreen, false) if view.tab == 1 => {
            tr("Choose what lies behind the lock screen's widgets")
        }
        (Kind::LockScreen, false) => {
            tr("Drag a widget anywhere, from here to add it, or here to take it away")
        }
    }
}

/// Draws `view` at `s` into `pixmap`, which is the card's size times it: the
/// sheet (`edit_kit::sheet`, glass or desktop by `style`), outlined when a
/// panel's widget is dragged over it, the heading and its hint (at most two
/// lines), each tile (`edit_kit::tile`: its icon or a panel's picture, its
/// title, at half strength when a panel holds it and at 35 percent while it
/// is dragged), then Undo and Done as the kit's buttons.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    l: &Layout,
    tokens: &Tokens,
    icons: Option<&mut edel::app_icons::Icons>,
    text: Option<&mut Text>,
    s: f32,
) {
    let mut icons = icons;
    let mut text = text;
    let style = style(view.kind);
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    if !view.bare {
        edit_kit::sheet(pixmap, l.card, tokens, style, s);
    }
    if view.taking {
        let (x, y, w, h) = l.card.device(s);
        outline(
            pixmap,
            (x, y, w, h),
            edit_kit::SHEET_RADIUS * style.scale * s,
            2.0 * s,
            tokens.accent,
        );
    }
    let heading = (tokens.panel_text_size + 2) as f32 * s;
    let small = tokens.panel_text_small_size as f32 * s;
    if view.bare {
        // The strip has no heading or hint: the edit's bar holds them.
    } else if let Some(text) = text.as_deref_mut() {
        let heading_words = match (&view.heading, view.kind) {
            (Some(words), _) => words.as_str(),
            (None, Kind::Panels) => tr("Edit panels"),
            (None, Kind::LockScreen) => tr("Edit lock screen"),
        };
        let mut line = text.line_in(heading_words, heading, Face::SEMIBOLD);
        let y = popup::middle(PAD, HEADING, heading, s);
        let ink = edit_kit::inks(style, tokens).text;
        text.draw(pixmap, &mut line, PAD * s, y, ink);
    }
    let hint_words = hint_words(view);
    let room = l.card.w - 2.0 * PAD;
    let lines = match text.as_deref_mut() {
        _ if view.bare => Vec::new(),
        Some(text) => wrap(hint_words, hint_lines(view), room * s, |t| {
            text.line(t, small).width
        }),
        None => vec![hint_words.to_string()],
    };
    for (i, words) in lines.iter().enumerate() {
        let top = PAD + HEADING + HINT_GAP + i as f32 * HINT_LINE;
        let rect = Rect::new(PAD, top, room, HINT_LINE);
        edit_kit::hint(pixmap, words, rect, tokens, text.as_deref_mut(), style, s);
    }
    if view.bare && !view.tabs.is_empty() {
        paint_bare_tabs(pixmap, view, l, tokens, text.as_deref_mut(), s);
        if let Some(Part::Tab(i)) = view.focus {
            if let Some(segment) = l.tabs.get(i) {
                popup::focus_ring(pixmap, *segment, tokens.radius_small as f32, s, tokens);
            }
        }
    } else if !view.tabs.is_empty() {
        let ink = edit_kit::inks(style, tokens);
        popup::segmented_with(
            pixmap,
            text.as_deref_mut(),
            (l.tabs_row, &l.tabs),
            &view.tabs,
            Some(view.tab),
            (ink.raised, ink.text),
            tokens,
            s,
        );
        if let Some(Part::Tab(i)) = view.focus {
            if let Some(segment) = l.tabs.get(i) {
                popup::focus_ring(pixmap, *segment, tokens.radius_small as f32, s, tokens);
            }
        }
    }
    for (i, (tile, rect)) in view.tiles.iter().zip(&l.tiles).enumerate() {
        let (x, y, w, h) = rect.device(s);
        let Some(mut layer) = Pixmap::new(w.round() as u32, h.round() as u32) else {
            continue;
        };
        let part = Part::Tile(i);
        // The focus ring lies outside the tile, so it is drawn on the card
        // below, not inside the tile's own layer (where it would be cut).
        let focused = view.focus == Some(part);
        let state = if view.hover == Some(part) || focused {
            edit_kit::State::Hover
        } else {
            edit_kit::State::Rest
        };
        if view.bare {
            // The strip's own tile (M5.10c7f): its preview and title stacked.
            let preview = tile.picture.as_ref().map(|Picture(p)| p.as_ref());
            let hover = state == edit_kit::State::Hover;
            bare_tile(
                &mut layer,
                rect.w,
                rect.h,
                &tile.title,
                preview,
                hover,
                tokens,
                text.as_deref_mut(),
                s,
            );
        } else {
            let tile_rect = Rect::new(0.0, 0.0, rect.w, rect.h);
            edit_kit::tile(
                &mut layer,
                tile_rect,
                tile.icon,
                &tile.title,
                state,
                false,
                tokens,
                text.as_deref_mut(),
                style,
                s,
            );
            // A panel's tile shows the widget as it is on a panel; a lock
            // screen's tile has its icon and title only.
            if let Some(Picture(picture)) = &tile.picture {
                thumbnail(&mut layer, picture, rect.w, rect.h, s);
            }
            if let (Kind::Panels, Some(widget)) = (view.kind, widget_of(&tile.name)) {
                picture(
                    &mut layer,
                    widget,
                    &tile.shown,
                    tokens,
                    text.as_deref_mut(),
                    icons.as_deref_mut(),
                    s,
                );
            }
        }
        let opacity = if view.lifted == Some(i) {
            LIFTED
        } else if tile.placed {
            CHOSEN_OPACITY
        } else {
            1.0
        };
        let paint = PixmapPaint {
            opacity,
            ..PixmapPaint::default()
        };
        pixmap.draw_pixmap(
            x as i32,
            y as i32,
            layer.as_ref(),
            &paint,
            Transform::identity(),
            None,
        );
        if focused || tile.chosen {
            popup::focus_ring(pixmap, *rect, edit_kit::TILE_RADIUS, s, tokens);
        }
    }
    for (part, rect, words, kind) in [
        (Part::Undo, l.undo, tr("Undo"), edit_kit::ButtonKind::Quiet),
        (Part::Done, l.done, tr("Done"), edit_kit::ButtonKind::Accent),
    ]
    .into_iter()
    .filter(|_| !view.bare)
    {
        let state = if view.focus == Some(part) {
            edit_kit::State::Focus
        } else if view.hover == Some(part) {
            edit_kit::State::Hover
        } else {
            edit_kit::State::Rest
        };
        edit_kit::button(
            pixmap,
            rect,
            words,
            kind,
            state,
            tokens,
            text.as_deref_mut(),
            style,
            s,
        );
    }
}

/// The bare strip's tabs (M5.10c7f), as the mockup's segmented control: a track
/// of white at 8 percent with a 1 px hairline of white at 10, the chosen tab a
/// white pill with dark words (`lock_edit_deep`, semibold), the others regular
/// at 80 percent white. The keyboard's focus ring is drawn by the caller.
fn paint_bare_tabs(
    pixmap: &mut Pixmap,
    view: &View,
    l: &Layout,
    tokens: &Tokens,
    text: Option<&mut Text>,
    s: f32,
) {
    let white = tokens.backdrop_text;
    let (x, y, w, h) = l.tabs_row.device(s);
    fill(
        pixmap,
        x,
        y,
        w,
        h,
        BARE_TRACK_RADIUS * s,
        Colour {
            a: BARE_TRACK_FILL,
            ..white
        },
    );
    edit_kit::stroke(
        pixmap,
        (x, y, w, h),
        BARE_TRACK_RADIUS * s,
        BARE_LINE * s,
        Colour {
            a: BARE_TRACK_EDGE,
            ..white
        },
        None,
    );
    if let Some(segment) = l.tabs.get(view.tab) {
        let (x, y, w, h) = segment.device(s);
        fill(pixmap, x, y, w, h, BARE_SEGMENT_RADIUS * s, white);
    }
    let Some(text) = text else {
        return;
    };
    let size = tokens.panel_text_size as f32 * s;
    for (j, (word, segment)) in view.tabs.iter().zip(&l.tabs).enumerate() {
        let chosen = j == view.tab;
        let (face, colour) = if chosen {
            (Face::SEMIBOLD, tokens.lock_edit_deep)
        } else {
            (
                Face::REGULAR,
                Colour {
                    a: BARE_OTHER_SHARE,
                    ..white
                },
            )
        };
        let mut line = text.fit_in(word, size, segment.w * s, face);
        let x = segment.x * s + ((segment.w * s - line.width) / 2.0).round();
        let y = popup::middle(segment.y, segment.h, size, s);
        text.draw(pixmap, &mut line, x, y, colour);
    }
}

/// A bare strip's tile (M5.10c7f, the mockup's strip) drawn into its own layer
/// of `w` by `h` logical pixels: a rounded tile of white at 6 percent (10 under
/// the pointer) with a 1 px hairline of white at 10 percent, and its live
/// preview and its title stacked and centred together, `BARE_GAP` apart, the
/// title 11.5 px regular. The preview is drawn at `PREVIEW_SCALE`, so it is
/// scaled back to its own box of `PREVIEW_BOX`.
#[allow(clippy::too_many_arguments)]
fn bare_tile(
    layer: &mut Pixmap,
    w: f32,
    h: f32,
    title: &str,
    preview: Option<&Pixmap>,
    hover: bool,
    tokens: &Tokens,
    text: Option<&mut Text>,
    s: f32,
) {
    let white = tokens.backdrop_text;
    let (x, y, dw, dh) = Rect::new(0.0, 0.0, w, h).device(s);
    let share = if hover { BARE_HOVER } else { BARE_FILL };
    let fill_colour = Colour { a: share, ..white };
    fill(layer, x, y, dw, dh, BARE_RADIUS * s, fill_colour);
    let edge = Colour {
        a: BARE_EDGE,
        ..white
    };
    edit_kit::stroke(
        layer,
        (x, y, dw, dh),
        BARE_RADIUS * s,
        BARE_LINE * s,
        edge,
        None,
    );
    let (box_w, box_h) = lock_editor::PREVIEW_BOX;
    let (preview_h, gap) = if preview.is_some() {
        (box_h, BARE_GAP)
    } else {
        (0.0, 0.0)
    };
    let group = preview_h + gap + BARE_TITLE_LINE;
    let top = ((h - group) / 2.0).round();
    if let Some(preview) = preview {
        // The preview's pixmap is drawn at `PREVIEW_SCALE`; this puts it at its
        // own box, centred across the tile.
        let k = s / lock_editor::PREVIEW_SCALE;
        let left = ((w - box_w) / 2.0).round();
        let transform = Transform::from_row(k, 0.0, 0.0, k, left * s, top * s);
        let paint = PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        };
        layer.draw_pixmap(0, 0, preview.as_ref(), &paint, transform, None);
    }
    let Some(text) = text else {
        return;
    };
    let size = BARE_TITLE * s;
    let mut line = text.fit_in(title, size, (w - 2.0 * BARE_GAP) * s, Face::REGULAR);
    let at = ((w * s - line.width) / 2.0).round();
    let baseline = popup::middle(top + preview_h + gap, BARE_TITLE_LINE, size, s);
    text.draw(layer, &mut line, at, baseline, white);
}

/// What a screen reader finds: each tile as a button, named by its title
/// and whether a panel holds it, then Undo and Done.
pub fn nodes(view: &View, l: &Layout) -> Vec<Node> {
    let at = |r: Rect| {
        accesskit::Rect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.y + r.h),
        )
    };
    let button = |label: String, bounds| Node {
        role: Role::Button,
        label,
        bounds,
        children: Vec::new(),
        toggled: None,
        value: None,
        value_text: None,
    };
    // The tabs first, each with its words, then the tiles.
    let tabs: Vec<Node> = view
        .tabs
        .iter()
        .zip(&l.tabs)
        .map(|(tab, rect)| Node {
            role: Role::Tab,
            label: tab.to_string(),
            bounds: at(*rect),
            children: Vec::new(),
            toggled: None,
            value: None,
            value_text: None,
        })
        .collect();
    let mut out: Vec<Node> = tabs;
    out.extend(view.tiles.iter().zip(&l.tiles).map(|(tile, rect)| {
        let label = if tile.placed {
            trf("{title}, on a panel", &[("title", &tile.title)])
        } else if tile.chosen {
            trf("{title}, in use", &[("title", &tile.title)])
        } else {
            trf("Add {title}", &[("title", &tile.title)])
        };
        button(label, at(*rect))
    }));
    if !view.bare {
        out.push(button(tr("Undo").into(), at(l.undo)));
        out.push(button(tr("Done").into(), at(l.done)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::tokens::Scheme;

    fn view(n: usize, width: u32, compact: bool) -> View {
        View {
            kind: Kind::Panels,
            tabs: Vec::new(),
            tab: 0,
            tallest: 0,
            tiles: (0..n)
                .map(|i| Tile {
                    icon: "",
                    name: "clock".into(),
                    title: format!("Widget {i}"),
                    shown: String::new(),
                    placed: i == 0,
                    chosen: false,
                    picture: None,
                })
                .collect(),
            compact,
            width,
            hover: None,
            focus: None,
            lifted: None,
            taking: false,
            heading: None,
            subtitle: None,
            bare: false,
        }
    }

    #[test]
    fn a_desktop_drawer_is_four_tiles_across_and_centred_in_560() {
        let l = layout(&view(6, 1280, false));
        assert_eq!(l.size.0, 560);
        // Four tiles and three gaps, centred in the 560 px card.
        let left = ((560.0 - (4.0 * TILE.0 + 3.0 * TILE_GAP)) / 2.0).round();
        assert_eq!(
            l.tiles[0],
            Rect::new(left, 69.0, TILE.0, TILE.1),
            "the hint on one line"
        );
        assert_eq!(l.tiles[3].x, left + 3.0 * (TILE.0 + TILE_GAP));
        assert_eq!(l.tiles[4].y, 69.0 + TILE.1 + TILE_GAP, "the second row");
    }

    #[test]
    fn a_compact_drawer_is_the_screen_wide_with_three_tiles_across() {
        let l = layout(&view(7, 360, true));
        assert_eq!(l.size.0, 360);
        // Three tiles fill the screen less the padding and the gaps.
        let tile_w = (360.0 - 2.0 * PAD - 2.0 * TILE_GAP) / 3.0;
        assert_eq!(l.tiles[0], Rect::new(PAD, 84.0, tile_w, TILE.1));
        assert_eq!(l.tiles[2].x, PAD + 2.0 * (tile_w + TILE_GAP));
        assert_eq!(l.done.h, BUTTON_TOUCH, "touch-sized buttons");
    }

    #[test]
    fn a_narrow_desktop_keeps_32_px_at_the_sides() {
        assert_eq!(layout(&view(1, 500, false)).size.0, 468);
    }

    #[test]
    fn hit_finds_a_tile_undo_and_done_and_nothing_in_the_padding() {
        let l = layout(&view(6, 1280, false));
        let tile = l.tiles[1];
        assert_eq!(hit(&l, tile.x + 1.0, tile.y + 1.0), Some(Part::Tile(1)));
        assert_eq!(hit(&l, l.undo.x + 1.0, l.undo.y + 1.0), Some(Part::Undo));
        assert_eq!(hit(&l, l.done.x + 1.0, l.done.y + 1.0), Some(Part::Done));
        assert_eq!(hit(&l, 2.0, 2.0), None, "the padding");
        assert_eq!(hit(&l, tile.x - 2.0, tile.y), None, "the gap beside a tile");
    }

    #[test]
    fn keys_move_between_tiles_and_the_footer_and_act() {
        let v = view(6, 1280, false);
        assert_eq!(
            key(&v, Some(Part::Tile(0)), Key::Right),
            Step::Focus(Part::Tile(1))
        );
        assert_eq!(
            key(&v, Some(Part::Tile(0)), Key::Down),
            Step::Focus(Part::Tile(4))
        );
        assert_eq!(
            key(&v, Some(Part::Tile(4)), Key::Down),
            Step::Focus(Part::Undo)
        );
        assert_eq!(
            key(&v, Some(Part::Undo), Key::Right),
            Step::Focus(Part::Done)
        );
        assert_eq!(
            key(&v, Some(Part::Done), Key::Tab),
            Step::Focus(Part::Tile(0)),
            "Tab wraps"
        );
        assert_eq!(
            key(&v, Some(Part::Undo), Key::Activate),
            Step::Act(Act::Undo)
        );
        assert_eq!(
            key(&v, Some(Part::Done), Key::Activate),
            Step::Act(Act::Done)
        );
        assert_eq!(key(&v, Some(Part::Tile(0)), Key::Activate), Step::Nothing);
        assert_eq!(key(&v, None, Key::Left), Step::Focus(Part::Tile(0)));
        assert_eq!(
            key(&v, Some(Part::Tile(2)), Key::Escape),
            Step::Act(Act::Close)
        );
    }

    /// A panel's row for the landing tests: a start group of `menu` and
    /// `windows`, and the end group's `clock`, with their places.
    fn row() -> Vec<(&'static str, Group, f32, f32)> {
        vec![
            ("menu", Group::Start, 0.0, 40.0),
            ("windows", Group::Start, 40.0, 100.0),
            ("clock", Group::End, 1200.0, 60.0),
        ]
    }

    const NO_EMPTY: [Option<(f32, f32)>; 3] = [None, None, None];

    #[test]
    fn a_drop_before_the_first_widget_lands_before_it() {
        let l = landing(&row(), NO_EMPTY, 5.0, None).unwrap();
        assert_eq!(
            l,
            Landing {
                group: Group::Start,
                before: Some("menu"),
                caret: 0.0
            }
        );
    }

    #[test]
    fn a_drop_after_the_last_of_a_group_lands_at_its_end() {
        let l = landing(&row(), NO_EMPTY, 130.0, None).unwrap();
        assert_eq!(
            l,
            Landing {
                group: Group::Start,
                before: None,
                caret: 140.0
            },
            "after windows' middle, the start group's end"
        );
        let l = landing(&row(), NO_EMPTY, 1230.0, None).unwrap();
        assert_eq!(l.group, Group::End);
        assert_eq!(l.before, None);
    }

    #[test]
    fn a_drop_inside_an_empty_group_lands_in_it_at_its_middle() {
        let mut empty = NO_EMPTY;
        empty[1] = Some((600.0, 40.0));
        let l = landing(&row(), empty, 610.0, None).unwrap();
        assert_eq!(
            l,
            Landing {
                group: Group::Centre,
                before: None,
                caret: 620.0
            }
        );
    }

    #[test]
    fn the_lifted_widget_is_not_a_place_to_land_by() {
        let r = vec![
            ("menu", Group::Start, 0.0, 40.0),
            ("windows", Group::Start, 40.0, 40.0),
        ];
        // Over windows, which is lifted: menu is the nearest, and after its
        // middle the group's end is the next widget left, none.
        let l = landing(&r, NO_EMPTY, 60.0, Some("windows")).unwrap();
        assert_eq!(
            l,
            Landing {
                group: Group::Start,
                before: None,
                caret: 40.0
            }
        );
    }

    #[test]
    fn a_gap_between_groups_goes_to_the_nearer_widget() {
        let r = vec![
            ("menu", Group::Start, 0.0, 40.0),
            ("clock", Group::End, 100.0, 40.0),
        ];
        let before_clock = landing(&r, NO_EMPTY, 80.0, None).unwrap();
        assert_eq!(
            before_clock,
            Landing {
                group: Group::End,
                before: Some("clock"),
                caret: 100.0
            },
            "80 is 20 from the clock and 40 from the menu"
        );
        let after_menu = landing(&r, NO_EMPTY, 60.0, None).unwrap();
        assert_eq!(after_menu.group, Group::Start, "60 is 20 from the menu");
        assert_eq!(after_menu.caret, 40.0);
    }

    #[test]
    fn an_empty_row_gives_no_landing() {
        assert_eq!(landing(&[], NO_EMPTY, 10.0, None), None);
        assert_eq!(landing(&[], NO_EMPTY, 10.0, Some("menu")), None);
    }

    #[test]
    fn places_name_every_tile_and_the_footer() {
        let v = view(2, 1280, false);
        let text = places(&layout(&v), &v);
        assert!(text.starts_with("card 560x"), "{text}");
        assert_eq!(text.matches(", tile clock ").count(), 2, "{text}");
        assert!(text.contains(", undo "), "{text}");
        assert!(text.contains(", done "), "{text}");
    }

    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    #[test]
    fn it_draws_light_and_dark_and_writes_pngs() {
        let Some(mut text) = fonts() else {
            return; // no fonts on this machine
        };
        // Classic's widgets are placed; Search is not, and Apps has no tile here.
        let panels = edel::presets::named(Some("classic")).0.panels;
        let placed: Vec<String> = panels
            .iter()
            .flat_map(|p| p.widgets())
            .map(String::from)
            .collect();
        let live = crate::widgets::Live::default();
        for (scheme, mode) in [(Scheme::Light, "light"), (Scheme::Dark, "dark")] {
            let tokens = Tokens::built_in_scheme(scheme);
            let tiles = crate::widgets::TABLE
                .iter()
                .filter(|w| w.name != "apps" && w.name != "search")
                .map(|w| Tile {
                    icon: "",
                    name: w.name.into(),
                    title: tr(w.title).to_string(),
                    shown: (w.shows)(&live),
                    placed: placed.iter().any(|p| p.as_str() == w.name),
                    chosen: false,
                    picture: None,
                })
                .collect();
            let v = View {
                kind: Kind::Panels,
                tabs: Vec::new(),
                tab: 0,
                tallest: 0,
                tiles,
                compact: false,
                width: 1280,
                hover: None,
                focus: Some(Part::Done),
                lifted: None,
                taking: false,
                heading: None,
                subtitle: None,
                bare: false,
            };
            let l = layout(&v);
            let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
            paint(&mut pixmap, &v, &l, &tokens, None, Some(&mut text), 2.0);
            let alpha = |x: f32, y: f32| {
                pixmap
                    .pixel((x * 2.0) as u32, (y * 2.0) as u32)
                    .unwrap()
                    .alpha()
            };
            assert_eq!(alpha(0.0, 0.0), 0, "{mode}: a round corner");
            let t = l.tiles[0];
            assert_ne!(alpha(t.x + 3.0, t.y + 3.0), 0, "{mode}: the first tile");
            if let Some(dir) = std::env::var_os("EDEL_EDITOR_PNG") {
                let dir = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&dir).unwrap();
                pixmap
                    .save_png(dir.join(format!("editor-{mode}.png")))
                    .unwrap();
            }
        }
    }

    /// The lock screen's drawer (M5.10c6): its heading and hint are the lock
    /// screen's, each tile is its title alone (no picture), and it lays out
    /// and paints like the panels' drawer. Written as `editor-lock.png` with
    /// `EDEL_EDITOR_PNG`.
    #[test]
    fn the_lock_screens_drawer_lays_out_and_paints_its_titles() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let tiles = vec![
            Tile {
                icon: "",
                name: "message".into(),
                title: "Message".into(),
                shown: String::new(),
                placed: false,
                chosen: false,
                picture: None,
            },
            Tile {
                icon: "",
                name: "battery".into(),
                title: "Battery".into(),
                shown: String::new(),
                placed: false,
                chosen: false,
                picture: None,
            },
        ];
        let v = View {
            kind: Kind::LockScreen,
            tabs: Vec::new(),
            tab: 0,
            tallest: 0,
            tiles,
            compact: false,
            width: 1280,
            hover: None,
            focus: Some(Part::Tile(0)),
            lifted: None,
            taking: false,
            heading: None,
            subtitle: None,
            bare: false,
        };
        let l = layout(&v);
        let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
        paint(&mut pixmap, &v, &l, &tokens, None, Some(&mut text), 2.0);
        let t = l.tiles[0];
        let alpha = |x: f32, y: f32| {
            pixmap
                .pixel((x * 2.0) as u32, (y * 2.0) as u32)
                .unwrap()
                .alpha()
        };
        assert_ne!(alpha(t.x + 3.0, t.y + 3.0), 0, "the first tile");
        assert!(
            places(&l, &v).contains(", tile message "),
            "the places name each tile by its part: {}",
            places(&l, &v)
        );
        if let Some(dir) = std::env::var_os("EDEL_EDITOR_PNG") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            pixmap.save_png(dir.join("editor-lock.png")).unwrap();
        }
    }

    /// A lock screen drawer of `tiles` tiles, its tabs and the tab shown.
    fn lock_drawer(tiles: usize, tab: usize, tallest: usize) -> View {
        View {
            kind: Kind::LockScreen,
            tiles: (0..tiles)
                .map(|i| Tile {
                    name: format!("tile{i}"),
                    icon: "",
                    title: format!("Tile {i}"),
                    shown: String::new(),
                    placed: false,
                    chosen: false,
                    picture: None,
                })
                .collect(),
            tabs: vec!["Widgets", "Looks", "Wallpaper"],
            tab,
            tallest,
            compact: false,
            width: 1280,
            hover: None,
            focus: None,
            lifted: None,
            taking: false,
            heading: None,
            subtitle: None,
            bare: false,
        }
    }

    #[test]
    fn the_lock_screen_drawer_has_tabs_and_the_panels_drawer_none() {
        let lock = lock_drawer(2, 0, 0);
        let l = layout(&lock);
        assert_eq!(l.tabs.len(), 3, "one segment for each tab");
        let middle = l.tabs[1];
        assert_eq!(
            hit(&l, middle.x + middle.w / 2.0, middle.y + middle.h / 2.0),
            Some(Part::Tab(1))
        );
        assert_eq!(ring(&lock)[..3], [Part::Tab(0), Part::Tab(1), Part::Tab(2)]);
        assert!(
            layout(&view(2, 1280, false)).tabs.is_empty(),
            "the panels have none"
        );
        assert!(
            places(&l, &lock).contains(", tab1 looks "),
            "{}",
            places(&l, &lock)
        );
    }

    #[test]
    fn left_and_right_step_the_tab_and_the_tabs_come_first_in_the_ring() {
        let lock = lock_drawer(2, 0, 0);
        assert_eq!(key(&lock, Some(Part::Tab(0)), Key::Right), Step::Tab(1));
        assert_eq!(
            key(&lock, Some(Part::Tab(2)), Key::Right),
            Step::Tab(2),
            "no wrap"
        );
        assert_eq!(
            key(&lock, Some(Part::Tab(0)), Key::Left),
            Step::Tab(0),
            "no wrap"
        );
        assert_eq!(key(&lock, Some(Part::Tab(1)), Key::Left), Step::Tab(0));
        assert_eq!(key(&lock, None, Key::Tab), Step::Focus(Part::Tab(0)));
        assert_eq!(
            key(&lock, Some(Part::Tab(2)), Key::Down),
            Step::Focus(Part::Tile(0))
        );
    }

    #[test]
    fn the_drawer_keeps_the_height_of_its_tallest_tab_on_every_tab() {
        let parts = layout(&lock_drawer(2, 0, 8));
        let looks = layout(&lock_drawer(4, 1, 8));
        let background = layout(&lock_drawer(8, 2, 0));
        assert_eq!(parts.size, looks.size);
        assert_eq!(parts.size, background.size);
        assert!(parts.size.1 > layout(&lock_drawer(2, 0, 0)).size.1);
    }
}
