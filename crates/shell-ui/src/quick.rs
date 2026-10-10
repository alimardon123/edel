//! Quick settings (M5.9a): the card the status area opens above itself, a
//! sheet across the screen at Compact width. Its look is the fifth round of
//! mockups' laptop board (`docs/mockups/shell/laptop.jpg`), drawn from the
//! tokens: a card of one square grid, then
//!
//! - **tiles** on the grid: a round toggle is one cell with its title under
//!   it; a pill (Wi-Fi, Bluetooth) is two cells wide, its round icon at the
//!   left and, behind it, its page (name, state and an arrow) that opens
//!   the tile's Settings page; a right click on a round toggle with a page
//!   opens it too;
//! - the **shelf**: a hairline, the brightness slider (M5.9c, where the
//!   machine has a backlight, full width, no chevron), the volume slider
//!   with its number in it, and a chevron that opens the list of outputs
//!   (with "Sound settings" last);
//! - a **footer**: the battery's pill with its charge and time, and the
//!   Settings button at the right.
//!
//! Which tiles, and in what order, is the preset's `[quick] tiles`
//! (`edel::presets::Quick`); a tile whose feature or hardware the machine
//! lacks is left out. Everything here is plain data and drawing, tested
//! without a display: [`tiles`] and [`View`] say what shows, [`layout`]
//! where it lies, [`paint`] draws it, [`hit`], [`key`] and [`volume_at`]
//! turn the pointer and the keyboard into an [`Act`], and [`items`] are
//! what a screen reader reads. `main.rs` owns the surface, runs the
//! commands off the drawing thread and keeps the card's buffers only while
//! it is open.

use accesskit::Role;
use edel::i18n::{tr, trf};
use edel::tokens::{Colour, Tokens};
use tiny_skia::Pixmap;

use crate::a11y::Item;
use crate::paint::{self, Face, Text, fill, lit, mix, outline};
use crate::popup::{self, Card, Rect, dim, icon_in, knob, raised, veil};
use crate::status::{Link, Status};

/// The card's width on a screen of any size from Compact up, logical
/// pixels; below `COMPACT_BELOW` it is as wide as the screen.
pub const WIDTH: u32 = 352;
/// A screen narrower than this is Compact (M5.6c's size classes): the
/// card is a sheet across the screen.
pub const COMPACT_BELOW: u32 = 600;
/// The card's corners, as the mockups draw them.
pub const RADIUS: f32 = 50.0;
/// The most outputs the list shows.
pub const MOST_OUTPUTS: usize = 6;
/// A step of the volume on the keyboard, percent.
pub const STEP: u32 = 5;
/// The Settings page the Dark style tile's arrow opens: Appearance, where
/// the colour scheme (`appearance.mode`) is set (M5.2q).
pub const DARK_PAGE: &str = "appearance";
/// The Settings page the list's last row opens.
pub const SOUND_PAGE: &str = "sound";
/// The Displays page of Settings, which a right click on the brightness opens (M5.9c).
pub const DISPLAYS_PAGE: &str = "displays";
/// The player's card's height and the room between it and quick settings'
/// card, logical pixels (M5.9d): above the card on a bottom panel, below
/// it on a top one.
pub const PLAYER_HEIGHT: f32 = 100.0;
pub const PLAYER_GAP: f32 = 8.0;
/// The cover's side, and the room between the player's parts.
const COVER: f32 = 64.0;
const PLAYER_SPACE: f32 = 10.0;
/// The title and artist's box, its height.
const WORDS: f32 = 36.0;

/// A tile of the card: the names are what `[quick] tiles` lists
/// (`edel::presets::TILES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Wifi,
    Bluetooth,
    Airplane,
    /// A plain toggle of `notifications.do_not_disturb` (M5.9b), with no
    /// page behind an arrow: its row is in the notification centre.
    DoNotDisturb,
    DarkStyle,
}

/// Every tile, in the order `edel::presets::TILES` lists them.
pub const ALL: [Tile; 5] = [
    Tile::Wifi,
    Tile::Bluetooth,
    Tile::Airplane,
    Tile::DoNotDisturb,
    Tile::DarkStyle,
];

impl Tile {
    pub fn name(self) -> &'static str {
        match self {
            Tile::Wifi => "wifi",
            Tile::Bluetooth => "bluetooth",
            Tile::Airplane => "airplane",
            Tile::DoNotDisturb => "do_not_disturb",
            Tile::DarkStyle => "dark_style",
        }
    }

    pub fn from_name(name: &str) -> Option<Tile> {
        ALL.into_iter().find(|t| t.name() == name)
    }

    pub fn title(self) -> &'static str {
        match self {
            Tile::Wifi => tr("Wi-Fi"),
            Tile::Bluetooth => tr("Bluetooth"),
            Tile::Airplane => tr("Airplane mode"),
            Tile::DoNotDisturb => tr("Do not disturb"),
            Tile::DarkStyle => tr("Dark style"),
        }
    }

    /// The word under a round toggle, as the mockups have it.
    pub fn short(self) -> &'static str {
        match self {
            Tile::Wifi => tr("Wi-Fi"),
            Tile::Bluetooth => tr("Bluetooth"),
            Tile::Airplane => tr("Airplane"),
            Tile::DoNotDisturb => tr("Do not disturb"),
            Tile::DarkStyle => tr("Dark"),
        }
    }

    /// Whether the tile is a pill, two cells wide, rather than a round
    /// toggle: the networks, whose state needs words.
    pub fn pill(self) -> bool {
        matches!(self, Tile::Wifi | Tile::Bluetooth)
    }

    /// The shell's icon in its circle.
    pub fn icon(self) -> &'static str {
        match self {
            Tile::Wifi => "net-wifi-3",
            Tile::Bluetooth => "page-bluetooth",
            Tile::Airplane => "airplane",
            Tile::DoNotDisturb => "do-not-disturb",
            Tile::DarkStyle => "moon",
        }
    }

    /// The Settings page its arrow opens, `edel-settings --page NAME`;
    /// none for a tile that has no page of its own.
    pub fn page(self) -> Option<&'static str> {
        match self {
            Tile::Wifi => Some("network"),
            Tile::Bluetooth => Some("bluetooth"),
            Tile::Airplane | Tile::DoNotDisturb => None,
            Tile::DarkStyle => Some(DARK_PAGE),
        }
    }
}

/// Whether a tile has a page part beside its round icon: a pill with a
/// page. Only such a part is on the card and in Tab's order.
fn has_page(tile: Tile) -> bool {
    tile.pill() && tile.page().is_some()
}

/// One tile as the card shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct TileView {
    pub tile: Tile,
    /// The line under its name: "Home 5G", "Off".
    pub state: String,
    pub on: bool,
}

/// The volume part.
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeView {
    pub percent: u32,
    pub muted: bool,
    /// The output in use.
    pub output: String,
    /// Every output, with the one in use marked.
    pub outputs: Vec<(String, bool)>,
}

/// The footer's battery.
#[derive(Debug, Clone, PartialEq)]
pub struct BatteryView {
    pub percent: u32,
    pub charging: bool,
    /// The time to empty or to full, "4 h 10 min", when known.
    pub time: Option<String>,
}

/// The player (M5.9d) as the card shows it: what plays, which of its
/// buttons work and its cover, decoded to a square picture when it has one.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerView {
    pub title: String,
    pub artist: String,
    /// The app's name, whose first letter is the cover when there is none.
    pub identity: String,
    pub playing: bool,
    pub can_previous: bool,
    pub can_next: bool,
    pub cover: Option<Pixmap>,
}

/// Where a pointer or the keyboard is on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The round toggle, or a pill's icon part, of tile `i`
    Toggle(usize),
    /// A pill's page part of tile `i`
    Page(usize),
    /// The brightness slider, above the volume's (M5.9c)
    Light,
    /// The slider
    Slider,
    /// The chevron that opens the list of outputs
    Sound,
    /// Row `i` of that list
    Choose(usize),
    /// The "Sound settings" row under the list
    SoundPage,
    Settings,
    /// The player's previous, play or pause and next buttons (M5.9d)
    Previous,
    PlayPause,
    Next,
}

/// Everything the card shows at one moment, so it is drawn again only when
/// it changes.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The card's width, logical pixels, and whether it is a sheet, with
    /// touch sizes.
    pub width: u32,
    pub compact: bool,
    pub tiles: Vec<TileView>,
    /// The screen's brightness in percent, none without a backlight (M5.9c).
    pub brightness: Option<u32>,
    pub volume: Option<VolumeView>,
    pub battery: Option<BatteryView>,
    /// Whether the Settings button is there.
    pub settings: bool,
    /// Whether the list of outputs is open.
    pub list: bool,
    /// Where the keyboard is, once a key was pressed, and the pointer.
    pub focus: Option<Focus>,
    pub hover: Option<Focus>,
    /// What plays, when something does (M5.9d), and whether the card hangs
    /// from a panel along the top, so the player goes below it.
    pub player: Option<PlayerView>,
    pub player_below: bool,
}

/// What a person's click or key asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    Toggle(usize),
    Page(usize),
    /// Open or close the list of outputs.
    List,
    Choose(usize),
    /// Mute or unmute (the slider's Space or Return).
    Mute,
    /// The volume to this percent.
    Volume(u32),
    /// The brightness to this percent (M5.9c).
    Brightness(u32),
    /// Open the Sound page of Settings.
    SoundPage,
    /// Open the Displays page of Settings (M5.9c).
    DisplaysPage,
    Settings,
    /// Ask the player to do this MPRIS method: `PlayPause`, `Previous` or
    /// `Next` (M5.9d). The card stays open.
    Player(&'static str),
    Close,
}

/// A key as the card understands it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Escape,
    /// Tab, with whether Shift is held
    Tab(bool),
    Left,
    Right,
    Up,
    Down,
    /// Space or Return
    Activate,
    Home,
    End,
}

// ---- What shows ----

/// The tiles `names` lists that this machine can show, in order, with
/// what each says now: a tile needs the thing it switches (the Wi-Fi tile
/// a Wi-Fi adapter, the Bluetooth tile an adapter, Airplane mode either)
/// and one named twice or not known is left out. `switches` says whether
/// the colour scheme is dark and whether banners are kept away, which
/// are lines of the settings file; `pending` holds what a person just
/// chose and the machine has not yet said, which shows at once.
pub fn tiles(
    names: &[String],
    status: &Status,
    switches: Switches,
    pending: &[(Tile, bool)],
) -> Vec<TileView> {
    let on = |tile: Tile, now: bool| {
        pending
            .iter()
            .rev()
            .find(|(t, _)| *t == tile)
            .map_or(now, |(_, on)| *on)
    };
    let switch = |on: bool| if on { tr("On") } else { tr("Off") }.to_string();
    let mut shown: Vec<TileView> = Vec::new();
    for name in names {
        let Some(tile) = Tile::from_name(name) else {
            continue;
        };
        if shown.iter().any(|t| t.tile == tile) {
            continue;
        }
        let view = match tile {
            Tile::Wifi => {
                let Some(network) = status.network.as_ref().filter(|n| n.has_wifi) else {
                    continue;
                };
                let is_on = on(tile, network.wifi_on);
                let state = match (&network.link, is_on) {
                    (_, false) => tr("Off").to_string(),
                    (Link::Wifi { name, .. }, true) if network.wifi_on => name.clone(),
                    (_, true) if network.wifi_on => tr("Not connected").to_string(),
                    _ => tr("On").to_string(),
                };
                TileView {
                    tile,
                    state,
                    on: is_on,
                }
            }
            Tile::Bluetooth => {
                let Some(powered) = status.bluetooth else {
                    continue;
                };
                let is_on = on(tile, powered);
                TileView {
                    tile,
                    state: switch(is_on),
                    on: is_on,
                }
            }
            Tile::Airplane => {
                let radios = status.has_wifi() || status.bluetooth.is_some();
                if !radios {
                    continue;
                }
                let is_on = on(tile, status.airplane());
                TileView {
                    tile,
                    state: switch(is_on),
                    on: is_on,
                }
            }
            Tile::DoNotDisturb => TileView {
                tile,
                state: switch(switches.dnd),
                on: switches.dnd,
            },
            Tile::DarkStyle => TileView {
                tile,
                state: switch(switches.dark),
                on: switches.dark,
            },
        };
        shown.push(view);
    }
    shown
}

/// The volume part of the card: the output in use and the list of all.
/// `percent` and `muted` are what the slider was just moved to or muted
/// at, which show over what the sound system last said.
pub fn volume(status: &Status, percent: Option<u32>, muted: Option<bool>) -> Option<VolumeView> {
    let v = status.volume.as_ref()?;
    Some(VolumeView {
        percent: percent.unwrap_or(v.percent),
        muted: muted.unwrap_or(v.muted),
        output: v.output.clone(),
        outputs: status
            .outputs
            .iter()
            .take(MOST_OUTPUTS)
            .map(|o| (o.name.clone(), o.default))
            .collect(),
    })
}

/// The two tiles that are lines of the person's settings file: whether
/// the colour scheme is dark and whether banners are kept away.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Switches {
    pub dark: bool,
    pub dnd: bool,
}

/// What the open card keeps besides what the machine says: its width, the
/// colour scheme, whether the list of outputs is open, where the keyboard
/// and the pointer are, what a person just chose and the machine has not
/// yet confirmed, and the volume a drag has reached.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub width: u32,
    pub compact: bool,
    pub dark: bool,
    pub dnd: bool,
    pub list: bool,
    pub focus: Option<Focus>,
    pub hover: Option<Focus>,
    pub pending: Vec<(Tile, bool)>,
    pub volume: Option<u32>,
    pub muted: Option<bool>,
    /// The brightness a drag has reached, as `volume` is (M5.9c).
    pub brightness: Option<u32>,
}

/// The card showing `status` and `state`, with the tiles `names` lists
/// (the preset's); `settings` says whether the machine has the Settings
/// app.
pub fn view(state: &State, status: &Status, names: &[String], settings: bool) -> View {
    View {
        width: state.width,
        compact: state.compact,
        tiles: tiles(
            names,
            status,
            Switches {
                dark: state.dark,
                dnd: state.dnd,
            },
            &state.pending,
        ),
        // What a drag has reached shows over what the screen says, as the volume does.
        brightness: status.brightness.map(|now| state.brightness.unwrap_or(now)),
        volume: volume(status, state.volume, state.muted),
        battery: status.battery.as_ref().map(|b| BatteryView {
            percent: b.percent,
            charging: b.charging,
            time: b.time.clone(),
        }),
        settings,
        list: state.list,
        focus: state.focus,
        hover: state.hover,
        player: None,
        player_below: false,
    }
}

/// The key of the settings file Dark style writes.
pub const MODE: &str = "appearance.mode";

/// Whether the colour scheme is dark: the person's file over the
/// machine's, `auto` and no key being the release's choice, light.
pub fn is_dark(machine: Option<&str>, person: Option<&str>) -> bool {
    edel::settings::chosen(MODE, machine, person)
        .as_deref()
        .and_then(edel::tokens::Scheme::parse)
        == Some(edel::tokens::Scheme::Dark)
}

/// What to write in the person's file to make the colour scheme dark, or
/// light when `dark` is false: none when what applies without their file
/// (the machine's, else the release's) is already that, as writers never
/// write a default (ADR-008).
pub fn mode_to_write(machine: Option<&str>, dark: bool) -> Option<&'static str> {
    (is_dark(machine, None) != dark).then_some(if dark { "dark" } else { "light" })
}

// ---- Where it lies ----

/// The sizes of the card's parts, logical pixels. The same on every screen:
/// a Compact card is as wide as the screen and keeps these sizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Padding left and right of the grid and the shelf.
    pub side: f32,
    /// Padding above the grid and below the footer.
    pub top: f32,
    pub bottom: f32,
    /// A round toggle's diameter, a grid cell's side.
    pub cell: f32,
    /// Between two columns.
    pub gap: f32,
    /// Between two rows of the grid.
    pub row_gap: f32,
    /// Extra height under a round toggle for its title.
    pub label: f32,
    /// The icon circle inside a pill.
    pub pill_icon: f32,
    /// A shelf slider's height, the chevron button, the footer's buttons
    /// and the battery pill's height.
    pub bar: f32,
    /// Between the grid, the shelf and the footer.
    pub section: f32,
    /// The glyph in a round toggle, and in a pill's icon circle.
    pub icon: f32,
    pub pill_glyph: f32,
}

/// The sizes of the card's parts, the mockups': the same at every width
/// but the bar-high parts (the sliders, their buttons, the list's rows
/// and the footer's), which are 44 px on a Compact screen, where a finger
/// lands.
pub fn metrics(compact: bool) -> Metrics {
    Metrics {
        side: 18.0,
        top: 12.0,
        bottom: 18.0,
        cell: 64.0,
        gap: 20.0,
        row_gap: 10.0,
        label: 10.0,
        pill_icon: 38.0,
        bar: if compact { 44.0 } else { 36.0 },
        section: 10.0,
        icon: 18.0,
        pill_glyph: 16.0,
    }
}

/// The columns of the grid at `width`: as many cells as fit beside the
/// side paddings, at most four and at least two.
pub fn columns(width: u32) -> usize {
    let m = metrics(false);
    let fit = ((width as f32 - 2.0 * m.side + m.gap) / (m.cell + m.gap)).floor();
    (fit.max(0.0) as usize).clamp(2, 4)
}

/// The size the card may grow to with the list of outputs open, so its
/// buffers do not change size when it opens.
pub fn most_size(size: (u32, u32)) -> (u32, u32) {
    // The Compact rows, the taller, so one size serves both.
    let m = metrics(true);
    let rows = MOST_OUTPUTS as u32 + 1;
    // And the player's card, so one appearing never needs a bigger pool.
    let player = (PLAYER_HEIGHT + PLAYER_GAP) as u32;
    (
        size.0,
        size.1 + player + rows * (m.bar as u32 + LIST_GAP as u32),
    )
}

/// The space between a pill's left edge and its icon circle, logical pixels.
const PILL_INSET: f32 = 8.0;
/// Between the rows of the list of outputs, logical pixels.
const LIST_GAP: f32 = 4.0;
/// The room under the grid's last titles before the shelf's hairline,
/// beside the sections' own, and under the hairline, as the mockups.
const GRID_BELOW: f32 = 6.0;
const RULE_ROOM: f32 = 17.0;
/// The hairline's inset from the card's sides.
const RULE_INSET: f32 = 10.0;
/// Between the slider's track and the chevron button.
const TRACK_GAP: f32 = 10.0;
/// How far the Settings button stands in from the card's right edge.
const SETTINGS_INSET: f32 = 14.0;
/// The text of a list row starts this far from its left edge.
const ROW_TEXT: f32 = 14.0;
/// Between the brightness slider and the volume's, as the mockups have it.
const SLIDER_GAP: f32 = 10.0;

/// One tile's parts. A round toggle's `toggle` is its whole cell; a pill's
/// `toggle` is its left part with the icon circle, and its `page` the rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileBox {
    pub whole: Rect,
    pub toggle: Rect,
    pub page: Option<Rect>,
}

/// Where everything of a [`View`] lies.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub size: (u32, u32),
    pub metrics: Metrics,
    pub tiles: Vec<TileBox>,
    /// The player's card (M5.9d), quick settings' own card (which moves
    /// below the player's when the player is above it), and the player's
    /// parts: its cover, the words, and the three buttons.
    pub player: Option<Rect>,
    pub quick: Rect,
    pub cover: Option<Rect>,
    pub words: Option<Rect>,
    pub previous: Option<Rect>,
    pub play: Option<Rect>,
    pub next: Option<Rect>,
    /// The brightness slider's track, full width, above the volume's when
    /// the machine has a backlight (M5.9c).
    pub light: Option<Rect>,
    /// The slider's track, and the chevron button beside it.
    pub track: Option<Rect>,
    pub sound: Option<Rect>,
    /// The rows of the list of outputs while it is open, and the row
    /// "Sound settings" after them.
    pub list: Vec<Rect>,
    pub sound_page: Option<Rect>,
    /// The hairline above the shelf, at its height.
    pub rule: Option<f32>,
    pub battery: Option<Rect>,
    pub settings: Option<Rect>,
}

/// Starts a section below what is above it, unless nothing is above it yet.
fn section(y: &mut f32, started: &mut bool, m: &Metrics) {
    if *started {
        *y += m.section;
    }
    *started = true;
}

/// Where the parts of `view` lie: the grid of tiles, then the shelf and
/// the footer, each after a section.
pub fn layout(view: &View) -> Layout {
    let m = metrics(view.compact);
    let w = view.width as f32;
    let cols = columns(view.width);
    let inner = w - 2.0 * m.side;
    let grid = cols as f32 * m.cell + (cols - 1) as f32 * m.gap;
    let left = (w - grid) / 2.0;
    // Each tile's row and first column: a pill takes two cells, and one
    // that does not fit in what is left of a row starts the next row.
    let (mut row, mut col) = (0usize, 0usize);
    let mut at = Vec::with_capacity(view.tiles.len());
    for t in &view.tiles {
        let span = if t.tile.pill() { 2 } else { 1 };
        if col + span > cols {
            row += 1;
            col = 0;
        }
        at.push((row, col));
        col += span;
    }
    let rows = if at.is_empty() { 0 } else { row + 1 };
    // A row holding a round toggle has room under the circles for titles.
    let mut heights = vec![m.cell; rows];
    for (t, &(r, _)) in view.tiles.iter().zip(&at) {
        if !t.tile.pill() {
            heights[r] = m.cell + m.label;
        }
    }
    let mut tops = Vec::with_capacity(rows);
    // A player above the card moves all of the card down by its height and the gap.
    let above = view.player.is_some() && !view.player_below;
    let shift = if above {
        PLAYER_HEIGHT + PLAYER_GAP
    } else {
        0.0
    };
    let mut y = m.top + shift;
    for h in &heights {
        tops.push(y);
        y += h + m.row_gap;
    }
    if rows > 0 {
        y -= m.row_gap;
    }
    let mut tiles = Vec::with_capacity(view.tiles.len());
    for (t, &(r, c)) in view.tiles.iter().zip(&at) {
        let x = left + c as f32 * (m.cell + m.gap);
        let top = tops[r];
        let b = if t.tile.pill() {
            let whole = Rect::new(x, top, 2.0 * m.cell + m.gap, m.cell);
            if has_page(t.tile) {
                let toggle = Rect::new(x, top, PILL_INSET + m.pill_icon, m.cell);
                let page = Rect::new(x + toggle.w, top, whole.w - toggle.w, m.cell);
                TileBox {
                    whole,
                    toggle,
                    page: Some(page),
                }
            } else {
                TileBox {
                    whole,
                    toggle: whole,
                    page: None,
                }
            }
        } else {
            let whole = Rect::new(x, top, m.cell, m.cell);
            TileBox {
                whole,
                toggle: whole,
                page: None,
            }
        };
        tiles.push(b);
    }
    let mut started = !view.tiles.is_empty();
    let (mut rule, mut light, mut track, mut sound, mut sound_page) =
        (None, None, None, None, None);
    let mut list = Vec::new();
    if view.volume.is_some() || view.brightness.is_some() {
        if started {
            y += GRID_BELOW;
        }
        section(&mut y, &mut started, &m);
        rule = Some(y);
        y += 1.0 + RULE_ROOM;
        // The brightness slider lies above the volume's, full width, with no
        // chevron beside it, as the mockups have it (M5.9c).
        if view.brightness.is_some() {
            light = Some(Rect::new(m.side, y, inner, m.bar));
            y += m.bar + SLIDER_GAP;
        }
        if let Some(volume) = &view.volume {
            track = Some(Rect::new(m.side, y, inner - m.bar - TRACK_GAP, m.bar));
            sound = Some(Rect::new(m.side + inner - m.bar, y, m.bar, m.bar));
            y += m.bar;
            if view.list {
                section(&mut y, &mut started, &m);
                for i in 0..volume.outputs.len() + 1 {
                    let r = Rect::new(m.side, y, inner, m.bar);
                    if i < volume.outputs.len() {
                        list.push(r);
                    } else {
                        sound_page = Some(r);
                    }
                    y += m.bar + LIST_GAP;
                }
                y -= LIST_GAP;
            }
        }
    }
    let (mut battery, mut settings) = (None, None);
    if view.battery.is_some() || view.settings {
        section(&mut y, &mut started, &m);
        if view.battery.is_some() {
            battery = Some(Rect::new(m.side, y, (inner - m.gap) / 2.0, m.bar));
        }
        if view.settings {
            settings = Some(Rect::new(
                m.side + inner - SETTINGS_INSET - m.bar,
                y,
                m.bar,
                m.bar,
            ));
        }
        y += m.bar;
    }
    y += m.bottom;
    let end = y.ceil();
    let quick = Rect::new(0.0, shift, w, end - shift);
    // The player's card, and its parts centred on its middle (M5.9d). The
    // three buttons are 44 px on a Compact screen, where a finger lands.
    let (mut player, mut cover, mut words, mut previous, mut play, mut next) =
        (None, None, None, None, None, None);
    let mut height = end;
    if view.player.is_some() {
        let p = if above {
            Rect::new(0.0, 0.0, w, PLAYER_HEIGHT)
        } else {
            height = end + PLAYER_GAP + PLAYER_HEIGHT;
            Rect::new(0.0, end + PLAYER_GAP, w, PLAYER_HEIGHT)
        };
        let middle = |side: f32| p.y + (PLAYER_HEIGHT - side) / 2.0;
        let pad = m.side;
        let small = if view.compact { 44.0 } else { 28.0 };
        let large = if view.compact { 44.0 } else { 32.0 };
        let c = Rect::new(pad, middle(COVER), COVER, COVER);
        let n = Rect::new(w - pad - small, middle(small), small, small);
        let pl = Rect::new(n.x - PLAYER_SPACE - large, middle(large), large, large);
        let pr = Rect::new(pl.x - PLAYER_SPACE - small, middle(small), small, small);
        let left = c.right() + PLAYER_SPACE;
        words = Some(Rect::new(
            left,
            p.y + (PLAYER_HEIGHT - WORDS) / 2.0,
            (pr.x - PLAYER_SPACE - left).max(0.0),
            WORDS,
        ));
        cover = Some(c);
        previous = Some(pr);
        play = Some(pl);
        next = Some(n);
        player = Some(p);
    }
    Layout {
        size: (view.width, height as u32),
        metrics: m,
        tiles,
        player,
        quick,
        cover,
        words,
        previous,
        play,
        next,
        light,
        track,
        sound,
        list,
        sound_page,
        rule,
        battery,
        settings,
    }
}

/// Where the card's parts lie, as one log line CI reads to click them:
/// `card WxH, NAME X+Y+WxH, ..., track X+Y+WxH, sound ..., settings ...`,
/// logical pixels from the card's corner; each tile by its name and its
/// whole box, then the brightness slider (`light`), the volume's track,
/// the chevron (`sound`) and the `settings` button.
pub fn places(view: &View, layout: &Layout) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut parts = vec![format!("card {}x{}", layout.size.0, layout.size.1)];
    if let Some(player) = layout.player {
        parts.push(format!("player {}", at(player)));
        for (name, rect) in [
            ("previous", layout.previous),
            ("play", layout.play),
            ("next", layout.next),
        ] {
            if let Some(rect) = rect {
                parts.push(format!("{name} {}", at(rect)));
            }
        }
    }
    for (t, b) in view.tiles.iter().zip(&layout.tiles) {
        parts.push(format!("{} {}", t.tile.name(), at(b.whole)));
    }
    for (name, rect) in [
        ("light", layout.light),
        ("track", layout.track),
        ("sound", layout.sound),
        ("settings", layout.settings),
    ] {
        if let Some(rect) = rect {
            parts.push(format!("{name} {}", at(rect)));
        }
    }
    parts.join(", ")
}

/// The part of the card at `x`, `y` (logical pixels from its corner). The
/// player's buttons come first, and Previous and Next only where the
/// player can do them (M5.9d).
pub fn hit(view: &View, layout: &Layout, x: f32, y: f32) -> Option<Focus> {
    if let Some(player) = &view.player {
        let at = |r: Option<Rect>| r.is_some_and(|r| r.contains(x, y));
        if player.can_previous && at(layout.previous) {
            return Some(Focus::Previous);
        }
        if at(layout.play) {
            return Some(Focus::PlayPause);
        }
        if player.can_next && at(layout.next) {
            return Some(Focus::Next);
        }
    }
    for (i, t) in layout.tiles.iter().enumerate() {
        if t.page.is_some_and(|p| p.contains(x, y)) {
            return Some(Focus::Page(i));
        }
        if t.toggle.contains(x, y) {
            return Some(Focus::Toggle(i));
        }
    }
    if layout.sound.is_some_and(|r| r.contains(x, y)) {
        return Some(Focus::Sound);
    }
    if let Some(i) = layout.list.iter().position(|r| r.contains(x, y)) {
        return Some(Focus::Choose(i));
    }
    if layout.sound_page.is_some_and(|r| r.contains(x, y)) {
        return Some(Focus::SoundPage);
    }
    if layout.light.is_some_and(|r| r.contains(x, y)) {
        return Some(Focus::Light);
    }
    if layout.track.is_some_and(|r| r.contains(x, y)) {
        return Some(Focus::Slider);
    }
    if layout.settings.is_some_and(|r| r.contains(x, y)) {
        return Some(Focus::Settings);
    }
    None
}

/// The percent at `x` along a slider's `track`, 0 to 100: the bar fills
/// from its left end to the pointer.
pub(crate) fn percent_at(track: Option<Rect>, x: f32) -> u32 {
    let Some(track) = track else {
        return 0;
    };
    let share = ((x - track.x) / track.w.max(1.0)).clamp(0.0, 1.0);
    (share * 100.0).round() as u32
}

/// The volume at `x` along the slider, percent, 0 to 100.
pub fn volume_at(layout: &Layout, x: f32) -> u32 {
    percent_at(layout.track, x)
}

/// The brightness at `x` along its slider, percent, 0 to 100 (M5.9c).
pub fn brightness_at(layout: &Layout, x: f32) -> u32 {
    percent_at(layout.light, x)
}

// ---- Keys ----

/// What a keyboard can reach, in Tab's order: the player's buttons (those
/// it can do), each tile's toggle and, for a pill, its page; the slider,
/// the chevron, the list's rows while it is open, and the Settings button.
pub fn ring(view: &View) -> Vec<Focus> {
    let mut ring = Vec::new();
    if let Some(player) = &view.player {
        if player.can_previous {
            ring.push(Focus::Previous);
        }
        ring.push(Focus::PlayPause);
        if player.can_next {
            ring.push(Focus::Next);
        }
    }
    for (i, t) in view.tiles.iter().enumerate() {
        ring.push(Focus::Toggle(i));
        if has_page(t.tile) {
            ring.push(Focus::Page(i));
        }
    }
    if view.brightness.is_some() {
        ring.push(Focus::Light);
    }
    if view.volume.is_some() {
        ring.push(Focus::Slider);
        ring.push(Focus::Sound);
        if let Some(volume) = &view.volume {
            if view.list {
                ring.extend((0..volume.outputs.len()).map(Focus::Choose));
                ring.push(Focus::SoundPage);
            }
        }
    }
    if view.settings {
        ring.push(Focus::Settings);
    }
    ring
}

/// What `key` does with the keyboard on `focus`: where it goes and what
/// it asks for. The first key to move gives the first part, or with Shift
/// and Up or Left the last. Up and Down move by a row of the grid.
pub fn key(view: &View, focus: Option<Focus>, key: Key) -> (Option<Focus>, Option<Act>) {
    let ring = ring(view);
    if key == Key::Escape {
        return (focus, Some(if view.list { Act::List } else { Act::Close }));
    }
    let Some(now) = focus else {
        let to = match key {
            Key::Tab(true) | Key::Up | Key::Left => ring.last().copied(),
            _ => ring.first().copied(),
        };
        return (to, None);
    };
    let at = ring.iter().position(|f| *f == now).unwrap_or(0);
    let next = || ring.get((at + 1) % ring.len().max(1)).copied();
    let before = || {
        ring.get((at + ring.len().max(1) - 1) % ring.len().max(1))
            .copied()
    };
    let volume = view.volume.as_ref().map_or(0, |v| v.percent);
    let brightness = view.brightness.unwrap_or(0);
    let cols = columns(view.width);
    match (key, now) {
        (Key::Activate, Focus::Toggle(i)) => (focus, Some(Act::Toggle(i))),
        (Key::Activate, Focus::Page(i)) => (focus, Some(Act::Page(i))),
        (Key::Activate, Focus::Sound) => (focus, Some(Act::List)),
        (Key::Activate, Focus::Choose(i)) => (focus, Some(Act::Choose(i))),
        (Key::Activate, Focus::SoundPage) => (focus, Some(Act::SoundPage)),
        (Key::Activate, Focus::Slider) => (focus, Some(Act::Mute)),
        (Key::Activate, Focus::Settings) => (focus, Some(Act::Settings)),
        (Key::Activate, Focus::Previous) => (focus, Some(Act::Player("Previous"))),
        (Key::Activate, Focus::PlayPause) => (focus, Some(Act::Player("PlayPause"))),
        (Key::Activate, Focus::Next) => (focus, Some(Act::Player("Next"))),
        (Key::Left, Focus::Light) => (
            focus,
            Some(Act::Brightness(brightness.saturating_sub(STEP))),
        ),
        (Key::Right, Focus::Light) => (focus, Some(Act::Brightness((brightness + STEP).min(100)))),
        (Key::Home, Focus::Light) => (focus, Some(Act::Brightness(0))),
        (Key::End, Focus::Light) => (focus, Some(Act::Brightness(100))),
        (Key::Left, Focus::Slider) => (focus, Some(Act::Volume(volume.saturating_sub(STEP)))),
        (Key::Right, Focus::Slider) => (focus, Some(Act::Volume((volume + STEP).min(100)))),
        (Key::Home, Focus::Slider) => (focus, Some(Act::Volume(0))),
        (Key::End, Focus::Slider) => (focus, Some(Act::Volume(100))),
        (Key::Tab(false) | Key::Right, _) => (next(), None),
        (Key::Tab(true) | Key::Left, _) => (before(), None),
        (Key::Down | Key::Up, Focus::Toggle(i) | Focus::Page(i)) => {
            let down = key == Key::Down;
            let j = if down {
                Some(i + cols)
            } else {
                i.checked_sub(cols)
            };
            let part = |j: usize| match now {
                Focus::Page(_) if view.tiles.get(j).is_some_and(|t| has_page(t.tile)) => {
                    Focus::Page(j)
                }
                _ => Focus::Toggle(j),
            };
            match j.filter(|j| view.tiles.get(*j).is_some()) {
                Some(j) => (Some(part(j)), None),
                None if down => {
                    let below = ring
                        .iter()
                        .find(|f| !matches!(f, Focus::Toggle(_) | Focus::Page(_)));
                    (below.copied().or(focus), None)
                }
                None => (focus, None),
            }
        }
        (Key::Down, _) => (next(), None),
        (Key::Up, _) => (before(), None),
        _ => (focus, None),
    }
}

// ---- What a screen reader reads ----

/// The card's parts as a screen reader finds them, in Tab's order, with
/// each one's place in logical pixels from the card's corner shifted by
/// `origin`; the index in the list of the part holding the keyboard.
pub fn items(view: &View, layout: &Layout, origin: (f64, f64)) -> (Vec<Item>, Option<usize>) {
    let rect = |r: Rect| {
        accesskit::Rect::new(
            origin.0 + f64::from(r.x),
            origin.1 + f64::from(r.y),
            origin.0 + f64::from(r.right()),
            origin.1 + f64::from(r.y + r.h),
        )
    };
    let zero = Rect::new(0.0, 0.0, 0.0, 0.0);
    let item = |role, label: String, bounds: Rect| Item {
        role,
        label,
        bounds: rect(bounds),
        children: Vec::new(),
        toggled: None,
        value: None,
    };
    let mut out = Vec::new();
    let mut focused = None;
    // The player's card is read first, as a label that says what plays.
    if let (Some(player), Some(card)) = (&view.player, layout.player) {
        let label = if player.artist.is_empty() {
            trf("Now playing: {title}", &[("title", &player.title)])
        } else {
            trf(
                "Now playing: {title} by {artist}",
                &[("title", &player.title), ("artist", &player.artist)],
            )
        };
        out.push(item(Role::Label, label, card));
    }
    let first = out.len();
    for (n, part) in ring(view).into_iter().enumerate() {
        let built = match part {
            Focus::Previous => item(
                Role::Button,
                tr("Previous").to_string(),
                layout.previous.unwrap_or(zero),
            ),
            Focus::PlayPause => {
                let playing = view.player.as_ref().is_some_and(|p| p.playing);
                let label = if playing { tr("Pause") } else { tr("Play") };
                item(Role::Button, label.to_string(), layout.play.unwrap_or(zero))
            }
            Focus::Next => item(
                Role::Button,
                tr("Next").to_string(),
                layout.next.unwrap_or(zero),
            ),
            Focus::Toggle(i) => {
                let (t, b) = (&view.tiles[i], &layout.tiles[i]);
                Item {
                    toggled: Some(t.on),
                    ..item(
                        Role::Switch,
                        trf(
                            "{title}, {state}",
                            &[("title", t.tile.title()), ("state", &t.state)],
                        ),
                        b.toggle,
                    )
                }
            }
            Focus::Page(i) => item(
                Role::Button,
                trf("{title} settings", &[("title", view.tiles[i].tile.title())]),
                layout.tiles[i].page.unwrap_or(layout.tiles[i].whole),
            ),
            Focus::Sound => item(
                Role::Button,
                tr("Sound: outputs").to_string(),
                layout.sound.unwrap_or(zero),
            ),
            Focus::Choose(i) => {
                let (name, current) = view
                    .volume
                    .as_ref()
                    .and_then(|v| v.outputs.get(i))
                    .map_or(("", false), |(n, d)| (n.as_str(), *d));
                Item {
                    toggled: Some(current),
                    ..item(Role::Button, name.to_string(), layout.list[i])
                }
            }
            Focus::SoundPage => item(
                Role::Button,
                tr("Sound settings").to_string(),
                layout.sound_page.unwrap_or(zero),
            ),
            Focus::Light => {
                let percent = view.brightness.unwrap_or(0);
                Item {
                    value: Some((f64::from(percent), 0.0, 100.0)),
                    ..item(
                        Role::Slider,
                        tr("Brightness").to_string(),
                        layout.light.unwrap_or(zero),
                    )
                }
            }
            Focus::Slider => {
                let percent = view.volume.as_ref().map_or(0, |v| v.percent);
                Item {
                    value: Some((f64::from(percent), 0.0, 100.0)),
                    ..item(
                        Role::Slider,
                        tr("Volume").to_string(),
                        layout.track.unwrap_or(zero),
                    )
                }
            }
            Focus::Settings => item(
                Role::Button,
                tr("Settings").to_string(),
                layout.settings.unwrap_or(zero),
            ),
        };
        if view.focus == Some(part) {
            focused = Some(first + n);
        }
        out.push(built);
    }
    (out, focused)
}

// ---- How it looks ----

/// The cards the popup draws: the player's (when there is one) and quick
/// settings', each with `RADIUS` corners (M5.9d).
pub fn cards(l: &Layout) -> Vec<Card> {
    let mut out = Vec::new();
    if let Some(player) = l.player {
        out.push(Card {
            rect: player,
            radius: RADIUS,
        });
    }
    out.push(Card {
        rect: l.quick,
        radius: RADIUS,
    });
    out
}

/// The keyboard's ring round a part: 2 px in the accent, 3 px outside the
/// part `r`, whose corners have radius `radius` (logical pixels).
fn focus_ring(pixmap: &mut Pixmap, r: Rect, radius: f32, s: f32, tokens: &Tokens) {
    let grown = Rect::new(r.x - 3.0, r.y - 3.0, r.w + 6.0, r.h + 6.0);
    let (x, y, w, h) = grown.device(s);
    outline(
        pixmap,
        (x, y, w, h),
        (radius + 3.0) * s,
        2.0 * s,
        tokens.accent,
    );
}

/// Draws `view` at `scale` into `pixmap`, which is the card's size times
/// it; without `text`, everything but the words.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    scale: f32,
) {
    let s = scale;
    let l = layout(view);
    popup::cards(pixmap, tokens, s, &cards(&l));
    for (i, (t, b)) in view.tiles.iter().zip(&l.tiles).enumerate() {
        if t.tile.pill() {
            pill_tile(pixmap, view, i, *b, tokens, text.as_deref_mut(), s);
        } else {
            round_tile(pixmap, view, i, *b, tokens, text.as_deref_mut(), s);
        }
    }
    shelf(pixmap, view, &l, tokens, text.as_deref_mut(), s);
    player(pixmap, view, &l, tokens, text.as_deref_mut(), s);
    footer(pixmap, view, &l, tokens, text, s);
}

/// The player's card (M5.9d): its cover, the title over the artist, and
/// the previous, play or pause and next buttons, with the keyboard's ring
/// and the hover light where they are. A cover is its picture in a circle,
/// else a circle of the accent going darker, with the app's first letter.
fn player(
    pixmap: &mut Pixmap,
    view: &View,
    l: &Layout,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    s: f32,
) {
    let (Some(p), Some(cover), Some(words)) = (&view.player, l.cover, l.words) else {
        return;
    };
    let (x, y, w, h) = cover.device(s);
    match &p.cover {
        Some(picture) => paint::circle_picture(pixmap, picture, (x, y, w, h)),
        None => {
            let bottom = mix(tokens.accent, tokens.panel_text, 0.45);
            paint::fill_vertical(pixmap, (x, y, w, h), w / 2.0, tokens.accent, bottom);
            if let Some(text) = text.as_deref_mut() {
                let initial = p
                    .identity
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default();
                let mut letter = text.fit_in(&initial, 22.0 * s, cover.w * s, Face::SEMIBOLD);
                let left = x + ((w - letter.width) / 2.0).round();
                let top = y + (h - 22.0 * s * 1.25) / 2.0;
                text.draw(pixmap, &mut letter, left, top, tokens.accent_text);
            }
        }
    }
    // The words: the title over the artist, from the box's left, beside
    // the cover, as the mockups set them.
    if let Some(text) = text {
        let room = words.w * s;
        let (wx, wy, _, wh) = words.device(s);
        let mut title = text.fit_in(&p.title, 12.5 * s, room, Face::SEMIBOLD);
        let title_h = 12.5 * s * 1.25;
        if p.artist.is_empty() {
            let left = wx;
            let top = wy + (wh - title_h) / 2.0;
            text.draw(pixmap, &mut title, left, top, tokens.panel_text);
        } else {
            let mut artist = text.fit(&p.artist, 11.0 * s, room);
            let block = title_h + 2.0 * s + 11.0 * s * 1.25;
            let top = wy + (wh - block) / 2.0;
            let left = wx;
            text.draw(pixmap, &mut title, left, top, tokens.panel_text);
            text.draw(
                pixmap,
                &mut artist,
                left,
                top + title_h + 2.0 * s,
                dim(tokens),
            );
        }
    }
    // The buttons: Previous and Next are dimmed where the player cannot
    // do them; Play or Pause is a filled circle with the glyph in the card.
    let sides = [
        (
            Focus::Previous,
            "media-previous",
            l.previous,
            p.can_previous,
        ),
        (Focus::Next, "media-next", l.next, p.can_next),
    ];
    for (focus, glyph, rect, able) in sides {
        let Some(rect) = rect else {
            continue;
        };
        if view.hover == Some(focus) && able {
            let (x, y, w, h) = rect.device(s);
            fill(pixmap, x, y, w, h, w / 2.0, veil(tokens, 0.06));
        }
        let ink = if able {
            tokens.panel_text
        } else {
            Colour {
                a: 0.5,
                ..dim(tokens)
            }
        };
        icon_in(pixmap, glyph, 14.0, rect, s, ink);
        if view.focus == Some(focus) {
            focus_ring(pixmap, rect, rect.h / 2.0, s, tokens);
        }
    }
    if let Some(rect) = l.play {
        let (x, y, w, h) = rect.device(s);
        fill(pixmap, x, y, w, h, w / 2.0, tokens.panel_text);
        if view.hover == Some(Focus::PlayPause) {
            fill(pixmap, x, y, w, h, w / 2.0, veil(tokens, 0.12));
        }
        let glyph = if p.playing {
            "media-pause"
        } else {
            "media-play"
        };
        icon_in(pixmap, glyph, 14.0, rect, s, tokens.panel);
        if view.focus == Some(Focus::PlayPause) {
            focus_ring(pixmap, rect, rect.h / 2.0, s, tokens);
        }
    }
}

/// A round toggle: its circle with the icon, its title under it, and the
/// focus ring or the hover light when they are on it.
fn round_tile(
    pixmap: &mut Pixmap,
    view: &View,
    i: usize,
    b: TileBox,
    tokens: &Tokens,
    text: Option<&mut Text>,
    s: f32,
) {
    let t = &view.tiles[i];
    let m = metrics(view.compact);
    let (x, y, w, h) = b.whole.device(s);
    let (disc, ink) = if t.on {
        (tokens.accent, tokens.accent_text)
    } else {
        (raised(tokens), tokens.panel_text)
    };
    fill(pixmap, x, y, w, h, w / 2.0, disc);
    if !t.on {
        outline(
            pixmap,
            (x, y, w, h),
            w / 2.0,
            (0.5 * s).max(1.0),
            tokens.line,
        );
    }
    if view.hover == Some(Focus::Toggle(i)) {
        fill(pixmap, x, y, w, h, w / 2.0, veil(tokens, 0.05));
    }
    icon_in(pixmap, t.tile.icon(), m.icon, b.whole, s, ink);
    if let Some(text) = text {
        // Under the circle, 4 px below it, centred on it.
        let room = (m.cell + m.gap - 4.0) * s;
        // Regular, not the mockups' medium: here a space in Inter's variable
        // font at weight 500 comes out three times as wide.
        let mut title = text.fit_in(t.tile.short(), 10.5 * s, room, Face::REGULAR);
        let centre = (b.whole.x + b.whole.w / 2.0) * s;
        let top = (b.whole.y + b.whole.h + 4.0) * s;
        let left = (centre - title.width / 2.0).round();
        text.draw(pixmap, &mut title, left, top.round(), dim(tokens));
    }
    if view.focus == Some(Focus::Toggle(i)) {
        focus_ring(pixmap, b.whole, b.whole.w / 2.0, s, tokens);
    }
}

/// A pill: a round icon at its left and, behind it, its name, state and
/// arrow. On, the whole pill is the accent.
fn pill_tile(
    pixmap: &mut Pixmap,
    view: &View,
    i: usize,
    b: TileBox,
    tokens: &Tokens,
    text: Option<&mut Text>,
    s: f32,
) {
    let t = &view.tiles[i];
    let m = metrics(view.compact);
    let on = t.on;
    let (x, y, w, h) = b.whole.device(s);
    let r = h / 2.0;
    let ink = if on {
        tokens.accent_text
    } else {
        tokens.panel_text
    };
    fill(
        pixmap,
        x,
        y,
        w,
        h,
        r,
        if on { tokens.accent } else { raised(tokens) },
    );
    if !on {
        outline(pixmap, (x, y, w, h), r, (0.5 * s).max(1.0), tokens.line);
    }
    if view.hover == Some(Focus::Page(i)) {
        fill(pixmap, x, y, w, h, r, veil(tokens, 0.05));
    }
    let circle = Rect::new(
        b.whole.x + PILL_INSET,
        b.whole.middle() - m.pill_icon / 2.0,
        m.pill_icon,
        m.pill_icon,
    );
    let (cx, cy, cw, ch) = circle.device(s);
    let disc = if on {
        Colour {
            a: 0.18,
            ..tokens.accent_text
        }
    } else {
        veil(tokens, 0.07)
    };
    fill(pixmap, cx, cy, cw, ch, cw / 2.0, disc);
    if view.hover == Some(Focus::Toggle(i)) {
        fill(pixmap, cx, cy, cw, ch, cw / 2.0, veil(tokens, 0.06));
    }
    icon_in(pixmap, t.tile.icon(), m.pill_glyph, circle, s, ink);
    // The arrow, centred 18 px from the pill's right end.
    let chevron = Rect::new(
        b.whole.right() - 18.0 - 6.0,
        b.whole.y + (m.cell - 12.0) / 2.0,
        12.0,
        12.0,
    );
    let right = if b.page.is_some() {
        chevron.x - 6.0
    } else {
        b.whole.right() - 12.0
    };
    if let Some(text) = text {
        let left = (circle.right() + 6.0) * s;
        let room = right * s - left;
        let state_ink = if on {
            Colour {
                a: 0.7,
                ..tokens.accent_text
            }
        } else {
            dim(tokens)
        };
        let mut title = text.fit_in(t.tile.title(), 12.5 * s, room, Face::SEMIBOLD);
        let mut state = text.fit(&t.state, 11.0 * s, room);
        let block = 12.5 * s * 1.25 + 11.0 * s * 1.25 + 2.0 * s;
        let top = y + (h - block) / 2.0;
        text.draw(pixmap, &mut title, left, top, ink);
        text.draw(
            pixmap,
            &mut state,
            left,
            top + 12.5 * s * 1.25 + 2.0 * s,
            state_ink,
        );
    }
    if b.page.is_some() {
        let ink = if on { tokens.accent_text } else { dim(tokens) };
        icon_in(pixmap, "chevron-right", 12.0, chevron, s, ink);
    }
    if view.focus == Some(Focus::Toggle(i)) {
        let ring_at = if b.page.is_some() { circle } else { b.whole };
        focus_ring(pixmap, ring_at, ring_at.h / 2.0, s, tokens);
    }
    if let Some(page) = b.page {
        if view.focus == Some(Focus::Page(i)) {
            focus_ring(pixmap, page, page.h / 2.0, s, tokens);
        }
    }
}

/// What a slider shows: its level, whether it is muted, the icon at its
/// left and whether the keyboard is on it.
pub(crate) struct Bar<'a> {
    pub(crate) percent: u32,
    pub(crate) muted: bool,
    pub(crate) icon: &'a str,
    pub(crate) focused: bool,
}

/// A slider's bar, drawn for the volume and the brightness alike (M5.9c):
/// the track, the fill to the level at least a bar wide, the icon 13 px
/// from the track's left end, the number 14 px in from its right end and
/// the keyboard's ring. A muted bar's fill is dimmed.
pub(crate) fn slider(
    pixmap: &mut Pixmap,
    track: Rect,
    bar: Bar,
    text: Option<&mut Text>,
    tokens: &Tokens,
    s: f32,
) {
    let (tx, ty, tw, th) = track.device(s);
    let round = th / 2.0;
    fill(pixmap, tx, ty, tw, th, round, veil(tokens, 0.07));
    let share = bar.percent.min(100) as f32 / 100.0;
    let filled = if bar.percent > 0 {
        (share * track.w).max(track.h).min(track.w)
    } else {
        0.0
    };
    let knob_colour = knob(tokens);
    if filled > 0.0 {
        let c = if bar.muted {
            mix(knob_colour, tokens.panel, 0.5)
        } else {
            knob_colour
        };
        fill(pixmap, tx, ty, filled * s, th, round, c);
    }
    // The icon, 16 px across; its ink is the card's text where the fill covers it.
    let icon = Rect::new(track.x + 13.0, track.y, 16.0, track.h);
    let covered = filled >= 29.0;
    let ink = if !covered {
        dim(tokens)
    } else if knob_colour == tokens.window {
        tokens.panel_text
    } else {
        tokens.panel
    };
    icon_in(pixmap, bar.icon, 16.0, icon, s, ink);
    if let Some(text) = text {
        let size = 11.5 * s;
        let mut number = text.line_in(&bar.percent.to_string(), size, Face::SEMIBOLD.tabular());
        let right = (track.right() - 14.0) * s;
        let top = ty + (th - size * 1.25) / 2.0;
        let left = right - number.width;
        text.draw(pixmap, &mut number, left, top, dim(tokens));
    }
    if bar.focused {
        focus_ring(pixmap, track, track.h / 2.0, s, tokens);
    }
}

/// The speaker by the volume: the muted one when muted or silent, else
/// low, medium or high (M5.9c: the volume pop-up shows the same one).
pub(crate) fn volume_icon(percent: u32, muted: bool) -> &'static str {
    if muted || percent == 0 {
        "volume-muted"
    } else if percent <= 33 {
        "volume-low"
    } else if percent <= 66 {
        "volume-medium"
    } else {
        "volume-high"
    }
}

/// The shelf: the hairline above it, the brightness slider where the machine
/// has a backlight (M5.9c), the volume slider with the volume in it, the
/// chevron that opens the outputs, and the list of outputs when it is open.
fn shelf(
    pixmap: &mut Pixmap,
    view: &View,
    l: &Layout,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    s: f32,
) {
    let m = l.metrics;
    let hair = (0.5 * s).max(1.0);
    if let Some(rule) = l.rule {
        let (x, y, w, _) = Rect::new(
            m.side + RULE_INSET,
            rule,
            view.width as f32 - 2.0 * (m.side + RULE_INSET),
            1.0,
        )
        .device(s);
        fill(pixmap, x, y, w, hair, 0.0, tokens.line);
    }
    if let (Some(light), Some(percent)) = (l.light, view.brightness) {
        let bar = Bar {
            percent,
            muted: false,
            icon: "brightness",
            focused: view.focus == Some(Focus::Light),
        };
        slider(pixmap, light, bar, text.as_deref_mut(), tokens, s);
    }
    let (Some(v), Some(track), Some(sound)) = (&view.volume, l.track, l.sound) else {
        return;
    };
    let bar = Bar {
        percent: v.percent,
        muted: v.muted,
        icon: volume_icon(v.percent, v.muted),
        focused: view.focus == Some(Focus::Slider),
    };
    slider(pixmap, track, bar, text.as_deref_mut(), tokens, s);
    // The chevron button: raised, edged, its arrow turning down while open.
    let (sx, sy, sw, sh) = sound.device(s);
    fill(pixmap, sx, sy, sw, sh, sw / 2.0, raised(tokens));
    outline(pixmap, (sx, sy, sw, sh), sw / 2.0, hair, tokens.line);
    if view.hover == Some(Focus::Sound) {
        fill(pixmap, sx, sy, sw, sh, sw / 2.0, veil(tokens, 0.05));
    }
    let glyph = if view.list {
        "chevron-down"
    } else {
        "chevron-right"
    };
    icon_in(pixmap, glyph, 14.0, sound, s, tokens.panel_text);
    if view.focus == Some(Focus::Sound) {
        focus_ring(pixmap, sound, m.bar / 2.0, s, tokens);
    }
    // The list of outputs, one row each, the one in use lit and ticked.
    for (i, (rect, (name, current))) in l.list.iter().zip(&v.outputs).enumerate() {
        let (rx, ry, rw, rh) = rect.device(s);
        let rr = rh / 2.0;
        if *current {
            fill(pixmap, rx, ry, rw, rh, rr, lit(tokens));
        } else if view.hover == Some(Focus::Choose(i)) {
            fill(pixmap, rx, ry, rw, rh, rr, veil(tokens, 0.05));
        }
        if let Some(text) = text.as_deref_mut() {
            let size = 13.0 * s;
            let room = rw - (ROW_TEXT + 30.0) * s;
            let mut line = text.fit(name, size, room);
            let top = ry + (rh - size * 1.25) / 2.0;
            text.draw(pixmap, &mut line, rx + ROW_TEXT * s, top, tokens.panel_text);
        }
        if *current {
            let tick = Rect::new(rect.right() - ROW_TEXT - 14.0, rect.y, 14.0, rect.h);
            icon_in(pixmap, "check", 14.0, tick, s, tokens.accent);
        }
        if view.focus == Some(Focus::Choose(i)) {
            outline(pixmap, (rx, ry, rw, rh), rr, 2.0 * s, tokens.accent);
        }
    }
    // "Sound settings", last, with an arrow.
    if let Some(page) = l.sound_page {
        let (px, py, pw, ph) = page.device(s);
        let rr = ph / 2.0;
        fill(pixmap, px, py, pw, ph, rr, veil(tokens, 0.05));
        if view.hover == Some(Focus::SoundPage) {
            fill(pixmap, px, py, pw, ph, rr, veil(tokens, 0.05));
        }
        if let Some(text) = text {
            let size = 13.0 * s;
            let mut line = text.fit(tr("Sound settings"), size, pw - (ROW_TEXT + 30.0) * s);
            let top = py + (ph - size * 1.25) / 2.0;
            text.draw(pixmap, &mut line, px + ROW_TEXT * s, top, tokens.panel_text);
        }
        let arrow = Rect::new(page.right() - ROW_TEXT - 14.0, page.y, 14.0, page.h);
        icon_in(pixmap, "chevron-right", 14.0, arrow, s, tokens.panel_text);
        if view.focus == Some(Focus::SoundPage) {
            outline(pixmap, (px, py, pw, ph), rr, 2.0 * s, tokens.accent);
        }
    }
}

/// The footer: the battery's pill with its charge and time, and the
/// Settings button at the right.
fn footer(
    pixmap: &mut Pixmap,
    view: &View,
    l: &Layout,
    tokens: &Tokens,
    text: Option<&mut Text>,
    s: f32,
) {
    if let (Some(b), Some(room)) = (&view.battery, l.battery) {
        let (x, y, w, h) = room.device(s);
        fill(pixmap, x, y, w, h, h / 2.0, veil(tokens, 0.05));
        let icon = (15.0 * s).round();
        let halo = s.round().max(1.0) as i32;
        let at = (x + (12.0 * s).round(), y + ((h - icon) / 2.0).round());
        paint::battery(pixmap, icon, at, (b.percent, b.charging), dim(tokens), halo);
        if let Some(text) = text {
            let size = 11.5 * s;
            let left = x + (12.0 + 15.0 + 8.0) * s;
            let right = x + w - 12.0 * s;
            let percent = trf("{percent}%", &[("percent", &b.percent.to_string())]);
            let mut pct = text.fit_in(&percent, size, right - left, Face::SEMIBOLD);
            let top = y + (h - size * 1.25) / 2.0;
            text.draw(pixmap, &mut pct, left, top, tokens.panel_text);
            let after = match (&b.time, b.charging) {
                (Some(time), _) => Some(time.clone()),
                (None, true) => Some(tr("charging").to_string()),
                (None, false) => None,
            };
            if let Some(after) = after {
                let at = left + pct.width + 7.0 * s;
                let mut time = text.fit(&after, size, right - at);
                text.draw(pixmap, &mut time, at, top, dim(tokens));
            }
        }
    }
    if let Some(button) = l.settings {
        let (x, y, w, h) = button.device(s);
        let hovered = view.hover == Some(Focus::Settings);
        let back = if hovered { 0.085 } else { 0.05 };
        fill(pixmap, x, y, w, h, w / 2.0, veil(tokens, back));
        icon_in(pixmap, "gear", 16.0, button, s, tokens.panel_text);
        if view.focus == Some(Focus::Settings) {
            focus_ring(pixmap, button, button.w / 2.0, s, tokens);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{Battery, Network, Output, Volume};
    use tiny_skia::{PixmapPaint, Transform};

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn network(link: Link, has_wifi: bool, wifi_on: bool) -> Option<Network> {
        Some(Network {
            link,
            has_wifi,
            wifi_on,
        })
    }

    /// The mockups' laptop: on Wi-Fi at home, Bluetooth off, a battery.
    fn laptop() -> Status {
        Status {
            network: network(
                Link::Wifi {
                    name: "Home 5G".into(),
                    bars: 3,
                },
                true,
                true,
            ),
            volume: Some(Volume {
                percent: 62,
                muted: false,
                output: "Speakers".into(),
            }),
            outputs: vec![
                Output {
                    id: 48,
                    name: "Speakers".into(),
                    default: true,
                },
                Output {
                    id: 52,
                    name: "HDMI Output".into(),
                    default: false,
                },
            ],
            battery: Some(Battery {
                percent: 82,
                charging: false,
                line: "82 percent, 4 hours 10 minutes left".into(),
                time: Some("4 h 10 min".into()),
            }),
            bluetooth: Some(false),
            brightness: None,
        }
    }

    /// The card on a screen `screen` px wide: a sheet across it below
    /// Compact's edge, else the 352 px card.
    fn view_of(status: &Status, dark: bool, screen: u32) -> View {
        let list = names(edel::presets::DEFAULT_TILES);
        let state = State {
            width: if screen < COMPACT_BELOW {
                screen
            } else {
                WIDTH
            },
            compact: screen < COMPACT_BELOW,
            dark,
            ..State::default()
        };
        view(&state, status, &list, true)
    }

    #[test]
    fn the_tiles_are_exactly_the_presets_names() {
        let ours: Vec<&str> = ALL.iter().map(|t| t.name()).collect();
        assert_eq!(
            ours,
            edel::presets::TILES,
            "a tile is a name there and a tile here"
        );
        for tile in ALL {
            assert_eq!(Tile::from_name(tile.name()), Some(tile));
            assert!(!tile.title().is_empty());
            assert!(!tile.short().is_empty());
            assert!(
                edel::icons::mask(tile.icon(), 16).is_some(),
                "{}",
                tile.icon()
            );
            // Every arrow opens a page Settings has a section for.
            if let Some(page) = tile.page() {
                assert!(edel::settings::page(page).is_some(), "{page}");
            }
        }
        assert_eq!(Tile::from_name("night_light"), None);
        assert_eq!(
            ALL.iter().filter(|t| t.pill()).count(),
            2,
            "Wi-Fi and Bluetooth are the pills"
        );
    }

    #[test]
    fn dark_styles_arrow_opens_a_page_the_settings_app_builds() {
        // The Settings app's pages are the arms of its page table; the
        // Dark style arrow opens its Appearance page (M5.2q).
        let main = include_str!("../../settings/src/main.rs");
        assert!(
            main.contains(&format!("\"{DARK_PAGE}\" => Some(Page {{"))
                || main.contains("\"appearance\" => Some(Page {"),
            "the Settings app has no page called {DARK_PAGE}"
        );
        if main.contains("\"appearance\" => Some(Page {") {
            assert_eq!(
                DARK_PAGE, "appearance",
                "Appearance exists now: point the arrow at it"
            );
        }
        assert!(
            main.contains(&format!("\"{SOUND_PAGE}\" => Some(Page {{")),
            "the Settings app has no page called {SOUND_PAGE}"
        );
    }

    #[test]
    fn a_laptop_shows_every_tile_in_the_presets_order_with_what_each_says() {
        let list = names(edel::presets::DEFAULT_TILES);
        let shown = tiles(&list, &laptop(), Switches::default(), &[]);
        let says: Vec<(&str, &str, bool)> = shown
            .iter()
            .map(|t| (t.tile.name(), t.state.as_str(), t.on))
            .collect();
        assert_eq!(
            says,
            [
                ("wifi", "Home 5G", true),
                ("bluetooth", "Off", false),
                ("airplane", "Off", false),
                ("do_not_disturb", "Off", false),
                ("dark_style", "Off", false),
            ]
        );
        let dark = tiles(
            &list,
            &laptop(),
            Switches {
                dark: true,
                dnd: true,
            },
            &[],
        );
        assert_eq!((dark[3].state.as_str(), dark[3].on), ("On", true));
        assert_eq!((dark[4].state.as_str(), dark[4].on), ("On", true));
    }

    #[test]
    fn a_tile_whose_hardware_is_missing_is_left_out() {
        let list = names(edel::presets::DEFAULT_TILES);
        // The test machine's VM: a network, no Wi-Fi adapter, no Bluetooth.
        let vm = Status {
            network: network(Link::Wired, false, false),
            ..Status::default()
        };
        let shown: Vec<_> = tiles(&list, &vm, Switches::default(), &[])
            .iter()
            .map(|t| t.tile)
            .collect();
        assert_eq!(shown, [Tile::DoNotDisturb, Tile::DarkStyle]);
        // Bluetooth alone is enough for flight mode.
        let bluetooth = Status {
            bluetooth: Some(true),
            ..Status::default()
        };
        let shown: Vec<_> = tiles(&list, &bluetooth, Switches::default(), &[])
            .iter()
            .map(|t| t.tile)
            .collect();
        assert_eq!(
            shown,
            [
                Tile::Bluetooth,
                Tile::Airplane,
                Tile::DoNotDisturb,
                Tile::DarkStyle
            ]
        );
        // A name twice or unknown shows nothing extra.
        let odd = names(&["dark_style", "dark_style", "night_light"]);
        assert_eq!(tiles(&odd, &laptop(), Switches::default(), &[]).len(), 1);
    }

    #[test]
    fn wifi_on_without_a_network_says_so_and_a_choice_shows_at_once() {
        let mut status = laptop();
        status.network = network(Link::Offline, true, true);
        let list = names(&["wifi", "airplane"]);
        assert_eq!(
            tiles(&list, &status, Switches::default(), &[])[0].state,
            "Not connected"
        );
        // Just switched off, the machine not yet saying so.
        let pending = [(Tile::Wifi, false), (Tile::Airplane, true)];
        let shown = tiles(&list, &laptop(), Switches::default(), &pending);
        assert_eq!((shown[0].state.as_str(), shown[0].on), ("Off", false));
        assert_eq!((shown[1].state.as_str(), shown[1].on), ("On", true));
    }

    #[test]
    fn the_card_is_352_wide_and_its_tiles_lie_in_one_square_grid() {
        let view = view_of(&laptop(), false, 1280);
        let l = layout(&view);
        assert_eq!(l.size.0, 352);
        // Two pills share the first row, 148 wide, 20 apart, centred on
        // four cells: 18 in, 186 in.
        assert_eq!(l.tiles[0].whole, Rect::new(18.0, 12.0, 148.0, 64.0));
        assert_eq!(l.tiles[1].whole, Rect::new(186.0, 12.0, 148.0, 64.0));
        // The round toggles lie in the second row, 10 below the first.
        let y = 12.0 + 64.0 + 10.0;
        for (i, x) in [(2, 18.0), (3, 102.0), (4, 186.0)] {
            assert_eq!(l.tiles[i].whole, Rect::new(x, y, 64.0, 64.0));
            assert_eq!(l.tiles[i].toggle, l.tiles[i].whole);
            assert_eq!(l.tiles[i].page, None);
        }
        // The pill's icon part is its left 46 px; its page is the rest.
        let page = l.tiles[0].page.unwrap();
        assert_eq!((page.x, page.w), (18.0 + 46.0, 148.0 - 46.0));
        assert_eq!(l.tiles[0].toggle.w, 46.0);
        // Every gap across the grid is 20 px.
        assert_eq!(l.tiles[1].whole.x - l.tiles[0].whole.right(), 20.0);
        assert_eq!(l.tiles[3].whole.x - l.tiles[2].whole.right(), 20.0);
        assert_eq!(l.tiles[4].whole.x - l.tiles[3].whole.right(), 20.0);
        // The footer's button is a bar square 14 px in from the shelf's end.
        let b = l.settings.unwrap();
        assert_eq!((b.w, b.h), (36.0, 36.0));
        assert_eq!(b.right(), 18.0 + 316.0 - 14.0);
        // The battery pill is half the inner width less a gap: 148.
        assert_eq!(l.battery.unwrap().w, 148.0);
        // Nothing lies outside the card.
        let bottom = l.size.1 as f32;
        for r in [l.tiles[4].whole, b, l.track.unwrap(), l.sound.unwrap()] {
            assert!(r.x >= 0.0 && r.right() <= 352.0 && r.y + r.h <= bottom);
        }
    }

    #[test]
    fn a_pill_that_does_not_fit_starts_a_new_row() {
        let list = names(&[
            "airplane",
            "wifi",
            "do_not_disturb",
            "dark_style",
            "bluetooth",
        ]);
        let state = State {
            width: WIDTH,
            ..State::default()
        };
        let view = view(&state, &laptop(), &list, false);
        let l = layout(&view);
        let x = |i: usize| l.tiles[i].whole.x;
        // Four columns: Airplane in column 0, Wi-Fi in 1 and 2, Do not
        // disturb in 3; Dark starts row 2, Bluetooth fills 1 and 2 of it.
        assert_eq!(x(0), 18.0);
        assert_eq!(x(1), 18.0 + 84.0);
        assert_eq!(l.tiles[1].whole.w, 148.0);
        assert_eq!(x(2), 18.0 + 3.0 * 84.0);
        assert_eq!(x(3), 18.0);
        assert!(l.tiles[3].whole.y > l.tiles[0].whole.y);
        assert_eq!(x(4), 18.0 + 84.0);
        assert_eq!(l.tiles[4].whole.w, 148.0);
        assert_eq!(l.tiles[4].whole.y, l.tiles[3].whole.y);
        // The second row has a round toggle, so it is 74 high.
        assert_eq!(l.tiles[3].whole.y, l.tiles[0].whole.y + 74.0 + 10.0);
    }

    #[test]
    fn at_compact_width_it_is_a_sheet_and_its_tiles_are_touch_sized() {
        let view = view_of(&laptop(), false, 360);
        assert_eq!(view.width, 360);
        let l = layout(&view);
        assert_eq!(l.size.0, 360);
        // Every tile's parts are at least 44 px where a finger lands.
        for t in &l.tiles {
            assert!(t.toggle.h >= 44.0 && t.toggle.w >= 44.0, "{t:?}");
            if let Some(page) = t.page {
                assert!(page.w >= 44.0 && page.h >= 44.0, "{page:?}");
            }
            assert!(t.whole.right() <= 360.0);
        }
        // The slider and the buttons beside and below it are 44 px high,
        // where the desktop's are the mockups' 36.
        assert_eq!(l.track.unwrap().h, 44.0);
        assert_eq!(l.settings.unwrap().h, 44.0);
        assert_eq!(l.sound.unwrap().w, 44.0);
        // From 600 px on it is the 352 px card, 16 px shorter.
        let wide = view_of(&laptop(), false, 1280);
        assert_eq!(layout(&wide).size.0, 352);
        assert_eq!(layout(&wide).size.1 + 16, l.size.1);
    }

    #[test]
    fn the_card_grows_with_its_tiles_and_the_list_and_loses_what_is_not_there() {
        let full = layout(&view_of(&laptop(), false, 1280)).size.1;
        let mut view = view_of(&laptop(), false, 1280);
        view.list = true;
        let open = layout(&view);
        // The list: a section, then two output rows and "Sound settings",
        // 36 high each and 4 apart.
        assert_eq!(open.size.1, full + 10 + 3 * 36 + 2 * 4, "the list's room");
        assert_eq!(open.list.len(), 2);
        let chevron = open.sound.unwrap();
        assert!(open.list[0].y >= chevron.y + chevron.h);
        // The list follows the volume row, the last row ends the card's shelf.
        let track = open.track.unwrap();
        assert!(open.list[0].y >= track.y + track.h);
        assert!(open.sound_page.unwrap().y > open.list[1].y);
        let mut bare = view_of(&Status::default(), false, 1280);
        bare.settings = false;
        let l = layout(&bare);
        assert_eq!(
            l.tiles.len(),
            2,
            "only the two tiles that are lines of the settings file"
        );
        assert!(l.track.is_none() && l.rule.is_none() && l.settings.is_none());
        let mut one = view_of(&laptop(), false, 1280);
        one.tiles.truncate(3);
        let l = layout(&one);
        assert_eq!(
            l.tiles[2].whole.x, 18.0,
            "a last one alone keeps the left column"
        );
    }

    #[test]
    fn the_log_line_says_where_each_part_lies() {
        let view = view_of(&laptop(), false, 1280);
        let l = layout(&view);
        let line = places(&view, &l);
        assert!(
            line.starts_with(&format!(
                "card 352x{}, wifi 18+12+148x64, bluetooth 186+12+148x64, airplane 18+86+64x64, ",
                l.size.1
            )),
            "{line}"
        );
        assert!(
            line.contains(", do_not_disturb 102+86+64x64, dark_style 186+86+64x64, track 18+"),
            "{line}"
        );
        assert!(
            line.ends_with(&format!(
                ", settings 284+{:.0}+36x36",
                l.settings.unwrap().y
            )),
            "{line}"
        );
    }

    #[test]
    fn the_colour_scheme_is_dark_by_the_files_and_written_only_when_it_differs() {
        let dark = "format = 1\n[appearance]\nmode = \"dark\"\n";
        let auto = "format = 1\n[appearance]\nmode = \"auto\"\n";
        assert!(!is_dark(None, None), "the release's default is light");
        assert!(is_dark(Some(dark), None));
        assert!(!is_dark(Some(auto), None));
        // The person's file over the machine's.
        assert!(!is_dark(
            Some(dark),
            Some("format = 1\n[appearance]\nmode = \"light\"\n")
        ));
        // No machine setting: dark is written, light is the absence of the key.
        assert_eq!(mode_to_write(None, true), Some("dark"));
        assert_eq!(mode_to_write(None, false), None);
        // A machine that is dark: light must be said, dark is the default.
        assert_eq!(mode_to_write(Some(dark), false), Some("light"));
        assert_eq!(mode_to_write(Some(dark), true), None);
        assert_eq!(edel::settings::choices(MODE), ["light", "dark", "auto"]);
    }

    #[test]
    fn a_choice_shows_at_once_and_the_slider_shows_where_it_was_dragged() {
        let list = names(edel::presets::DEFAULT_TILES);
        let state = State {
            width: WIDTH,
            pending: vec![(Tile::Wifi, false)],
            volume: Some(30),
            muted: Some(true),
            ..State::default()
        };
        let v = view(&state, &laptop(), &list, false);
        assert!(!v.tiles[0].on, "Wi-Fi shows off before the machine says so");
        let volume = v.volume.unwrap();
        assert_eq!((volume.percent, volume.muted), (30, true));
        assert!(!v.settings);
        let machine = view(&State::default(), &laptop(), &list, true);
        assert_eq!(machine.volume.unwrap().percent, 62);
    }

    #[test]
    fn the_brightness_slider_lies_above_the_volume_when_there_is_a_backlight() {
        let mut status = laptop();
        status.brightness = Some(70);
        let view = view_of(&status, false, 1280);
        let l = layout(&view);
        let light = l.light.expect("a backlight has a slider");
        let track = l.track.expect("the volume has its slider");
        assert!(light.y < track.y, "the brightness lies above the volume");
        assert_eq!(
            light.w,
            l.size.0 as f32 - 2.0 * l.metrics.side,
            "full width"
        );
        assert_eq!(
            l.sound.map(|r| r.y),
            Some(track.y),
            "the chevron is beside the volume only"
        );
        assert_eq!(
            hit(&view, &l, light.x + 40.0, light.middle()),
            Some(Focus::Light)
        );
        assert!(
            places(&view, &l).contains("light "),
            "CI reads where it lies"
        );
        assert_eq!(brightness_at(&l, light.x + light.w / 2.0), 50);
        assert_eq!(brightness_at(&l, light.x + light.w + 40.0), 100);
        // Without a backlight there is no such slider, and the volume's row is where it was.
        let without = layout(&view_of(&laptop(), false, 1280));
        assert_eq!(without.light, None);
        assert_eq!(
            without.track.map(|r| r.y),
            l.track.map(|r| r.y - light.h - SLIDER_GAP)
        );
    }

    #[test]
    fn keys_move_the_brightness_and_it_never_goes_dark_from_the_keyboard() {
        let mut status = laptop();
        status.brightness = Some(70);
        let view = view_of(&status, false, 1280);
        let light = Some(Focus::Light);
        assert_eq!(key(&view, light, Key::Right).1, Some(Act::Brightness(75)));
        assert_eq!(key(&view, light, Key::Left).1, Some(Act::Brightness(65)));
        assert_eq!(key(&view, light, Key::Home).1, Some(Act::Brightness(0)));
        assert_eq!(key(&view, light, Key::End).1, Some(Act::Brightness(100)));
        assert_eq!(key(&view, light, Key::Activate).1, None);
        // The ring holds the brightness just above the volume.
        let ring = ring(&view);
        let at = ring.iter().position(|f| *f == Focus::Light).unwrap();
        assert_eq!(ring[at + 1], Focus::Slider);
    }

    #[test]
    fn a_pointer_finds_the_halves_of_a_tile_and_the_shelf() {
        let view = view_of(&laptop(), false, 1280);
        let l = layout(&view);
        let t = l.tiles[0];
        assert_eq!(
            hit(&view, &l, t.toggle.x + 10.0, t.toggle.middle()),
            Some(Focus::Toggle(0))
        );
        let p = t.page.unwrap();
        assert_eq!(hit(&view, &l, p.x + 4.0, p.middle()), Some(Focus::Page(0)));
        // A round toggle's whole cell toggles.
        let a = l.tiles[2];
        assert_eq!(
            hit(&view, &l, a.whole.right() - 3.0, a.whole.middle()),
            Some(Focus::Toggle(2))
        );
        assert_eq!(hit(&view, &l, 1.0, 1.0), None);
        let track = l.track.unwrap();
        assert_eq!(
            hit(&view, &l, track.x + 40.0, track.middle()),
            Some(Focus::Slider)
        );
        let sound = l.sound.unwrap();
        assert_eq!(
            hit(&view, &l, sound.x + 5.0, sound.middle()),
            Some(Focus::Sound)
        );
        let b = l.settings.unwrap();
        assert_eq!(hit(&view, &l, b.x + 3.0, b.y + 3.0), Some(Focus::Settings));
    }

    #[test]
    fn the_slider_reads_the_pointer_from_end_to_end() {
        let view = view_of(&laptop(), false, 1280);
        let l = layout(&view);
        let t = l.track.unwrap();
        assert_eq!(volume_at(&l, t.x - 50.0), 0);
        assert_eq!(volume_at(&l, t.x), 0);
        assert_eq!(volume_at(&l, t.x + t.w / 2.0), 50);
        assert_eq!(volume_at(&l, t.x + t.w), 100);
        assert_eq!(volume_at(&l, t.x + t.w + 40.0), 100);
        // A quarter of the way along is a quarter of the volume.
        assert_eq!(volume_at(&l, t.x + t.w * 0.25), 25);
    }

    #[test]
    fn keys_move_between_tiles_and_the_slider_and_change_the_volume() {
        let view = view_of(&laptop(), false, 1280);
        let first = ring(&view)[0];
        assert_eq!(first, Focus::Toggle(0));
        assert_eq!(key(&view, None, Key::Tab(false)), (Some(first), None));
        assert_eq!(
            key(&view, None, Key::Tab(true)).0,
            Some(Focus::Settings),
            "Shift+Tab from nowhere is the last"
        );
        // Right goes from a pill's icon to its page, then to the next tile.
        let (f, _) = key(&view, Some(Focus::Toggle(0)), Key::Right);
        assert_eq!(f, Some(Focus::Page(0)));
        assert_eq!(key(&view, f, Key::Right).0, Some(Focus::Toggle(1)));
        // Down moves by a row of four cells: from Wi-Fi to Dark style (the
        // fifth tile, the round toggle at the row's end).
        assert_eq!(
            key(&view, Some(Focus::Toggle(0)), Key::Down).0,
            Some(Focus::Toggle(4))
        );
        // From the last row, Down goes to the slider.
        assert_eq!(
            key(&view, Some(Focus::Toggle(3)), Key::Down).0,
            Some(Focus::Slider)
        );
        assert_eq!(
            key(&view, Some(Focus::Toggle(4)), Key::Up).0,
            Some(Focus::Toggle(0))
        );
        // Space or Return does what a click does.
        assert_eq!(
            key(&view, Some(Focus::Toggle(1)), Key::Activate).1,
            Some(Act::Toggle(1))
        );
        assert_eq!(
            key(&view, Some(Focus::Page(0)), Key::Activate).1,
            Some(Act::Page(0))
        );
        assert_eq!(
            key(&view, Some(Focus::Sound), Key::Activate).1,
            Some(Act::List)
        );
        assert_eq!(
            key(&view, Some(Focus::Slider), Key::Activate).1,
            Some(Act::Mute)
        );
        assert_eq!(
            key(&view, Some(Focus::Settings), Key::Activate).1,
            Some(Act::Settings)
        );
        // Left and Right on the slider move the volume by 5; it stays in 0 to 100.
        let slider = Some(Focus::Slider);
        assert_eq!(key(&view, slider, Key::Right).1, Some(Act::Volume(67)));
        assert_eq!(key(&view, slider, Key::Left).1, Some(Act::Volume(57)));
        assert_eq!(key(&view, slider, Key::End).1, Some(Act::Volume(100)));
        assert_eq!(key(&view, slider, Key::Home).1, Some(Act::Volume(0)));
        // Escape closes the list first, then the card.
        assert_eq!(key(&view, slider, Key::Escape).1, Some(Act::Close));
        let mut open = view.clone();
        open.list = true;
        assert_eq!(key(&open, slider, Key::Escape).1, Some(Act::List));
        assert!(ring(&open).contains(&Focus::Choose(1)));
        // The list's last row opens the Sound page.
        assert_eq!(
            key(&open, Some(Focus::SoundPage), Key::Activate).1,
            Some(Act::SoundPage)
        );
    }

    #[test]
    fn a_screen_reader_finds_every_part_with_its_state() {
        let mut view = view_of(&laptop(), false, 1280);
        view.focus = Some(Focus::Slider);
        let l = layout(&view);
        let (items, focused) = items(&view, &l, (900.0, 500.0));
        assert_eq!(items.len(), ring(&view).len());
        let first = &items[0];
        assert_eq!(first.role, Role::Switch);
        assert_eq!(first.label, "Wi-Fi, Home 5G");
        assert_eq!(first.toggled, Some(true));
        assert_eq!(items[1].label, "Wi-Fi settings");
        let slider = &items[focused.unwrap()];
        assert_eq!(
            (slider.role, slider.label.as_str()),
            (Role::Slider, "Volume")
        );
        assert_eq!(slider.value, Some((62.0, 0.0, 100.0)));
        assert_eq!(first.bounds.x0, 900.0 + 18.0);
        assert_eq!(items.last().unwrap().label, "Settings");
        assert!(items.iter().any(|i| i.label == "Sound: outputs"));
    }

    /// A player of Night Drive by Lumen, playing, with `can` for each of
    /// Previous and Next, and no cover.
    fn playing(can: bool) -> PlayerView {
        PlayerView {
            title: "Night Drive".into(),
            artist: "Lumen".into(),
            identity: "Lumen".into(),
            playing: true,
            can_previous: can,
            can_next: can,
            cover: None,
        }
    }

    #[test]
    fn a_player_sits_above_the_card_on_a_bottom_panel() {
        let plain = view_of(&laptop(), false, 1280);
        let mut with = plain.clone();
        with.player = Some(playing(true));
        let (a, b) = (layout(&plain), layout(&with));
        // The player's card at the top, 352 by 100, and the card 108 lower.
        assert_eq!(b.player, Some(Rect::new(0.0, 0.0, 352.0, 100.0)));
        assert_eq!(b.quick, Rect::new(0.0, 108.0, 352.0, a.size.1 as f32));
        assert_eq!(b.size.1, a.size.1 + 108);
        for (x, y) in a.tiles.iter().zip(&b.tiles) {
            assert_eq!(y.whole.y, x.whole.y + 108.0);
        }
        assert_eq!(b.track.unwrap().y, a.track.unwrap().y + 108.0);
        assert_eq!(b.settings.unwrap().y, a.settings.unwrap().y + 108.0);
    }

    #[test]
    fn a_player_sits_below_the_card_on_a_top_panel() {
        let mut view = view_of(&laptop(), false, 1280);
        let plain = layout(&view);
        view.player = Some(playing(true));
        view.player_below = true;
        let l = layout(&view);
        // The card stays where it is, the player hangs 8 px below it.
        assert_eq!(l.quick, Rect::new(0.0, 0.0, 352.0, plain.size.1 as f32));
        assert_eq!(l.player.unwrap().y, (l.quick.y + l.quick.h) + 8.0);
        assert_eq!(l.player.unwrap().h, 100.0);
        assert_eq!(l.size.1, plain.size.1 + 108);
    }

    #[test]
    fn the_player_buttons_are_found_and_act() {
        let mut view = view_of(&laptop(), false, 1280);
        view.player = Some(playing(true));
        let l = layout(&view);
        let play = l.play.unwrap();
        let (x, y) = (play.x + play.w / 2.0, play.middle());
        assert_eq!(hit(&view, &l, x, y), Some(Focus::PlayPause));
        assert_eq!(
            key(&view, Some(Focus::PlayPause), Key::Activate).1,
            Some(Act::Player("PlayPause"))
        );
        // A player that cannot go back has no Previous in the ring, and its
        // button is not hit, though it is drawn.
        view.player = Some(PlayerView {
            can_previous: false,
            ..playing(true)
        });
        let l = layout(&view);
        let prev = l.previous.unwrap();
        assert_eq!(hit(&view, &l, prev.x + prev.w / 2.0, prev.middle()), None);
        let keys = ring(&view);
        assert!(!keys.contains(&Focus::Previous));
        assert_eq!(keys.first(), Some(&Focus::PlayPause));
        assert!(keys.contains(&Focus::Next));
    }

    #[test]
    fn places_name_the_player_and_its_buttons() {
        let mut view = view_of(&laptop(), false, 1280);
        view.player = Some(playing(true));
        let l = layout(&view);
        let line = places(&view, &l);
        assert!(
            line.starts_with(&format!("card 352x{}, player 0+0+352x100, ", l.size.1)),
            "{line}"
        );
        assert!(line.contains(", previous "), "{line}");
        assert!(line.contains(", play "), "{line}");
        assert!(line.contains(", next "), "{line}");
        // The buttons come before the tiles.
        assert!(
            line.find(", play ").unwrap() < line.find(", wifi ").unwrap(),
            "{line}"
        );
    }

    #[test]
    fn a_screen_reader_reads_the_player_first() {
        let mut view = view_of(&laptop(), false, 1280);
        view.player = Some(playing(true));
        view.focus = Some(Focus::PlayPause);
        let l = layout(&view);
        let (items, focused) = items(&view, &l, (0.0, 0.0));
        assert_eq!(items[0].role, Role::Label);
        assert_eq!(items[0].label, "Now playing: Night Drive by Lumen");
        assert_eq!(items[1].label, "Previous");
        let play = &items[focused.unwrap()];
        assert_eq!((play.role, play.label.as_str()), (Role::Button, "Pause"));
        assert_eq!(items.len(), ring(&view).len() + 1);
    }

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pixmap.pixel(x, y).unwrap().demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn an_on_round_toggle_is_the_accent_and_an_on_pill_is_tinted() {
        let tokens = Tokens::built_in();
        let mut view = view_of(&laptop(), false, 1280);
        // Do not disturb is on; Wi-Fi is on (the laptop's); Airplane is off.
        view.tiles[3].on = true;
        let l = layout(&view);
        for s in [1u32, 2] {
            let (w, h) = (WIDTH * s, l.size.1 * s);
            let mut pixmap = Pixmap::new(w, h).unwrap();
            paint(&mut pixmap, &view, &tokens, None, s as f32);
            assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "a round corner");
            let card = tokens.panel.bytes();
            // The card's own colour between the tiles and above them.
            assert_eq!(pixel(&pixmap, 180 * s, 10 * s), card);
            // Four pixels in from a round toggle's left end, at its middle,
            // it is the accent when on and not when off.
            let at = |r: Rect, dx: f32, dy: f32| {
                pixel(
                    &pixmap,
                    ((r.x + dx) * s as f32) as u32,
                    ((r.y + dy) * s as f32) as u32,
                )
            };
            let dnd = l.tiles[3].whole;
            assert_eq!(at(dnd, 8.0, 32.0)[..3], tokens.accent.bytes()[..3]);
            let airplane = l.tiles[2].whole;
            assert_ne!(at(airplane, 8.0, 32.0)[..3], tokens.accent.bytes()[..3]);
            // Wi-Fi's pill is the accent over its page; Bluetooth's is not.
            let wifi = l.tiles[0].whole;
            let bluetooth = l.tiles[1].whole;
            assert_eq!(at(wifi, 74.0, 4.0)[..3], tokens.accent.bytes()[..3]);
            assert_ne!(at(bluetooth, 74.0, 4.0)[..3], tokens.accent.bytes()[..3]);
            assert_ne!(
                at(bluetooth, 74.0, 4.0),
                card,
                "the off pill is not the card"
            );
        }
    }

    #[test]
    fn the_slider_fills_with_the_knob_to_the_volume() {
        let tokens = Tokens::built_in();
        let mut view = view_of(&laptop(), false, 1280);
        view.volume.as_mut().unwrap().percent = 20;
        let l = layout(&view);
        let mut pixmap = Pixmap::new(WIDTH, l.size.1).unwrap();
        paint(&mut pixmap, &view, &tokens, None, 1.0);
        let t = l.track.unwrap();
        let y = t.middle() as u32;
        // At 20 percent the fill reaches a fifth of the track, the knob's
        // colour at a tenth of it; the track's own faint colour near its end.
        let at = |share: f32| pixel(&pixmap, (t.x + t.w * share) as u32, y);
        assert_eq!(at(0.1)[..3], knob(&tokens).bytes()[..3]);
        assert_ne!(at(0.95)[..3], knob(&tokens).bytes()[..3]);
    }

    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    #[test]
    fn the_card_draws_with_fonts_light_and_dark_and_writes_pngs() {
        let Some(mut text) = fonts() else {
            return; // no fonts on this machine
        };
        for (scheme, mode) in [
            (edel::tokens::Scheme::Light, "light"),
            (edel::tokens::Scheme::Dark, "dark"),
        ] {
            let tokens = Tokens::built_in_scheme(scheme);
            let dark = mode == "dark";
            for (width, name, list) in [
                (1280, "desktop", false),
                (1280, "desktop-list", true),
                (1280, "desktop-player", false),
                (360, "compact", false),
            ] {
                let card = if width < COMPACT_BELOW { width } else { WIDTH };
                // A backlight too, so the card shows both sliders.
                let mut status = laptop();
                status.brightness = Some(70);
                let mut view = view_of(&status, dark, width);
                view.list = list;
                if name == "desktop" {
                    view.hover = Some(Focus::Toggle(1));
                }
                if name == "desktop-player" {
                    view.player = Some(playing(true));
                }
                let l = layout(&view);
                let mut pixmap = Pixmap::new(card * 2, l.size.1 * 2).unwrap();
                paint(&mut pixmap, &view, &tokens, Some(&mut text), 2.0);
                // Not one colour, and the words are drawn: some ink in each tile.
                let ink = |r: Rect| {
                    let (x, y, w, h) = r.device(2.0);
                    (y as u32..(y + h) as u32)
                        .flat_map(|py| (x as u32..(x + w) as u32).map(move |px| (px, py)))
                        .filter(|&(px, py)| {
                            pixel(&pixmap, px, py)[..3] != tokens.panel.bytes()[..3]
                        })
                        .count()
                };
                if let Some(player) = l.player {
                    assert!(ink(player) > 20, "{name} {mode}: the player is drawn");
                }
                for (t, b) in view.tiles.iter().zip(&l.tiles) {
                    // A round toggle's title sits under its circle.
                    let label = if t.tile.pill() {
                        0.0
                    } else {
                        metrics(view.compact).label
                    };
                    let r = Rect::new(b.whole.x, b.whole.y, b.whole.w, b.whole.h + label);
                    assert!(ink(r) > 20, "{name} {mode}: {} is drawn", t.tile.name());
                }
                if let Some(dir) = std::env::var_os("EDEL_QUICK_PNG") {
                    let dir = std::path::PathBuf::from(dir);
                    std::fs::create_dir_all(&dir).unwrap();
                    // Over the screen's colour, with the shadow's room.
                    let room = 24u32 * 2;
                    let mut screen =
                        Pixmap::new(pixmap.width() + 2 * room, pixmap.height() + 2 * room).unwrap();
                    screen.fill(
                        tiny_skia::Color::from_rgba(
                            tokens.background.r,
                            tokens.background.g,
                            tokens.background.b,
                            1.0,
                        )
                        .unwrap(),
                    );
                    if let Some(shadow) = paint::shadow(
                        pixmap.width(),
                        pixmap.height(),
                        room,
                        RADIUS * 2.0,
                        &tokens,
                        2.0,
                    ) {
                        screen.draw_pixmap(
                            0,
                            0,
                            shadow.as_ref(),
                            &PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                    }
                    screen.draw_pixmap(
                        room as i32,
                        room as i32,
                        pixmap.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                    screen
                        .save_png(dir.join(format!("quick-{name}-{mode}.png")))
                        .unwrap();
                }
            }
        }
    }
}
