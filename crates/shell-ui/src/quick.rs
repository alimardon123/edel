//! Quick settings (M5.9a): the card the status area opens above itself, a
//! sheet across the screen at Compact width. Its look is the mockups'
//! (`docs/mockups/quick-settings.jpg`, `phone-quick.jpg`), drawn from the
//! tokens: the menus' card, then
//!
//! - **tiles** in two columns, `radius_control` corners, never pills: the
//!   left part toggles, a chevron part behind a hairline opens the tile's
//!   Settings page; an "on" tile has a soft accent tint, an accent
//!   hairline, an accent icon circle and an accent state line;
//! - the **volume**: a caption with the output named in a chip that opens
//!   the list of outputs, a slider, the speaker icon muting;
//! - a **footer**: the battery's line, and a square Settings button.
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
use crate::popup;
use crate::status::{Link, Status};

/// The card's width on a screen of any size from Compact up, logical
/// pixels; below `COMPACT_BELOW` it is as wide as the screen.
pub const WIDTH: u32 = 360;
/// A screen narrower than this is Compact (M5.6c's size classes): the
/// card is a sheet and its touch targets are at least 44 px.
pub const COMPACT_BELOW: u32 = 600;
/// The most outputs the list shows.
pub const MOST_OUTPUTS: usize = 6;
/// A step of the volume on the keyboard, percent.
pub const STEP: u32 = 5;
/// The Settings page the Dark style tile's arrow opens: Appearance has no
/// page in Settings yet (M5.12), so Layout, which has the colour scheme's
/// neighbours; one name to change when it does.
pub const DARK_PAGE: &str = "layout";

/// A tile of the card: the names are what `[quick] tiles` lists
/// (`edel::presets::TILES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Wifi,
    Bluetooth,
    Airplane,
    DarkStyle,
}

/// Every tile, in the order `edel::presets::TILES` lists them.
pub const ALL: [Tile; 4] = [Tile::Wifi, Tile::Bluetooth, Tile::Airplane, Tile::DarkStyle];

impl Tile {
    pub fn name(self) -> &'static str {
        match self {
            Tile::Wifi => "wifi",
            Tile::Bluetooth => "bluetooth",
            Tile::Airplane => "airplane",
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
            Tile::DarkStyle => tr("Dark style"),
        }
    }

    /// The shell's icon in its circle.
    pub fn icon(self) -> &'static str {
        match self {
            Tile::Wifi => "net-wifi-3",
            Tile::Bluetooth => "page-bluetooth",
            Tile::Airplane => "airplane",
            Tile::DarkStyle => "moon",
        }
    }

    /// The Settings page its arrow opens, `edel-settings --page NAME`;
    /// none for a tile that has no page of its own.
    pub fn page(self) -> Option<&'static str> {
        match self {
            Tile::Wifi => Some("network"),
            Tile::Bluetooth => Some("bluetooth"),
            Tile::Airplane => None,
            Tile::DarkStyle => Some(DARK_PAGE),
        }
    }
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
    /// The output in use and the width of the chip that names it,
    /// logical pixels.
    pub output: String,
    pub chip: f32,
    /// Every output, with the one in use marked.
    pub outputs: Vec<(String, bool)>,
}

/// The footer's battery.
#[derive(Debug, Clone, PartialEq)]
pub struct BatteryView {
    pub percent: u32,
    pub charging: bool,
    pub line: String,
}

/// Where a pointer or the keyboard is on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The left part of tile `i`
    Toggle(usize),
    /// Its chevron part
    Page(usize),
    /// The chip that opens the list of outputs
    Output,
    /// Row `i` of that list
    Choose(usize),
    /// The speaker icon
    Mute,
    Slider,
    Settings,
}

/// Everything the card shows at one moment, so it is drawn again only when
/// it changes.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The card's width, logical pixels, and whether it is a sheet.
    pub width: u32,
    pub compact: bool,
    pub tiles: Vec<TileView>,
    pub volume: Option<VolumeView>,
    pub battery: Option<BatteryView>,
    /// Whether the Settings button is there.
    pub settings: bool,
    /// Whether the list of outputs is open.
    pub list: bool,
    /// Where the keyboard is, once a key was pressed, and the pointer.
    pub focus: Option<Focus>,
    pub hover: Option<Focus>,
}

/// What a person's click or key asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    Toggle(usize),
    Page(usize),
    /// Open or close the list of outputs.
    List,
    Choose(usize),
    Mute,
    /// The volume to this percent.
    Volume(u32),
    Settings,
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
/// and one named twice or not known is left out. `dark` is whether the
/// colour scheme is dark; `pending` holds what a person just chose and
/// the machine has not yet said, which shows at once.
pub fn tiles(
    names: &[String],
    status: &Status,
    dark: bool,
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
            Tile::DarkStyle => TileView {
                tile,
                state: switch(dark),
                on: dark,
            },
        };
        shown.push(view);
    }
    shown
}

/// The volume part of the card: the output in use and the list of all.
/// `percent` and `muted` are what the slider was just moved to or the
/// speaker just clicked, which show over what the sound system last said;
/// `chip` is the width of the chip naming the output, measured with the
/// card's fonts.
pub fn volume(
    status: &Status,
    percent: Option<u32>,
    muted: Option<bool>,
    chip: impl FnOnce(&str) -> f32,
) -> Option<VolumeView> {
    let v = status.volume.as_ref()?;
    Some(VolumeView {
        percent: percent.unwrap_or(v.percent),
        muted: muted.unwrap_or(v.muted),
        chip: chip(&v.output),
        output: v.output.clone(),
        outputs: status
            .outputs
            .iter()
            .take(MOST_OUTPUTS)
            .map(|o| (o.name.clone(), o.default))
            .collect(),
    })
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
    pub list: bool,
    pub focus: Option<Focus>,
    pub hover: Option<Focus>,
    pub pending: Vec<(Tile, bool)>,
    pub volume: Option<u32>,
    pub muted: Option<bool>,
}

/// The card showing `status` and `state`, with the tiles `names` lists
/// (the preset's); `settings` says whether the machine has the Settings
/// app, and `chip` measures the output's chip.
pub fn view(
    state: &State,
    status: &Status,
    names: &[String],
    settings: bool,
    chip: impl FnOnce(&str) -> f32,
) -> View {
    View {
        width: state.width,
        compact: state.compact,
        tiles: tiles(names, status, state.dark, &state.pending),
        volume: volume(status, state.volume, state.muted, chip),
        battery: status.battery.as_ref().map(|b| BatteryView {
            percent: b.percent,
            charging: b.charging,
            line: b.line.clone(),
        }),
        settings,
        list: state.list,
        focus: state.focus,
        hover: state.hover,
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

/// The width of the chip naming an output whose name is `text_width`
/// logical pixels wide: room before the name, the name, the chevron after
/// it, at most `most`.
pub fn chip_width(text_width: f32, most: f32) -> f32 {
    (CHIP_LEFT + text_width + CHIP_GAP + CHIP_CHEVRON + CHIP_RIGHT).min(most)
}

/// The widest the output's chip grows, logical pixels.
pub const CHIP_MOST: f32 = 190.0;
const CHIP_LEFT: f32 = 9.0;
const CHIP_GAP: f32 = 5.0;
const CHIP_CHEVRON: f32 = 11.0;
const CHIP_RIGHT: f32 = 6.0;

// ---- Where it lies ----

/// A rectangle, logical pixels from the card's top left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn right(&self) -> f32 {
        self.x + self.w
    }

    fn middle(&self) -> f32 {
        self.y + self.h / 2.0
    }

    /// At scale `s`, on whole pixels.
    fn device(&self, s: f32) -> (f32, f32, f32, f32) {
        let (x, y) = ((self.x * s).round(), (self.y * s).round());
        let (r, b) = (
            ((self.x + self.w) * s).round(),
            ((self.y + self.h) * s).round(),
        );
        (x, y, r - x, b - y)
    }
}

/// The sizes of the card's parts: touch sizes on a Compact screen (at
/// least 44 px where a finger lands), the mockups' otherwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub pad: f32,
    pub gap: f32,
    pub tile: f32,
    pub chevron: f32,
    pub circle: f32,
    pub caption: f32,
    pub slider: f32,
    pub row: f32,
    pub button: f32,
    pub icon: f32,
}

pub fn metrics(compact: bool) -> Metrics {
    if compact {
        Metrics {
            pad: 14.0,
            gap: 8.0,
            tile: 64.0,
            chevron: 44.0,
            circle: 32.0,
            caption: 44.0,
            slider: 44.0,
            row: 44.0,
            button: 44.0,
            icon: 17.0,
        }
    } else {
        Metrics {
            pad: 12.0,
            gap: 8.0,
            tile: 56.0,
            chevron: 28.0,
            circle: 30.0,
            caption: 24.0,
            slider: 26.0,
            row: 32.0,
            button: 28.0,
            icon: 15.0,
        }
    }
}

/// Between the card's sections, and the footer's room above its row.
const SECTION: f32 = 12.0;
/// What a section keeps from the tiles' edge on each side.
const INSET: f32 = 4.0;
/// Where the slider's track starts after the speaker, and how far its
/// knob stays from each end, logical pixels.
const SPEAKER: f32 = 26.0;
const KNOB: f32 = 18.0;

/// One tile's parts.
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
    pub caption: Option<Rect>,
    pub chip: Option<Rect>,
    pub list: Vec<Rect>,
    pub mute: Option<Rect>,
    /// The slider: its track beside the speaker on a desktop, the whole
    /// bar on a Compact screen.
    pub track: Option<Rect>,
    /// The footer's hairline, its battery and its button.
    pub rule: Option<f32>,
    pub battery: Option<Rect>,
    pub settings: Option<Rect>,
}

/// Where the parts of `view` lie.
pub fn layout(view: &View) -> Layout {
    let m = metrics(view.compact);
    let w = view.width as f32;
    let mut y = m.pad;
    let mut first = true;
    let mut gap = |y: &mut f32| {
        if !first {
            *y += SECTION;
        }
        first = false;
    };
    // The tiles, two columns; a last one alone keeps the left column.
    let mut tiles = Vec::new();
    if !view.tiles.is_empty() {
        gap(&mut y);
        let col = ((w - 2.0 * m.pad - m.gap) / 2.0).floor();
        let second = m.pad + col + m.gap;
        for (i, t) in view.tiles.iter().enumerate() {
            let (c, r) = (i % 2, i / 2);
            let x = if c == 0 { m.pad } else { second };
            let width = if c == 0 { col } else { w - m.pad - second };
            let top = y + r as f32 * (m.tile + m.gap);
            let whole = Rect::new(x, top, width, m.tile);
            let has_page = t.tile.page().is_some();
            let (toggle, page) = if has_page {
                (
                    Rect::new(x, top, width - m.chevron, m.tile),
                    Some(Rect::new(x + width - m.chevron, top, m.chevron, m.tile)),
                )
            } else {
                (whole, None)
            };
            tiles.push(TileBox {
                whole,
                toggle,
                page,
            });
        }
        let rows = view.tiles.len().div_ceil(2);
        y += rows as f32 * m.tile + (rows - 1) as f32 * m.gap;
    }
    let (mut caption, mut chip, mut list, mut mute, mut track) =
        (None, None, Vec::new(), None, None);
    if let Some(volume) = &view.volume {
        gap(&mut y);
        let (x, inner) = (m.pad + INSET, w - 2.0 * (m.pad + INSET));
        let row = Rect::new(x, y, inner, m.caption);
        caption = Some(row);
        let chip_w = volume.chip.min(inner);
        // A chip's hit area is its row's whole height.
        chip = Some(Rect::new(row.right() - chip_w, y, chip_w, m.caption));
        y += m.caption;
        if view.list {
            for i in 0..volume.outputs.len() {
                list.push(Rect::new(
                    m.pad,
                    y + i as f32 * m.row,
                    w - 2.0 * m.pad,
                    m.row,
                ));
            }
            y += volume.outputs.len() as f32 * m.row;
        }
        y += 2.0;
        if view.compact {
            let bar = Rect::new(m.pad, y, w - 2.0 * m.pad, m.slider);
            mute = Some(Rect::new(bar.x, bar.y, m.slider, m.slider));
            track = Some(bar);
        } else {
            mute = Some(Rect::new(x, y, SPEAKER, m.slider));
            track = Some(Rect::new(x + SPEAKER, y, inner - SPEAKER, m.slider));
        }
        y += m.slider;
    }
    let (mut rule, mut battery, mut settings) = (None, None, None);
    if view.battery.is_some() || view.settings {
        gap(&mut y);
        rule = Some(y);
        let row = y + 1.0 + SECTION;
        let (x, inner) = (m.pad + INSET, w - 2.0 * (m.pad + INSET));
        let button = Rect::new(x + inner - m.button, row, m.button, m.button);
        if view.settings {
            settings = Some(button);
        }
        if view.battery.is_some() {
            let room = if view.settings {
                inner - m.button - 8.0
            } else {
                inner
            };
            battery = Some(Rect::new(x, row, room, m.button));
        }
        y = row + m.button;
    }
    y += m.pad;
    Layout {
        size: (view.width, y.ceil() as u32),
        metrics: m,
        tiles,
        caption,
        chip,
        list,
        mute,
        track,
        rule,
        battery,
        settings,
    }
}

/// Where the card's parts lie, as one log line CI reads to click them:
/// `card WxH, NAME X+Y+WxH, ..., track X+Y+WxH, ...`, logical pixels from
/// the card's corner; each tile by its name, then the volume's track, the
/// speaker (`mute`), the output's `chip` and the `settings` button.
pub fn places(view: &View, layout: &Layout) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut parts = vec![format!("card {}x{}", layout.size.0, layout.size.1)];
    for (t, b) in view.tiles.iter().zip(&layout.tiles) {
        parts.push(format!("{} {}", t.tile.name(), at(b.whole)));
    }
    for (name, rect) in [
        ("track", layout.track),
        ("mute", layout.mute),
        ("chip", layout.chip),
        ("settings", layout.settings),
    ] {
        if let Some(rect) = rect {
            parts.push(format!("{name} {}", at(rect)));
        }
    }
    parts.join(", ")
}

/// The part of the card at `x`, `y` (logical pixels from its corner).
pub fn hit(layout: &Layout, x: f32, y: f32) -> Option<Focus> {
    for (i, t) in layout.tiles.iter().enumerate() {
        if t.page.is_some_and(|p| p.contains(x, y)) {
            return Some(Focus::Page(i));
        }
        if t.toggle.contains(x, y) {
            return Some(Focus::Toggle(i));
        }
    }
    if layout.chip.is_some_and(|r| r.contains(x, y)) {
        return Some(Focus::Output);
    }
    if let Some(i) = layout.list.iter().position(|r| r.contains(x, y)) {
        return Some(Focus::Choose(i));
    }
    if layout.mute.is_some_and(|r| r.contains(x, y)) {
        return Some(Focus::Mute);
    }
    if layout.track.is_some_and(|r| r.contains(x, y)) {
        return Some(Focus::Slider);
    }
    if layout.settings.is_some_and(|r| r.contains(x, y)) {
        return Some(Focus::Settings);
    }
    None
}

/// The volume at `x` along the slider, percent, 0 to 100: the knob keeps
/// half its width from each end of the track on a desktop; the Compact
/// bar fills from its left end to the finger.
pub fn volume_at(layout: &Layout, compact: bool, x: f32) -> u32 {
    let Some(track) = layout.track else {
        return 0;
    };
    let (from, span) = if compact {
        (track.x, track.w)
    } else {
        (track.x + KNOB / 2.0, track.w - KNOB)
    };
    let share = ((x - from) / span.max(1.0)).clamp(0.0, 1.0);
    (share * 100.0).round() as u32
}

// ---- Keys ----

/// What a keyboard can reach, in Tab's order: each tile's toggle and its
/// arrow, the output chip, the list's rows while it is open, the speaker,
/// the slider and the Settings button.
pub fn ring(view: &View) -> Vec<Focus> {
    let mut ring = Vec::new();
    for (i, t) in view.tiles.iter().enumerate() {
        ring.push(Focus::Toggle(i));
        if t.tile.page().is_some() {
            ring.push(Focus::Page(i));
        }
    }
    if let Some(volume) = &view.volume {
        ring.push(Focus::Output);
        if view.list {
            ring.extend((0..volume.outputs.len()).map(Focus::Choose));
        }
        ring.push(Focus::Mute);
        ring.push(Focus::Slider);
    }
    if view.settings {
        ring.push(Focus::Settings);
    }
    ring
}

/// What `key` does with the keyboard on `focus`: where it goes and what
/// it asks for. The first key to move gives the first part, or with Shift
/// and Up or Left the last.
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
    match (key, now) {
        (Key::Activate, Focus::Toggle(i)) => (focus, Some(Act::Toggle(i))),
        (Key::Activate, Focus::Page(i)) => (focus, Some(Act::Page(i))),
        (Key::Activate, Focus::Output) => (focus, Some(Act::List)),
        (Key::Activate, Focus::Choose(i)) => (focus, Some(Act::Choose(i))),
        (Key::Activate, Focus::Mute | Focus::Slider) => (focus, Some(Act::Mute)),
        (Key::Activate, Focus::Settings) => (focus, Some(Act::Settings)),
        (Key::Left, Focus::Slider) => (focus, Some(Act::Volume(volume.saturating_sub(STEP)))),
        (Key::Right, Focus::Slider) => (focus, Some(Act::Volume((volume + STEP).min(100)))),
        (Key::Home, Focus::Slider) => (focus, Some(Act::Volume(0))),
        (Key::End, Focus::Slider) => (focus, Some(Act::Volume(100))),
        (Key::Tab(false) | Key::Right, _) => (next(), None),
        (Key::Tab(true) | Key::Left, _) => (before(), None),
        (Key::Down | Key::Up, Focus::Toggle(i) | Focus::Page(i)) => {
            let down = key == Key::Down;
            let j = if down { i + 2 } else { i.wrapping_sub(2) };
            let part = |j: usize| match now {
                Focus::Page(_) if view.tiles.get(j).is_some_and(|t| t.tile.page().is_some()) => {
                    Focus::Page(j)
                }
                _ => Focus::Toggle(j),
            };
            if view.tiles.get(j).is_some() {
                (Some(part(j)), None)
            } else if down {
                let below = ring
                    .iter()
                    .find(|f| !matches!(f, Focus::Toggle(_) | Focus::Page(_)));
                (below.copied().or(focus), None)
            } else {
                (focus, None)
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
    let mut out = Vec::new();
    let mut focused = None;
    for (n, part) in ring(view).into_iter().enumerate() {
        let item = |role, label: String, bounds: Rect| Item {
            role,
            label,
            bounds: rect(bounds),
            children: Vec::new(),
            toggled: None,
            value: None,
        };
        let built = match part {
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
            Focus::Output => {
                let name = view.volume.as_ref().map_or("", |v| v.output.as_str());
                item(
                    Role::Button,
                    trf("Output: {name}", &[("name", name)]),
                    layout.chip.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
                )
            }
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
            Focus::Mute => {
                let muted = view.volume.as_ref().is_some_and(|v| v.muted);
                Item {
                    toggled: Some(muted),
                    ..item(
                        Role::Switch,
                        tr("Mute").to_string(),
                        layout.mute.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
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
                        layout.track.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
                    )
                }
            }
            Focus::Settings => item(
                Role::Button,
                tr("Settings").to_string(),
                layout.settings.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
        };
        if view.focus == Some(part) {
            focused = Some(n);
        }
        out.push(built);
    }
    (out, focused)
}

// ---- How it looks ----

/// The panel's text dimmed towards the card: secondary text and the
/// quieter kind, as the mockups' `text-2` and `text-3`.
fn dim(tokens: &Tokens) -> Colour {
    mix(tokens.panel_text, tokens.panel, 0.38)
}

/// The text's colour at `alpha` over the card: what the mockups' `fill`
/// and `fill-2` are.
fn veil(tokens: &Tokens, alpha: f32) -> Colour {
    Colour {
        a: alpha,
        ..tokens.panel_text
    }
}

/// The slider's knob: the lighter of the window's and the text's colour,
/// white on the light scheme and near white on the dark.
fn knob(tokens: &Tokens) -> Colour {
    let light = |c: Colour| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if light(tokens.window) > light(tokens.panel_text) {
        tokens.window
    } else {
        tokens.panel_text
    }
}

/// An icon centred in `r`, `px` logical pixels across.
fn icon_in(pixmap: &mut Pixmap, name: &str, px: f32, r: Rect, s: f32, c: Colour) {
    let side = (px * s).round();
    let (x, y, w, h) = r.device(s);
    paint::icon(
        pixmap,
        name,
        side,
        x + ((w - side) / 2.0).round(),
        y + ((h - side) / 2.0).round(),
        c,
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
    popup::card(pixmap, tokens, s);
    let l = layout(view);
    let m = l.metrics;
    let hair = (0.5 * s).max(1.0);
    let lit_by = |f: Focus| view.hover == Some(f);
    for (i, (t, b)) in view.tiles.iter().zip(&l.tiles).enumerate() {
        let (x, y, w, h) = b.whole.device(s);
        let r = tokens.radius_control as f32 * s;
        let back = if t.on {
            lit(tokens)
        } else {
            veil(tokens, 0.055)
        };
        fill(pixmap, x, y, w, h, r, back);
        let edge = if t.on {
            Colour {
                a: 0.34,
                ..tokens.accent
            }
        } else {
            tokens.line
        };
        outline(pixmap, (x, y, w, h), r, hair, edge);
        if lit_by(Focus::Toggle(i)) {
            fill(pixmap, x, y, w, h, r, veil(tokens, 0.04));
        }
        // The circle holding the icon.
        let c = m.circle;
        let lead = if view.compact { 8.0 } else { 10.0 };
        let circle = Rect::new(b.whole.x + lead, b.whole.middle() - c / 2.0, c, c);
        let (cx, cy, cw, ch) = circle.device(s);
        let (disc, ink) = if t.on {
            (tokens.accent, tokens.accent_text)
        } else {
            (veil(tokens, 0.085), tokens.panel_text)
        };
        fill(pixmap, cx, cy, cw, ch, cw / 2.0, disc);
        icon_in(pixmap, t.tile.icon(), m.icon, circle, s, ink);
        if let Some(text) = text.as_deref_mut() {
            // A Compact tile's words are a half pixel smaller and closer,
            // as the mockups' touch tiles, to fit beside the 44 px arrow.
            let (title_px, next) = if view.compact {
                (tokens.panel_text_size as f32 - 0.5, 8.0)
            } else {
                (tokens.panel_text_size as f32, 9.0)
            };
            let title_size = title_px * s;
            let state_size = tokens.panel_text_small_size as f32 * s;
            let left = (circle.right() + next) * s;
            let room = (b.toggle.right() - 6.0) * s - left;
            let mut title = text.fit_in(t.tile.title(), title_size, room, Face::SEMIBOLD);
            let mut state = text.fit(&t.state, state_size, room);
            let block = title_size * 1.25 + state_size * 1.25 + s;
            let top = y + (h - block) / 2.0;
            text.draw(pixmap, &mut title, left, top, tokens.panel_text);
            let ink = if t.on { tokens.accent } else { dim(tokens) };
            text.draw(pixmap, &mut state, left, top + title_size * 1.25 + s, ink);
        }
        if let Some(page) = b.page {
            let (px, py, pw, ph) = page.device(s);
            let rule = if t.on {
                Colour {
                    a: 0.24,
                    ..tokens.accent
                }
            } else {
                tokens.line
            };
            // A hairline the tile's height less its edge, its first pixel
            // the part's own.
            fill(pixmap, px, py + hair, hair, ph - 2.0 * hair, 0.0, rule);
            if lit_by(Focus::Page(i)) {
                fill(pixmap, px, py, pw, ph, r, veil(tokens, 0.06));
            }
            let ink = if t.on { tokens.accent } else { dim(tokens) };
            icon_in(pixmap, "chevron-right", 12.0, page, s, ink);
        }
        let ring = |pixmap: &mut Pixmap, r: Rect| {
            let (x, y, w, h) = r.device(s);
            outline(
                pixmap,
                (x, y, w, h),
                tokens.radius_control as f32 * s,
                2.0 * s,
                tokens.accent,
            );
        };
        if view.focus == Some(Focus::Toggle(i)) {
            ring(pixmap, b.toggle);
        }
        if view.focus == Some(Focus::Page(i)) {
            if let Some(page) = b.page {
                ring(pixmap, page);
            }
        }
    }
    if let (Some(v), Some(caption), Some(chip)) = (&view.volume, l.caption, l.chip) {
        volume_part(
            pixmap,
            view,
            v,
            &l,
            (caption, chip),
            tokens,
            text.as_deref_mut(),
            s,
        );
    }
    if let Some(rule) = l.rule {
        let (x, y, w, _) = Rect::new(m.pad, rule, view.width as f32 - 2.0 * m.pad, 1.0).device(s);
        fill(pixmap, x, y, w, hair, 0.0, tokens.line);
    }
    if let (Some(b), Some(room)) = (&view.battery, l.battery) {
        let px = (15.0 * s).round();
        let (x, y, _, h) = room.device(s);
        let halo = s.round().max(1.0) as i32;
        paint::battery(
            pixmap,
            px,
            (x, y + ((h - px) / 2.0).round()),
            (b.percent, b.charging),
            dim(tokens),
            halo,
        );
        if let Some(text) = text {
            let size = (tokens.panel_text_size as f32 - 1.0) * s;
            let left = x + (15.0 + 9.0) * s;
            let mut line = text.fit(&b.line, size, room.right() * s - left);
            let top = y + (h - size * 1.25) / 2.0;
            text.draw(pixmap, &mut line, left, top, dim(tokens));
        }
    }
    if let Some(button) = l.settings {
        let (x, y, w, h) = button.device(s);
        let r = tokens.radius_control as f32 * s;
        let back = if lit_by(Focus::Settings) {
            0.085
        } else {
            0.055
        };
        fill(pixmap, x, y, w, h, r, veil(tokens, back));
        icon_in(
            pixmap,
            "gear",
            14.0 * m.icon / 15.0,
            button,
            s,
            tokens.panel_text,
        );
        if view.focus == Some(Focus::Settings) {
            outline(pixmap, (x, y, w, h), r, 2.0 * s, tokens.accent);
        }
    }
}

/// The volume's caption, chip, list and slider.
#[allow(clippy::too_many_arguments)]
fn volume_part(
    pixmap: &mut Pixmap,
    view: &View,
    v: &VolumeView,
    l: &Layout,
    (caption, chip): (Rect, Rect),
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    s: f32,
) {
    let hair = (0.5 * s).max(1.0);
    let small = (tokens.panel_text_size as f32 - 1.0) * s;
    let chip_h = if view.compact { 32.0 } else { 24.0 };
    let shown = Rect::new(chip.x, chip.middle() - chip_h / 2.0, chip.w, chip_h);
    let (x, y, w, h) = shown.device(s);
    let r = tokens.radius_small as f32 * s;
    let hovered = view.hover == Some(Focus::Output);
    fill(
        pixmap,
        x,
        y,
        w,
        h,
        r,
        veil(tokens, if hovered || view.list { 0.085 } else { 0.055 }),
    );
    if let Some(text) = text.as_deref_mut() {
        let mut label = text.line_in(tr("Volume"), small, Face::SEMIBOLD);
        let top = (caption.middle() * s - small * 0.625).round();
        text.draw(
            pixmap,
            &mut label,
            (caption.x * s).round(),
            top,
            dim(tokens),
        );
        let room = (shown.w - CHIP_LEFT - CHIP_GAP - CHIP_CHEVRON - CHIP_RIGHT) * s;
        let mut name = text.fit_in(&v.output, small, room, Face::MEDIUM);
        let at = x + CHIP_LEFT * s;
        text.draw(
            pixmap,
            &mut name,
            at,
            y + (h - small * 1.25) / 2.0,
            dim(tokens),
        );
    }
    let chevron = Rect::new(
        shown.right() - CHIP_RIGHT - CHIP_CHEVRON,
        shown.y,
        CHIP_CHEVRON,
        shown.h,
    );
    icon_in(
        pixmap,
        "chevron-down",
        CHIP_CHEVRON,
        chevron,
        s,
        dim(tokens),
    );
    if view.focus == Some(Focus::Output) {
        outline(pixmap, (x, y, w, h), r, 2.0 * s, tokens.accent);
    }
    // The list of outputs, open under the caption.
    for (i, (rect, (name, current))) in l.list.iter().zip(&v.outputs).enumerate() {
        let (rx, ry, rw, rh) = rect.device(s);
        let rr = tokens.radius_control as f32 * s;
        if *current {
            fill(pixmap, rx, ry, rw, rh, rr, lit(tokens));
        } else if view.hover == Some(Focus::Choose(i)) {
            fill(pixmap, rx, ry, rw, rh, rr, veil(tokens, 0.05));
        }
        if let Some(text) = text.as_deref_mut() {
            let size = tokens.panel_text_size as f32 * s;
            let left = rx + popup::INSET * s;
            let mut line = text.fit(name, size, rw - (popup::INSET * 2.0 + 20.0) * s);
            text.draw(
                pixmap,
                &mut line,
                left,
                ry + (rh - size * 1.25) / 2.0,
                tokens.panel_text,
            );
        }
        if *current {
            let tick = Rect::new(rect.right() - 12.0 - 16.0, rect.y, 16.0, rect.h);
            icon_in(pixmap, "check", 14.0, tick, s, tokens.accent);
        }
        if view.focus == Some(Focus::Choose(i)) {
            outline(pixmap, (rx, ry, rw, rh), rr, 2.0 * s, tokens.accent);
        }
    }
    let (Some(mute), Some(track)) = (l.mute, l.track) else {
        return;
    };
    let share = v.percent.min(100) as f32 / 100.0;
    let fill_colour = if v.muted {
        mix(tokens.accent, tokens.panel, 0.55)
    } else {
        tokens.accent
    };
    let icon = if v.muted || v.percent == 0 {
        "volume-muted"
    } else if v.percent <= 33 {
        "volume-low"
    } else if v.percent <= 66 {
        "volume-medium"
    } else {
        "volume-high"
    };
    if view.compact {
        // A bar the finger fills: the speaker sits in its left end.
        let (bx, by, bw, bh) = track.device(s);
        let rr = tokens.radius_menu as f32 * s * 0.85;
        fill(pixmap, bx, by, bw, bh, rr, veil(tokens, 0.085));
        let filled = (bw * share).round();
        if filled > 0.0 {
            fill(pixmap, bx, by, filled.max(rr), bh, rr, fill_colour);
        }
        let covered = filled >= (mute.w * s) * 0.7;
        let ink = if covered {
            tokens.accent_text
        } else {
            dim(tokens)
        };
        icon_in(pixmap, icon, 18.0, mute, s, ink);
        if view.focus == Some(Focus::Slider) || view.focus == Some(Focus::Mute) {
            outline(pixmap, (bx, by, bw, bh), rr, 2.0 * s, tokens.accent);
        }
        return;
    }
    let hovered = view.hover == Some(Focus::Mute);
    let ink = if hovered {
        tokens.panel_text
    } else {
        dim(tokens)
    };
    let speaker = Rect::new(mute.x, mute.y, 16.0, mute.h);
    icon_in(pixmap, icon, 16.0, speaker, s, ink);
    let (tx, ty, tw, th) = track.device(s);
    let thick = (6.0 * s).round();
    let (a, b) = (tx + KNOB / 2.0 * s, tx + tw - KNOB / 2.0 * s);
    let at = a + (b - a) * share;
    let top = ty + ((th - thick) / 2.0).round();
    fill(pixmap, tx, top, tw, thick, thick / 2.0, veil(tokens, 0.085));
    fill(
        pixmap,
        tx,
        top,
        (at - tx).max(thick),
        thick,
        thick / 2.0,
        fill_colour,
    );
    // The knob, with a soft shadow under it and a hairline round it.
    let d = (KNOB * s).round();
    let (kx, ky) = ((at - d / 2.0).round(), ty + ((th - d) / 2.0).round());
    let shadow = Colour {
        a: 0.22,
        ..tokens.shadow
    };
    fill(pixmap, kx, ky + s, d, d, d / 2.0, shadow);
    fill(pixmap, kx, ky, d, d, d / 2.0, knob(tokens));
    outline(pixmap, (kx, ky, d, d), d / 2.0, hair, tokens.line);
    if view.focus == Some(Focus::Slider) {
        outline(
            pixmap,
            (kx - 2.0 * s, ky - 2.0 * s, d + 4.0 * s, d + 4.0 * s),
            d / 2.0 + 2.0 * s,
            2.0 * s,
            tokens.accent,
        );
    }
    if view.focus == Some(Focus::Mute) {
        let (sx, sy, sw, sh) = speaker.device(s);
        outline(
            pixmap,
            (sx - 2.0 * s, sy, sw + 4.0 * s, sh),
            tokens.radius_small as f32 * s,
            2.0 * s,
            tokens.accent,
        );
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
            }),
            bluetooth: Some(false),
        }
    }

    /// The card on a screen `screen` px wide: a sheet below Compact's edge,
    /// else the 360 px card.
    fn view_of(status: &Status, dark: bool, screen: u32) -> View {
        let list = names(edel::presets::DEFAULT_TILES);
        let compact = screen < COMPACT_BELOW;
        let state = State {
            width: if compact { screen } else { WIDTH },
            compact,
            dark,
            ..State::default()
        };
        view(&state, status, &list, true, |_| 90.0)
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
    }

    #[test]
    fn dark_styles_arrow_opens_a_page_the_settings_app_builds() {
        // The Settings app has no Appearance page yet (M5.12): its pages
        // are the arms of its page table. When it has, DARK_PAGE becomes
        // "appearance" and this test says so.
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
    }

    #[test]
    fn a_laptop_shows_every_tile_in_the_presets_order_with_what_each_says() {
        let list = names(edel::presets::DEFAULT_TILES);
        let shown = tiles(&list, &laptop(), false, &[]);
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
                ("dark_style", "Off", false),
            ]
        );
        let dark = tiles(&list, &laptop(), true, &[]);
        assert_eq!((dark[3].state.as_str(), dark[3].on), ("On", true));
    }

    #[test]
    fn a_tile_whose_hardware_is_missing_is_left_out() {
        let list = names(edel::presets::DEFAULT_TILES);
        // The test machine's VM: a network, no Wi-Fi adapter, no Bluetooth.
        let vm = Status {
            network: network(Link::Wired, false, false),
            ..Status::default()
        };
        let shown: Vec<_> = tiles(&list, &vm, false, &[])
            .iter()
            .map(|t| t.tile)
            .collect();
        assert_eq!(shown, [Tile::DarkStyle]);
        // Bluetooth alone is enough for flight mode.
        let bluetooth = Status {
            bluetooth: Some(true),
            ..Status::default()
        };
        let shown: Vec<_> = tiles(&list, &bluetooth, false, &[])
            .iter()
            .map(|t| t.tile)
            .collect();
        assert_eq!(shown, [Tile::Bluetooth, Tile::Airplane, Tile::DarkStyle]);
        // A name twice or unknown shows nothing extra.
        let odd = names(&["dark_style", "dark_style", "night_light"]);
        assert_eq!(tiles(&odd, &laptop(), false, &[]).len(), 1);
    }

    #[test]
    fn wifi_on_without_a_network_says_so_and_a_choice_shows_at_once() {
        let mut status = laptop();
        status.network = network(Link::Offline, true, true);
        let list = names(&["wifi", "airplane"]);
        assert_eq!(tiles(&list, &status, false, &[])[0].state, "Not connected");
        // Just switched off, the machine not yet saying so.
        let pending = [(Tile::Wifi, false), (Tile::Airplane, true)];
        let shown = tiles(&list, &laptop(), false, &pending);
        assert_eq!((shown[0].state.as_str(), shown[0].on), ("Off", false));
        assert_eq!((shown[1].state.as_str(), shown[1].on), ("On", true));
    }

    #[test]
    fn the_card_is_360_wide_and_its_tiles_lie_in_two_columns() {
        let view = view_of(&laptop(), false, 1280);
        let l = layout(&view);
        assert_eq!(l.size.0, 360);
        let m = l.metrics;
        // 12 px in, 8 between: two tiles of 164.
        assert_eq!(l.tiles[0].whole, Rect::new(12.0, 12.0, 164.0, 56.0));
        assert_eq!(l.tiles[1].whole, Rect::new(184.0, 12.0, 164.0, 56.0));
        assert_eq!(l.tiles[2].whole.y, 12.0 + 56.0 + 8.0);
        // The arrow is the tile's right 28 px; Airplane has none.
        let page = l.tiles[0].page.unwrap();
        assert_eq!((page.x, page.w), (12.0 + 164.0 - 28.0, 28.0));
        assert_eq!(l.tiles[0].toggle.w, 164.0 - 28.0);
        assert_eq!(l.tiles[2].page, None);
        assert_eq!(l.tiles[2].toggle, l.tiles[2].whole);
        // Nothing lies outside the card.
        let bottom = l.size.1 as f32;
        for r in [l.tiles[3].whole, l.settings.unwrap(), l.track.unwrap()] {
            assert!(r.x >= 0.0 && r.right() <= 360.0 && r.y + r.h <= bottom - m.pad + 0.5);
        }
        // The footer's button is square and at the card's bottom right.
        let b = l.settings.unwrap();
        assert_eq!((b.w, b.h), (28.0, 28.0));
        assert_eq!(b.right(), 360.0 - 12.0 - 4.0);
    }

    #[test]
    fn at_compact_width_it_is_a_sheet_with_touch_targets() {
        let mut view = view_of(&laptop(), false, 360);
        assert!(view.compact);
        for width in [360, 599] {
            view.width = width;
            let l = layout(&view);
            assert_eq!(l.size.0, width);
            let targets = l
                .tiles
                .iter()
                .flat_map(|t| [Some(t.toggle), t.page])
                .flatten()
                .chain([l.chip, l.mute, l.track, l.settings].into_iter().flatten());
            for r in targets {
                assert!(r.h >= 44.0, "{r:?} is under 44 px high");
            }
            for r in l.tiles.iter().filter_map(|t| t.page).chain(l.settings) {
                assert!(r.w >= 44.0, "{r:?} is under 44 px wide");
            }
            assert!(l.tiles[1].whole.right() <= width as f32);
        }
        // From 600 px on it is the 360 px card.
        view.compact = false;
        view.width = WIDTH;
        assert!(layout(&view).size.1 < 300);
    }

    #[test]
    fn the_card_grows_with_its_tiles_and_the_list_and_loses_what_is_not_there() {
        let full = layout(&view_of(&laptop(), false, 1280)).size.1;
        let mut view = view_of(&laptop(), false, 1280);
        view.list = true;
        let open = layout(&view);
        assert_eq!(open.size.1, full + 64, "two rows of 32");
        assert_eq!(open.list.len(), 2);
        // The list lies between the caption and the slider.
        assert!(
            open.list[0].y
                >= open
                    .caption
                    .unwrap()
                    .right()
                    .min(open.caption.unwrap().y + 24.0)
        );
        assert!(open.list[1].y + 32.0 <= open.track.unwrap().y);
        let mut bare = view_of(&Status::default(), false, 1280);
        bare.settings = false;
        let l = layout(&bare);
        assert_eq!(l.tiles.len(), 1, "only Dark style");
        assert!(l.track.is_none() && l.rule.is_none() && l.settings.is_none());
        let mut one = view_of(&laptop(), false, 1280);
        one.tiles.truncate(3);
        let l = layout(&one);
        assert_eq!(
            l.tiles[2].whole.right(),
            12.0 + 164.0,
            "a last one keeps the left column"
        );
    }

    #[test]
    fn the_log_line_says_where_each_part_lies() {
        let view = view_of(&laptop(), false, 1280);
        let l = layout(&view);
        let line = places(&view, &l);
        assert!(
            line.starts_with(&format!(
                "card 360x{}, wifi 12+12+164x56, bluetooth 184+12+164x56, ",
                l.size.1
            )),
            "{line}"
        );
        assert!(
            line.contains(", dark_style 184+76+164x56, track 42+"),
            "{line}"
        );
        assert!(
            line.ends_with(&format!(
                ", settings 316+{:.0}+28x28",
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
        let v = view(&state, &laptop(), &list, false, |_| 70.0);
        assert!(!v.tiles[0].on, "Wi-Fi shows off before the machine says so");
        let volume = v.volume.unwrap();
        assert_eq!(
            (volume.percent, volume.muted, volume.chip),
            (30, true, 70.0)
        );
        assert!(!v.settings);
        let machine = view(&State::default(), &laptop(), &list, true, |_| 70.0);
        assert_eq!(machine.volume.unwrap().percent, 62);
    }

    #[test]
    fn a_pointer_finds_the_halves_of_a_tile_and_the_slider() {
        let view = view_of(&laptop(), false, 1280);
        let l = layout(&view);
        let t = l.tiles[0];
        assert_eq!(
            hit(&l, t.toggle.x + 10.0, t.toggle.middle()),
            Some(Focus::Toggle(0))
        );
        let p = t.page.unwrap();
        assert_eq!(hit(&l, p.x + 4.0, p.middle()), Some(Focus::Page(0)));
        // Airplane mode's whole tile toggles.
        let a = l.tiles[2];
        assert_eq!(
            hit(&l, a.whole.right() - 3.0, a.whole.middle()),
            Some(Focus::Toggle(2))
        );
        assert_eq!(hit(&l, 1.0, 1.0), None);
        let track = l.track.unwrap();
        assert_eq!(hit(&l, track.x + 40.0, track.middle()), Some(Focus::Slider));
        let mute = l.mute.unwrap();
        assert_eq!(hit(&l, mute.x + 5.0, mute.middle()), Some(Focus::Mute));
        assert_eq!(
            hit(&l, l.chip.unwrap().x + 3.0, l.chip.unwrap().middle()),
            Some(Focus::Output)
        );
        let b = l.settings.unwrap();
        assert_eq!(hit(&l, b.x + 3.0, b.y + 3.0), Some(Focus::Settings));
    }

    #[test]
    fn the_slider_reads_the_pointer_from_end_to_end() {
        let view = view_of(&laptop(), false, 1280);
        let l = layout(&view);
        let t = l.track.unwrap();
        assert_eq!(volume_at(&l, false, t.x - 50.0), 0);
        assert_eq!(volume_at(&l, false, t.x + 9.0), 0);
        assert_eq!(volume_at(&l, false, t.x + t.w / 2.0), 50);
        assert_eq!(volume_at(&l, false, t.x + t.w - 9.0), 100);
        assert_eq!(volume_at(&l, false, t.x + t.w + 40.0), 100);
        // The Compact bar fills from its left edge to the finger.
        let sheet = view_of(&laptop(), false, 360);
        let l = layout(&sheet);
        let bar = l.track.unwrap();
        assert_eq!(volume_at(&l, true, bar.x + bar.w * 0.25), 25);
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
        // Right goes from a toggle to its arrow, then to the next tile.
        let (f, _) = key(&view, Some(Focus::Toggle(0)), Key::Right);
        assert_eq!(f, Some(Focus::Page(0)));
        assert_eq!(key(&view, f, Key::Right).0, Some(Focus::Toggle(1)));
        // Down goes to the tile below; below the last row, to the volume.
        assert_eq!(
            key(&view, Some(Focus::Toggle(0)), Key::Down).0,
            Some(Focus::Toggle(2))
        );
        assert_eq!(
            key(&view, Some(Focus::Page(1)), Key::Down).0,
            Some(Focus::Page(3))
        );
        assert_eq!(
            key(&view, Some(Focus::Toggle(3)), Key::Down).0,
            Some(Focus::Output)
        );
        assert_eq!(
            key(&view, Some(Focus::Toggle(2)), Key::Up).0,
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
        assert_eq!(first.bounds.x0, 900.0 + 12.0);
        assert_eq!(items.last().unwrap().label, "Settings");
        assert!(items.iter().any(|i| i.label == "Output: Speakers"));
    }

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pixmap.pixel(x, y).unwrap().demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn an_on_tile_is_tinted_and_its_circle_is_the_accent_at_any_scale() {
        let tokens = Tokens::built_in();
        let view = view_of(&laptop(), false, 1280);
        let l = layout(&view);
        for s in [1u32, 2] {
            let (w, h) = (WIDTH * s, l.size.1 * s);
            let mut pixmap = Pixmap::new(w, h).unwrap();
            paint(&mut pixmap, &view, &tokens, None, s as f32);
            assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "a round corner");
            let card = tokens.panel.bytes();
            // The card's own colour between the tiles and at its foot.
            assert_eq!(pixel(&pixmap, 180 * s, 10 * s), card);
            // Wi-Fi is on: its circle is the accent, Bluetooth's is not.
            let circle = |i: usize| {
                let t = l.tiles[i].whole;
                pixel(
                    &pixmap,
                    ((t.x + 10.0 + 4.0) * s as f32) as u32,
                    ((t.y + 28.0) * s as f32) as u32,
                )
            };
            // Four pixels in from the circle's left end, at its middle
            // height, it is inside it and clear of the icon.
            assert_eq!(circle(0)[..3], tokens.accent.bytes()[..3]);
            assert_ne!(circle(1)[..3], tokens.accent.bytes()[..3]);
            // The on tile is tinted, the off one is not the same colour.
            let tint = |i: usize| {
                let t = l.tiles[i].whole;
                pixel(
                    &pixmap,
                    ((t.x + 100.0) * s as f32) as u32,
                    ((t.y + 3.0) * s as f32) as u32,
                )
            };
            assert_ne!(tint(0), card);
            assert_ne!(tint(0), tint(1), "on and off tiles differ");
        }
    }

    #[test]
    fn the_slider_fills_with_the_accent_to_the_volume() {
        let tokens = Tokens::built_in();
        let view = view_of(&laptop(), false, 1280);
        let l = layout(&view);
        let mut pixmap = Pixmap::new(WIDTH, l.size.1).unwrap();
        paint(&mut pixmap, &view, &tokens, None, 1.0);
        let t = l.track.unwrap();
        let y = t.middle() as u32;
        // 62 percent: the accent at a quarter of the track, the track's
        // own faint colour near its end.
        let at = |share: f32| pixel(&pixmap, (t.x + 9.0 + (t.w - 18.0) * share) as u32, y);
        assert_eq!(at(0.25)[..3], tokens.accent.bytes()[..3]);
        assert_ne!(at(0.95)[..3], tokens.accent.bytes()[..3]);
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
                (360, "compact", false),
            ] {
                let card = if width < COMPACT_BELOW { width } else { WIDTH };
                let mut view = view_of(&laptop(), dark, width);
                view.list = list;
                if let Some(v) = view.volume.as_mut() {
                    let chip = text.line_in(&v.output, 12.0, Face::MEDIUM).width;
                    v.chip = chip_width(chip, 190.0);
                }
                if name == "desktop" {
                    view.hover = Some(Focus::Toggle(1));
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
                            pixel(&pixmap, px, py)[..3] == tokens.panel_text.bytes()[..3]
                        })
                        .count()
                };
                for t in &l.tiles {
                    assert!(ink(t.toggle) > 20, "{name} {mode}: a tile's title is drawn");
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
                        tokens.radius_menu as f32 * 2.0,
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
