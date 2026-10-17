//! The lock screen's part cards (M5.10c6c2): the small card the edit mode
//! opens beside a clock, a date, a message or a picture, to change that
//! part's options live. Plain data and drawing, tested without a display.
//! `lock_editor_card.rs` owns the surface, the pointer and the keyboard, puts
//! each `apply` result into `view.parts` and writes Done as it writes
//! everything. The segmented rows are `popup::segmented`, the panel menu's
//! too (one copy, ADR-010); the check boxes, the field and the button are
//! drawn here. Options are the settings file's own (`edel::lock_screen`), so
//! a choice of a default writes nothing.

use std::sync::OnceLock;

use accesskit::Role;
use edel::i18n::{tr, trf};
use edel::lock_screen::{self, Anchor, DatePart, DateStyle, Face, Part, Placed, Size};
use edel::tokens::{self, Colour, Tokens};
use edel::widgets::{self, Behind};
use tiny_skia::Pixmap;

use crate::a11y::Item;
use crate::clock_face;
use crate::edit_kit::{self, ButtonKind, State, Style};
use crate::lock;
use crate::lock_editor;
use crate::paint::{Text, fill};
use crate::popup::{self, INSET, PAD, Rect};
use crate::trayview::Key;

/// The card's width, logical pixels: the panel's menu's.
pub const WIDTH: f32 = 300.0;
/// A segment of a segmented row, logical pixels across: four fit beside a
/// row's label on the card.
const SEGMENT: f32 = 48.0;
/// The room between a card and its part, logical pixels.
const GAP: f32 = 8.0;
/// Where a clock's zone and label fields start, as a share of the row's width.
const FIELD_AT: f32 = 0.4;
/// The date's parts in the order they show, whatever order the file gives.
const ORDER: [DatePart; 4] = [
    DatePart::Weekday,
    DatePart::Day,
    DatePart::Month,
    DatePart::Year,
];
/// The date's parts when the file says nothing: weekday, day and month.
const DEFAULT_SHOW: [DatePart; 3] = [DatePart::Weekday, DatePart::Day, DatePart::Month];
const FACES: [Face; 4] = [Face::Digital, Face::Stacked, Face::Analog, Face::Minimal];
const HOURS: [u8; 2] = [24, 12];
const SIZES: [Size; 3] = [Size::Small, Size::Medium, Size::Large];
const STYLES: [DateStyle; 3] = [DateStyle::Long, DateStyle::Short, DateStyle::Numeric];
/// The colour grid's columns (M8.21 part 21k2): six swatches across, so the
/// twelve of `widgets::swatches` take two rows.
const SWATCH_COLUMNS: usize = 6;

/// Whether a part has a card: it has lines to show (the clock, the date, the
/// message and the picture; M5.33b). The count, the player, the person and the
/// battery have none, so a part whose options gain no line gets no empty card
/// (M8.21 part 21k1).
pub fn card_for(part: Part) -> bool {
    !lines(part).is_empty()
}

/// A line of a card (M5.10c6c2): a segmented row, a date part's check box,
/// the message's field, a picture's file name or its button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Face,
    Hours,
    Size,
    Style,
    Show(DatePart),
    Field,
    /// A clock's time zone and label, two fields of their own (M5.10c7d).
    Zone,
    Label,
    File,
    Button,
    /// A widget's colour, as a grid of swatches (M8.21 part 21k2).
    Colour,
    /// The card's "Use on every widget" button (M8.21 part 21k2).
    Every,
}

/// What a control chooses: one option of a part, or the message's or a
/// clock's words.
#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    Face(Face),
    Hours(u8),
    Size(Size),
    Style(DateStyle),
    Show(DatePart),
    Words(String),
    /// A clock's time zone, its name; empty is this machine's time.
    Zone(String),
    /// A clock's label; empty is none.
    Label(String),
    /// A widget's colour, as the file names it (`None` is automatic).
    Colour(Option<String>),
}

/// What the card shows (M5.10c6c2): the part it changes, by its index in the
/// lock screen's parts, the part as it stands now, the control the keyboard
/// holds, the one the pointer is over, and whether a field takes typed
/// letters (it does while a field holds the keyboard). `draft` is the text a
/// clock's zone or label field holds while what was typed does not apply
/// yet, such as a zone name that is not finished (M5.10c7d).
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub index: usize,
    pub placed: Placed,
    pub focus: Option<Line>,
    pub hover: Option<Line>,
    pub typing: bool,
    pub draft: Option<(Line, String)>,
    /// The disclosure row open in the docked card, one at a time (M5.10c7f).
    pub open: Option<Line>,
    /// Whether the card is docked in the right panel (M5.10c7f): its
    /// disclosure rows show their controls only while they are open.
    pub docked: bool,
    /// The clock's number among the clocks, from 2, for its heading's badge
    /// (M5.10c7f), None for the first or the only one.
    pub number: Option<usize>,
    /// Whether the pointer is on the docked card's Remove (M5.10c7g), so it
    /// is lit.
    pub remove_hover: bool,
    /// The colour swatches of the part's colour line (M8.21 part 21k2), in
    /// `widgets::swatches`' order; empty until the card is opened with them.
    pub swatches: Vec<Swatch>,
}

/// One swatch of a part's colour line (M8.21 part 21k2): the colour it names
/// (None is automatic), the ink it draws in over what lies behind the widget
/// (None when a `wallpaper` colour has no source, drawn as a ring), and
/// whether that ink reads there (`widgets::problem`). One that does not read
/// cannot be chosen.
#[derive(Clone, Debug, PartialEq)]
pub struct Swatch {
    pub value: Option<String>,
    pub fill: Option<Colour>,
    pub reads: bool,
}

/// Where a press lands on a card: a segment of a segmented row, a date
/// part's row, a field or the button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Segment(Line, usize),
    Toggle(Line),
    Field(Line),
    Button,
    /// The docked card's Remove (M5.10c7f).
    Remove,
    /// The card's "Use on every widget" (M8.21 part 21k2).
    Every,
}

impl Hit {
    /// The line the press was on, which takes the keyboard's focus; None for
    /// Remove, which is no line.
    pub fn line(self) -> Option<Line> {
        match self {
            Hit::Segment(line, _) | Hit::Toggle(line) | Hit::Field(line) => Some(line),
            Hit::Button => Some(Line::Button),
            Hit::Every => Some(Line::Every),
            Hit::Remove => None,
        }
    }
}

/// Whether a line is a field, which takes typed letters.
fn is_field(line: Line) -> bool {
    matches!(line, Line::Field | Line::Zone | Line::Label)
}

/// What a key or a press asks for: move the focus, choose an option, open
/// Settings' Lock screen page, close the card, or nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Focus(Option<Line>),
    Choose(Choice),
    /// Opens or closes a disclosure row of the docked card (M5.10c7f).
    Disclose(Line),
    Settings,
    /// Takes the part away (M5.10c7f: the card's Remove, or Delete).
    Remove,
    /// Gives the card's colour to every widget that takes one (M8.21 part 21k2).
    Every,
    Close,
    Nothing,
}

/// Where one line lies: its row, its control (the segmented control, the
/// check box, the field or the button; none for the file's name), each
/// segment of a segmented row, and where its label is drawn (M5.10c7e: None
/// for a line with no label of its own), logical pixels in the screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub line: Line,
    pub rect: Rect,
    pub control: Option<Rect>,
    pub segments: Vec<Rect>,
    pub label: Option<Rect>,
}

/// The docked card (M5.10c7g): its room round the edge, logical pixels, and
/// the room a section keeps from the next, as the heading and Remove keep.
const DOCK_PAD: f32 = 16.0;
const SECTION_GAP: f32 = 16.0;
/// Between a plain line and the line before it (a picture's file and button).
const PLAIN_GAP: f32 = 12.0;
/// The heading row (24 px high), its title (15 px semibold) and the round
/// badge that numbers a second clock (20 px across, its figure 11 px
/// semibold), `BADGE_GAP` after the title's last letter.
const HEAD_H: f32 = 24.0;
const TITLE_SIZE: f32 = 14.0;
/// The docked card's words (M5.10c7f, the mockup's): labels and values 12.5 px,
/// the labels white at 75 percent, the chevrons at 50, the group filled with
/// white at 5 percent and its hairlines at 8, the badge at 10.
const DOCK_TEXT: f32 = 12.5;
const DOCK_LABEL_SHARE: f32 = 0.75;
const CHEVRON_SHARE: f32 = 0.50;
const DOCK_GROUP_SHARE: f32 = 0.05;
const DOCK_RULE_SHARE: f32 = 0.08;
const DOCK_BADGE_SHARE: f32 = 0.10;
const DOCK_FACE_SHARE: f32 = 0.07;
const BADGE: f32 = 20.0;
const BADGE_SIZE: f32 = 11.0;
const BADGE_GAP: f32 = 8.0;
/// The group of rows: its corners, a row's height, the room its labels and
/// values keep from its edges, and the room an open control keeps inside it.
const GROUP_RADIUS: f32 = 12.0;
const ROW_H: f32 = 40.0;
const GROUP_INSET: f32 = 12.0;
const OPEN_INSET: f32 = 8.0;
/// A value's space before its chevron, and the chevron's size.
const VALUE_GAP: f32 = 6.0;
const CHEVRON: f32 = 12.0;
/// Remove's pill: as high as a group's row, its ends round.
const REMOVE_H: f32 = ROW_H;
/// A clock's face tile in the docked grid: two across, 56 high, with
/// `FACE_TILE_GAP` between and the corners `FACE_RADIUS`.
const FACE_TILE_H: f32 = 48.0;
const FACE_TILE_GAP: f32 = 6.0;
const FACE_RADIUS: f32 = 10.0;
/// The faces are drawn at 9:41 in this weight, as the mockups' previews.
const SAMPLE_TIME: (i8, i8, i8) = (9, 41, 0);
const FACE_WEIGHT: u16 = 400;
/// A switch, logical pixels across and high.
const SWITCH: (f32, f32) = (30.0, 18.0);

/// Where everything on the card lies: the card's rectangle and its rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub card: Rect,
    pub rows: Vec<Row>,
    /// Docked (M5.10c7f): the heading's rectangle and the Remove button's.
    pub docked: bool,
    pub heading: Option<Rect>,
    pub remove: Option<Rect>,
    /// Docked (M5.10c7g): the rounded group that holds the switch and
    /// disclosure rows, None when it has none.
    pub group: Option<Rect>,
}

/// The card's view of `index`'s part, nothing focused.
pub fn open(index: usize, placed: Placed) -> View {
    View {
        index,
        placed,
        focus: None,
        hover: None,
        typing: false,
        draft: None,
        open: None,
        docked: false,
        number: None,
        remove_hover: false,
        swatches: Vec::new(),
    }
}

/// Whether a line is a disclosure row of the docked card (M5.10c7f): its
/// control opens in place under its row, one at a time.
fn disclosure(line: Line) -> bool {
    matches!(
        line,
        Line::Size | Line::Style | Line::Field | Line::Zone | Line::Label
    )
}

/// Whether a line's control is shown: all of them, except a disclosure row of
/// the docked card, whose control shows only while that row is open.
pub fn shows(view: &View, line: Line) -> bool {
    !view.docked || !disclosure(line) || view.open == Some(line)
}

/// Opens the disclosure row `line`, or closes it when it is open (M5.10c7f),
/// and gives the keyboard's focus to it.
pub fn disclose(view: &mut View, line: Line) {
    view.open = if view.open == Some(line) {
        None
    } else {
        Some(line)
    };
    set_focus(view, Some(line));
}

/// The words a docked disclosure row shows at its right, before its `›`: the
/// value the part has now (M5.10c7f).
fn value_of(view: &View, line: Line) -> String {
    match line {
        Line::Size => size_word(view.placed.size.unwrap_or(Size::Large)).to_string(),
        Line::Style => style_word(view.placed.style.unwrap_or(DateStyle::Long)).to_string(),
        Line::Zone => view
            .placed
            .zone
            .as_deref()
            .map_or_else(|| tr("This computer").to_string(), lock_screen::zone_city),
        Line::Label => view
            .placed
            .label
            .clone()
            .unwrap_or_else(|| tr("None").to_string()),
        Line::Field => words_of(&view.placed).to_string(),
        _ => String::new(),
    }
}

/// The card's heading (M5.10c7f): the part's title in capitals, with its
/// number when it is the second or later clock.
fn heading_of(view: &View) -> String {
    lock_editor::part_title(view.placed.part)
}

/// The docked card's group name for a screen reader (M5.10c7g): the title,
/// with the number of a second clock (`Clock 2`).
fn group_name(view: &View, docked: bool) -> String {
    let title = heading_of(view);
    match view.number {
        Some(n) if docked => format!("{title} {n}"),
        _ => title,
    }
}

/// A row's label (M5.10c7f): the docked card's Hours row is the 24-hour switch.
fn row_label(line: Line, docked: bool) -> &'static str {
    match line {
        Line::Hours if docked => tr("24-hour"),
        _ => label(line),
    }
}

/// The hours a press or Return on the 24-hour switch chooses: the other one.
fn hours_toggled(view: &View) -> Choice {
    Choice::Hours(if view.placed.hours.unwrap_or(24) == 24 {
        12
    } else {
        24
    })
}

/// The lines a part's card has, in the order the keyboard goes round them.
pub fn lines(part: Part) -> Vec<Line> {
    let colour = [Line::Colour, Line::Every];
    match part {
        Part::Clock => {
            let mut lines = vec![Line::Face, Line::Hours, Line::Size, Line::Zone, Line::Label];
            lines.extend(colour);
            lines
        }
        Part::Date => {
            let mut lines = vec![Line::Style];
            lines.extend(ORDER.map(Line::Show));
            lines.extend(colour);
            lines
        }
        Part::Message => {
            let mut lines = vec![Line::Field];
            lines.extend(colour);
            lines
        }
        Part::Picture => vec![Line::File, Line::Button],
        Part::Battery => colour.to_vec(),
        _ => Vec::new(),
    }
}

/// The lines the keyboard's Tab goes round: every line but the file's name.
pub fn ring(part: Part) -> Vec<Line> {
    lines(part)
        .into_iter()
        .filter(|line| *line != Line::File)
        .collect()
}

fn face_word(face: Face) -> &'static str {
    match face {
        Face::Digital => tr("Digital"),
        Face::Stacked => tr("Stacked"),
        Face::Analog => tr("Analog"),
        Face::Minimal => tr("Minimal"),
    }
}

fn size_word(size: Size) -> &'static str {
    match size {
        Size::Small => tr("Small"),
        Size::Medium => tr("Medium"),
        Size::Large => tr("Large"),
    }
}

fn style_word(style: DateStyle) -> &'static str {
    match style {
        DateStyle::Long => tr("Long"),
        DateStyle::Short => tr("Short"),
        DateStyle::Numeric => tr("Numeric"),
    }
}

fn show_word(part: DatePart) -> &'static str {
    match part {
        DatePart::Weekday => tr("Weekday"),
        DatePart::Day => tr("Day"),
        DatePart::Month => tr("Month"),
        DatePart::Year => tr("Year"),
    }
}

/// The word a swatch of the colour line is read and named by (M8.21 part 21k2):
/// automatic, the accent, the wallpaper's, or the accent's title.
fn swatch_word(value: Option<&str>) -> &'static str {
    match value {
        None => tr("Automatic"),
        Some(widgets::ACCENT) => tr("Accent"),
        Some(tokens::WALLPAPER_ACCENT) => tr("From wallpaper"),
        Some(name) => tokens::accent_title(name).unwrap_or_else(|| tr("Accent")),
    }
}

/// The words of a segmented line's segments, each with what choosing it is.
fn segments(line: Line) -> Vec<(&'static str, Choice)> {
    match line {
        Line::Face => FACES
            .iter()
            .map(|&f| (face_word(f), Choice::Face(f)))
            .collect(),
        Line::Hours => HOURS
            .iter()
            .map(|&h| (if h == 12 { "12" } else { "24" }, Choice::Hours(h)))
            .collect(),
        Line::Size => SIZES
            .iter()
            .map(|&s| (size_word(s), Choice::Size(s)))
            .collect(),
        Line::Style => STYLES
            .iter()
            .map(|&s| (style_word(s), Choice::Style(s)))
            .collect(),
        Line::Colour => widgets::swatches()
            .into_iter()
            .map(|value| (swatch_word(value.as_deref()), Choice::Colour(value)))
            .collect(),
        _ => Vec::new(),
    }
}

/// The label a line's row shows, and the words a screen reader hears for it.
fn label(line: Line) -> &'static str {
    match line {
        Line::Face => tr("Face"),
        Line::Hours => tr("Hours"),
        Line::Size => tr("Size"),
        Line::Style => tr("Style"),
        Line::Show(part) => show_word(part),
        Line::Field => tr("Words"),
        Line::Zone => tr("Time zone"),
        Line::Label => tr("Label"),
        Line::File => tr("Picture"),
        Line::Button => tr("Choose in Settings"),
        Line::Colour => tr("Colour"),
        Line::Every => tr("Use on every widget"),
    }
}

/// The name a line's places are logged by: the settings file's word for it.
fn prefix(line: Line) -> &'static str {
    match line {
        Line::Face => "face",
        Line::Hours => "hours",
        Line::Size => "size",
        Line::Style => "style",
        Line::Show(_) => "show",
        Line::Field => "text",
        Line::Zone => "zone",
        Line::Label => "label",
        Line::File => "file",
        Line::Button => "settings",
        Line::Colour => "colour",
        Line::Every => "every",
    }
}

/// The settings file's word for a choice, as the places name it.
fn key_of(choice: &Choice) -> String {
    match choice {
        Choice::Face(f) => f.name().into(),
        Choice::Hours(h) => h.to_string(),
        Choice::Size(s) => s.name().into(),
        Choice::Style(s) => s.name().into(),
        Choice::Show(p) => p.name().into(),
        Choice::Colour(v) => v.clone().unwrap_or_else(|| "automatic".into()),
        Choice::Words(_) | Choice::Zone(_) | Choice::Label(_) => String::new(),
    }
}

/// The date's parts that show, in their order.
fn shown(placed: &Placed) -> Vec<DatePart> {
    let list = placed.show.clone().unwrap_or_else(|| DEFAULT_SHOW.to_vec());
    ORDER.into_iter().filter(|p| list.contains(p)).collect()
}

/// The message's words, empty when it has none.
fn words_of(placed: &Placed) -> &str {
    placed.text.as_deref().unwrap_or("")
}

/// The text a field shows and types on: the draft when the field holds it,
/// else the part's own words, zone or label (empty when it has none).
fn text_of(view: &View, line: Line) -> String {
    match &view.draft {
        Some((drafting, text)) if *drafting == line => text.clone(),
        _ => match line {
            Line::Zone => view.placed.zone.clone(),
            Line::Label => view.placed.label.clone(),
            _ => view.placed.text.clone(),
        }
        .unwrap_or_default(),
    }
}

/// The text a field keeps when what was typed does not apply, as a zone
/// name that is not finished or a label too long (M5.10c7d): the field keeps
/// it so typing shows what was typed. None once it applies, and for the
/// other choices.
pub fn draft_of(choice: &Choice, applied: &Placed) -> Option<(Line, String)> {
    match choice {
        Choice::Zone(text)
            if !text.is_empty() && applied.zone.as_deref() != Some(text.as_str()) =>
        {
            Some((Line::Zone, text.clone()))
        }
        Choice::Label(text)
            if !text.is_empty() && applied.label.as_deref() != Some(text.as_str()) =>
        {
            Some((Line::Label, text.clone()))
        }
        _ => None,
    }
}

/// A part with no options, at `at`, as the check's probe and a default
/// arrangement start from.
fn plain(part: Part, at: Anchor) -> Placed {
    Placed {
        part,
        at,
        x: 0.0,
        y: 0.0,
        text: None,
        path: None,
        face: None,
        hours: None,
        size: None,
        weight: None,
        show: None,
        style: None,
        zone: None,
        label: None,
        colour: None,
    }
}

/// The most characters a message's words may have: `lock_screen::check`'s
/// own limit, found by asking it, so the number stays in one place (ADR-010).
pub fn most_letters() -> usize {
    static MOST: OnceLock<usize> = OnceLock::new();
    *MOST.get_or_init(|| {
        let fits = |n: usize| {
            let mut message = plain(Part::Message, Anchor::Centre);
            message.text = Some("a".repeat(n));
            lock_screen::check(&[plain(Part::Person, Anchor::Centre), message]).is_ok()
        };
        (1..=4096).take_while(|&n| fits(n)).last().unwrap_or(0)
    })
}

/// The option `placed` has now for a segmented line, as an index into its
/// segments; None for the other lines.
fn chosen(placed: &Placed, line: Line) -> Option<usize> {
    let now = match line {
        Line::Face => Choice::Face(placed.face.unwrap_or(Face::Digital)),
        Line::Hours => Choice::Hours(placed.hours.unwrap_or(24)),
        Line::Size => Choice::Size(placed.size.unwrap_or(Size::Large)),
        Line::Style => Choice::Style(placed.style.unwrap_or(DateStyle::Long)),
        Line::Colour => Choice::Colour(placed.colour.clone()),
        _ => return None,
    };
    segments(line).iter().position(|(_, choice)| *choice == now)
}

/// The change a choice makes to `placed`: the one function that changes a
/// part's options. A default writes `None`, as the settings file never
/// writes one (Digital, 24 hours, Large, Long, the weekday, day and month
/// shown). The last date part on cannot be turned off, and a choice for
/// another part changes nothing. Words stop at `most_letters`.
pub fn apply(placed: &Placed, choice: &Choice) -> Placed {
    let mut next = placed.clone();
    match (placed.part, choice) {
        (Part::Clock, Choice::Face(f)) => next.face = (*f != Face::Digital).then_some(*f),
        (Part::Clock, Choice::Hours(h)) => next.hours = (*h != 24).then_some(*h),
        (Part::Clock, Choice::Size(s)) => next.size = (*s != Size::Large).then_some(*s),
        (Part::Date, Choice::Style(s)) => next.style = (*s != DateStyle::Long).then_some(*s),
        (Part::Date, Choice::Show(part)) => {
            let mut list = shown(placed);
            if let Some(i) = list.iter().position(|p| p == part) {
                if list.len() == 1 {
                    return placed.clone();
                }
                list.remove(i);
            } else {
                list.push(*part);
            }
            let list: Vec<DatePart> = ORDER.into_iter().filter(|p| list.contains(p)).collect();
            next.show = if list == DEFAULT_SHOW {
                None
            } else {
                Some(list)
            };
        }
        (Part::Message, Choice::Words(words)) => {
            next.text = Some(words.chars().take(most_letters()).collect());
        }
        (part, Choice::Colour(colour)) if part.widget().takes(widgets::COLOUR) => {
            next.colour = colour.clone();
        }
        // A zone is applied when it is a zone name (`lock_screen::is_zone_name`),
        // a label when it is one (`lock_screen::is_label`); else nothing changes.
        (Part::Clock, Choice::Zone(zone)) => {
            if zone.is_empty() {
                next.zone = None;
            } else if lock_screen::is_zone_name(zone) {
                next.zone = Some(zone.clone());
            } else {
                return placed.clone();
            }
        }
        (Part::Clock, Choice::Label(label)) => {
            if label.is_empty() {
                next.label = None;
            } else if lock_screen::is_label(label) {
                next.label = Some(label.clone());
            } else {
                return placed.clone();
            }
        }
        _ => {}
    }
    next
}

/// The message after an edit ends (Undo, Escape, a press away): when its
/// words were taken all away, it keeps the words it had when its card
/// opened (`before`), since an empty message is never written.
pub fn settled(placed: Placed, before: &Placed) -> Placed {
    let mut placed = placed;
    if placed.part == Part::Message && placed.text.as_deref().is_none_or(str::is_empty) {
        placed.text = before.text.clone();
    }
    placed
}

/// The log words of a change (`lock editor WORDS`), for the placed part after
/// `choice` was applied: `clock face analog`, `clock hours 12`, `clock size
/// small`, `date style short`, `date show weekday,day`, `message words N
/// letters`.
pub fn said(choice: &Choice, placed: &Placed) -> String {
    match choice {
        Choice::Face(f) => format!("clock face {}", f.name()),
        Choice::Hours(h) => format!("clock hours {h}"),
        Choice::Size(s) => format!("clock size {}", s.name()),
        Choice::Style(s) => format!("date style {}", s.name()),
        Choice::Show(_) => format!(
            "date show {}",
            shown(placed)
                .iter()
                .map(|p| p.name())
                .collect::<Vec<_>>()
                .join(",")
        ),
        Choice::Words(_) => format!("message words {} letters", words_of(placed).chars().count()),
        Choice::Zone(_) => format!(
            "clock zone {}",
            placed.zone.as_deref().unwrap_or("this machine")
        ),
        Choice::Label(_) => format!("clock label {}", placed.label.as_deref().unwrap_or("none")),
        Choice::Colour(v) => format!(
            "{} colour {}",
            placed.part.name(),
            v.as_deref().unwrap_or("automatic")
        ),
    }
}

/// Sets the keyboard's focus to `line`, a field taking typed letters while
/// it has it.
pub fn set_focus(view: &mut View, line: Option<Line>) {
    view.focus = line;
    view.typing = line.is_some_and(is_field);
}

/// The next line of the ring after the focused one, backwards or forwards,
/// wrapping; from no focus, the first or the last (Tab, Shift+Tab).
fn step(view: &View, backwards: bool) -> Option<Line> {
    let ring = ring(view.placed.part);
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

/// Whether segment `j` of `line` can be chosen: every segment but a colour
/// swatch that does not read over what lies behind its widget (M8.21 part
/// 21k2, `Swatch::reads`), which is only shown.
fn choosable(view: &View, line: Line, j: usize) -> bool {
    line != Line::Colour || view.swatches.get(j).is_some_and(|swatch| swatch.reads)
}

/// Left and Right on the focused segmented line: the segment beside the
/// chosen one, when there is one and it can be chosen.
fn segment_step(view: &View, line: Line, backwards: bool) -> Step {
    let Some(now) = chosen(&view.placed, line) else {
        return Step::Nothing;
    };
    let options = segments(line);
    let next = if backwards {
        now.checked_sub(1)
    } else {
        Some(now + 1).filter(|j| *j < options.len())
    };
    match next
        .filter(|j| choosable(view, line, *j))
        .and_then(|j| options.into_iter().nth(j))
    {
        Some((_, choice)) => Step::Choose(choice),
        None => Step::Nothing,
    }
}

/// The choice a field's text makes, for the field's line (`text_of`'s words).
fn choice_of(line: Line, words: String) -> Choice {
    match line {
        Line::Zone => Choice::Zone(words),
        Line::Label => Choice::Label(words),
        _ => Choice::Words(words),
    }
}

/// What a key does on the card (M5.10c6c2): Escape closes it; Tab and
/// Shift+Tab step the ring; while the field takes letters, a typed letter
/// is added (at most `most_letters`) and Backspace takes one back; Left and
/// Right choose the neighbouring segment of the focused segmented line; Return
/// and Space choose a focused date part or press the focused button. `key`
/// is None for keys the card does not use, and `typed` is the letter a key
/// types, if any.
pub fn key(view: &View, key: Option<Key>, shift: bool, typed: Option<char>) -> Step {
    match key {
        Some(Key::Escape) => return Step::Close,
        Some(Key::Tab) => return Step::Focus(step(view, shift)),
        _ => {}
    }
    if let Some(line) = view
        .focus
        .filter(|line| view.typing && is_field(*line) && shows(view, *line))
    {
        let mut words = text_of(view, line);
        let most = match line {
            Line::Zone => lock_screen::MOST_ZONE,
            Line::Label => lock_screen::MOST_LABEL,
            _ => most_letters(),
        };
        if let Some(c) = typed.filter(|c| !c.is_control()) {
            if words.chars().count() >= most {
                return Step::Nothing;
            }
            words.push(c);
            return Step::Choose(choice_of(line, words));
        }
        if key == Some(Key::Delete) && words.pop().is_some() {
            return Step::Choose(choice_of(line, words));
        }
        return Step::Nothing;
    }
    let Some(line) = view.focus else {
        return Step::Nothing;
    };
    match (key, line) {
        (Some(Key::Delete), _) => Step::Remove,
        (Some(Key::Left), _) if chosen(&view.placed, line).is_some() => {
            segment_step(view, line, true)
        }
        (Some(Key::Right), _) if chosen(&view.placed, line).is_some() => {
            segment_step(view, line, false)
        }
        (Some(Key::Activate), Line::Show(part)) => Step::Choose(Choice::Show(part)),
        (Some(Key::Activate), Line::Hours) if view.docked => Step::Choose(hours_toggled(view)),
        (Some(Key::Activate), line) if view.docked && disclosure(line) => Step::Disclose(line),
        (Some(Key::Activate), Line::Button) => Step::Settings,
        (Some(Key::Activate), Line::Every) => Step::Every,
        _ => Step::Nothing,
    }
}

/// Where a press lands on the card: the choice it makes, the row it opens,
/// the button's page, the part's removal, or nothing (inside the card, on no
/// control). `view` is the card as it is now, for the switch.
pub fn press(view: &View, l: &Layout, x: f32, y: f32) -> Step {
    match hit(l, x, y) {
        Some(Hit::Segment(line, j)) if !choosable(view, line, j) => Step::Nothing,
        Some(Hit::Segment(line, j)) => segments(line)
            .into_iter()
            .nth(j)
            .map_or(Step::Nothing, |(_, choice)| Step::Choose(choice)),
        Some(Hit::Toggle(Line::Hours)) => Step::Choose(hours_toggled(view)),
        Some(Hit::Toggle(Line::Show(part))) => Step::Choose(Choice::Show(part)),
        Some(Hit::Toggle(line)) => Step::Disclose(line),
        Some(Hit::Field(line)) => Step::Focus(Some(line)),
        Some(Hit::Button) => Step::Settings,
        Some(Hit::Every) => Step::Every,
        Some(Hit::Remove) => Step::Remove,
        None => Step::Nothing,
    }
}

/// Where a point lies on the card, if on a control (M5.10c6c2): a segment
/// of a segmented row, a date part's row, the field or the button.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Hit> {
    if l.remove.is_some_and(|r| r.contains(x, y)) {
        return Some(Hit::Remove);
    }
    let row = l.rows.iter().find(|row| row.rect.contains(x, y))?;
    let segment = || {
        row.segments
            .iter()
            .position(|s| s.contains(x, y))
            .map(|j| Hit::Segment(row.line, j))
    };
    // A docked row's head is its top row (M5.10c7g), whatever its label is.
    let on_head = if l.docked {
        head_of(row).contains(x, y)
    } else {
        row.label.is_some_and(|r| r.contains(x, y))
    };
    match row.line {
        Line::Face | Line::Colour => segment(),
        Line::Every => Some(Hit::Every),
        Line::Hours if l.docked => Some(Hit::Toggle(row.line)),
        Line::Hours => segment(),
        Line::Size | Line::Style => {
            segment().or_else(|| (l.docked && on_head).then_some(Hit::Toggle(row.line)))
        }
        Line::Show(_) => Some(Hit::Toggle(row.line)),
        Line::Field | Line::Zone | Line::Label => {
            let in_control = row.control.is_some_and(|c| c.contains(x, y));
            if l.docked && !in_control {
                Some(Hit::Toggle(row.line))
            } else {
                Some(Hit::Field(row.line))
            }
        }
        Line::Button => Some(Hit::Button),
        Line::File => None,
    }
}

/// Where the card goes and where everything on it lies (M5.10c6c2, M5.10c7e):
/// the one function that places the card and every control. Floating (no
/// `dock`) it stands to the right of `near` (the part, in the screen's logical
/// pixels) when there is room on the screen, else to its left, kept inside the
/// screen with the lock's margin, and its top lies level with the part's, kept
/// inside it too. Docked (`dock`, the right panel's rectangle) it is that
/// rectangle's width and stands at its top left, with every line stacked: its
/// label on a row of its own, its control under it across the full width.
pub fn layout(
    view: &View,
    near: Rect,
    screen: (f32, f32),
    tokens: &Tokens,
    dock: Option<Rect>,
) -> Layout {
    match dock {
        Some(panel) => docked(view, panel, tokens),
        None => floating(view, near, screen, tokens),
    }
}

/// The swatches a part's colour line shows (M8.21 part 21k2): each colour
/// `widgets::swatches` names, with the ink it draws in over `behind` (automatic
/// draws the release's text colour) and whether that ink reads there.
pub fn swatches_for(part: Part, tokens: &Tokens, behind: Behind) -> Vec<Swatch> {
    let widget = part.widget();
    widgets::swatches()
        .into_iter()
        .map(|value| {
            let fill = match value.as_deref() {
                None => Some(tokens.backdrop_text),
                v => widgets::ink(v, widget, tokens, behind),
            };
            let reads = value
                .as_deref()
                .is_none_or(|v| widgets::problem(widget, v, tokens, behind.surface).is_none());
            Swatch { value, fill, reads }
        })
        .collect()
}

/// The cells of a swatch grid (M8.21 part 21k2): `SWATCH_COLUMNS` across and
/// `cell_h` high from `rect`'s top left, one for each of `n` swatches.
fn swatch_cells(rect: Rect, n: usize, cell_h: f32) -> Vec<Rect> {
    let cell_w = rect.w / SWATCH_COLUMNS as f32;
    (0..n)
        .map(|j| {
            Rect::new(
                rect.x + (j % SWATCH_COLUMNS) as f32 * cell_w,
                rect.y + (j / SWATCH_COLUMNS) as f32 * cell_h,
                cell_w,
                cell_h,
            )
        })
        .collect()
}

/// The rows of the colour grid: how many lines of `SWATCH_COLUMNS` its swatches take.
fn swatch_rows() -> usize {
    segments(Line::Colour).len().div_ceil(SWATCH_COLUMNS)
}

/// A line's height on a floating card (M8.21 part 21k2): a row, or each row
/// of the colour grid.
fn floating_h(line: Line, row_h: f32) -> f32 {
    match line {
        Line::Colour => swatch_rows() as f32 * row_h,
        _ => row_h,
    }
}

/// The card floating beside its part (`layout`, no dock).
fn floating(view: &View, near: Rect, screen: (f32, f32), tokens: &Tokens) -> Layout {
    let lines = lines(view.placed.part);
    let row_h = tokens.row as f32;
    let heights: Vec<f32> = lines.iter().map(|line| floating_h(*line, row_h)).collect();
    let height = 2.0 * PAD + heights.iter().sum::<f32>();
    let margin = lock::MARGIN;
    let right_x = near.right() + GAP;
    let x = if right_x + WIDTH <= screen.0 - margin {
        right_x
    } else {
        (near.x - GAP - WIDTH).clamp(margin, (screen.0 - margin - WIDTH).max(margin))
    };
    let y = near
        .y
        .clamp(margin, (screen.1 - margin - height).max(margin));
    let card = Rect::new(x, y, WIDTH, height);
    let panel_control = tokens.panel_control as f32;
    let glyph = tokens.panel_glyph as f32;
    let mut top = card.y + PAD;
    let rows = lines
        .into_iter()
        .zip(heights)
        .map(|(line, h)| {
            let rect = Rect::new(card.x + PAD, top, WIDTH - 2.0 * PAD, h);
            top += h;
            let right = rect.right() - INSET;
            let left = rect.x + INSET;
            let (control, cells, label) = match line {
                Line::Face | Line::Hours | Line::Size | Line::Style => {
                    let (control, cells) = popup::segments_at(
                        rect,
                        right,
                        segments(line).len(),
                        SEGMENT,
                        panel_control,
                    );
                    let label = Rect::new(left, rect.y, control.x - 8.0 - left, rect.h);
                    (Some(control), cells, Some(label))
                }
                Line::Show(_) => {
                    let box_ =
                        Rect::new(right - glyph, rect.y + (rect.h - glyph) / 2.0, glyph, glyph);
                    let label = Rect::new(left, rect.y, box_.x - 8.0 - left, rect.h);
                    (Some(box_), Vec::new(), Some(label))
                }
                Line::Field | Line::Button | Line::Every => (
                    Some(Rect::new(
                        rect.x + INSET,
                        rect.y + (rect.h - panel_control) / 2.0,
                        rect.w - 2.0 * INSET,
                        panel_control,
                    )),
                    Vec::new(),
                    None,
                ),
                // The swatches fill the row, with no label of their own.
                Line::Colour => (
                    Some(rect),
                    swatch_cells(rect, segments(line).len(), row_h),
                    None,
                ),
                // A clock's zone and label: the row's name at the left, the
                // field for the words at the right, from `FIELD_AT` across.
                Line::Zone | Line::Label => {
                    let x = rect.x + rect.w * FIELD_AT;
                    let field = Rect::new(
                        x,
                        rect.y + (rect.h - panel_control) / 2.0,
                        right - x,
                        panel_control,
                    );
                    let label = Rect::new(left, rect.y, field.x - 8.0 - left, rect.h);
                    (Some(field), Vec::new(), Some(label))
                }
                Line::File => {
                    let label = Rect::new(left, rect.y, rect.w - 2.0 * INSET, rect.h);
                    (None, Vec::new(), Some(label))
                }
            };
            Row {
                line,
                rect,
                control,
                segments: cells,
                label,
            }
        })
        .collect();
    Layout {
        card,
        rows,
        docked: false,
        heading: None,
        remove: None,
        group: None,
    }
}

/// The parts of a docked card in the order they stack (M5.10c7g): the faces
/// grid, the one rounded group of rows, and the plain lines (a picture's file
/// and button).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Grid,
    Group,
    Plain,
}

fn section(line: Line) -> Section {
    match line {
        Line::Face => Section::Grid,
        Line::Hours
        | Line::Size
        | Line::Style
        | Line::Show(_)
        | Line::Field
        | Line::Zone
        | Line::Label => Section::Group,
        Line::Colour => Section::Grid,
        Line::File | Line::Button | Line::Every => Section::Plain,
    }
}

/// The room before a section: none between two rows of the group, `PLAIN_GAP`
/// before a plain line that follows a plain line or the group, `SECTION_GAP`
/// else (under the heading, the grid, or the group).
fn gap(prev: Option<Section>, next: Section) -> f32 {
    match (prev, next) {
        (Some(Section::Group), Section::Group) => 0.0,
        (Some(Section::Group | Section::Plain), Section::Plain) => PLAIN_GAP,
        _ => SECTION_GAP,
    }
}

/// Whether a line is a row of the docked group (M5.10c7g): only ever on a
/// docked card.
fn grouped(l: &Layout, line: Line) -> bool {
    l.docked && section(line) == Section::Group
}

/// A docked row's head: its top `ROW_H` across the row's width.
fn head_of(row: &Row) -> Rect {
    Rect::new(row.rect.x, row.rect.y, row.rect.w, ROW_H)
}

/// The rectangle the keyboard's ring goes round on a docked card (M5.10c7g):
/// a control (the faces, the button, an open row's control), else a group
/// row's head.
fn docked_target(row: &Row) -> Option<Rect> {
    match row.line {
        Line::File => None,
        Line::Face | Line::Colour | Line::Button | Line::Every => row.control,
        line if disclosure(line) && row.control.is_some() => row.control,
        _ => Some(head_of(row)),
    }
}

/// The card docked in the right panel (`layout`, M5.10c7e, M5.10c7g): the
/// panel's width, from its top left. Under the heading come the faces grid
/// (a clock), then one rounded group of rows with hairlines between them, then
/// any plain lines (a picture's file and button), and Remove at the foot,
/// `SECTION_GAP` under the last of them.
fn docked(view: &View, panel: Rect, tokens: &Tokens) -> Layout {
    let control_h = tokens.panel_control as f32;
    let row_h = tokens.row as f32;
    let inner_x = panel.x + DOCK_PAD;
    let inner_w = panel.w - 2.0 * DOCK_PAD;
    let heading = Rect::new(inner_x, panel.y + DOCK_PAD, inner_w, HEAD_H);
    let mut y = heading.y + heading.h;
    let mut prev: Option<Section> = None;
    let mut rows = Vec::new();
    for line in lines(view.placed.part) {
        let next = section(line);
        y += gap(prev, next);
        prev = Some(next);
        let (row, used) = match line {
            // The colour grid (M8.21 part 21k2): touch-sized cells, six across.
            Line::Colour => {
                let rows = swatch_rows();
                let grid = Rect::new(inner_x, y, inner_w, rows as f32 * edit_kit::BUTTON_TOUCH);
                let row = Row {
                    line,
                    rect: grid,
                    control: Some(grid),
                    segments: swatch_cells(grid, segments(line).len(), edit_kit::BUTTON_TOUCH),
                    label: None,
                };
                (row, grid.h)
            }
            Line::Face => {
                let tile_w = (inner_w - FACE_TILE_GAP) / 2.0;
                let cells: Vec<Rect> = (0..4)
                    .map(|j| {
                        Rect::new(
                            inner_x + (j % 2) as f32 * (tile_w + FACE_TILE_GAP),
                            y + (j / 2) as f32 * (FACE_TILE_H + FACE_TILE_GAP),
                            tile_w,
                            FACE_TILE_H,
                        )
                    })
                    .collect();
                let grid = Rect::new(inner_x, y, inner_w, 2.0 * FACE_TILE_H + FACE_TILE_GAP);
                let row = Row {
                    line,
                    rect: grid,
                    control: Some(grid),
                    segments: cells,
                    label: None,
                };
                (row, grid.h)
            }
            // A switch at the right of its row: the 24-hour one and a date part.
            Line::Hours | Line::Show(_) => {
                let rect = Rect::new(inner_x, y, inner_w, ROW_H);
                let switch = Rect::new(
                    rect.right() - GROUP_INSET - SWITCH.0,
                    rect.y + (ROW_H - SWITCH.1) / 2.0,
                    SWITCH.0,
                    SWITCH.1,
                );
                let label = Rect::new(
                    rect.x + GROUP_INSET,
                    rect.y,
                    switch.x - 8.0 - (rect.x + GROUP_INSET),
                    ROW_H,
                );
                let row = Row {
                    line,
                    rect,
                    control: Some(switch),
                    segments: Vec::new(),
                    label: Some(label),
                };
                (row, ROW_H)
            }
            // A disclosure row: its value at the right of its head, and its
            // control under the head only while it is open.
            Line::Size | Line::Style | Line::Field | Line::Zone | Line::Label => {
                let control = shows(view, line).then(|| {
                    Rect::new(
                        inner_x + OPEN_INSET,
                        y + ROW_H + OPEN_INSET,
                        inner_w - 2.0 * OPEN_INSET,
                        control_h,
                    )
                });
                let segments = match (control, line) {
                    (Some(c), Line::Size | Line::Style) => {
                        let n = segments(line).len().max(1);
                        let cell = c.w / n as f32;
                        (0..n)
                            .map(|j| Rect::new(c.x + j as f32 * cell, c.y, cell, c.h))
                            .collect()
                    }
                    _ => Vec::new(),
                };
                let used = control.map_or(ROW_H, |c| c.y + c.h + OPEN_INSET - y);
                let label = Rect::new(inner_x + GROUP_INSET, y, inner_w - 2.0 * GROUP_INSET, ROW_H);
                let row = Row {
                    line,
                    rect: Rect::new(inner_x, y, inner_w, used),
                    control,
                    segments,
                    label: Some(label),
                };
                (row, used)
            }
            Line::Button | Line::Every => {
                let control = Rect::new(inner_x, y, inner_w, control_h);
                let row = Row {
                    line,
                    rect: control,
                    control: Some(control),
                    segments: Vec::new(),
                    label: None,
                };
                (row, control_h)
            }
            Line::File => {
                let rect = Rect::new(inner_x, y, inner_w, row_h);
                let row = Row {
                    line,
                    rect,
                    control: None,
                    segments: Vec::new(),
                    label: Some(rect),
                };
                (row, row_h)
            }
        };
        y += used;
        rows.push(row);
    }
    let group = match (
        rows.iter().find(|r| section(r.line) == Section::Group),
        rows.iter()
            .rev()
            .find(|r| section(r.line) == Section::Group),
    ) {
        (Some(first), Some(last)) => Some(Rect::new(
            inner_x,
            first.rect.y,
            inner_w,
            last.rect.y + last.rect.h - first.rect.y,
        )),
        _ => None,
    };
    let remove = Rect::new(inner_x, y + SECTION_GAP, inner_w, REMOVE_H);
    let bottom = remove.y + remove.h + DOCK_PAD;
    let card = Rect::new(panel.x, panel.y, panel.w, bottom - panel.y);
    Layout {
        card,
        rows,
        docked: true,
        heading: Some(heading),
        remove: Some(remove),
        group,
    }
}

/// The name of the picture's file, as the card shows it.
fn file_name(placed: &Placed) -> String {
    placed
        .path
        .as_deref()
        .and_then(|path| std::path::Path::new(path).file_name())
        .map_or_else(
            || tr("No picture").to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
}

/// Draws a row's label, its words fitted to where the layout put the label
/// (M5.10c7e); a row with no label draws nothing.
fn draw_label(
    pixmap: &mut Pixmap,
    text: &mut Text,
    row: &Row,
    words: &str,
    size: f32,
    s: f32,
    colour: edel::tokens::Colour,
) {
    let Some(at) = row.label else {
        return;
    };
    let mut line = text.fit(words, size, at.w * s);
    let y = popup::middle(at.y, at.h, size, s);
    text.draw(pixmap, &mut line, at.x * s, y, colour);
}

/// Draws a docked disclosure row's value at the right of its head (M5.10c7g):
/// its words in `dim`, then a chevron 6 px after them, the chevron's right
/// edge `GROUP_INSET` from the group's.
fn draw_value(
    pixmap: &mut Pixmap,
    text: &mut Text,
    row: &Row,
    words: &str,
    size: f32,
    s: f32,
    dim: edel::tokens::Colour,
) {
    let head = head_of(row);
    let chevron = Rect::new(
        head.right() - GROUP_INSET - CHEVRON,
        head.y + (ROW_H - CHEVRON) / 2.0,
        CHEVRON,
        CHEVRON,
    );
    let room = (chevron.x - VALUE_GAP - (head.x + GROUP_INSET)) * s;
    let mut line = text.fit(words, size, room);
    let x = chevron.x * s - VALUE_GAP * s - line.width;
    let y = popup::middle(head.y, ROW_H, size, s);
    text.draw(pixmap, &mut line, x, y, dim);
    let chevron_ink = Colour {
        a: CHEVRON_SHARE,
        ..dim
    };
    popup::icon_in(pixmap, "chevron-right", CHEVRON, chevron, s, chevron_ink);
}

/// Draws a docked switch (M5.10c7f) at `switch`: a pill in the accent when on,
/// the raised colour when off, with its knob at the right or the left.
fn draw_switch(pixmap: &mut Pixmap, switch: Rect, on: bool, ink: &edit_kit::Inks, s: f32) {
    let (x, y, w, h) = switch.device(s);
    fill(
        pixmap,
        x,
        y,
        w,
        h,
        h / 2.0,
        if on { ink.accent } else { ink.raised },
    );
    let knob = switch.h - 4.0;
    let kx = if on {
        switch.right() - 2.0 - knob
    } else {
        switch.x + 2.0
    };
    let (x, y, w, h) = Rect::new(kx, switch.y + 2.0, knob, knob).device(s);
    fill(
        pixmap,
        x,
        y,
        w,
        h,
        h / 2.0,
        if on { ink.accent_text } else { ink.text },
    );
}

/// Draws the docked clock's four faces (M5.10c7f): each in its tile, the
/// chosen one filled with the accent, the face drawn in miniature at 9:41.
fn draw_faces(
    pixmap: &mut Pixmap,
    view: &View,
    row: &Row,
    ink: &edit_kit::Inks,
    text: &mut Text,
    s: f32,
) {
    let now = chosen(&view.placed, Line::Face);
    let hours = view.placed.hours.unwrap_or(24);
    for (j, (_, choice)) in segments(Line::Face).into_iter().enumerate() {
        let (Some(cell), Choice::Face(face)) = (row.segments.get(j), choice) else {
            continue;
        };
        let on = now == Some(j);
        let (x, y, w, h) = cell.device(s);
        let unchosen = Colour {
            a: DOCK_FACE_SHARE,
            ..ink.text
        };
        let fill_colour = if on { ink.accent } else { unchosen };
        fill(pixmap, x, y, w, h, FACE_RADIUS * s, fill_colour);
        let colour = if on { ink.accent_text } else { ink.text };
        let clock_size = match face {
            Face::Analog => 34.0,
            Face::Digital => 18.0,
            Face::Stacked | Face::Minimal => 16.0,
        };
        clock_face::draw(
            pixmap,
            *cell,
            SAMPLE_TIME,
            face,
            hours,
            clock_size,
            FACE_WEIGHT,
            colour,
            text,
            s,
        );
    }
}

/// Draws the docked card's heading (M5.10c7g): the part's title in sentence
/// case, 15 px semibold in the text's ink, and for a second clock its number
/// in a round badge `BADGE_GAP` after the title.
fn draw_heading(
    pixmap: &mut Pixmap,
    view: &View,
    heading: Rect,
    ink: &edit_kit::Inks,
    text: &mut Text,
    s: f32,
) {
    let size = TITLE_SIZE * s;
    let mut title = text.fit_in(
        &heading_of(view),
        size,
        heading.w * s,
        crate::paint::Face::SEMIBOLD,
    );
    let x = heading.x * s;
    let y = popup::middle(heading.y, heading.h, size, s);
    let width = title.width;
    text.draw(pixmap, &mut title, x, y, ink.text);
    let Some(n) = view.number else {
        return;
    };
    let top = heading.y + (heading.h - BADGE) / 2.0;
    let badge_x = x + width + BADGE_GAP * s;
    let badge = Colour {
        a: DOCK_BADGE_SHARE,
        ..ink.text
    };
    fill(
        pixmap,
        badge_x,
        top * s,
        BADGE * s,
        BADGE * s,
        BADGE / 2.0 * s,
        badge,
    );
    let size = BADGE_SIZE * s;
    let mut figure = text.line_in(&n.to_string(), size, crate::paint::Face::SEMIBOLD);
    let fx = badge_x + (BADGE * s - figure.width) / 2.0;
    let fy = popup::middle(top, BADGE, size, s);
    text.draw(pixmap, &mut figure, fx, fy, ink.text);
}

/// Draws the card at scale `s` over what is under it (the lock and the
/// corner; it clears nothing) as the lock's edit mode dresses its drawer
/// (M5.10c6c3, `edit_kit`): the glass sheet with its shadow, a lit row under
/// the pointer, each line's label, its segmented control with the chosen
/// segment, each date part's check box, the field's words (and its caret
/// while it takes letters), the picture's name and the button, the
/// keyboard's ring round the focused control. Docked (M5.10c7f) it has the
/// heading with Remove, the faces grid, the switches and the `›` rows.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    l: &Layout,
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let ink = edit_kit::inks(Style::GLASS, tokens);
    if l.docked {
        // Docked in the right panel, the card is a side panel with no shadow.
        edit_kit::side_panel(pixmap, l.card, tokens, s);
    } else {
        lock::lift(pixmap, l.card, edit_kit::SHEET_RADIUS, s, tokens);
        edit_kit::sheet(pixmap, l.card, tokens, Style::GLASS, s);
    }
    let size = tokens.panel_text_size as f32 * s;
    // A docked card's labels and values are smaller, its labels dimmer.
    let (text_px, label_ink) = if l.docked {
        (
            DOCK_TEXT * s,
            Colour {
                a: DOCK_LABEL_SHARE,
                ..ink.text
            },
        )
    } else {
        (size, ink.text)
    };
    let radius = tokens.radius_control as f32 * s;
    // A faint ink: the group's hairlines and the controls drawn in the group.
    let faint = edel::tokens::Colour {
        a: 0.10,
        ..ink.text
    };
    if let Some(heading) = l.heading {
        draw_heading(pixmap, view, heading, &ink, text, s);
    }
    if let Some(group) = l.group {
        let (x, y, w, h) = group.device(s);
        let group_fill = if l.docked {
            Colour {
                a: DOCK_GROUP_SHARE,
                ..ink.text
            }
        } else {
            ink.raised
        };
        fill(pixmap, x, y, w, h, GROUP_RADIUS * s, group_fill);
    }
    let first = l.rows.iter().position(|r| grouped(l, r.line));
    let last = l.rows.iter().rposition(|r| grouped(l, r.line));
    for (i, row) in l.rows.iter().enumerate() {
        let in_group = grouped(l, row.line);
        if view.hover == Some(row.line) {
            let (x, y, w, h) = row.rect.device(s);
            if !in_group {
                fill(pixmap, x, y, w, h, radius, ink.lit);
            } else {
                // The group's first and last rows take its corners, the lit
                // row never sticks out of the group.
                let r = GROUP_RADIUS * s;
                let (top, bottom) = (Some(i) == first, Some(i) == last);
                if top && bottom {
                    fill(pixmap, x, y, w, h, r, ink.lit);
                } else if top {
                    fill(pixmap, x, y, w, h, r, ink.lit);
                    fill(pixmap, x, y + r, w, h - r, 0.0, ink.lit);
                } else if bottom {
                    fill(pixmap, x, y, w, h, r, ink.lit);
                    fill(pixmap, x, y, w, h - r, 0.0, ink.lit);
                } else {
                    fill(pixmap, x, y, w, h, 0.0, ink.lit);
                }
            }
        }
        if in_group && Some(i) != first {
            if let Some(group) = l.group {
                let rule = Rect::new(
                    group.x + GROUP_INSET,
                    row.rect.y,
                    group.w - GROUP_INSET,
                    1.0,
                );
                let (x, y, w, h) = rule.device(s);
                let rule_ink = if l.docked {
                    Colour {
                        a: DOCK_RULE_SHARE,
                        ..ink.text
                    }
                } else {
                    faint
                };
                fill(pixmap, x, y, w, h, 0.0, rule_ink);
            }
        }
        match row.line {
            Line::Face if l.docked => draw_faces(pixmap, view, row, &ink, text, s),
            Line::Hours if l.docked => {
                if let Some(switch) = row.control {
                    let on = view.placed.hours.unwrap_or(24) == 24;
                    draw_switch(pixmap, switch, on, &ink, s);
                }
                draw_label(
                    pixmap,
                    text,
                    row,
                    row_label(row.line, true),
                    text_px,
                    s,
                    label_ink,
                );
            }
            Line::Colour => {
                let now = chosen(&view.placed, Line::Colour);
                for (j, cell) in row.segments.iter().enumerate() {
                    let Some(swatch) = view.swatches.get(j) else {
                        continue;
                    };
                    edit_kit::swatch(
                        pixmap,
                        *cell,
                        swatch.fill,
                        now == Some(j),
                        swatch.reads,
                        &ink,
                        s,
                    );
                }
            }
            Line::Face | Line::Hours | Line::Size | Line::Style => {
                if let Some(control) = row.control {
                    let words: Vec<&str> = segments(row.line).iter().map(|(w, _)| *w).collect();
                    let track = if l.docked {
                        (faint, ink.text)
                    } else {
                        (ink.raised, ink.text)
                    };
                    popup::segmented_with(
                        pixmap,
                        Some(&mut *text),
                        (control, &row.segments),
                        &words,
                        chosen(&view.placed, row.line),
                        track,
                        tokens,
                        s,
                    );
                }
                draw_label(pixmap, text, row, label(row.line), text_px, s, label_ink);
                if l.docked {
                    let words = value_of(view, row.line);
                    draw_value(pixmap, text, row, &words, text_px, s, ink.text);
                }
            }
            Line::Show(part) if l.docked => {
                if let Some(switch) = row.control {
                    let on = shown(&view.placed).contains(&part);
                    draw_switch(pixmap, switch, on, &ink, s);
                }
                draw_label(pixmap, text, row, label(row.line), text_px, s, label_ink);
            }
            Line::Show(part) => {
                let Some(box_) = row.control else { continue };
                let (x, y, w, h) = box_.device(s);
                fill(pixmap, x, y, w, h, radius, ink.raised);
                if shown(&view.placed).contains(&part) {
                    popup::icon_in(
                        pixmap,
                        "check",
                        tokens.panel_glyph as f32,
                        box_,
                        s,
                        ink.accent,
                    );
                }
                draw_label(pixmap, text, row, label(row.line), text_px, s, label_ink);
            }
            Line::Field | Line::Zone | Line::Label => {
                if let Some(field) = row.control {
                    let (x, y, w, h) = field.device(s);
                    let track = if l.docked { faint } else { ink.raised };
                    fill(pixmap, x, y, w, h, radius, track);
                    let words = text_of(view, row.line);
                    let room = (field.w - 2.0 * PAD) * s;
                    let mut line = text.fit(&words, size, room);
                    let x0 = (field.x + PAD) * s;
                    let y0 = popup::middle(field.y, field.h, size, s);
                    text.draw(pixmap, &mut line, x0, y0, ink.text);
                    if view.typing && view.focus == Some(row.line) {
                        let caret_x = (x0 + line.width).round();
                        let (top, bottom) = (field.y + field.h * 0.2, field.y + field.h * 0.8);
                        fill(
                            pixmap,
                            caret_x,
                            top * s,
                            2.0 * s,
                            (bottom - top) * s,
                            0.0,
                            ink.accent,
                        );
                    }
                }
                // A clock's zone and label name their field, at its left or above it.
                draw_label(pixmap, text, row, label(row.line), text_px, s, label_ink);
                if l.docked {
                    let words = value_of(view, row.line);
                    draw_value(pixmap, text, row, &words, text_px, s, ink.text);
                }
            }
            Line::File => {
                let name = file_name(&view.placed);
                draw_label(pixmap, text, row, &name, size, s, ink.text);
            }
            Line::Button | Line::Every => {
                let Some(button) = row.control else { continue };
                edit_kit::button(
                    pixmap,
                    button,
                    label(row.line),
                    ButtonKind::Accent,
                    State::Rest,
                    tokens,
                    Some(&mut *text),
                    Style::GLASS,
                    s,
                );
            }
        }
        if view.focus == Some(row.line) {
            // The ring goes round the control, or the head of a closed row.
            let target = if l.docked {
                docked_target(row)
            } else {
                row.control
            };
            if let Some(target) = target {
                popup::focus_ring(pixmap, target, tokens.radius_control as f32, s, tokens);
            }
        }
    }
    if let Some(remove) = l.remove {
        // Remove's pill: filled in the red, the word in the glass's white,
        // lifted a little under the pointer.
        let (x, y, w, h) = remove.device(s);
        fill(
            pixmap,
            x,
            y,
            w,
            h,
            REMOVE_H / 2.0 * s,
            tokens.title_close_hover,
        );
        if view.remove_hover {
            fill(pixmap, x, y, w, h, REMOVE_H / 2.0 * s, ink.lit);
        }
        let words_size = tokens.panel_text_size as f32 * s;
        let mut words = text.line_in(tr("Remove"), words_size, crate::paint::Face::SEMIBOLD);
        let x0 = (remove.x + remove.w / 2.0) * s - words.width / 2.0;
        let y0 = popup::middle(remove.y, remove.h, words_size, s);
        text.draw(pixmap, &mut words, x0, y0, ink.text);
    }
}

/// The card's place in the screen reader's tree (M5.10c6c2): a group named
/// after the part, holding a radio group for each segmented line (its
/// segments as radio buttons, the chosen one on), a check box for each date
/// part (on or off), the field as a text input holding its words and the
/// button, as a button.
pub fn nodes(view: &View, l: &Layout) -> Vec<Item> {
    let at = |r: Rect| {
        accesskit::Rect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.y + r.h),
        )
    };
    let item = |role: Role, label: String, bounds, toggled, children| Item {
        role,
        label,
        bounds,
        children,
        toggled,
        value: None,
        value_text: None,
        disabled: false,
    };
    let mut children: Vec<Item> = l
        .rows
        .iter()
        .filter_map(|row| {
            let bounds = at(row.rect);
            match row.line {
                Line::Hours if l.docked => Some(item(
                    Role::CheckBox,
                    row_label(row.line, true).into(),
                    bounds,
                    Some(view.placed.hours.unwrap_or(24) == 24),
                    Vec::new(),
                )),
                Line::Face | Line::Hours | Line::Size | Line::Style | Line::Colour => {
                    if l.docked && row.control.is_none() {
                        let name = format!("{}: {}", label(row.line), value_of(view, row.line));
                        return Some(item(Role::Button, name, bounds, None, Vec::new()));
                    }
                    let now = chosen(&view.placed, row.line);
                    let buttons = segments(row.line)
                        .into_iter()
                        .enumerate()
                        .filter_map(|(j, (word, _))| {
                            let cell = row.segments.get(j)?;
                            let name = match row.line {
                                Line::Colour => trf("Colour, {name}", &[("name", word)]),
                                _ => word.into(),
                            };
                            Some(Item {
                                disabled: !choosable(view, row.line, j),
                                ..item(
                                    Role::RadioButton,
                                    name,
                                    at(*cell),
                                    Some(now == Some(j)),
                                    Vec::new(),
                                )
                            })
                        })
                        .collect();
                    Some(item(
                        Role::RadioGroup,
                        label(row.line).into(),
                        bounds,
                        None,
                        buttons,
                    ))
                }
                Line::Show(part) => Some(item(
                    Role::CheckBox,
                    show_word(part).into(),
                    bounds,
                    Some(shown(&view.placed).contains(&part)),
                    Vec::new(),
                )),
                Line::Field | Line::Zone | Line::Label => {
                    let Some(control) = row.control else {
                        // A closed row of the docked card: a button that opens it.
                        let name = format!("{}: {}", label(row.line), value_of(view, row.line));
                        return Some(item(Role::Button, name, bounds, None, Vec::new()));
                    };
                    let words = text_of(view, row.line);
                    let name = match row.line {
                        Line::Field => trf("Words: {words}", &[("words", words.as_str())]),
                        Line::Zone => trf("Time zone: {words}", &[("words", words.as_str())]),
                        _ => trf("Label: {words}", &[("words", words.as_str())]),
                    };
                    Some(item(Role::TextInput, name, at(control), None, Vec::new()))
                }
                Line::File => Some(item(
                    Role::Label,
                    file_name(&view.placed),
                    bounds,
                    None,
                    Vec::new(),
                )),
                Line::Button | Line::Every => Some(item(
                    Role::Button,
                    label(row.line).into(),
                    at(row.control?),
                    None,
                    Vec::new(),
                )),
            }
        })
        .collect();
    if let Some(remove) = l.remove {
        children.push(item(
            Role::Button,
            tr("Remove").into(),
            at(remove),
            None,
            Vec::new(),
        ));
    }
    vec![item(
        Role::Group,
        group_name(view, l.docked),
        at(l.card),
        None,
        children,
    )]
}
/// Where the card and its controls lie, the log line CI reads (M5.10c6c2),
/// in the screen's logical pixels: `card WxH at X+Y`, then each control that
/// the card has: `face_digital X+Y+WxH` (each segment of a segmented line,
/// by its settings word), `show_weekday X+Y+WxH` (a date part's check box),
/// `text X+Y+WxH` (the field) and `settings X+Y+WxH` (the button).
pub fn places(l: &Layout) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut out = format!(
        "card {:.0}x{:.0} at {:.0}+{:.0}",
        l.card.w, l.card.h, l.card.x, l.card.y
    );
    for row in &l.rows {
        match row.line {
            Line::Face | Line::Hours | Line::Size | Line::Style | Line::Colour => {
                for (cell, (_, choice)) in row.segments.iter().zip(segments(row.line).iter()) {
                    out.push_str(&format!(
                        ", {}_{} {}",
                        prefix(row.line),
                        key_of(choice),
                        at(*cell)
                    ));
                }
            }
            Line::Show(part) => {
                if let Some(control) = row.control {
                    out.push_str(&format!(", show_{} {}", part.name(), at(control)));
                }
            }
            Line::Field | Line::Zone | Line::Label => {
                if let Some(control) = row.control {
                    out.push_str(&format!(", {} {}", prefix(row.line), at(control)));
                }
            }
            Line::Button => {
                if let Some(control) = row.control {
                    out.push_str(&format!(", settings {}", at(control)));
                }
            }
            Line::Every => {
                if let Some(control) = row.control {
                    out.push_str(&format!(", every {}", at(control)));
                }
            }
            Line::File => {}
        }
    }
    if let Some(remove) = l.remove {
        out.push_str(&format!(", remove {}", at(remove)));
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::corner;

    fn clock(placed: Option<Face>) -> Placed {
        let mut p = plain(Part::Clock, Anchor::TopRight);
        p.face = placed;
        p
    }

    fn date() -> Placed {
        plain(Part::Date, Anchor::Bottom)
    }

    fn tokens() -> Tokens {
        Tokens::built_in()
    }

    /// The card over a clock on the left half at 1280 by 800, its part's
    /// rectangle `near`.
    fn near_left() -> Rect {
        Rect::new(40.0, 300.0, 240.0, 120.0)
    }

    #[test]
    fn card_for_is_true_for_the_parts_with_lines_and_the_battery_too() {
        for part in [
            Part::Clock,
            Part::Date,
            Part::Message,
            Part::Picture,
            Part::Battery,
        ] {
            assert!(card_for(part), "{part:?} has a card");
        }
        for part in [Part::Count, Part::Player, Part::Person] {
            assert!(!card_for(part), "{part:?} has no card");
        }
    }

    #[test]
    fn apply_chooses_each_clock_option_and_the_default_writes_none() {
        let p = plain(Part::Clock, Anchor::TopRight);
        let analog = apply(&p, &Choice::Face(Face::Analog));
        assert_eq!(analog.face, Some(Face::Analog));
        assert_eq!(apply(&analog, &Choice::Face(Face::Digital)).face, None);
        let twelve = apply(&p, &Choice::Hours(12));
        assert_eq!(twelve.hours, Some(12));
        assert_eq!(apply(&twelve, &Choice::Hours(24)).hours, None);
        let small = apply(&p, &Choice::Size(Size::Small));
        assert_eq!(small.size, Some(Size::Small));
        assert_eq!(apply(&small, &Choice::Size(Size::Large)).size, None);
    }

    #[test]
    fn apply_chooses_the_date_style_and_each_part_and_the_default_writes_none() {
        let p = plain(Part::Date, Anchor::Bottom);
        let short = apply(&p, &Choice::Style(DateStyle::Short));
        assert_eq!(short.style, Some(DateStyle::Short));
        assert_eq!(apply(&short, &Choice::Style(DateStyle::Long)).style, None);
        let with_year = apply(&p, &Choice::Show(DatePart::Year));
        assert_eq!(
            with_year.show,
            Some(vec![
                DatePart::Weekday,
                DatePart::Day,
                DatePart::Month,
                DatePart::Year
            ])
        );
        let back = apply(&with_year, &Choice::Show(DatePart::Year));
        assert_eq!(back.show, None, "the default shows writes no list");
        let weekday_day = apply(
            &apply(&p, &Choice::Show(DatePart::Month)),
            &Choice::Show(DatePart::Weekday),
        );
        assert_eq!(
            weekday_day.show,
            Some(vec![DatePart::Day]),
            "a list in the order the date shows it"
        );
    }

    #[test]
    fn the_last_date_part_on_cannot_be_turned_off() {
        let mut p = date();
        p.show = Some(vec![DatePart::Year]);
        assert_eq!(apply(&p, &Choice::Show(DatePart::Year)), p);
        let two = apply(&p, &Choice::Show(DatePart::Weekday));
        assert_eq!(two.show, Some(vec![DatePart::Weekday, DatePart::Year]));
        assert_eq!(
            apply(&two, &Choice::Show(DatePart::Weekday)).show,
            Some(vec![DatePart::Year])
        );
    }

    #[test]
    fn apply_sets_the_message_words_and_stops_them_at_the_most() {
        let mut m = plain(Part::Message, Anchor::Bottom);
        m.text = Some("old".into());
        assert_eq!(
            apply(&m, &Choice::Words("new".into())).text.as_deref(),
            Some("new")
        );
        let long = "x".repeat(most_letters() + 40);
        let kept = apply(&m, &Choice::Words(long)).text.unwrap_or_default();
        assert_eq!(kept.chars().count(), most_letters());
        assert_eq!(most_letters(), 120, "lock_screen's most characters");
    }

    #[test]
    fn an_empty_message_keeps_its_words_when_the_edit_settles() {
        let mut before = plain(Part::Message, Anchor::Bottom);
        before.text = Some("hello".into());
        let mut emptied = before.clone();
        emptied.text = Some(String::new());
        assert_eq!(settled(emptied, &before).text.as_deref(), Some("hello"));
        let mut typed = before.clone();
        typed.text = Some("bye".into());
        assert_eq!(settled(typed, &before).text.as_deref(), Some("bye"));
    }

    #[test]
    fn a_choice_is_logged_as_the_edit_mode_says_it() {
        let p = plain(Part::Clock, Anchor::TopRight);
        let analog = apply(&p, &Choice::Face(Face::Analog));
        assert_eq!(
            said(&Choice::Face(Face::Analog), &analog),
            "clock face analog"
        );
        let twelve = apply(&p, &Choice::Hours(12));
        assert_eq!(said(&Choice::Hours(12), &twelve), "clock hours 12");
        let small = apply(&p, &Choice::Size(Size::Small));
        assert_eq!(said(&Choice::Size(Size::Small), &small), "clock size small");
        let mut d = plain(Part::Date, Anchor::Bottom);
        d.style = Some(DateStyle::Short);
        assert_eq!(
            said(&Choice::Style(DateStyle::Short), &d),
            "date style short"
        );
        let shows = apply(&date(), &Choice::Show(DatePart::Year));
        assert_eq!(
            said(&Choice::Show(DatePart::Year), &shows),
            "date show weekday,day,month,year"
        );
        let mut m = plain(Part::Message, Anchor::Bottom);
        m.text = Some("four".into());
        assert_eq!(
            said(&Choice::Words("four".into()), &m),
            "message words 4 letters"
        );
    }

    #[test]
    fn layout_puts_the_card_right_of_a_part_on_the_left_half_and_left_of_one_at_the_edge() {
        let t = tokens();
        let screen = (1280.0, 800.0);
        let v = open(0, clock(None));
        let l = layout(&v, near_left(), screen, &t, None);
        assert!(l.card.x >= near_left().right(), "right of the part");
        assert!(l.card.right() <= screen.0 - lock::MARGIN);
        assert!(l.card.y >= lock::MARGIN && l.card.y + l.card.h <= screen.1 - lock::MARGIN);

        let at_edge = Rect::new(1280.0 - 24.0 - 240.0, 300.0, 240.0, 120.0);
        let l = layout(&v, at_edge, screen, &t, None);
        assert!(
            l.card.right() <= at_edge.x,
            "left of the part at the right edge"
        );
        assert!(l.card.x >= lock::MARGIN);
    }

    #[test]
    fn layout_keeps_the_card_inside_a_phone_screen_too() {
        let t = tokens();
        let screen = (360.0, 740.0);
        let v = open(0, clock(None));
        let near = Rect::new(20.0, 200.0, 200.0, 100.0);
        let l = layout(&v, near, screen, &t, None);
        assert!(l.card.x >= lock::MARGIN, "inside the margin");
        assert!(l.card.right() <= screen.0 - lock::MARGIN);
        assert!(l.card.y >= lock::MARGIN && l.card.y + l.card.h <= screen.1 - lock::MARGIN);
        let date_near = Rect::new(120.0, 600.0, 220.0, 40.0);
        let l = layout(&open(0, date()), date_near, screen, &t, None);
        assert!(l.card.right() <= screen.0 - lock::MARGIN && l.card.x >= lock::MARGIN);
        assert!(l.card.y + l.card.h <= screen.1 - lock::MARGIN);
    }

    #[test]
    fn hit_finds_each_segment_and_each_check_box_and_nothing_in_a_label() {
        let t = tokens();
        let l = layout(
            &open(0, clock(None)),
            near_left(),
            (1280.0, 800.0),
            &t,
            None,
        );
        let face = l.rows[0].clone();
        for (j, seg) in face.segments.iter().enumerate() {
            let (cx, cy) = (seg.x + seg.w / 2.0, seg.y + seg.h / 2.0);
            assert_eq!(hit(&l, cx, cy), Some(Hit::Segment(Line::Face, j)));
        }
        assert_eq!(
            hit(&l, face.rect.x + 2.0, face.rect.y + 2.0),
            None,
            "the label"
        );
        let d = layout(&open(0, date()), near_left(), (1280.0, 800.0), &t, None);
        let year = d
            .rows
            .iter()
            .find(|r| r.line == Line::Show(DatePart::Year))
            .expect("year");
        let c = year.control.expect("a check box");
        assert_eq!(
            hit(&d, c.x + 1.0, c.y + 1.0),
            Some(Hit::Toggle(Line::Show(DatePart::Year)))
        );
    }

    #[test]
    fn keys_step_left_and_right_on_a_segmented_row_and_stop_at_its_ends() {
        let mut v = open(0, clock(None));
        set_focus(&mut v, Some(Line::Face));
        assert_eq!(
            key(&v, Some(Key::Right), false, None),
            Step::Choose(Choice::Face(Face::Stacked))
        );
        assert_eq!(
            key(&v, Some(Key::Left), false, None),
            Step::Nothing,
            "digital is the first"
        );
        v.placed.face = Some(Face::Minimal);
        assert_eq!(
            key(&v, Some(Key::Right), false, None),
            Step::Nothing,
            "minimal is the last"
        );
        assert_eq!(
            key(&v, Some(Key::Left), false, None),
            Step::Choose(Choice::Face(Face::Analog))
        );
    }

    #[test]
    fn tab_steps_round_the_controls_and_escape_closes_the_card() {
        let mut v = open(0, clock(None));
        assert_eq!(
            key(&v, Some(Key::Tab), false, None),
            Step::Focus(Some(Line::Face))
        );
        set_focus(&mut v, Some(Line::Size));
        assert_eq!(
            key(&v, Some(Key::Tab), false, None),
            Step::Focus(Some(Line::Zone)),
            "Tab goes on to the time zone"
        );
        set_focus(&mut v, Some(Line::Label));
        assert_eq!(
            key(&v, Some(Key::Tab), false, None),
            Step::Focus(Some(Line::Colour)),
            "Tab goes on to the colour"
        );
        set_focus(&mut v, Some(Line::Every));
        assert_eq!(
            key(&v, Some(Key::Tab), false, None),
            Step::Focus(Some(Line::Face)),
            "Tab wraps"
        );
        assert_eq!(key(&v, Some(Key::Escape), false, None), Step::Close);
    }

    #[test]
    fn the_message_field_takes_letters_and_backspace_and_stops_at_the_most() {
        let mut m = plain(Part::Message, Anchor::Bottom);
        m.text = Some(String::new());
        let mut v = open(0, m);
        set_focus(&mut v, Some(Line::Field));
        assert!(v.typing, "the field takes letters once it has the keyboard");
        let Step::Choose(Choice::Words(one)) = key(&v, None, false, Some('h')) else {
            panic!("a typed letter goes to the field");
        };
        assert_eq!(one, "h");
        v.placed = apply(&v.placed, &Choice::Words(one));
        let Step::Choose(Choice::Words(two)) = key(&v, None, false, Some('i')) else {
            panic!("a second letter goes to the field");
        };
        v.placed = apply(&v.placed, &Choice::Words(two));
        assert_eq!(words_of(&v.placed), "hi");
        assert_eq!(
            key(&v, Some(Key::Delete), false, None),
            Step::Choose(Choice::Words("h".into()))
        );
        v.placed.text = Some("x".repeat(most_letters()));
        assert_eq!(
            key(&v, None, false, Some('y')),
            Step::Nothing,
            "at the most, no more letters"
        );
        assert_eq!(
            key(&v, None, false, Some('\u{1b}')),
            Step::Nothing,
            "a control is no letter"
        );
    }

    /// A clock's zone and label are two fields after its three rows (M5.10c7d):
    /// they take letters, a zone or label applies when it is one, else the
    /// field keeps what was typed as the draft, and each names its field.
    #[test]
    fn a_clocks_zone_and_label_are_fields_that_take_letters() {
        assert_eq!(
            lines(Part::Clock),
            vec![
                Line::Face,
                Line::Hours,
                Line::Size,
                Line::Zone,
                Line::Label,
                Line::Colour,
                Line::Every
            ]
        );
        let mut v = open(0, clock(None));
        set_focus(&mut v, Some(Line::Zone));
        assert!(v.typing, "the zone field takes letters");
        let Step::Choose(Choice::Zone(one)) = key(&v, None, false, Some('U')) else {
            panic!("a letter goes to the zone");
        };
        assert_eq!(one, "U");
        set_focus(&mut v, Some(Line::Label));
        let Step::Choose(Choice::Label(label)) = key(&v, None, false, Some('x')) else {
            panic!("a letter goes to the label");
        };
        assert_eq!(label, "x");
        v.placed.label = Some("x".repeat(lock_screen::MOST_LABEL));
        assert_eq!(
            key(&v, None, false, Some('y')),
            Step::Nothing,
            "24 letters at most"
        );
        assert_eq!(
            key(&v, Some(Key::Delete), false, None),
            Step::Choose(Choice::Label("x".repeat(23)))
        );
        // A zone that is a name applies; one that is not keeps the field's text.
        let placed = clock(None);
        let tokyo = apply(&placed, &Choice::Zone("Asia/Tokyo".into()));
        assert_eq!(tokyo.zone.as_deref(), Some("Asia/Tokyo"));
        assert_eq!(draft_of(&Choice::Zone("Asia/Tokyo".into()), &tokyo), None);
        let typed = Choice::Zone("Asia Tokyo".into());
        let unchanged = apply(&placed, &typed);
        assert_eq!(unchanged.zone, None);
        assert_eq!(
            draft_of(&typed, &unchanged),
            Some((Line::Zone, "Asia Tokyo".into()))
        );
        assert_eq!(apply(&tokyo, &Choice::Zone(String::new())).zone, None);
        // A label applies within 24 characters; a longer one is kept as a draft.
        let long = Choice::Label("x".repeat(25));
        assert_eq!(apply(&placed, &long).label, None);
        assert_eq!(
            draft_of(&long, &apply(&placed, &long)),
            Some((Line::Label, "x".repeat(25)))
        );
        let office = apply(&placed, &Choice::Label("Office".into()));
        assert_eq!(office.label.as_deref(), Some("Office"));
        assert_eq!(
            said(&Choice::Zone("Asia/Tokyo".into()), &tokyo),
            "clock zone Asia/Tokyo"
        );
        assert_eq!(
            said(&Choice::Label("Office".into()), &office),
            "clock label Office"
        );
        // The draft shows only in its own field.
        v.draft = Some((Line::Zone, "Asia Tok".into()));
        assert_eq!(text_of(&v, Line::Zone), "Asia Tok");
        assert_eq!(
            text_of(&v, Line::Label),
            "x".repeat(lock_screen::MOST_LABEL)
        );
    }

    /// The zone and label fields lie on the card's rows, each with its
    /// control, and `places` names them `zone` and `label` (M5.10c7d).
    #[test]
    fn the_zone_and_label_fields_have_their_controls_and_places() {
        let t = tokens();
        let l = layout(
            &open(0, clock(None)),
            near_left(),
            (1280.0, 800.0),
            &t,
            None,
        );
        let zone = l
            .rows
            .iter()
            .find(|r| r.line == Line::Zone)
            .and_then(|r| r.control)
            .expect("a zone field");
        assert_eq!(
            hit(&l, zone.x + 1.0, zone.y + 1.0),
            Some(Hit::Field(Line::Zone))
        );
        assert_eq!(
            press(&open(0, clock(None)), &l, zone.x + 1.0, zone.y + 1.0),
            Step::Focus(Some(Line::Zone))
        );
        let log = places(&l);
        assert!(log.contains(", zone ") && log.contains(", label "), "{log}");
        assert!(l.card.h > 2.0 * PAD + 3.0 * t.row as f32, "five rows");
    }

    /// The right panel of the edit mode on 1280 by 800 (M5.10c7e): 208 wide,
    /// from the canvas's top.
    fn dock() -> Rect {
        // As high as an 800 pixel screen's canvas (`lock_editor::panels`).
        Rect::new(1048.0, 76.0, 208.0, 640.0)
    }

    /// Docked, a clock's card is the panel's width at its top left, inside it,
    /// its four Face segments are equal cells across the inner width, each
    /// label sits above its control, and no row overlaps another.
    #[test]
    fn docked_the_card_fills_the_panel_width_and_stacks_its_lines() {
        let t = tokens();
        let mut v = open(0, clock(None));
        v.docked = true;
        let l = layout(&v, near_left(), (1280.0, 800.0), &t, Some(dock()));
        assert_eq!(
            (l.card.x, l.card.y, l.card.w),
            (1048.0, 76.0, 208.0),
            "the panel's width, at its top left"
        );
        assert!(l.card.h <= dock().h, "the rows fit the panel: {:?}", l.card);
        // The faces are a 2 by 2 grid filling the inner width (208 less 16 at
        // each side) in two columns 6 px apart, each 48 high (M5.10c7f): each
        // column is (176 - 6) / 2 = 85 wide.
        let face = &l.rows[0];
        assert_eq!(face.segments.len(), 4);
        assert!(
            face.segments
                .iter()
                .all(|c| (c.w - 85.0).abs() < 0.01 && (c.h - 48.0).abs() < 0.01)
        );
        assert_eq!(face.segments[0].x, l.card.x + 16.0);
        assert!((face.segments[1].x - (face.segments[0].x + 91.0)).abs() < 0.01);
        assert!((face.segments[2].y - (face.segments[0].y + 54.0)).abs() < 0.01);
        assert!((face.segments[3].x - face.segments[1].x).abs() < 0.01);
        // The heading sits 16 px under the panel's top, and the group under the grid.
        let heading = l.heading.expect("docked cards have a heading");
        assert!((heading.y - (l.card.y + 16.0)).abs() < 0.01);
        let group = l
            .group
            .expect("the clock's switch and disclosure rows are grouped");
        assert!((group.y - (face.rect.y + face.rect.h + 16.0)).abs() < 0.01);
        assert!((group.w - (208.0 - 32.0)).abs() < 0.01);
        // Four rows of 40 px, closed (Hours, Size, Zone, Label).
        assert!((group.h - 160.0).abs() < 0.01, "{group:?}");
        // Remove is the card's last line, 16 px under the clock's last line,
        // the colour's "Use on every widget" button, and the card ends 16 px
        // under it.
        let remove = l.remove.expect("docked cards have Remove");
        let every = l
            .rows
            .iter()
            .find(|r| r.line == Line::Every)
            .expect("the every button");
        assert!((remove.y - (every.rect.y + every.rect.h + 16.0)).abs() < 0.01);
        assert!((remove.h - ROW_H).abs() < 0.01);
        assert!((l.card.y + l.card.h - (remove.y + remove.h + 16.0)).abs() < 0.01);
        assert!(face.label.is_none(), "the grid has no label of its own");
        for pair in l.rows.windows(2) {
            assert!(
                pair[0].rect.y + pair[0].rect.h <= pair[1].rect.y,
                "{:?} overlaps {:?}",
                pair[0].rect,
                pair[1].rect
            );
        }
        for row in &l.rows {
            assert!(row.rect.x >= l.card.x && row.rect.right() <= l.card.right());
        }
    }

    /// A date's card docked lies inside the panel too, its check boxes at the
    /// right of their rows, and the message's field is the card's full width.
    #[test]
    fn docked_the_date_and_the_message_lie_inside_the_panel() {
        let t = tokens();
        let d = layout(
            &open(0, date()),
            near_left(),
            (1280.0, 800.0),
            &t,
            Some(dock()),
        );
        assert!(d.card.y + d.card.h <= 800.0 - lock::MARGIN, "{:?}", d.card);
        let year = d
            .rows
            .iter()
            .find(|r| r.line == Line::Show(DatePart::Year))
            .expect("year");
        assert!(year.control.expect("a box").right() <= d.card.right());
        let mut m = plain(Part::Message, Anchor::Bottom);
        m.text = Some(String::new());
        let m = layout(&open(0, m), near_left(), (1280.0, 800.0), &t, Some(dock()));
        // The field sits in the group's open row, 8 px in from its sides.
        let field = m.rows[0].control.expect("the field");
        assert!(
            (field.w - (208.0 - 2.0 * 16.0 - 2.0 * 8.0)).abs() < 0.01,
            "{field:?}"
        );
    }

    #[test]
    fn return_and_space_choose_a_date_part_and_press_the_button() {
        let mut v = open(0, date());
        set_focus(&mut v, Some(Line::Show(DatePart::Day)));
        assert_eq!(
            key(&v, Some(Key::Activate), false, Some(' ')),
            Step::Choose(Choice::Show(DatePart::Day))
        );
        let mut p = plain(Part::Picture, Anchor::Bottom);
        p.path = Some("/a/b.png".into());
        let mut picture = open(0, p);
        set_focus(&mut picture, Some(Line::Button));
        assert_eq!(
            key(&picture, Some(Key::Activate), false, None),
            Step::Settings
        );
    }

    #[test]
    fn a_press_on_a_segment_chooses_it_and_a_press_on_the_field_focuses_it() {
        let t = tokens();
        let v = open(0, clock(None));
        let l = layout(&v, near_left(), (1280.0, 800.0), &t, None);
        let seg = l.rows[1].segments[0];
        assert_eq!(
            press(&v, &l, seg.x + 1.0, seg.y + 1.0),
            Step::Choose(Choice::Hours(24))
        );
        let text = open(0, plain(Part::Message, Anchor::Bottom));
        let m = layout(&text, near_left(), (1280.0, 800.0), &t, None);
        let field = m.rows[0].control.expect("the field");
        assert_eq!(
            press(&text, &m, field.x + 1.0, field.y + 1.0),
            Step::Focus(Some(Line::Field))
        );
    }

    #[test]
    fn the_places_name_each_control_the_card_has() {
        let t = tokens();
        let l = layout(
            &open(0, clock(None)),
            near_left(),
            (1280.0, 800.0),
            &t,
            None,
        );
        let log = places(&l);
        assert!(log.starts_with("card 300x"), "{log}");
        for name in [
            "face_digital",
            "face_stacked",
            "face_analog",
            "face_minimal",
            "hours_24",
            "hours_12",
            "size_small",
            "size_medium",
            "size_large",
        ] {
            assert!(log.contains(&format!(", {name} ")), "{name} in {log}");
        }
        assert!(!log.contains("show_"), "a clock has no date parts: {log}");
        assert!(!log.contains("text "), "a clock has no field: {log}");
    }

    #[test]
    fn the_nodes_are_a_group_named_after_the_part_with_its_controls() {
        let t = tokens();
        let l = layout(
            &open(0, clock(Some(Face::Analog))),
            near_left(),
            (1280.0, 800.0),
            &t,
            None,
        );
        let v = open(0, clock(Some(Face::Analog)));
        let items = nodes(&v, &l);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].role, Role::Group);
        assert_eq!(
            items[0].children.len(),
            7,
            "face, hours, size, time zone, label, colour and use on every widget"
        );
        let face = &items[0].children[0];
        assert_eq!(face.role, Role::RadioGroup);
        assert_eq!(
            face.children
                .iter()
                .filter(|c| c.toggled == Some(true))
                .count(),
            1
        );
        assert_eq!(face.children[2].toggled, Some(true), "analog is chosen");
    }

    #[test]
    fn each_card_draws_over_the_lock_inside_the_screen_and_writes_its_pngs_when_asked() {
        let t = tokens();
        let mut text = Text::load(&t.font);
        let screen = (1280.0, 800.0);
        let corner_at = corner::layout(
            screen.0,
            screen.1,
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
        let parts = lock_screen::classic(false);
        let look = lock::Look {
            parts: &parts,
            player: None,
            people: false,
            keep_out: lock::keep_out(&corner_at),
            battery: false,
            background: lock_screen::Background::Backdrop,
            dim: lock_screen::DIM,
        };
        let at = lock::layout(screen.0, screen.1, &t, true, &look, &mut text);
        let lock_view = lock::View::new("14:06".into(), "Saturday 3 October".into(), "Ali".into());
        let mut clock = parts
            .iter()
            .find(|p| p.part == Part::Clock)
            .expect("the classic lock has a clock")
            .clone();
        clock.face = Some(Face::Analog);
        let mut date_part = parts
            .iter()
            .find(|p| p.part == Part::Date)
            .expect("the classic lock has a date")
            .clone();
        date_part.show = Some(vec![
            DatePart::Weekday,
            DatePart::Day,
            DatePart::Month,
            DatePart::Year,
        ]);
        for (file, placed) in [
            ("lock-card-clock.png", clock),
            ("lock-card-date.png", date_part),
        ] {
            let index = parts
                .iter()
                .position(|p| p.part == placed.part)
                .expect("the part is on the lock");
            let near = at.placed[index].expect("the part is drawn");
            let focus = match placed.part {
                Part::Date => Line::Show(DatePart::Year),
                _ => Line::Face,
            };
            let mut card = open(index, placed);
            set_focus(&mut card, Some(focus));
            let l = layout(&card, near, screen, &t, None);
            assert!(
                l.card.right() <= screen.0 - lock::MARGIN,
                "{file}: inside the screen"
            );
            assert!(
                l.card.y + l.card.h <= screen.1 - lock::MARGIN,
                "{file}: inside the screen"
            );
            let mut pixmap = Pixmap::new(1280, 800).expect("a pixmap");
            lock::paint(
                &mut pixmap,
                &lock_view,
                &at,
                &t,
                &mut text,
                None,
                &mut lock::Pictures::default(),
                &mut lock::Backdrops::default(),
                &edel::wallpapers::Wallpaper::Background,
                1.0,
            );
            paint(&mut pixmap, &card, &l, &t, &mut text, 1.0);
            if let Some(dir) = std::env::var_os("EDEL_LOCK_PNG") {
                let dir = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&dir).expect("the PNG folder");
                pixmap.save_png(dir.join(file)).expect("the PNG");
            }
        }
    }

    /// The docked card's disclosure rows (M5.10c7f): a closed row shows no
    /// control, opening one shows its control under its row and closes the
    /// other; the 24-hour switch toggles the hours; a press on a row's head
    /// opens it.
    #[test]
    fn docked_disclosure_rows_open_one_at_a_time_and_the_switch_toggles() {
        let t = tokens();
        let mut v = open(0, clock(None));
        v.docked = true;
        let at = |v: &View| layout(v, near_left(), (1280.0, 800.0), &t, Some(dock()));
        let find = |l: &Layout, line: Line| {
            l.rows
                .iter()
                .find(|r| r.line == line)
                .cloned()
                .expect("a row")
        };
        let l = at(&v);
        assert!(
            find(&l, Line::Size).control.is_none(),
            "a closed row has no control"
        );
        let switch = find(&l, Line::Hours).control.expect("the switch");
        assert_eq!(
            press(&v, &l, switch.x + 1.0, switch.y + 1.0),
            Step::Choose(Choice::Hours(12))
        );
        let head = find(&l, Line::Size).label.expect("its head");
        assert_eq!(
            press(&v, &l, head.x + 2.0, head.y + 2.0),
            Step::Disclose(Line::Size)
        );
        disclose(&mut v, Line::Size);
        let l = at(&v);
        assert!(
            find(&l, Line::Size).control.is_some(),
            "open, its segments show"
        );
        assert_eq!(find(&l, Line::Size).segments.len(), 3);
        disclose(&mut v, Line::Zone);
        assert_eq!(v.open, Some(Line::Zone), "one open at a time");
        let l = at(&v);
        assert!(find(&l, Line::Size).control.is_none());
        assert!(find(&l, Line::Zone).control.is_some());
        assert!(shows(&v, Line::Zone) && !shows(&v, Line::Size));
    }

    /// The swatches of `part` over `surface` (M8.21 part 21k2).
    fn swatches_over(part: Part, surface: Colour) -> Vec<Swatch> {
        swatches_for(
            part,
            &tokens(),
            Behind {
                surface,
                source: None,
            },
        )
    }

    /// A card of the clock with its swatches over `surface`, docked or floating.
    fn clock_with(surface: Colour, dock: Option<Rect>) -> (View, Layout) {
        let t = tokens();
        let mut v = open(0, clock(None));
        v.swatches = swatches_over(Part::Clock, surface);
        let l = layout(&v, near_left(), (1280.0, 800.0), &t, dock);
        (v, l)
    }

    #[test]
    fn colour_lines_are_on_the_widgets_that_take_a_colour_and_not_the_others() {
        assert_eq!(&lines(Part::Clock)[5..], &[Line::Colour, Line::Every]);
        assert_eq!(
            &lines(Part::Date)[lines(Part::Date).len() - 2..],
            &[Line::Colour, Line::Every]
        );
        assert_eq!(
            lines(Part::Message),
            vec![Line::Field, Line::Colour, Line::Every]
        );
        assert_eq!(lines(Part::Battery), vec![Line::Colour, Line::Every]);
        assert!(
            lines(Part::Picture)
                .iter()
                .all(|line| *line != Line::Colour)
        );
        assert!(lines(Part::Count).is_empty());
    }

    #[test]
    fn the_colour_line_has_twelve_swatches_and_apply_sets_or_clears_the_colour() {
        assert_eq!(segments(Line::Colour).len(), 12);
        assert_eq!(segments(Line::Colour)[0].0, tr("Automatic"));
        assert_eq!(segments(Line::Colour)[1].0, tr("Accent"));
        assert_eq!(segments(Line::Colour)[11].0, tr("From wallpaper"));
        let blue = apply(&clock(None), &Choice::Colour(Some("blue".into())));
        assert_eq!(blue.colour.as_deref(), Some("blue"));
        assert_eq!(
            said(&Choice::Colour(Some("blue".into())), &blue),
            "clock colour blue"
        );
        let automatic = apply(&blue, &Choice::Colour(None));
        assert_eq!(automatic.colour, None, "automatic writes none");
        assert_eq!(
            said(&Choice::Colour(None), &automatic),
            "clock colour automatic"
        );
        assert_eq!(
            apply(
                &plain(Part::Picture, Anchor::Centre),
                &Choice::Colour(Some("blue".into()))
            )
            .colour,
            None,
            "a picture takes no colour"
        );
        let blue_at = segments(Line::Colour)
            .iter()
            .position(|(_, choice)| *choice == Choice::Colour(Some("blue".into())))
            .expect("blue is a swatch");
        assert_eq!(chosen(&blue, Line::Colour), Some(blue_at));
        assert_eq!(chosen(&automatic, Line::Colour), Some(0));
        let hex = apply(&clock(None), &Choice::Colour(Some("#e8b04a".into())));
        assert_eq!(chosen(&hex, Line::Colour), None, "a hex is in no swatch");
    }

    #[test]
    fn a_swatch_that_does_not_read_cannot_be_chosen_and_is_announced_disabled() {
        // The accent over its own colour does not read: its contrast is 1:1.
        let accent = tokens().accent;
        let (mut v, l) = clock_with(accent, None);
        assert!(!v.swatches[1].reads, "the accent over itself");
        assert!(v.swatches[0].reads, "automatic always reads");
        let cell = find_colour(&l, 1);
        let (cx, cy) = (cell.x + cell.w / 2.0, cell.y + cell.h / 2.0);
        assert_eq!(press(&v, &l, cx, cy), Step::Nothing);
        let auto = find_colour(&l, 0);
        assert_eq!(
            press(&v, &l, auto.x + auto.w / 2.0, auto.y + auto.h / 2.0),
            Step::Choose(Choice::Colour(None))
        );
        set_focus(&mut v, Some(Line::Colour));
        assert_eq!(
            key(&v, Some(Key::Right), false, None),
            Step::Nothing,
            "next swatch reads no"
        );
        let items = nodes(&v, &l);
        let grid = items[0]
            .children
            .iter()
            .find(|c| c.role == Role::RadioGroup && c.label == "Colour")
            .expect("the colour grid");
        assert!(
            grid.children[1].label.starts_with("Colour, "),
            "named Colour, Accent"
        );
        assert!(
            grid.children[1].disabled,
            "not reading is announced disabled"
        );
        assert!(!grid.children[0].disabled, "automatic is not disabled");
    }

    /// The control of the colour swatch `j` on `l`.
    fn find_colour(l: &Layout, j: usize) -> Rect {
        let row = l
            .rows
            .iter()
            .find(|row| row.line == Line::Colour)
            .expect("the colour line");
        row.segments[j]
    }

    #[test]
    fn the_swatches_are_hit_one_by_one_and_every_is_its_own_button() {
        let (v, l) = clock_with(tokens().background, None);
        for j in 0..12 {
            let cell = find_colour(&l, j);
            let (cx, cy) = (cell.x + cell.w / 2.0, cell.y + cell.h / 2.0);
            assert_eq!(hit(&l, cx, cy), Some(Hit::Segment(Line::Colour, j)));
        }
        let every = l
            .rows
            .iter()
            .find(|r| r.line == Line::Every)
            .expect("every");
        let c = every.control.expect("the button");
        assert_eq!(hit(&l, c.x + 2.0, c.y + 2.0), Some(Hit::Every));
        assert_eq!(press(&v, &l, c.x + 2.0, c.y + 2.0), Step::Every);
        let mut v = v;
        set_focus(&mut v, Some(Line::Every));
        assert_eq!(key(&v, Some(Key::Activate), false, None), Step::Every);
    }

    #[test]
    fn the_docked_swatches_are_touch_sized_six_across_and_the_floating_grid_is_two_rows() {
        let (_, docked) = clock_with(tokens().background, Some(dock()));
        let row = docked
            .rows
            .iter()
            .find(|r| r.line == Line::Colour)
            .expect("the colour grid");
        assert_eq!(row.segments.len(), 12);
        assert!(row.segments.iter().all(|c| c.h >= 44.0), "touch sizes");
        let first = row.segments[0].y;
        assert_eq!(row.segments.iter().filter(|c| c.y == first).count(), 6);
        let (_, floating) = clock_with(tokens().background, None);
        let grid = floating
            .rows
            .iter()
            .find(|r| r.line == Line::Colour)
            .expect("the colour grid");
        assert_eq!(grid.rect.h, 2.0 * tokens().row as f32);
    }
}
