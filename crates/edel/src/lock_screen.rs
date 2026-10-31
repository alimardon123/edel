//! The lock and login screens as parts in places (M5.10c5, ADR-008's same
//! names decision): the data model, its check, the default arrangements and
//! the one pure layout function. No drawing here: shell-ui draws the parts,
//! and the settings file holds the person's list as `lock_screen.widgets`
//! ([`crate::settings`]). `Placed` and `arrange` place the desktop's widgets
//! too (M5.33c, `appearance.widgets`). Each part stands at an anchor, the edge
//! or the middle of the screen, with an offset from it, so one arrangement fits
//! a phone, a laptop and a 4K screen.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::i18n::{tr, trf};

/// A part of the lock and login screens (M5.10c5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    /// The time, large
    Clock,
    /// The day and the date
    Date,
    /// How many notifications wait
    Count,
    /// What plays now, with its buttons
    Player,
    /// The person: their name and the field their password is typed in.
    /// It stands exactly once.
    Person,
    /// Words of the person's own, from `text`
    Message,
    /// A picture, from `path`
    Picture,
    /// The battery's charge
    Battery,
}

impl Part {
    /// Every part, in the order of the enum (M5.33b).
    pub const ALL: [Part; 8] = [
        Part::Clock,
        Part::Date,
        Part::Count,
        Part::Player,
        Part::Person,
        Part::Message,
        Part::Picture,
        Part::Battery,
    ];

    /// The name the file writes the part by, the same as serde's: the
    /// widget's name in the catalogue.
    pub fn name(self) -> &'static str {
        self.widget().name
    }

    /// The part the file names `name`, if the release has it (M5.33b).
    pub fn named(name: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The part's entry in the widget catalogue (M5.33b), which owns its
    /// name, title and options.
    pub fn widget(self) -> &'static crate::widgets::Widget {
        use crate::widgets;
        match self {
            Part::Clock => &widgets::CLOCK,
            Part::Date => &widgets::DATE,
            Part::Count => &widgets::COUNT,
            Part::Player => &widgets::PLAYER,
            Part::Person => &widgets::PERSON,
            Part::Message => &widgets::MESSAGE,
            Part::Picture => &widgets::PICTURE,
            Part::Battery => &widgets::BATTERY,
        }
    }
}

/// Where a part stands: a corner, an edge's middle or the screen's middle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Centre,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    /// The name the file writes the anchor by, the same as serde's.
    pub fn name(self) -> &'static str {
        match self {
            Anchor::TopLeft => "top_left",
            Anchor::Top => "top",
            Anchor::TopRight => "top_right",
            Anchor::Left => "left",
            Anchor::Centre => "centre",
            Anchor::Right => "right",
            Anchor::BottomLeft => "bottom_left",
            Anchor::Bottom => "bottom",
            Anchor::BottomRight => "bottom_right",
        }
    }

    /// Where the anchor stands across the screen: at its left edge, its
    /// middle or its right edge.
    fn horizontal(self) -> Side {
        match self {
            Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => Side::Start,
            Anchor::Top | Anchor::Centre | Anchor::Bottom => Side::Middle,
            Anchor::TopRight | Anchor::Right | Anchor::BottomRight => Side::End,
        }
    }

    /// Where the anchor stands down the screen: at its top edge, its middle
    /// or its bottom edge.
    fn vertical(self) -> Side {
        match self {
            Anchor::TopLeft | Anchor::Top | Anchor::TopRight => Side::Start,
            Anchor::Left | Anchor::Centre | Anchor::Right => Side::Middle,
            Anchor::BottomLeft | Anchor::Bottom | Anchor::BottomRight => Side::End,
        }
    }
}

/// How the clock is drawn (M5.10c5b). Absent is `digital`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Face {
    /// One line of figures, `14:06`
    Digital,
    /// The hour over the minutes, each on a line of its own
    Stacked,
    /// A round dial with an hour hand and a minute hand
    Analog,
    /// One thin line, smaller than the others
    Minimal,
}

impl Face {
    /// The name the file writes the face by, the same as serde's.
    pub fn name(self) -> &'static str {
        match self {
            Face::Digital => "digital",
            Face::Stacked => "stacked",
            Face::Analog => "analog",
            Face::Minimal => "minimal",
        }
    }
}

/// How big the clock is (M5.10c5b). Absent is `large`, the token's size;
/// `medium` is two thirds of it and `small` a half.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    /// The name the file writes the size by, the same as serde's.
    pub fn name(self) -> &'static str {
        match self {
            Size::Small => "small",
            Size::Medium => "medium",
            Size::Large => "large",
        }
    }
}

/// A part of the date (M5.10c5b): the weekday, the day of the month, the
/// month and the year.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatePart {
    Weekday,
    Day,
    Month,
    Year,
}

impl DatePart {
    /// The name the file writes the part by, the same as serde's.
    pub fn name(self) -> &'static str {
        match self {
            DatePart::Weekday => "weekday",
            DatePart::Day => "day",
            DatePart::Month => "month",
            DatePart::Year => "year",
        }
    }
}

/// How the date is written (M5.10c5b). Absent is `long`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateStyle {
    /// `Saturday 3 October 2026`
    Long,
    /// `Sat 3 Oct 2026`
    Short,
    /// `Sat 03.10.2026`
    Numeric,
}

impl DateStyle {
    /// The name the file writes the style by, the same as serde's.
    pub fn name(self) -> &'static str {
        match self {
            DateStyle::Long => "long",
            DateStyle::Short => "short",
            DateStyle::Numeric => "numeric",
        }
    }
}

/// A clock's face when the file says nothing
pub const DEFAULT_FACE: Face = Face::Digital;
/// A clock's hours when the file says nothing
pub const DEFAULT_HOURS: u8 = 24;
/// The date's parts when the file says nothing
pub const DEFAULT_SHOW: [DatePart; 3] = [DatePart::Weekday, DatePart::Day, DatePart::Month];
/// The date's style when the file says nothing
pub const DEFAULT_STYLE: DateStyle = DateStyle::Long;

/// One part in its place: its anchor and an offset from it, each a share of the
/// screen's width (`x`) and height (`y`), from -1 to 1; a message's words, a
/// picture's path and the options of a clock or a date.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    /// Which widget this is
    #[serde(rename = "widget")]
    pub part: Part,
    /// Where it stands
    pub at: Anchor,
    /// An offset across the screen, a share of its width, from -1 to 1
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x: f64,
    /// An offset down the screen, a share of its height, from -1 to 1
    #[serde(default, skip_serializing_if = "is_zero")]
    pub y: f64,
    /// A message's words, 1 to 120 characters on one line
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A picture's file, an absolute path ending `.png`, `.jpg` or `.jpeg`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// A clock's face: `digital`, `stacked`, `analog` or `minimal`; absent is
    /// [`DEFAULT_FACE`], `digital`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<Face>,
    /// A clock's hours, 12 or 24; absent is [`DEFAULT_HOURS`], 24
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours: Option<u8>,
    /// A clock's size: `small`, `medium` or `large`; absent is `large`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// A clock's weight, from 100 (thin) to 900 (black) in steps of 100; absent
    /// is the token `font.lock_clock_weight`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u16>,
    /// The date's parts, each once; they show in the order weekday, day, month,
    /// year whatever order is written. Absent is [`DEFAULT_SHOW`], weekday, day and
    /// month.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<Vec<DatePart>>,
    /// The date's style: `long`, `short` or `numeric`; absent is [`DEFAULT_STYLE`],
    /// `long`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<DateStyle>,
    /// A clock's time zone, an IANA name such as `Asia/Tokyo`; absent is this
    /// machine's time (M5.10c7d)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// A clock's label, drawn under it: 1 to 24 characters on one line; absent
    /// with a zone is the zone's city, absent without one there is none
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A widget's colour (M5.33d): `accent`, a named accent such as `blue`,
    /// `wallpaper` or `#rrggbb`; absent is automatic, the text colour that
    /// reads over what lies behind it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
}

/// Offsets are f64, as TOML's floats are, so a value read and written back
/// stays as written (0.14, not 0.14000000059604645).
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

/// The most messages, and the most pictures, a lock screen holds.
const MOST_EACH: usize = 3;

/// The most characters a message's words may have.
const MOST_TEXT: usize = 120;

/// The most characters a clock's label may have (M5.10c7d).
pub const MOST_LABEL: usize = 24;

/// The most characters a time zone's name may have (M5.10c7d).
pub const MOST_ZONE: usize = 64;

/// Whether `label` is a clock's label: 1 to [`MOST_LABEL`] characters on one
/// line (M5.10c7d). The checker and the edit mode both ask it.
pub fn is_label(label: &str) -> bool {
    (1..=MOST_LABEL).contains(&label.chars().count()) && !label.contains(['\n', '\r'])
}

/// Whether `name` looks like an IANA time zone's name, such as `Asia/Tokyo`
/// or `UTC`: 1 to 64 characters of ASCII letters, digits, `/`, `_`, `-` and
/// `+`, not starting or ending with `/`, with no `//`. Whether the machine has
/// the zone is not asked here (M5.10c7d): the checker runs without the zone
/// files, and the lock falls back to this machine's time.
pub fn is_zone_name(name: &str) -> bool {
    let length = name.chars().count();
    (1..=MOST_ZONE).contains(&length)
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/_-+".contains(c))
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.contains("//")
}

/// The city a zone's name ends in, for a clock's label (M5.10c7d): the part
/// after the last `/` with `_` as a space, so `America/New_York` reads
/// `New York`.
pub fn zone_city(zone: &str) -> String {
    zone.rsplit('/').next().unwrap_or(zone).replace('_', " ")
}

/// The words under a clock (M5.10c7d): its label, else the zone's city when
/// it has a zone, else none.
pub fn clock_label(part: &Placed) -> Option<String> {
    part.label
        .clone()
        .or_else(|| part.zone.as_deref().map(zone_city))
}

/// Whether a picture's path is an absolute path to a picture file this
/// release shows.
fn is_picture_path(path: &str) -> bool {
    path.starts_with('/')
        && [".png", ".jpg", ".jpeg"]
            .iter()
            .any(|ext| path.ends_with(ext))
}

/// Checks a lock screen's parts (strict, as `edel settings check` is): the
/// person once, each other part at most once, at most [`MOST_EACH`]
/// messages and pictures, offsets from -1 to 1, words only on a message and
/// a path only on a picture. The message names the part and says what to do.
pub fn check(parts: &[Placed]) -> Result<(), String> {
    let count = |part: Part| parts.iter().filter(|p| p.part == part).count();
    match count(Part::Person) {
        0 => {
            return Err(tr(
                "the lock screen needs the person and their password field; add { widget = \"person\", at = \"centre\" }",
            )
            .into());
        }
        1 => {}
        _ => return Err(tr("the person stands once; take out the second").into()),
    }
    check_common(parts)
}

/// What [`check`] checks after the person's rule, which the desktop's list
/// does not take (M8.21 part 21j1): the counts, and each part's offsets,
/// words, path and options.
fn check_common(parts: &[Placed]) -> Result<(), String> {
    let count = |part: Part| parts.iter().filter(|p| p.part == part).count();
    if count(Part::Clock) > MOST_EACH {
        return Err(trf(
            "at most {most} clocks; take one out",
            &[("most", &MOST_EACH.to_string())],
        ));
    }
    for part in [Part::Date, Part::Count, Part::Player, Part::Battery] {
        if count(part) > 1 {
            return Err(trf(
                "the {part} stands once; take out the second",
                &[("part", part.name())],
            ));
        }
    }
    if count(Part::Message) > MOST_EACH {
        return Err(trf(
            "at most {most} messages; take one out",
            &[("most", &MOST_EACH.to_string())],
        ));
    }
    if count(Part::Picture) > MOST_EACH {
        return Err(trf(
            "at most {most} pictures; take one out",
            &[("most", &MOST_EACH.to_string())],
        ));
    }
    for p in parts {
        for (axis, value) in [("x", p.x), ("y", p.y)] {
            if !(-1.0..=1.0).contains(&value) {
                return Err(trf(
                    "the {part}'s offset {axis} is {value}; an offset is a share of the screen from -1 to 1, such as 0.1",
                    &[
                        ("part", p.part.name()),
                        ("axis", axis),
                        ("value", &value.to_string()),
                    ],
                ));
            }
        }
        match p.part {
            Part::Message => match p.text.as_deref() {
                None => {
                    return Err(trf(
                        "the message needs text; add text = \"...\", with 1 to {most} characters",
                        &[("most", &MOST_TEXT.to_string())],
                    ));
                }
                Some(text) => {
                    if text.contains(['\n', '\r']) {
                        return Err(tr("the message's text has a line break; use one line").into());
                    }
                    let length = text.chars().count();
                    if length == 0 || length > MOST_TEXT {
                        return Err(trf(
                            "the message's text is {count} characters; use 1 to {most}",
                            &[
                                ("count", &length.to_string()),
                                ("most", &MOST_TEXT.to_string()),
                            ],
                        ));
                    }
                }
            },
            _ if p.text.is_some() && !p.part.widget().takes("text") => {
                return Err(trf(
                    "the {part} has text; only a message has words, so take text out",
                    &[("part", p.part.name())],
                ));
            }
            _ => {}
        }
        match p.part {
            Part::Picture => match p.path.as_deref() {
                None => {
                    return Err(tr(
                        "the picture needs path, the whole path of a .png, .jpg or .jpeg file, such as \"/home/you/lock.png\"",
                    )
                    .into());
                }
                Some(path) if !is_picture_path(path) => {
                    return Err(trf(
                        "{path} is not a picture path; give the whole path, starting with / and ending in .png, .jpg or .jpeg",
                        &[("path", &format!("{path:?}"))],
                    ));
                }
                Some(_) => {}
            },
            _ if p.path.is_some() && !p.part.widget().takes("path") => {
                return Err(trf(
                    "the {part} has a path; only a picture has one, so take path out",
                    &[("part", p.part.name())],
                ));
            }
            _ => {}
        }
        check_options(p)?;
    }
    Ok(())
}

/// Checks the desktop's widgets (M8.21 part 21j1, M5.33c): each one must
/// stand on the desktop, as the catalogue says (the clock or the date), and
/// the rest is [`check`]'s own. The desktop has no person, so none is needed.
pub fn check_desktop(parts: &[Placed]) -> Result<(), String> {
    use crate::widgets::{CATALOGUE, Surface};
    let names: Vec<&str> = CATALOGUE
        .iter()
        .filter(|w| w.fits(Surface::Desktop))
        .map(|w| w.name)
        .collect();
    for p in parts {
        if !p.part.widget().fits(Surface::Desktop) {
            return Err(trf(
                "the {part} does not stand on the desktop; the desktop takes {names}",
                &[("part", p.part.name()), ("names", &names.join(" or "))],
            ));
        }
    }
    check_common(parts)
}

/// Checks `parts` for the surface `on` (M8.21 part 21j2): the desktop's
/// rule for the desktop ([`check_desktop`]), the lock screen's for the lock.
pub fn check_on(parts: &[Placed], on: crate::widgets::Surface) -> Result<(), String> {
    match on {
        crate::widgets::Surface::Desktop => check_desktop(parts),
        _ => check(parts),
    }
}

/// The key the desktop's widgets are kept under (M8.21 part 21j1): the
/// clock and the date over the wallpaper, under the windows.
pub const DESKTOP_WIDGETS: &str = "appearance.widgets";

/// The desktop's widgets as the release shows them first: the clock, small,
/// at the top right, and the date under it (M8.21 part 21j1).
pub fn desktop_default() -> Vec<Placed> {
    vec![
        Placed {
            size: Some(Size::Small),
            ..part_at(Part::Clock, Anchor::TopRight, 0.0)
        },
        part_at(Part::Date, Anchor::TopRight, 0.0),
    ]
}

/// The clock's and the date's options: each refused on a part whose catalogue
/// entry does not take it (M5.33b), and the clock's hours and weight and the
/// date's `show` checked. The message names the part and says what to do.
fn check_options(p: &Placed) -> Result<(), String> {
    let part = p.part.name();
    if p.colour.is_some() && !p.part.widget().takes(crate::widgets::COLOUR) {
        return Err(trf(
            "the {part} takes no colour, so take colour out",
            &[("part", part)],
        ));
    }
    if let Some(colour) = &p.colour {
        if !crate::widgets::is_colour(colour) {
            return Err(trf(
                "the {part}'s colour {value} is not a colour; choose accent, {choices} or a colour such as \"#e8b04a\"",
                &[
                    ("part", part),
                    ("value", &format!("{colour:?}")),
                    ("choices", &crate::settings::accent_choices()),
                ],
            ));
        }
    }
    if p.face.is_some() && !p.part.widget().takes("face") {
        return Err(trf(
            "the {part} has a face; only the clock has one, so take face out",
            &[("part", part)],
        ));
    }
    if p.hours.is_some() && !p.part.widget().takes("hours") {
        return Err(trf(
            "the {part} has hours; only the clock has them, so take hours out",
            &[("part", part)],
        ));
    }
    if p.size.is_some() && !p.part.widget().takes("size") {
        return Err(trf(
            "the {part} has a size; only the clock has one, so take size out",
            &[("part", part)],
        ));
    }
    if p.weight.is_some() && !p.part.widget().takes("weight") {
        return Err(trf(
            "the {part} has a weight; only the clock has one, so take weight out",
            &[("part", part)],
        ));
    }
    if p.zone.is_some() && !p.part.widget().takes("zone") {
        return Err(trf(
            "the {part} has a time zone; only the clock has one, so take zone out",
            &[("part", part)],
        ));
    }
    if p.label.is_some() && !p.part.widget().takes("label") {
        return Err(trf(
            "the {part} has a label; only the clock has one, so take label out",
            &[("part", part)],
        ));
    }
    if p.show.is_some() && !p.part.widget().takes("show") {
        return Err(trf(
            "the {part} has show; only the date has it, so take show out",
            &[("part", part)],
        ));
    }
    if p.style.is_some() && !p.part.widget().takes("style") {
        return Err(trf(
            "the {part} has a style; only the date has one, so take style out",
            &[("part", part)],
        ));
    }
    if let Some(hours) = p.hours {
        if hours != 12 && hours != 24 {
            return Err(trf(
                "the clock's hours is {hours}; use 12 or 24",
                &[("hours", &hours.to_string())],
            ));
        }
    }
    if let Some(weight) = p.weight {
        if !(100..=900).contains(&weight) || weight % 100 != 0 {
            return Err(trf(
                "the clock's weight is {weight}; use 100 to 900 in steps of 100, such as 300",
                &[("weight", &weight.to_string())],
            ));
        }
    }
    if let Some(zone) = &p.zone {
        if !is_zone_name(zone) {
            return Err(trf(
                "the clock's time zone {zone} is not a time zone's name; use one such as \"Asia/Tokyo\"",
                &[("zone", &format!("{zone:?}"))],
            ));
        }
    }
    if let Some(label) = &p.label {
        if !is_label(label) {
            return Err(tr("a clock's label is 1 to 24 characters on one line").into());
        }
    }
    if let Some(show) = &p.show {
        if show.is_empty() {
            return Err(tr(
                "the date's show is empty; name at least one of weekday, day, month and year",
            )
            .into());
        }
        for (i, shown) in show.iter().enumerate() {
            if show[..i].contains(shown) {
                return Err(trf(
                    "the date's show names the {part} twice; take one out",
                    &[("part", shown.name())],
                ));
            }
        }
    }
    Ok(())
}

/// The parts with `colour` written on each one that takes a colour and reads
/// with it (M5.33d, the editor's "Use on every widget"): `reads` says whether
/// a part reads with the colour on it. The others are left as they are.
pub fn coloured(
    parts: &[Placed],
    colour: Option<&str>,
    reads: impl Fn(&Placed) -> bool,
) -> Vec<Placed> {
    parts
        .iter()
        .map(|p| {
            if !p.part.widget().takes(crate::widgets::COLOUR) {
                return p.clone();
            }
            let next = Placed {
                colour: colour.map(str::to_string),
                ..p.clone()
            };
            if reads(&next) { next } else { p.clone() }
        })
        .collect()
}

/// A part at `at` with an offset down the screen of `y` (and none across).
fn part_at(part: Part, at: Anchor, y: f64) -> Placed {
    Placed {
        part,
        at,
        x: 0.0,
        y,
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

/// The person's part, as the classic arrangement places it: centred, below
/// the time on a screen, at the bottom above the player on a compact one.
fn person(compact: bool) -> Placed {
    if compact {
        part_at(Part::Person, Anchor::Bottom, -0.06)
    } else {
        part_at(Part::Person, Anchor::Centre, 0.21)
    }
}

/// The arrangement shown while the key is absent, the mockup's
/// (`docs/mockups/shell/lock.jpg`). Parts at the same anchor stack: the
/// layout pushes each one after the one before.
pub fn classic(compact: bool) -> Vec<Placed> {
    if compact {
        vec![
            part_at(Part::Date, Anchor::Top, 0.1),
            part_at(Part::Clock, Anchor::Top, 0.1),
            part_at(Part::Count, Anchor::Top, 0.1),
            part_at(Part::Player, Anchor::Top, 0.1),
            person(true),
        ]
    } else {
        vec![
            part_at(Part::Date, Anchor::Top, 0.11),
            part_at(Part::Clock, Anchor::Top, 0.11),
            part_at(Part::Count, Anchor::Top, 0.11),
            person(false),
            part_at(Part::Player, Anchor::BottomLeft, 0.0),
        ]
    }
}

/// The parts a lock screen shows: the person's or the machine's list when it
/// is given and passes [`check`], else [`classic`]. A list without a person
/// gets classic's person added at the end, so nobody is ever locked out
/// (Reliable, the first principle).
pub fn applying(parts: Option<&[Placed]>, compact: bool) -> Vec<Placed> {
    let Some(parts) = parts else {
        return classic(compact);
    };
    let mut list = parts.to_vec();
    if !list.iter().any(|p| p.part == Part::Person) {
        list.push(person(compact));
    }
    if check(&list).is_ok() {
        list
    } else {
        classic(compact)
    }
}

/// The built-in looks, in the order people see them (M5.10c5c).
pub const LOOKS: &[&str] = &[CLASSIC, "magazine", "minimal"];

/// The look that applies while none is chosen, and the one an unknown name
/// falls back to.
pub const CLASSIC: &str = "classic";

/// What `lock_screen.notifications` takes: the count shown (absent is
/// this) or hidden (M5.10c5c).
pub const NOTIFICATIONS: &[&str] = &[COUNT_SHOWN, COUNT_HIDDEN];
/// The unread notifications' count shows.
pub const COUNT_SHOWN: &str = "count";
/// The unread notifications' count is hidden.
pub const COUNT_HIDDEN: &str = "none";

/// The most characters a look's name may have.
const MOST_LOOK: usize = 32;

/// How much the built-in backgrounds are darkened, in percent, so the white
/// words read (M5.10c7g): a blurred or a coloured background by `DIM`, a sharp
/// one or a picture by `DIM_SHARP`. The lock picture's dim (`lock_screen.dim`)
/// is the person's, else the look's, else these; shell-ui reads them here.
pub const DIM: u32 = 30;
pub const DIM_SHARP: u32 = 40;
/// The most a dim may be, in percent (`lock_screen.dim`'s range from 0).
pub const MOST_DIM: u32 = 80;

/// What lies behind the lock screen's parts (M5.10c5c).
#[derive(Clone, Debug, PartialEq)]
pub enum Background {
    /// The mockup's navy scene (`lock.rs`'s `backdrop`).
    Backdrop,
    /// The wallpaper that applies, blurred for glass.
    Blurred,
    /// The wallpaper that applies, sharp.
    Sharp,
    /// One colour, `#rrggbb`.
    Colour(crate::tokens::Colour),
    /// A picture of the person's, an absolute path to a PNG or JPEG.
    Picture(PathBuf),
}

/// A part at `at` with an offset across the screen of `x` and down it of `y`.
fn offset(part: Part, at: Anchor, x: f64, y: f64) -> Placed {
    Placed {
        x,
        ..part_at(part, at, y)
    }
}

/// The parts of a built-in look (M5.10c5c), or None for a name that is not
/// one. `classic` is [`classic`]; `magazine` a large clock at the left with
/// the date under it and the person at the right; `minimal` the time alone.
/// Compact (under 600 px) each is laid out for a phone.
pub fn look_parts(name: &str, compact: bool) -> Option<Vec<Placed>> {
    Some(match (name, compact) {
        ("classic", _) => classic(compact),
        ("magazine", true) => classic(true)
            .into_iter()
            .map(|p| match p.part {
                Part::Clock => Placed {
                    weight: Some(200),
                    ..p
                },
                _ => p,
            })
            .collect(),
        ("magazine", false) => vec![
            Placed {
                size: Some(Size::Large),
                weight: Some(200),
                ..offset(Part::Clock, Anchor::TopLeft, 0.06, 0.12)
            },
            offset(Part::Date, Anchor::TopLeft, 0.06, 0.12),
            offset(Part::Count, Anchor::TopLeft, 0.06, 0.12),
            offset(Part::Person, Anchor::Right, -0.08, 0.0),
            offset(Part::Player, Anchor::BottomLeft, 0.06, 0.0),
        ],
        ("minimal", true) => vec![
            Placed {
                face: Some(Face::Minimal),
                ..part_at(Part::Clock, Anchor::Top, 0.12)
            },
            part_at(Part::Player, Anchor::Top, 0.12),
            part_at(Part::Person, Anchor::Bottom, -0.06),
        ],
        ("minimal", false) => vec![
            Placed {
                face: Some(Face::Minimal),
                ..part_at(Part::Clock, Anchor::Centre, -0.18)
            },
            part_at(Part::Person, Anchor::Centre, 0.16),
            part_at(Part::Player, Anchor::Bottom, 0.0),
        ],
        _ => return None,
    })
}

/// What lies behind a look (M5.10c5c): classic's navy scene, and the
/// wallpaper blurred for every other look, built-in or a person's own.
pub fn look_background(name: &str) -> Background {
    if name == CLASSIC {
        Background::Backdrop
    } else {
        Background::Blurred
    }
}

/// Reads a background (M5.10c5c): `backdrop`, `blurred`, `sharp`, a colour
/// `#rrggbb` (as [`crate::tokens::Colour::parse`] takes it) or an absolute
/// path ending `.png`, `.jpg` or `.jpeg`. Anything else is None.
pub fn parse_background(value: &str) -> Option<Background> {
    match value {
        "backdrop" => Some(Background::Backdrop),
        "blurred" => Some(Background::Blurred),
        "sharp" => Some(Background::Sharp),
        _ if value.starts_with('#') => crate::tokens::Colour::parse(value).map(Background::Colour),
        _ if is_picture_path(value) => Some(Background::Picture(PathBuf::from(value))),
        _ => None,
    }
}

/// The text a background is written as, the inverse of [`parse_background`].
pub fn background_value(background: &Background) -> String {
    match background {
        Background::Backdrop => "backdrop".into(),
        Background::Blurred => "blurred".into(),
        Background::Sharp => "sharp".into(),
        Background::Colour(colour) => colour.hex(),
        Background::Picture(path) => path.display().to_string(),
    }
}

/// The dim a background gets when nothing names one (M5.10c7g): `DIM_SHARP`
/// for a sharp picture or a picture of the person's, else `DIM`. Backdrop
/// lies under no dim, so its value is never drawn.
pub fn default_dim(background: &Background) -> u32 {
    match background {
        Background::Sharp | Background::Picture(_) => DIM_SHARP,
        Background::Backdrop | Background::Blurred | Background::Colour(_) => DIM,
    }
}

/// Whether a look's name is letters, digits and hyphens, not starting with a
/// hyphen, up to 32 characters (M5.10c5c).
pub fn is_look_name(name: &str) -> bool {
    (1..=MOST_LOOK).contains(&name.len())
        && !name.starts_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Checks that `look` names a built-in look or one of the file's own
/// (`own`, the `[lock_screen.looks]` tables). The message names the looks.
pub fn check_look(
    look: &str,
    own: Option<&BTreeMap<String, crate::settings::OwnLook>>,
) -> Result<(), String> {
    if LOOKS.contains(&look) || own.is_some_and(|own| own.contains_key(look)) {
        return Ok(());
    }
    let look = format!("{look:?}");
    match own.filter(|own| !own.is_empty()) {
        None => Err(trf(
            "{look} is not a look; choose {choices}",
            &[
                ("look", &look),
                ("choices", &crate::settings::or_list(LOOKS)),
            ],
        )),
        Some(own) => {
            let names: Vec<&str> = own.keys().map(String::as_str).collect();
            Err(trf(
                "{look} is not a look; choose {builtins}, or one of yours ({own})",
                &[
                    ("look", &look),
                    ("builtins", &LOOKS.join(", ")),
                    ("own", &crate::settings::or_list(&names)),
                ],
            ))
        }
    }
}

/// Checks a person's own looks (M5.10c5c): each name is letters, digits and
/// hyphens and not a built-in look's, and each look's parts pass [`check`]
/// (so each has the person, as a look must). The message names the look.
pub fn check_looks(looks: &BTreeMap<String, crate::settings::OwnLook>) -> Result<(), String> {
    for (name, look) in looks {
        let shown = format!("{name:?}");
        if !is_look_name(name) {
            return Err(trf(
                "{name} is not a look's name; use letters, digits and hyphens, such as \"home\"",
                &[("name", &shown)],
            ));
        }
        if LOOKS.contains(&name.as_str()) {
            return Err(trf(
                "{name} is a built-in look; name yours another way, such as \"home\"",
                &[("name", &shown)],
            ));
        }
        if let Some(background) = &look.background {
            if parse_background(background).is_none() {
                return Err(trf(
                    "the look {name} has the wallpaper {background}; use backdrop, blurred, sharp, a colour such as \"#1f2b48\" or a picture's full path",
                    &[("name", &shown), ("background", &format!("{background:?}"))],
                ));
            }
        }
        if let Some(dim) = look.dim.filter(|dim| *dim > MOST_DIM) {
            return Err(trf(
                "the look {name} has the dim {dim}; use a whole number from 0 to {most}",
                &[
                    ("name", &shown),
                    ("dim", &dim.to_string()),
                    ("most", &MOST_DIM.to_string()),
                ],
            ));
        }
        check(&look.parts).map_err(|why| {
            trf(
                "the look {name} does not check: {why}",
                &[("name", &shown), ("why", &why)],
            )
        })?;
    }
    Ok(())
}

/// Whether `name` is a built-in look, which never changes: editing one
/// saves a new look of the person's (Alimardon, 2026-10-11).
pub fn is_built_in(name: &str) -> bool {
    LOOKS.contains(&name)
}

/// The name a look shows as: a built-in look's translated, else the
/// person's own with a capital and spaces for hyphens (`classic-2` reads
/// Classic 2). Settings and the lock's editor both show this.
pub fn look_title(name: &str) -> String {
    match name {
        CLASSIC => tr("Classic").to_string(),
        "magazine" => tr("Magazine").to_string(),
        "minimal" => tr("Minimal").to_string(),
        own => {
            let words = own.replace('-', " ");
            let mut chars = words.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => words,
            }
        }
    }
}

/// A free name for a new look made from `base` (a copy, or the changes to a
/// built-in look): `base-2`, else `base-3` and on, cut so it stays a look's
/// name; `taken` says whether a name is in use.
pub fn new_look_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    // A copy of a copy counts on from its base: classic-2 gives classic-3.
    let stem = match base.rsplit_once('-') {
        Some((stem, n)) if !stem.is_empty() && n.parse::<u32>().is_ok() => stem,
        _ => base,
    };
    (2..)
        .map(|n| {
            let tail = format!("-{n}");
            let room = MOST_LOOK.saturating_sub(tail.len());
            format!("{}{tail}", &stem[..stem.len().min(room)])
        })
        .find(|name| !LOOKS.contains(&name.as_str()) && !taken(name))
        .unwrap_or_default()
}

/// What applies now as a look of the person's (Save as, Duplicate, and Done
/// on a built-in look): the wide arrangement and the background, written as
/// a value only when it is not what a new look gets without one.
pub fn as_own_look(lock: &crate::settings::LockScreen) -> crate::settings::OwnLook {
    let applied = applied(lock, false);
    crate::settings::OwnLook {
        parts: applied.parts,
        background: (applied.background != Background::Blurred)
            .then(|| background_value(&applied.background)),
        dim: (applied.dim != default_dim(&applied.background)).then_some(applied.dim),
    }
}

/// The value of `lock_screen.looks` with `name` set to `look` (None takes
/// it out), as TOML for `edel settings set`; None when no look is left, so
/// the key is reset rather than written empty.
pub fn looks_value(
    own: Option<&BTreeMap<String, crate::settings::OwnLook>>,
    name: &str,
    look: Option<crate::settings::OwnLook>,
) -> Option<String> {
    let mut own = own.cloned().unwrap_or_default();
    match look {
        Some(look) => {
            own.insert(name.to_string(), look);
        }
        None => {
            own.remove(name);
        }
    }
    if own.is_empty() {
        return None;
    }
    toml::Value::try_from(&own)
        .ok()
        .map(|value| value.to_string())
}

/// What the lock screen shows (M5.10c5c), from [`applied`].
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub parts: Vec<Placed>,
    pub background: Background,
    /// How much the background is darkened, in percent (M5.10c7g): the
    /// person's `lock_screen.dim`, else the look's, else `default_dim`.
    pub dim: u32,
    /// Whether the unread notifications' count shows
    pub notifications: bool,
}

/// What applies from `[lock_screen]` (the person's over the machine's, merged
/// by the caller), leniently (M5.10c5c). `parts` when set wins over the look,
/// and a list that fails [`check`] is classic's, as [`applying`] does; else
/// the look named by `look`, a built-in one or a person's own, else classic.
/// The background is `background` when it parses, else the look's: classic's
/// navy scene, or the blurred wallpaper for every other look. `notifications
/// = "none"` hides the count.
pub fn applied(lock: &crate::settings::LockScreen, compact: bool) -> Applied {
    let name = lock.look.as_deref().unwrap_or(CLASSIC);
    let list = look_parts(name, compact).or_else(|| {
        lock.looks
            .as_ref()
            .and_then(|own| own.get(name))
            .map(|own| own.parts.clone())
    });
    // A look this file does not have is classic, so its background too.
    let look = if list.is_some() { name } else { CLASSIC };
    let own = lock.looks.as_ref().and_then(|own| own.get(look));
    let own_background = own
        .and_then(|own| own.background.as_deref())
        .and_then(parse_background);
    let background = lock
        .background
        .as_deref()
        .and_then(parse_background)
        .or(own_background)
        .unwrap_or_else(|| look_background(look));
    let dim = lock
        .dim
        .or(own.and_then(|own| own.dim))
        .unwrap_or_else(|| default_dim(&background))
        .min(MOST_DIM);
    let parts = match (&lock.parts, list) {
        (Some(parts), _) => applying(Some(parts), compact),
        (None, Some(list)) => applying(Some(&list), compact),
        (None, None) => classic(compact),
    };
    Applied {
        parts,
        background,
        dim,
        notifications: lock.notifications.as_deref() != Some(COUNT_HIDDEN),
    }
}

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// Whether the two overlap. Touching edges do not overlap.
    fn overlaps(self, other: Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    /// The rectangle grown by `gap` on every side.
    fn grown(self, gap: f32) -> Rect {
        Rect {
            x: self.x - gap,
            y: self.y - gap,
            w: self.w + 2.0 * gap,
            h: self.h + 2.0 * gap,
        }
    }
}

/// What the layout needs to know of a part: where it wants to stand and its
/// size, measured by the one who draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub at: Anchor,
    /// An offset across the screen, a share of its width
    pub x: f32,
    /// An offset down the screen, a share of its height
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// One side of a rectangle's anchor: at its start, its middle or its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Start,
    Middle,
    End,
}

/// Where each item stands on a `screen` of logical pixels, in the items'
/// order, or None for one that cannot stand anywhere without overlapping
/// another or leaving the screen. `keep_out` are places no part may cover
/// (the corner's buttons); `gap` is the room kept between two parts and
/// round `keep_out`; `margin` the room kept from the screen's edges.
///
/// Each item stands at its anchor plus its offset and is clamped into the
/// screen less the margin, then moves past any part it overlaps: down for a
/// top or middle anchor, up for a bottom one. If that leaves the screen it
/// starts again and moves the other way. Deterministic.
pub fn arrange(
    screen: (f32, f32),
    items: &[Item],
    keep_out: &[Rect],
    gap: f32,
    margin: f32,
) -> Vec<Option<Rect>> {
    let mut obstacles: Vec<Rect> = keep_out.iter().map(|r| r.grown(gap)).collect();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let placed = place(item, screen, &obstacles, margin);
        if let Some(rect) = placed {
            obstacles.push(rect.grown(gap));
        }
        out.push(placed);
    }
    out
}

/// Where one item stands, given the obstacles placed so far (each grown by
/// the gap already).
fn place(
    item: &Item,
    (screen_w, screen_h): (f32, f32),
    obstacles: &[Rect],
    margin: f32,
) -> Option<Rect> {
    if item.w > screen_w - 2.0 * margin || item.h > screen_h - 2.0 * margin {
        return None;
    }
    let point_x = match item.at.horizontal() {
        Side::Start => margin,
        Side::Middle => screen_w / 2.0,
        Side::End => screen_w - margin,
    } + item.x * screen_w;
    let point_y = match item.at.vertical() {
        Side::Start => margin,
        Side::Middle => screen_h / 2.0,
        Side::End => screen_h - margin,
    } + item.y * screen_h;
    let x = match item.at.horizontal() {
        Side::Start => point_x,
        Side::Middle => point_x - item.w / 2.0,
        Side::End => point_x - item.w,
    };
    let y = match item.at.vertical() {
        Side::Start => point_y,
        Side::Middle => point_y - item.h / 2.0,
        Side::End => point_y - item.h,
    };
    let start = Rect {
        x: x.clamp(margin, screen_w - margin - item.w),
        y: y.clamp(margin, screen_h - margin - item.h),
        w: item.w,
        h: item.h,
    };
    // A bottom anchor moves up, every other one down.
    let down = item.at.vertical() != Side::End;
    slide(start, obstacles, (screen_w, screen_h), margin, down)
        .or_else(|| slide(start, obstacles, (screen_w, screen_h), margin, !down))
}

/// Moves `start` past the obstacles it overlaps, in one direction. Each move
/// passes at least one obstacle and never comes back to it, so
/// `obstacles.len() + 1` checks are enough. None when a move would leave the
/// screen less the margin, or when obstacles still block it after that.
fn slide(
    start: Rect,
    obstacles: &[Rect],
    (_, screen_h): (f32, f32),
    margin: f32,
    down: bool,
) -> Option<Rect> {
    let mut rect = start;
    for _ in 0..=obstacles.len() {
        let Some(hit) = obstacles.iter().find(|o| rect.overlaps(**o)) else {
            return Some(rect);
        };
        rect.y = if down { hit.y + hit.h } else { hit.y - rect.h };
        if rect.y < margin || rect.y + rect.h > screen_h - margin {
            return None;
        }
    }
    None
}

/// The reach, in logical pixels, within which a dragged part snaps to a
/// guide (M5.10c6).
pub const SNAP: f32 = 8.0;

/// The step, as a share of the screen, one press of an arrow key moves a part
/// by in the edit mode (M5.10c6): a hundredth.
pub const NUDGE: f64 = 0.01;

/// Offsets are written to 4 decimals, so a dragged part's file line reads
/// well and a nudge does not leave float noise behind (M5.10c6). Three
/// decimals would leave a part up to 2 px off on a 4K screen, past the 1 px
/// the round trip allows; four leave at most 0.4 px.
const DECIMALS: f64 = 10000.0;

/// Rounds an offset to [`DECIMALS`] and keeps it within -1 to 1.
fn round_offset(value: f64) -> f64 {
    ((value * DECIMALS).round() / DECIMALS).clamp(-1.0, 1.0)
}

/// The anchor at a side across and a side down the screen.
fn anchor_at(across: Side, down: Side) -> Anchor {
    match (down, across) {
        (Side::Start, Side::Start) => Anchor::TopLeft,
        (Side::Start, Side::Middle) => Anchor::Top,
        (Side::Start, Side::End) => Anchor::TopRight,
        (Side::Middle, Side::Start) => Anchor::Left,
        (Side::Middle, Side::Middle) => Anchor::Centre,
        (Side::Middle, Side::End) => Anchor::Right,
        (Side::End, Side::Start) => Anchor::BottomLeft,
        (Side::End, Side::Middle) => Anchor::Bottom,
        (Side::End, Side::End) => Anchor::BottomRight,
    }
}

/// The anchor and offset that put a part of size `rect.w` by `rect.h` at
/// `rect` on `screen`, as `arrange` places a part alone (M5.10c6): the anchor
/// is the one of the nine whose third of the screen holds `rect`'s middle,
/// and the offsets are what `place` needs to put the rectangle exactly there,
/// as shares of the screen, rounded to 4 decimals and kept within -1 to 1.
/// The inverse of `place`'s point maths, so a dragged part lands where it was
/// dropped.
pub fn placed_at(rect: Rect, screen: (f32, f32), margin: f32) -> (Anchor, f64, f64) {
    let (sw, sh) = (f64::from(screen.0), f64::from(screen.1));
    let (x, y) = (f64::from(rect.x), f64::from(rect.y));
    let (w, h) = (f64::from(rect.w), f64::from(rect.h));
    let margin = f64::from(margin);
    let third = |middle: f64, size: f64| {
        if middle < size / 3.0 {
            Side::Start
        } else if middle < size * 2.0 / 3.0 {
            Side::Middle
        } else {
            Side::End
        }
    };
    let across = third(x + w / 2.0, sw);
    let down = third(y + h / 2.0, sh);
    let ox = match across {
        Side::Start => (x - margin) / sw,
        Side::Middle => (x + w / 2.0 - sw / 2.0) / sw,
        Side::End => (x + w - (sw - margin)) / sw,
    };
    let oy = match down {
        Side::Start => (y - margin) / sh,
        Side::Middle => (y + h / 2.0 - sh / 2.0) / sh,
        Side::End => (y + h - (sh - margin)) / sh,
    };
    (anchor_at(across, down), round_offset(ox), round_offset(oy))
}

/// The same part as `placed`, with its options, text and path kept, standing
/// at `rect` on `screen` (M5.10c6; see [`placed_at`]).
pub fn moved(placed: &Placed, rect: Rect, screen: (f32, f32), margin: f32) -> Placed {
    let (at, x, y) = placed_at(rect, screen, margin);
    Placed {
        at,
        x,
        y,
        ..placed.clone()
    }
}

/// A line a dragged part snaps to (M5.10c6): across (`vertical` false, at a
/// y) or down (`vertical` true, at an x), in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub vertical: bool,
    pub at: f32,
}

/// The nearest line within `reach` of any of `edges`, as the shift that
/// moves that edge onto it and the line itself. None when nothing is in reach.
fn nearest(edges: &[f32], lines: &[f32], reach: f32) -> Option<(f32, f32)> {
    let mut best: Option<(f32, f32, f32)> = None; // (distance, shift, line)
    for edge in edges {
        for line in lines {
            let distance = (line - edge).abs();
            if distance <= reach && best.is_none_or(|(d, _, _)| distance < d) {
                best = Some((distance, line - edge, *line));
            }
        }
    }
    best.map(|(_, shift, line)| (shift, line))
}

/// `rect` moved onto the nearest guides within `reach` logical pixels, one
/// across and one down at most, and the guides it now lies on (M5.10c6).
/// Down lines: the margin at each side, the screen's middle, and each of
/// `others`' left edge, middle and right edge; across lines: the margin at
/// the top and bottom, the screen's middle, and each of `others`' top, middle
/// and bottom. A rectangle's left edge, middle and right edge each snap to a
/// down line (the nearest of the three pairs wins), its top, middle and bottom
/// to an across line. The result is clamped inside the margin, and a guide
/// the clamp moves the rectangle off is dropped. Nothing within reach: `rect`
/// unchanged and no guides.
pub fn snap(
    rect: Rect,
    others: &[Rect],
    screen: (f32, f32),
    margin: f32,
    reach: f32,
) -> (Rect, Vec<Guide>) {
    let (sw, sh) = screen;
    let mut down = vec![margin, sw - margin, sw / 2.0];
    let mut across = vec![margin, sh - margin, sh / 2.0];
    for other in others {
        down.extend([other.x, other.x + other.w / 2.0, other.x + other.w]);
        across.extend([other.y, other.y + other.h / 2.0, other.y + other.h]);
    }
    let x_edges = [rect.x, rect.x + rect.w / 2.0, rect.x + rect.w];
    let y_edges = [rect.y, rect.y + rect.h / 2.0, rect.y + rect.h];
    let snapped_x = nearest(&x_edges, &down, reach);
    let snapped_y = nearest(&y_edges, &across, reach);
    let mut out = rect;
    if let Some((shift, _)) = snapped_x {
        out.x += shift;
    }
    if let Some((shift, _)) = snapped_y {
        out.y += shift;
    }
    // The margin last, so a rectangle larger than the screen sits at it.
    out.x = out.x.min(sw - margin - out.w).max(margin);
    out.y = out.y.min(sh - margin - out.h).max(margin);
    let on = |edges: [f32; 3], line: f32| edges.iter().any(|e| (e - line).abs() < 0.5);
    let mut guides = Vec::new();
    if let Some((_, line)) = snapped_x {
        let edges = [out.x, out.x + out.w / 2.0, out.x + out.w];
        if on(edges, line) {
            guides.push(Guide {
                vertical: true,
                at: line,
            });
        }
    }
    if let Some((_, line)) = snapped_y {
        let edges = [out.y, out.y + out.h / 2.0, out.y + out.h];
        if on(edges, line) {
            guides.push(Guide {
                vertical: false,
                at: line,
            });
        }
    }
    (out, guides)
}

/// The parts the edit mode's drawer offers to add to `parts` (M5.10c6), in
/// `Part`'s order: each part `parts` lacks, and a message and a picture while
/// fewer than [`MOST_EACH`] stand. The person is never offered (it always
/// stands) and nor is the battery on a machine without one (`battery` false).
pub fn missing(parts: &[Placed], battery: bool) -> Vec<Part> {
    let count = |part: Part| parts.iter().filter(|p| p.part == part).count();
    [
        Part::Clock,
        Part::Date,
        Part::Count,
        Part::Player,
        Part::Message,
        Part::Picture,
        Part::Battery,
    ]
    .into_iter()
    .filter(|part| match part {
        Part::Clock | Part::Message | Part::Picture => count(*part) < MOST_EACH,
        Part::Battery => battery && count(*part) == 0,
        _ => count(*part) == 0,
    })
    .collect()
}

/// [`missing`] for the surface `on` (M8.21 part 21j2): only the parts that
/// stand there. On the desktop a clock may be added while fewer than
/// [`MOST_EACH`] stand, so a desktop with one clock still offers one.
pub fn missing_on(parts: &[Placed], battery: bool, on: crate::widgets::Surface) -> Vec<Part> {
    missing(parts, battery)
        .into_iter()
        .filter(|part| part.widget().fits(on))
        .collect()
}

/// The words a new message starts with, until the person writes their own
/// (M5.10c6).
pub fn message_placeholder() -> String {
    tr("Your message").into()
}

/// `parts` with `part` added at `rect`'s place (`placed_at`), or the same list
/// when it may not be added (`missing` does not offer it, a battery only
/// where the machine has one, `battery`) or when
/// the result would not pass [`check`], such as empty `words`. A message gets
/// `words`; a picture is not added here (it needs a file; the edit mode's card
/// chooses one, part c), so `Part::Picture` returns the list unchanged.
pub fn with_added(
    parts: &[Placed],
    part: Part,
    rect: Rect,
    screen: (f32, f32),
    margin: f32,
    words: &str,
    battery: bool,
) -> Vec<Placed> {
    with_added_on(
        parts,
        part,
        rect,
        screen,
        margin,
        words,
        battery,
        crate::widgets::Surface::LockScreen,
    )
}

/// [`with_added`] for the surface `on` (M8.21 part 21j2): the same rules,
/// with [`missing_on`] and [`check_on`], so the desktop takes a clock or a
/// date and no person, message or picture.
#[allow(clippy::too_many_arguments)]
pub fn with_added_on(
    parts: &[Placed],
    part: Part,
    rect: Rect,
    screen: (f32, f32),
    margin: f32,
    words: &str,
    battery: bool,
    on: crate::widgets::Surface,
) -> Vec<Placed> {
    if part == Part::Picture || !missing_on(parts, battery, on).contains(&part) {
        return parts.to_vec();
    }
    let (at, x, y) = placed_at(rect, screen, margin);
    let text = (part == Part::Message).then(|| words.to_string());
    let mut list = parts.to_vec();
    list.push(Placed {
        x,
        text,
        ..part_at(part, at, y)
    });
    if check_on(&list, on).is_ok() {
        list
    } else {
        parts.to_vec()
    }
}

/// The line `appearance.widgets` gets for the desktop's `parts` (M8.21 part
/// 21j2): `Ok(None)` when it equals the machine's list, so a default is never
/// written (the absence of a key is the default, ADR-008); otherwise the
/// list's text, `[]` included, or the refusal of [`check_desktop`].
pub fn desktop_line(parts: &[Placed], machine: Option<&str>) -> Result<Option<String>, String> {
    check_desktop(parts)?;
    if crate::settings::desktop_widgets_in(machine, None) == parts {
        return Ok(None);
    }
    widgets_value(parts).map(Some)
}

/// Whether the part at `index` may be taken away: every part but the person
/// (M5.10c6).
pub fn removable(parts: &[Placed], index: usize) -> bool {
    parts.get(index).is_some_and(|p| p.part != Part::Person)
}

/// `parts` without the part at `index`, or the same list when that is the
/// person (nobody is ever locked out) or `index` is out of range (M5.10c6).
pub fn without(parts: &[Placed], index: usize) -> Vec<Placed> {
    let mut list = parts.to_vec();
    if removable(parts, index) {
        list.remove(index);
    }
    list
}

/// The part moved by `dx`, `dy` steps of [`NUDGE`] by the arrow keys in the
/// edit mode (M5.10c6), its anchor kept, each offset kept within -1 to 1. An
/// axis with no step keeps its offset as it is.
pub fn nudged(placed: &Placed, dx: i32, dy: i32) -> Placed {
    let step = |value: f64, n: i32| {
        if n == 0 {
            value
        } else {
            round_offset(value + f64::from(n) * NUDGE)
        }
    };
    Placed {
        x: step(placed.x, dx),
        y: step(placed.y, dy),
        ..placed.clone()
    }
}

/// The settings key the list of widgets is written to (M5.10c5, M5.10c6): the
/// lock and login screens' edit mode and Settings' Lock screen page write it.
pub const WIDGETS: &str = "lock_screen.widgets";

/// The settings key the look's name is written to (M5.10c5c, M5.10c6c3).
pub const LOOK: &str = "lock_screen.look";

/// The settings key the wallpaper is written to (M5.10c5c, M5.10c6c3).
pub const WALLPAPER: &str = "lock_screen.wallpaper";

/// The settings key the dim is written to (M5.10c7g): a whole number of
/// percent, `MOST_DIM` at most.
pub const DIM_KEY: &str = "lock_screen.dim";

/// The text `lock_screen.widgets` takes, as `edel settings set` reads it: the
/// list of tables in one line, absent options left out (M5.10c6). Settings'
/// Lock screen page and the edit mode both write the list this way, so the
/// one function is here (ADR-010).
pub fn widgets_value(parts: &[Placed]) -> Result<String, String> {
    toml::Value::try_from(parts)
        .map(|value| value.to_string())
        .map_err(|e| e.to_string())
}

/// The settings key the person's own looks are written to (M5.10c7h).
pub const OWN_LOOKS: &str = "lock_screen.looks";

/// The lock screen's settings as the person's file sets them over the
/// machine's, with the looks of both (M5.10c7h): what the lock, its edit
/// mode, the looks switcher and Settings read, one merge through
/// [`crate::settings::Layers`] (ADR-010), where each look of the person's
/// replaces the machine's of its name. A text that does not read counts as
/// empty; no looks at all is none.
pub fn lock_over(machine: Option<&str>, person: Option<&str>) -> crate::settings::LockScreen {
    let mut lock = crate::settings::Layers::read(machine, person)
        .merged()
        .lock_screen
        .clone();
    lock.looks = lock.looks.filter(|looks| !looks.is_empty());
    lock
}

/// Whether the person's own text sets `key` (not the machine's alone).
fn person_sets(person: &str, machine: Option<&str>, key: &str) -> bool {
    matches!(
        crate::settings::source(key, machine, Some(person)),
        crate::settings::Source::Person(_)
    )
}

/// Sets `key` in a settings text (`value`), or takes it out (None). A text
/// with nothing in it yet starts with its format line, as `settings::write`
/// starts a new file; a removal from nothing changes nothing.
fn edit(text: String, key: &str, value: Option<&str>) -> Result<String, String> {
    match value {
        Some(value) => {
            let base = if text.trim().is_empty() {
                format!("format = {}\n", crate::settings::FORMAT)
            } else {
                text
            };
            crate::settings::set(&base, key, value).map_err(|e| format!("{e:#}"))
        }
        None if text.trim().is_empty() => Ok(text),
        None => crate::settings::unset(&text, key).map_err(|e| format!("{e:#}")),
    }
}

/// The text of the person's settings file after choosing the look `name`
/// (M5.10c7h): the one writer Settings' Lock screen page and the looks
/// switcher share. Classic is what applies without the key, so it takes
/// `look` out, unless the machine's file names another look, which classic
/// then sets. Any other look sets `look`. The person's own `parts` and
/// `background` lines go, as a look brings both. Returns `person` itself when
/// nothing changes, so nothing is written.
pub fn chose_text(person: &str, machine: Option<&str>, name: &str) -> Result<String, String> {
    let mut text = person.to_string();
    if name == CLASSIC {
        let without =
            crate::settings::chosen(LOOK, machine, None).unwrap_or_else(|| CLASSIC.to_string());
        if without == CLASSIC {
            if person_sets(person, machine, LOOK) {
                text = edit(text, LOOK, None)?;
            }
        } else {
            text = edit(text, LOOK, Some(CLASSIC))?;
        }
    } else {
        text = edit(text, LOOK, Some(&format!("{name:?}")))?;
    }
    for key in [WIDGETS, WALLPAPER] {
        if person_sets(person, machine, key) {
            text = edit(text, key, None)?;
        }
    }
    Ok(text)
}

/// The text of the person's settings file after saving a copy of what
/// applies as a look of the person's, named after `base` (M5.10c7h): New
/// look, Duplicate and Keep as a look, and the looks switcher's plus. The
/// copy is chosen, and the person's `widgets` and `wallpaper` lines go, as
/// the look keeps them. Returns the new text and the name the copy got.
pub fn saved_as_text(
    person: &str,
    machine: Option<&str>,
    base: &str,
) -> Result<(String, String), String> {
    let lock = lock_over(machine, Some(person));
    let own = crate::settings::read(person)
        .ok()
        .and_then(|read| read.file.lock_screen.looks);
    let taken = |name: &str| own.as_ref().is_some_and(|own| own.contains_key(name));
    let name = new_look_name(base, taken);
    let mut text = person.to_string();
    if let Some(value) = looks_value(own.as_ref(), &name, Some(as_own_look(&lock))) {
        text = edit(text, OWN_LOOKS, Some(&value))?;
    }
    text = edit(text, LOOK, Some(&format!("{name:?}")))?;
    for key in [WIDGETS, WALLPAPER] {
        if person_sets(person, machine, key) {
            text = edit(text, key, None)?;
        }
    }
    Ok((text, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look_of(text: &str) -> Option<String> {
        crate::settings::read(text).unwrap().file.lock_screen.look
    }

    #[test]
    fn choosing_a_look_sets_it_and_takes_the_persons_own_lines_out() {
        let text = chose_text(
            "format = 1\n[lock_screen]\nwallpaper = \"sharp\"\n",
            None,
            "magazine",
        )
        .unwrap();
        assert_eq!(look_of(&text).as_deref(), Some("magazine"));
        assert!(!text.contains("wallpaper"), "{text}");
    }

    #[test]
    fn choosing_classic_takes_the_key_out_unless_the_machine_names_another_look() {
        let person = "format = 1\n[lock_screen]\nlook = \"magazine\"\n";
        let text = chose_text(person, None, CLASSIC).unwrap();
        assert_eq!(look_of(&text), None);
        let machine = "format = 1\n[lock_screen]\nlook = \"minimal\"\n";
        let text = chose_text(person, Some(machine), CLASSIC).unwrap();
        assert_eq!(look_of(&text).as_deref(), Some(CLASSIC));
    }

    #[test]
    fn choosing_the_look_already_in_use_writes_nothing() {
        assert_eq!(chose_text("", None, CLASSIC).unwrap(), "");
        let person = "format = 1\n[lock_screen]\nlook = \"minimal\"\n";
        assert_eq!(chose_text(person, None, "minimal").unwrap(), person);
    }

    #[test]
    fn saving_a_copy_names_it_after_the_look_and_chooses_it() {
        let (text, name) = saved_as_text("", None, CLASSIC).unwrap();
        assert_eq!(name, "classic-2");
        assert_eq!(look_of(&text).as_deref(), Some("classic-2"));
        let own = crate::settings::read(&text).unwrap().file.lock_screen.looks;
        assert!(own.is_some_and(|own| own.contains_key("classic-2")));
        let (_, again) = saved_as_text(&text, None, CLASSIC).unwrap();
        assert_eq!(again, "classic-3");
    }

    #[test]
    fn the_lock_settings_merge_the_persons_lines_over_the_machines() {
        let machine = "format = 1\n[lock_screen]\nlook = \"magazine\"\ndim = 20\n";
        let person = "format = 1\n[lock_screen]\nlook = \"minimal\"\n";
        let lock = lock_over(Some(machine), Some(person));
        assert_eq!(lock.look.as_deref(), Some("minimal"));
        assert_eq!(lock.dim, Some(20));
        assert!(lock.looks.is_none());
    }

    const SCREENS: [(f32, f32); 5] = [
        (360.0, 740.0),
        (800.0, 1280.0),
        (1280.0, 800.0),
        (1920.0, 1080.0),
        (3840.0, 2160.0),
    ];
    const MARGIN: f32 = 24.0;
    const GAP: f32 = 16.0;
    const TOL: f32 = 0.01;
    const ANCHORS: [Anchor; 9] = [
        Anchor::TopLeft,
        Anchor::Top,
        Anchor::TopRight,
        Anchor::Left,
        Anchor::Centre,
        Anchor::Right,
        Anchor::BottomLeft,
        Anchor::Bottom,
        Anchor::BottomRight,
    ];

    fn item(at: Anchor, w: f32, h: f32) -> Item {
        Item {
            at,
            x: 0.0,
            y: 0.0,
            w,
            h,
        }
    }

    /// The corner's buttons, which no part may cover.
    fn corner(screen: (f32, f32)) -> Rect {
        Rect {
            x: screen.0 - 24.0 - 100.0,
            y: screen.1 - 24.0 - 40.0,
            w: 100.0,
            h: 40.0,
        }
    }

    /// Whether two rectangles are at least `gap` apart in one direction.
    fn apart(a: Rect, b: Rect, gap: f32) -> bool {
        a.x + a.w + gap <= b.x + TOL
            || b.x + b.w + gap <= a.x + TOL
            || a.y + a.h + gap <= b.y + TOL
            || b.y + b.h + gap <= a.y + TOL
    }

    /// Every rectangle is on the screen, and clear of the others and of
    /// `keep_out` by the gap.
    fn check_layout(screen: (f32, f32), keep_out: &[Rect], out: &[Option<Rect>]) {
        let (w, h) = screen;
        let placed: Vec<Rect> = out.iter().flatten().copied().collect();
        for r in &placed {
            assert!(r.x >= MARGIN - TOL, "{r:?} leaves the left of {screen:?}");
            assert!(r.y >= MARGIN - TOL, "{r:?} leaves the top of {screen:?}");
            assert!(r.x + r.w <= w - MARGIN + TOL, "{r:?} leaves {screen:?}");
            assert!(r.y + r.h <= h - MARGIN + TOL, "{r:?} leaves {screen:?}");
            for k in keep_out {
                assert!(apart(*r, *k, GAP), "{r:?} covers {k:?} on {screen:?}");
            }
        }
        for (i, a) in placed.iter().enumerate() {
            for b in &placed[i + 1..] {
                assert!(apart(*a, *b, GAP), "{a:?} and {b:?} touch on {screen:?}");
            }
        }
    }

    /// The items of `parts` at a screen's size: the sizes `lock` draws, and
    /// on a compact screen (under 600 px) scaled to its width, since the
    /// clock's 420 px would not fit 360 px less the margins.
    fn lock_items(screen: (f32, f32), parts: &[Placed]) -> Vec<Item> {
        let compact = screen.0 < 600.0;
        let scale = if compact {
            ((screen.0 - 2.0 * MARGIN) / 420.0).min(1.0)
        } else {
            1.0
        };
        parts
            .iter()
            .map(|p| {
                let (w, h) = match p.part {
                    Part::Date => (220.0, 25.0),
                    Part::Clock => (420.0, 180.0),
                    Part::Count => (160.0, 36.0),
                    Part::Person => (300.0, 200.0),
                    Part::Player => (400.0, 116.0),
                    _ => (200.0, 40.0),
                };
                Item {
                    at: p.at,
                    x: p.x as f32,
                    y: p.y as f32,
                    w: w * scale,
                    h: h * scale,
                }
            })
            .collect()
    }

    fn rect_of(parts: &[Placed], out: &[Option<Rect>], part: Part) -> Rect {
        let i = parts.iter().position(|p| p.part == part).expect("a part");
        out[i].expect("the part stands")
    }

    #[test]
    fn names_are_the_serde_names() {
        for part in Part::ALL {
            let value = toml::Value::try_from(part).unwrap();
            assert_eq!(value, toml::Value::String(part.name().into()));
        }
        for anchor in ANCHORS {
            let value = toml::Value::try_from(anchor).unwrap();
            assert_eq!(value, toml::Value::String(anchor.name().into()));
        }
    }

    #[test]
    fn each_option_is_taken_only_by_the_widgets_the_catalogue_names() {
        let mut options: Vec<&str> = crate::widgets::CATALOGUE
            .iter()
            .flat_map(|w| w.options.iter().copied())
            .collect();
        options.sort_unstable();
        options.dedup();
        for part in Part::ALL {
            for option in &options {
                let mut placed = part_at(part, Anchor::Top, 0.1);
                // A message needs its words and a picture its path to pass.
                if part == Part::Message {
                    placed.text = Some("Hi".into());
                }
                if part == Part::Picture {
                    placed.path = Some("/a.png".into());
                }
                match *option {
                    "face" => placed.face = Some(Face::Digital),
                    "hours" => placed.hours = Some(24),
                    "size" => placed.size = Some(Size::Large),
                    "weight" => placed.weight = Some(300),
                    "zone" => placed.zone = Some("Asia/Tokyo".into()),
                    "label" => placed.label = Some("Home".into()),
                    "show" => placed.show = Some(vec![DatePart::Weekday]),
                    "style" => placed.style = Some(DateStyle::Long),
                    "text" => placed.text = Some("Hi".into()),
                    "path" => placed.path = Some("/a.png".into()),
                    "colour" => placed.colour = Some("accent".into()),
                    other => panic!("no value for the option {other}"),
                }
                // The person stands alone; the others stand beside one.
                let parts = if part == Part::Person {
                    vec![placed]
                } else {
                    vec![person(false), placed]
                };
                let result = check(&parts);
                assert_eq!(
                    result.is_ok(),
                    part.widget().takes(option),
                    "{} with {option}: {result:?}",
                    part.name()
                );
            }
        }
    }

    #[test]
    fn the_classic_arrangements_pass_the_check() {
        assert!(check(&classic(false)).is_ok());
        assert!(check(&classic(true)).is_ok());
    }

    #[test]
    fn the_check_names_the_part_and_says_what_would_do() {
        let person = person(false);
        let message = |text: Option<&str>| Placed {
            text: text.map(String::from),
            ..part_at(Part::Message, Anchor::Bottom, 0.0)
        };
        let picture = |path: Option<&str>| Placed {
            path: path.map(String::from),
            ..part_at(Part::Picture, Anchor::Top, 0.0)
        };
        let cases: Vec<(Vec<Placed>, &str)> = vec![
            (vec![], "add { widget = \"person\", at = \"centre\" }"),
            (
                vec![person.clone(), person.clone()],
                "the person stands once; take out the second",
            ),
            (
                vec![
                    person.clone(),
                    part_at(Part::Clock, Anchor::Top, 0.0),
                    part_at(Part::Clock, Anchor::Top, 0.1),
                    part_at(Part::Clock, Anchor::Top, 0.2),
                    part_at(Part::Clock, Anchor::Top, 0.3),
                ],
                "at most 3 clocks; take one out",
            ),
            (
                vec![
                    person.clone(),
                    message(Some("a")),
                    message(Some("b")),
                    message(Some("c")),
                    message(Some("d")),
                ],
                "at most 3 messages",
            ),
            (
                vec![
                    person.clone(),
                    picture(Some("/a.png")),
                    picture(Some("/b.png")),
                    picture(Some("/c.png")),
                    picture(Some("/d.png")),
                ],
                "at most 3 pictures",
            ),
            (
                vec![Placed {
                    x: 2.0,
                    ..person.clone()
                }],
                "the person's offset x is 2",
            ),
            (
                vec![
                    Placed {
                        y: -1.5,
                        ..part_at(Part::Clock, Anchor::Top, 0.0)
                    },
                    person.clone(),
                ],
                "the clock's offset y is -1.5",
            ),
            (
                vec![person.clone(), message(None)],
                "the message needs text",
            ),
            (
                vec![person.clone(), message(Some(""))],
                "the message's text is 0 characters",
            ),
            (
                vec![person.clone(), message(Some(&"x".repeat(121)))],
                "the message's text is 121 characters; use 1 to 120",
            ),
            (
                vec![person.clone(), message(Some("two\nlines"))],
                "the message's text has a line break",
            ),
            (
                vec![
                    person.clone(),
                    Placed {
                        text: Some("hi".into()),
                        ..part_at(Part::Clock, Anchor::Top, 0.0)
                    },
                ],
                "the clock has text; only a message has words",
            ),
            (
                vec![person.clone(), picture(None)],
                "the picture needs path",
            ),
            (
                vec![person.clone(), picture(Some("lock.png"))],
                "\"lock.png\" is not a picture path",
            ),
            (
                vec![person.clone(), picture(Some("/lock.gif"))],
                "\"/lock.gif\" is not a picture path",
            ),
            (
                vec![
                    person.clone(),
                    Placed {
                        path: Some("/x.png".into()),
                        ..part_at(Part::Date, Anchor::Top, 0.0)
                    },
                ],
                "the date has a path; only a picture has one",
            ),
        ];
        for (parts, expected) in cases {
            let error = check(&parts).expect_err(expected);
            assert!(error.contains(expected), "{error}");
        }
        assert!(check(&[person.clone(), message(Some("Hello"))]).is_ok());
        assert!(check(&[person, picture(Some("/usr/share/edel/lock.jpeg"))]).is_ok());
    }

    #[test]
    fn applying_uses_the_list_when_it_passes_else_the_classic_one() {
        // No key: the classic arrangement, for either size class.
        assert_eq!(applying(None, false), classic(false));
        assert_eq!(applying(None, true), classic(true));
        // A list that passes is the one used.
        let list = vec![part_at(Part::Clock, Anchor::TopRight, 0.0), person(false)];
        assert_eq!(applying(Some(&list), false), list);
        // A list without a person gets classic's person at the end.
        let no_person = vec![part_at(Part::Clock, Anchor::TopRight, 0.0)];
        let mut expected = no_person.clone();
        expected.push(person(false));
        assert_eq!(applying(Some(&no_person), false), expected);
        assert_eq!(applying(Some(&no_person), true)[1], person(true));
        // A list that fails is the classic one, whatever it says.
        let two_persons = vec![person(false), person(true)];
        assert_eq!(applying(Some(&two_persons), false), classic(false));
        assert_eq!(applying(Some(&two_persons), true), classic(true));
        // An empty list still gets the person, so nobody is locked out.
        let empty: Vec<Placed> = Vec::new();
        assert_eq!(applying(Some(&empty), false), vec![person(false)]);
    }

    #[test]
    fn every_combination_of_anchors_and_sizes_stands_clear() {
        let mut cases = 0;
        for screen in SCREENS {
            let keep_out = [corner(screen)];
            let mut run = |items: &[Item]| {
                let first = arrange(screen, items, &keep_out, GAP, MARGIN);
                check_layout(screen, &keep_out, &first);
                assert_eq!(first, arrange(screen, items, &keep_out, GAP, MARGIN));
                cases += 1;
            };
            // Each anchor alone, with two sizes.
            for at in ANCHORS {
                run(&[item(at, 300.0, 60.0)]);
                run(&[item(at, 200.0, 150.0)]);
            }
            // Every ordered pair of anchors.
            for a in ANCHORS {
                for b in ANCHORS {
                    run(&[item(a, 300.0, 60.0), item(b, 200.0, 150.0)]);
                }
            }
            // Three parts at one anchor.
            for at in ANCHORS {
                run(&[
                    item(at, 300.0, 60.0),
                    item(at, 200.0, 150.0),
                    item(at, 160.0, 36.0),
                ]);
            }
            // The arrangement the lock draws, at the screen's own size class.
            let items = lock_items(screen, &classic(screen.0 < 600.0));
            run(&items);
        }
        assert_eq!(cases, SCREENS.len() * (9 * 2 + 81 + 9 + 1));
    }

    #[test]
    fn classic_places_every_part_in_its_order() {
        for screen in SCREENS {
            let parts = classic(screen.0 < 600.0);
            let keep_out = [corner(screen)];
            let out = arrange(screen, &lock_items(screen, &parts), &keep_out, GAP, MARGIN);
            check_layout(screen, &keep_out, &out);
            if screen == (1280.0, 800.0) || screen == (360.0, 740.0) {
                assert!(out.iter().all(Option::is_some), "{screen:?}: {out:?}");
                let date = rect_of(&parts, &out, Part::Date);
                let clock = rect_of(&parts, &out, Part::Clock);
                let person = rect_of(&parts, &out, Part::Person);
                assert!(date.y < clock.y, "{screen:?}: {date:?} {clock:?}");
                assert!(clock.y < person.y, "{screen:?}: {clock:?} {person:?}");
            }
        }
    }

    #[test]
    fn an_item_larger_than_the_screen_stands_nowhere() {
        for screen in SCREENS {
            let wide = item(Anchor::Centre, screen.0, 10.0);
            let tall = item(Anchor::Centre, 10.0, screen.1);
            assert_eq!(
                arrange(screen, &[wide, tall], &[], GAP, MARGIN),
                vec![None, None]
            );
        }
    }

    #[test]
    fn a_part_that_cannot_clear_the_others_stands_nowhere() {
        // A part that fills the screen's height leaves no room for a second.
        let screen = (360.0, 740.0);
        let full = item(Anchor::Top, 100.0, 680.0);
        let extra = item(Anchor::Top, 100.0, 40.0);
        let out = arrange(screen, &[full, extra], &[], GAP, MARGIN);
        assert!(out[0].is_some());
        assert_eq!(out[1], None);
    }

    /// A settings file's lock screen, as the tables `[[lock_screen.widgets]]`.
    #[derive(Deserialize, Serialize)]
    struct Doc {
        #[serde(rename = "widgets")]
        parts: Vec<Placed>,
    }

    #[test]
    fn option_names_are_the_serde_names() {
        for face in [Face::Digital, Face::Stacked, Face::Analog, Face::Minimal] {
            let value = toml::Value::try_from(face).unwrap();
            assert_eq!(value, toml::Value::String(face.name().into()));
        }
        for size in [Size::Small, Size::Medium, Size::Large] {
            let value = toml::Value::try_from(size).unwrap();
            assert_eq!(value, toml::Value::String(size.name().into()));
        }
        for part in [
            DatePart::Weekday,
            DatePart::Day,
            DatePart::Month,
            DatePart::Year,
        ] {
            let value = toml::Value::try_from(part).unwrap();
            assert_eq!(value, toml::Value::String(part.name().into()));
        }
        for style in [DateStyle::Long, DateStyle::Short, DateStyle::Numeric] {
            let value = toml::Value::try_from(style).unwrap();
            assert_eq!(value, toml::Value::String(style.name().into()));
        }
    }

    #[test]
    fn clock_and_date_options_read_back_from_toml() {
        let text = "[[widgets]]\nwidget = \"person\"\nat = \"centre\"\n\n\
            [[widgets]]\nwidget = \"clock\"\nat = \"top\"\ny = 0.14\nface = \"analog\"\n\
            hours = 12\nsize = \"medium\"\nweight = 300\n\n\
            [[widgets]]\nwidget = \"date\"\nat = \"top\"\nshow = [\"day\", \"month\"]\n\
            style = \"numeric\"\n";
        let doc: Doc = toml::from_str(text).unwrap();
        check(&doc.parts).unwrap();
        let clock = &doc.parts[1];
        assert_eq!(clock.face, Some(Face::Analog));
        assert_eq!(clock.hours, Some(12));
        assert_eq!(clock.size, Some(Size::Medium));
        assert_eq!(clock.weight, Some(300));
        let date = &doc.parts[2];
        assert_eq!(date.show, Some(vec![DatePart::Day, DatePart::Month]));
        assert_eq!(date.style, Some(DateStyle::Numeric));
        // Written back as they were read.
        let again: Doc = toml::from_str(&toml::to_string(&doc).unwrap()).unwrap();
        assert_eq!(again.parts, doc.parts);
    }

    #[test]
    fn an_absent_option_is_never_written() {
        let text = toml::to_string(&Doc {
            parts: classic(false),
        })
        .unwrap();
        for option in [
            "face", "hours", "size", "weight", "show", "style", "zone", "label",
        ] {
            assert!(!text.contains(option), "{option} written: {text}");
        }
    }

    /// A lock holds up to three clocks, each with its own zone and label; a
    /// fourth is refused (M5.10c7d).
    #[test]
    fn a_lock_holds_up_to_three_clocks() {
        let person = person(false);
        let clock = |y: f64| part_at(Part::Clock, Anchor::Top, y);
        for count in 1..=3 {
            let mut parts = vec![person.clone()];
            parts.extend((0..count).map(|i| clock(0.1 * i as f64)));
            assert!(check(&parts).is_ok(), "{count} clocks");
        }
        let four = [vec![person.clone()], vec![clock(0.0); 4]].concat();
        assert_eq!(check(&four), Err("at most 3 clocks; take one out".into()));
        // The drawer keeps offering one while fewer than three stand.
        assert!(missing(&[person.clone(), clock(0.0), clock(0.1)], false).contains(&Part::Clock));
        assert!(!missing(&four, false).contains(&Part::Clock));
    }

    #[test]
    fn zone_names_are_checked_by_their_shape_only() {
        for good in [
            "UTC",
            "Asia/Tokyo",
            "America/New_York",
            "Etc/GMT+5",
            "Europe/Kyiv",
        ] {
            assert!(is_zone_name(good), "{good}");
        }
        for bad in [
            "",
            "/Asia",
            "Asia/",
            "Asia//Tokyo",
            "Asia Tokyo",
            "Asia/Tokyo.x",
            "é",
        ] {
            assert!(!is_zone_name(bad), "{bad:?}");
        }
        let longest = "a".repeat(64);
        assert!(is_zone_name(&longest));
        assert!(!is_zone_name(&format!("{longest}a")));
    }

    #[test]
    fn a_zones_city_is_its_last_name_with_spaces() {
        assert_eq!(zone_city("America/New_York"), "New York");
        assert_eq!(zone_city("Asia/Tokyo"), "Tokyo");
        assert_eq!(zone_city("UTC"), "UTC");
        let zoned = Placed {
            zone: Some("Asia/Tokyo".into()),
            ..part_at(Part::Clock, Anchor::Top, 0.0)
        };
        assert_eq!(clock_label(&zoned), Some("Tokyo".into()));
        let labelled = Placed {
            label: Some("Office".into()),
            ..zoned.clone()
        };
        assert_eq!(clock_label(&labelled), Some("Office".into()));
        assert_eq!(clock_label(&part_at(Part::Clock, Anchor::Top, 0.0)), None);
    }

    #[test]
    fn a_clocks_zone_and_label_are_checked_and_read_back() {
        let person = person(false);
        let clock = |zone: Option<&str>, label: Option<&str>| Placed {
            zone: zone.map(Into::into),
            label: label.map(Into::into),
            ..part_at(Part::Clock, Anchor::TopRight, 0.0)
        };
        let good = clock(Some("Asia/Tokyo"), Some("Tokyo office"));
        assert!(check(&[person.clone(), good.clone()]).is_ok());
        let text = toml::to_string(&Doc {
            parts: vec![person.clone(), good.clone()],
        })
        .unwrap();
        assert!(text.contains("zone = \"Asia/Tokyo\""), "{text}");
        let again: Doc = toml::from_str(&text).unwrap();
        assert_eq!(again.parts[1], good);
        let cases = [
            (
                clock(Some("Asia Tokyo"), None),
                "the clock's time zone \"Asia Tokyo\" is not a time zone's name",
            ),
            (
                clock(Some("Asia/"), None),
                "the clock's time zone \"Asia/\" is not a time zone's name",
            ),
            (
                clock(None, Some("")),
                "a clock's label is 1 to 24 characters on one line",
            ),
            (
                clock(None, Some("x".repeat(25).as_str())),
                "a clock's label is 1 to 24 characters on one line",
            ),
            (
                clock(None, Some("two\nlines")),
                "a clock's label is 1 to 24 characters on one line",
            ),
            (
                Placed {
                    zone: Some("UTC".into()),
                    ..part_at(Part::Date, Anchor::Top, 0.0)
                },
                "the date has a time zone; only the clock has one",
            ),
            (
                Placed {
                    label: Some("hi".into()),
                    ..part_at(Part::Date, Anchor::Top, 0.0)
                },
                "the date has a label; only the clock has one",
            ),
        ];
        for (part, expected) in cases {
            let error = check(&[person.clone(), part]).expect_err(expected);
            assert!(error.contains(expected), "{error}");
        }
        // A label of 24 characters, and a zone with no label, pass.
        assert!(check(&[person, clock(Some("UTC"), Some("x".repeat(24).as_str()))]).is_ok());
    }

    #[test]
    fn the_clock_and_date_options_are_checked() {
        let person = person(false);
        let cases: Vec<(Placed, &str)> = vec![
            (
                Placed {
                    face: Some(Face::Analog),
                    ..part_at(Part::Date, Anchor::Top, 0.0)
                },
                "the date has a face; only the clock has one",
            ),
            (
                Placed {
                    hours: Some(24),
                    ..part_at(Part::Date, Anchor::Top, 0.0)
                },
                "the date has hours; only the clock has them",
            ),
            (
                Placed {
                    size: Some(Size::Small),
                    text: Some("hi".into()),
                    ..part_at(Part::Message, Anchor::Top, 0.0)
                },
                "the message has a size; only the clock has one",
            ),
            (
                Placed {
                    weight: Some(400),
                    ..part_at(Part::Date, Anchor::Top, 0.0)
                },
                "the date has a weight; only the clock has one",
            ),
            (
                Placed {
                    show: Some(vec![DatePart::Day]),
                    ..part_at(Part::Clock, Anchor::Top, 0.0)
                },
                "the clock has show; only the date has it",
            ),
            (
                Placed {
                    style: Some(DateStyle::Short),
                    ..part_at(Part::Clock, Anchor::Top, 0.0)
                },
                "the clock has a style; only the date has one",
            ),
            (
                Placed {
                    hours: Some(13),
                    ..part_at(Part::Clock, Anchor::Top, 0.0)
                },
                "the clock's hours is 13; use 12 or 24",
            ),
            (
                Placed {
                    weight: Some(150),
                    ..part_at(Part::Clock, Anchor::Top, 0.0)
                },
                "the clock's weight is 150; use 100 to 900 in steps of 100",
            ),
            (
                Placed {
                    weight: Some(1000),
                    ..part_at(Part::Clock, Anchor::Top, 0.0)
                },
                "the clock's weight is 1000",
            ),
            (
                Placed {
                    weight: Some(0),
                    ..part_at(Part::Clock, Anchor::Top, 0.0)
                },
                "the clock's weight is 0",
            ),
            (
                Placed {
                    show: Some(vec![]),
                    ..part_at(Part::Date, Anchor::Top, 0.0)
                },
                "the date's show is empty",
            ),
            (
                Placed {
                    show: Some(vec![DatePart::Day, DatePart::Month, DatePart::Day]),
                    ..part_at(Part::Date, Anchor::Top, 0.0)
                },
                "the date's show names the day twice; take one out",
            ),
        ];
        for (option, expected) in cases {
            let parts = vec![person.clone(), option];
            let error = check(&parts).expect_err(expected);
            assert!(error.contains(expected), "{error}");
        }
        // Good values pass: each clock face, both hour counts, the weights at
        // the ends and in between, and every date part in any order.
        for face in [Face::Digital, Face::Stacked, Face::Analog, Face::Minimal] {
            let good = Placed {
                face: Some(face),
                ..part_at(Part::Clock, Anchor::Top, 0.0)
            };
            assert!(check(&[person.clone(), good]).is_ok(), "{face:?}");
        }
        for (hours, weight) in [(12, 100), (24, 900), (12, 300)] {
            let good = Placed {
                hours: Some(hours),
                weight: Some(weight),
                size: Some(Size::Small),
                ..part_at(Part::Clock, Anchor::Top, 0.0)
            };
            assert!(check(&[person.clone(), good]).is_ok(), "{hours} {weight}");
        }
        let good = Placed {
            show: Some(vec![DatePart::Year, DatePart::Weekday]),
            style: Some(DateStyle::Short),
            ..part_at(Part::Date, Anchor::Top, 0.0)
        };
        assert!(check(&[person.clone(), good]).is_ok());
    }

    /// Every built-in look passes the check and lays out with every part placed
    /// at the sizes the classic test uses (M5.10c5c).
    #[test]
    fn every_built_in_look_passes_the_check_and_stands_clear() {
        for name in LOOKS {
            for screen in [(1280.0, 800.0), (1920.0, 1080.0), (360.0, 740.0)] {
                let compact = screen.0 < 600.0;
                let parts = look_parts(name, compact).expect(name);
                assert!(
                    check(&parts).is_ok(),
                    "{name} {screen:?}: {:?}",
                    check(&parts)
                );
                let keep_out = [corner(screen)];
                let items = lock_items(screen, &parts);
                let out = arrange(screen, &items, &keep_out, GAP, MARGIN);
                check_layout(screen, &keep_out, &out);
                assert!(
                    out.iter().all(Option::is_some),
                    "{name} {screen:?}: {out:?}"
                );
            }
        }
        assert!(look_parts("gallery", false).is_none());
    }

    #[test]
    fn a_look_background_is_classic_backdrop_and_blurred_for_the_rest() {
        assert_eq!(look_background("classic"), Background::Backdrop);
        assert_eq!(look_background("magazine"), Background::Blurred);
        assert_eq!(look_background("home"), Background::Blurred);
    }

    #[test]
    fn backgrounds_read_and_write_back() {
        for (text, background) in [
            ("backdrop", Background::Backdrop),
            ("blurred", Background::Blurred),
            ("sharp", Background::Sharp),
            (
                "#1f2b48",
                Background::Colour(crate::tokens::Colour::parse("#1f2b48").unwrap()),
            ),
            (
                "/home/you/lock.png",
                Background::Picture("/home/you/lock.png".into()),
            ),
        ] {
            assert_eq!(parse_background(text), Some(background.clone()), "{text}");
            assert_eq!(background_value(&background), text);
        }
        for bad in ["lock.png", "/lock.gif", "navy", "#12", "Blurred", ""] {
            assert_eq!(parse_background(bad), None, "{bad}");
        }
    }

    #[test]
    fn look_names_are_letters_digits_and_hyphens() {
        assert!(is_look_name("home"));
        assert!(is_look_name("my-look-2"));
        assert!(!is_look_name(""));
        assert!(!is_look_name("my look"));
        assert!(!is_look_name("-home"));
        assert!(!is_look_name(&"x".repeat(33)));
    }

    #[test]
    fn applied_is_classic_with_the_backdrop_and_the_count_when_absent() {
        let lock = crate::settings::LockScreen::default();
        for compact in [false, true] {
            let applied = applied(&lock, compact);
            assert_eq!(applied.parts, classic(compact));
            assert_eq!(applied.background, Background::Backdrop);
            assert!(applied.notifications);
        }
    }

    #[test]
    fn applied_takes_the_look_then_the_parts_background_and_count_over_it() {
        let mut lock = crate::settings::LockScreen {
            look: Some("minimal".into()),
            ..Default::default()
        };
        let minimal = applied(&lock, false);
        assert_eq!(minimal.parts, look_parts("minimal", false).unwrap());
        assert_eq!(minimal.background, Background::Blurred);
        // A background set wins over the look's.
        lock.background = Some("sharp".into());
        assert_eq!(applied(&lock, false).background, Background::Sharp);
        // A background that does not parse is the look's.
        lock.background = Some("lock.png".into());
        assert_eq!(applied(&lock, false).background, Background::Blurred);
        // A list of parts wins over the look's.
        lock.background = None;
        let list = vec![part_at(Part::Clock, Anchor::TopRight, 0.0), person(false)];
        lock.parts = Some(list.clone());
        assert_eq!(applied(&lock, false).parts, list);
        // A list that fails the check is classic's.
        lock.parts = Some(vec![person(false), person(true)]);
        assert_eq!(applied(&lock, false).parts, classic(false));
        // The count hides with notifications = "none".
        lock.notifications = Some("none".into());
        assert!(!applied(&lock, false).notifications);
        lock.notifications = Some("count".into());
        assert!(applied(&lock, false).notifications);
    }

    #[test]
    fn applied_uses_a_persons_own_look_and_falls_back_to_classic() {
        let own = crate::settings::OwnLook {
            parts: vec![part_at(Part::Clock, Anchor::Top, 0.2), person(false)],
            background: None,
            dim: None,
        };
        let lock = crate::settings::LockScreen {
            look: Some("home".into()),
            looks: Some([("home".to_string(), own.clone())].into_iter().collect()),
            ..Default::default()
        };
        let home = applied(&lock, false);
        assert_eq!(home.parts, own.parts);
        assert_eq!(home.background, Background::Blurred);
        // An unknown look is classic, with its backdrop.
        let unknown = crate::settings::LockScreen {
            look: Some("gallery".into()),
            ..Default::default()
        };
        assert_eq!(applied(&unknown, true).parts, classic(true));
        assert_eq!(applied(&unknown, true).background, Background::Backdrop);
    }

    #[test]
    fn an_own_look_brings_its_background() {
        let own = crate::settings::OwnLook {
            parts: classic(false),
            background: Some("#336699".into()),
            dim: None,
        };
        let mut lock = crate::settings::LockScreen {
            look: Some("home".into()),
            looks: Some([("home".to_string(), own)].into_iter().collect()),
            ..Default::default()
        };
        let colour = crate::tokens::Colour::parse("#336699").unwrap();
        assert_eq!(applied(&lock, false).background, Background::Colour(colour));
        // The key over the look still wins, as on the command line.
        lock.background = Some("sharp".into());
        assert_eq!(applied(&lock, false).background, Background::Sharp);
        // A look written back keeps its background, and blurred is not written.
        lock.background = None;
        assert_eq!(as_own_look(&lock).background.as_deref(), Some("#336699"));
        lock.looks = None;
        lock.look = Some("magazine".into());
        assert_eq!(as_own_look(&lock).background, None);
        let mut looks = BTreeMap::new();
        looks.insert(
            "bad".to_string(),
            crate::settings::OwnLook {
                parts: classic(false),
                background: Some("purple".into()),
                dim: None,
            },
        );
        assert!(check_looks(&looks).unwrap_err().contains("\"purple\""));
    }

    #[test]
    fn new_looks_get_free_names_and_titles() {
        assert_eq!(new_look_name("classic", |_| false), "classic-2");
        assert_eq!(new_look_name("classic", |n| n == "classic-2"), "classic-3");
        assert_eq!(
            new_look_name("classic-2", |n| n == "classic-2"),
            "classic-3"
        );
        let long = "a".repeat(MOST_LOOK);
        let name = new_look_name(&long, |_| false);
        assert!(is_look_name(&name) && name.ends_with("-2"));
        assert_eq!(look_title("classic-2"), "Classic 2");
        assert_eq!(look_title("minimal"), "Minimal");
        assert!(is_built_in("magazine") && !is_built_in("home"));
    }

    #[test]
    fn dim_is_the_look_s_default_for_its_background_then_the_look_s_then_the_persons() {
        // Classic is the navy scene, blurred and colour grounds take DIM, a
        // sharp one or a picture of the person's DIM_SHARP.
        let plain = crate::settings::LockScreen::default();
        assert_eq!(applied(&plain, false).dim, DIM);
        let minimal = crate::settings::LockScreen {
            look: Some("minimal".into()),
            ..Default::default()
        };
        assert_eq!(applied(&minimal, false).dim, DIM);
        let sharp = crate::settings::LockScreen {
            look: Some("minimal".into()),
            background: Some("sharp".into()),
            ..Default::default()
        };
        assert_eq!(applied(&sharp, false).dim, DIM_SHARP);
        // The person's key wins over the look's own, and the look's own over the default.
        let own = crate::settings::OwnLook {
            parts: classic(false),
            background: Some("#336699".into()),
            dim: Some(55),
        };
        let mut lock = crate::settings::LockScreen {
            look: Some("home".into()),
            looks: Some([("home".to_string(), own)].into_iter().collect()),
            ..Default::default()
        };
        assert_eq!(applied(&lock, false).dim, 55);
        lock.dim = Some(12);
        assert_eq!(applied(&lock, false).dim, 12);
        // A value past the range is held to it.
        lock.dim = Some(200);
        assert_eq!(applied(&lock, false).dim, MOST_DIM);
    }

    #[test]
    fn a_dim_the_default_does_not_give_is_saved_with_the_look() {
        let mut lock = crate::settings::LockScreen {
            look: Some("minimal".into()),
            ..Default::default()
        };
        // The default for a blurred background is not written into a look.
        assert_eq!(as_own_look(&lock).dim, None);
        lock.dim = Some(70);
        assert_eq!(as_own_look(&lock).dim, Some(70));
        // Sharp's default is DIM_SHARP: the same value there is the default.
        lock.background = Some("sharp".into());
        lock.dim = Some(DIM_SHARP);
        assert_eq!(as_own_look(&lock).dim, None);
    }

    #[test]
    fn a_look_s_dim_must_be_within_the_range() {
        let mut looks = BTreeMap::new();
        looks.insert(
            "home".to_string(),
            crate::settings::OwnLook {
                parts: classic(false),
                background: None,
                dim: Some(81),
            },
        );
        let why = check_looks(&looks).unwrap_err();
        assert!(why.contains("the dim 81"), "{why}");
        looks.get_mut("home").unwrap().dim = Some(MOST_DIM);
        assert!(check_looks(&looks).is_ok());
    }

    #[test]
    fn looks_value_adds_and_takes_out() {
        let look = crate::settings::OwnLook {
            parts: vec![person(false)],
            background: None,
            dim: None,
        };
        let one = looks_value(None, "home", Some(look.clone())).unwrap();
        let text = format!("format = 1\n[lock_screen]\nlooks = {one}\n");
        let read = crate::settings::read(&text).unwrap();
        assert_eq!(read.file.lock_screen.looks.unwrap()["home"], look);
        let own: BTreeMap<_, _> = [("home".to_string(), look)].into_iter().collect();
        assert_eq!(looks_value(Some(&own), "home", None), None);
    }

    #[test]
    fn check_look_names_the_looks_it_takes() {
        assert!(check_look("magazine", None).is_ok());
        assert_eq!(
            check_look("gallery", None).unwrap_err(),
            "\"gallery\" is not a look; choose classic, magazine or minimal"
        );
        let own: BTreeMap<String, crate::settings::OwnLook> = [(
            "home".to_string(),
            crate::settings::OwnLook {
                parts: classic(false),
                background: None,
                dim: None,
            },
        )]
        .into_iter()
        .collect();
        assert!(check_look("home", Some(&own)).is_ok());
        assert_eq!(
            check_look("gallery", Some(&own)).unwrap_err(),
            "\"gallery\" is not a look; choose classic, magazine, minimal, or one of yours (home)"
        );
    }

    /// The rectangles of the round trip: sizes 120 by 40, 300 by 200 and 420
    /// by 180, each at every tenth of the screen across and down that fits
    /// inside the margin.
    fn grid(screen: (f32, f32)) -> Vec<Rect> {
        let mut rects = Vec::new();
        for (w, h) in [(120.0, 40.0), (300.0, 200.0), (420.0, 180.0)] {
            for i in 0..10 {
                for j in 0..10 {
                    let x = screen.0 * i as f32 / 10.0;
                    let y = screen.1 * j as f32 / 10.0;
                    let fits = x >= MARGIN
                        && y >= MARGIN
                        && x + w <= screen.0 - MARGIN
                        && y + h <= screen.1 - MARGIN;
                    if fits {
                        rects.push(Rect { x, y, w, h });
                    }
                }
            }
        }
        rects
    }

    #[test]
    fn a_placed_rectangle_comes_back_from_arrange() {
        let template = part_at(Part::Clock, Anchor::Centre, 0.0);
        let mut cases = 0;
        for screen in SCREENS {
            for rect in grid(screen) {
                let placed = moved(&template, rect, screen, MARGIN);
                // Through `arrange`, as the lock screen places it.
                let item = Item {
                    at: placed.at,
                    x: placed.x as f32,
                    y: placed.y as f32,
                    w: rect.w,
                    h: rect.h,
                };
                let out = arrange(screen, &[item], &[], GAP, MARGIN);
                let got = out[0].expect("the part stands");
                assert!(
                    (got.x - rect.x).abs() <= 1.0 && (got.y - rect.y).abs() <= 1.0,
                    "{rect:?} came back as {got:?} on {screen:?} ({placed:?})"
                );
                assert_eq!((got.w, got.h), (rect.w, rect.h));
                // The anchor follows the third the middle is in.
                let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
                let across = if cx < screen.0 / 3.0 {
                    0
                } else if cx < 2.0 * screen.0 / 3.0 {
                    1
                } else {
                    2
                };
                let down = if cy < screen.1 / 3.0 {
                    0
                } else if cy < 2.0 * screen.1 / 3.0 {
                    1
                } else {
                    2
                };
                let want = [
                    [Anchor::TopLeft, Anchor::Top, Anchor::TopRight],
                    [Anchor::Left, Anchor::Centre, Anchor::Right],
                    [Anchor::BottomLeft, Anchor::Bottom, Anchor::BottomRight],
                ][down][across];
                assert_eq!(placed.at, want, "{rect:?} on {screen:?}");
                cases += 1;
            }
        }
        assert!(cases > 100, "the grid has {cases} rectangles");
    }

    #[test]
    fn the_corners_of_the_grid_name_their_anchors() {
        let screen = (1920.0, 1080.0);
        let at = |x: f32, y: f32| {
            placed_at(
                Rect {
                    x,
                    y,
                    w: 120.0,
                    h: 40.0,
                },
                screen,
                MARGIN,
            )
            .0
        };
        assert_eq!(at(MARGIN, MARGIN), Anchor::TopLeft);
        assert_eq!(at(960.0 - 60.0, 540.0 - 20.0), Anchor::Centre);
        assert_eq!(
            at(1920.0 - MARGIN - 120.0, 1080.0 - MARGIN - 40.0),
            Anchor::BottomRight
        );
    }

    #[test]
    fn moving_keeps_the_options_text_and_path() {
        let clock = Placed {
            face: Some(Face::Analog),
            hours: Some(12),
            size: Some(Size::Small),
            weight: Some(300),
            ..part_at(Part::Clock, Anchor::Top, 0.0)
        };
        let rect = Rect {
            x: 100.0,
            y: 200.0,
            w: 120.0,
            h: 40.0,
        };
        let moved_clock = moved(&clock, rect, (1280.0, 800.0), MARGIN);
        assert_eq!(
            Placed {
                at: clock.at,
                x: clock.x,
                y: clock.y,
                ..moved_clock.clone()
            },
            clock
        );
        assert_ne!(moved_clock.at, clock.at);
        let message = Placed {
            text: Some("Hello".into()),
            ..part_at(Part::Message, Anchor::Top, 0.0)
        };
        assert_eq!(
            moved(&message, rect, (1280.0, 800.0), MARGIN)
                .text
                .as_deref(),
            Some("Hello")
        );
    }

    #[test]
    fn a_rectangle_snaps_onto_the_screens_middle() {
        let screen = (800.0, 1280.0);
        // Its middle, 345 + 60, is 5 px right of the screen's middle, 400.
        let rect = Rect {
            x: 345.0,
            y: 100.0,
            w: 120.0,
            h: 40.0,
        };
        let (out, guides) = snap(rect, &[], screen, MARGIN, SNAP);
        assert_eq!(out.x, 340.0);
        assert_eq!(out.y, 100.0);
        assert_eq!(
            guides,
            vec![Guide {
                vertical: true,
                at: 400.0
            }]
        );
    }

    #[test]
    fn a_left_edge_snaps_onto_another_parts_right_edge() {
        let screen = (800.0, 1280.0);
        let other = Rect {
            x: 100.0,
            y: 600.0,
            w: 200.0,
            h: 50.0,
        };
        // Its left edge is 5 px right of the other's right edge, 300.
        let rect = Rect {
            x: 305.0,
            y: 300.0,
            w: 120.0,
            h: 40.0,
        };
        let (out, guides) = snap(rect, &[other], screen, MARGIN, SNAP);
        assert_eq!(out.x, 300.0);
        assert_eq!(
            guides,
            vec![Guide {
                vertical: true,
                at: 300.0
            }]
        );
    }

    #[test]
    fn a_rectangle_20_px_from_every_line_stays_put() {
        let screen = (800.0, 1280.0);
        let other = Rect {
            x: 500.0,
            y: 700.0,
            w: 100.0,
            h: 100.0,
        };
        // Its left edge is 20 px from the margin and nothing is nearer.
        let rect = Rect {
            x: 44.0,
            y: 200.0,
            w: 120.0,
            h: 40.0,
        };
        assert_eq!(snap(rect, &[other], screen, MARGIN, SNAP), (rect, vec![]));
    }

    #[test]
    fn a_rectangle_near_two_lines_picks_the_nearer() {
        let screen = (800.0, 1280.0);
        // Its left edge is 2 px from the margin, 24, and 4 px from another
        // part's left edge, 30; the margin wins.
        let other = Rect {
            x: 30.0,
            y: 600.0,
            w: 50.0,
            h: 50.0,
        };
        let rect = Rect {
            x: 26.0,
            y: 300.0,
            w: 120.0,
            h: 40.0,
        };
        let (out, guides) = snap(rect, &[other], screen, MARGIN, SNAP);
        assert_eq!(out.x, 24.0);
        assert_eq!(
            guides,
            vec![Guide {
                vertical: true,
                at: 24.0
            }]
        );
    }

    #[test]
    fn a_rectangle_takes_at_most_one_guide_per_direction_and_stays_inside() {
        let screen = (800.0, 1280.0);
        let others = [
            Rect {
                x: 390.0,
                y: 630.0,
                w: 20.0,
                h: 20.0,
            },
            Rect {
                x: 10.0,
                y: 10.0,
                w: 780.0,
                h: 60.0,
            },
        ];
        let mut snapped = 0;
        for x in (0..800).step_by(3) {
            for y in (0..1280).step_by(7) {
                let rect = Rect {
                    x: x as f32,
                    y: y as f32,
                    w: 120.0,
                    h: 40.0,
                };
                let (out, guides) = snap(rect, &others, screen, MARGIN, SNAP);
                assert!(guides.iter().filter(|g| g.vertical).count() <= 1);
                assert!(guides.iter().filter(|g| !g.vertical).count() <= 1);
                assert!(out.x >= MARGIN && out.x + out.w <= screen.0 - MARGIN);
                assert!(out.y >= MARGIN && out.y + out.h <= screen.1 - MARGIN);
                if !guides.is_empty() {
                    snapped += 1;
                }
            }
        }
        assert!(snapped > 0);
        // A rectangle that is too big for the screen is clamped to the margin.
        let huge = Rect {
            x: 0.0,
            y: 0.0,
            w: 2000.0,
            h: 2000.0,
        };
        let (out, _) = snap(huge, &[], screen, MARGIN, SNAP);
        assert_eq!((out.x, out.y), (MARGIN, MARGIN));
    }

    #[test]
    fn the_drawer_offers_what_a_lock_screen_lacks() {
        // A clock is offered while fewer than three stand, as a message is.
        assert_eq!(
            missing(&classic(false), false),
            vec![Part::Clock, Part::Message, Part::Picture]
        );
        assert_eq!(
            missing(&classic(false), true),
            vec![Part::Clock, Part::Message, Part::Picture, Part::Battery]
        );
        let all = missing(&[], true);
        assert_eq!(
            all,
            vec![
                Part::Clock,
                Part::Date,
                Part::Count,
                Part::Player,
                Part::Message,
                Part::Picture,
                Part::Battery,
            ]
        );
        assert!(!all.contains(&Part::Person));
    }

    #[test]
    fn messages_and_pictures_stop_being_offered_at_three() {
        let message = Placed {
            text: Some("hi".into()),
            ..part_at(Part::Message, Anchor::Top, 0.0)
        };
        let picture = Placed {
            path: Some("/a.png".into()),
            ..part_at(Part::Picture, Anchor::Top, 0.0)
        };
        let two = vec![
            message.clone(),
            message.clone(),
            picture.clone(),
            picture.clone(),
        ];
        assert!(missing(&two, false).contains(&Part::Message));
        assert!(missing(&two, false).contains(&Part::Picture));
        let three = [two.clone(), vec![message.clone()]].concat();
        assert!(!missing(&three, false).contains(&Part::Message));
        let three_pictures = [two.clone(), vec![picture]].concat();
        assert!(!missing(&three_pictures, false).contains(&Part::Picture));
    }

    #[test]
    fn adding_a_message_places_it_where_the_rectangle_is() {
        let screen = (1280.0, 800.0);
        let rect = Rect {
            x: 900.0,
            y: 600.0,
            w: 300.0,
            h: 40.0,
        };
        let base = classic(false);
        let list = with_added(&base, Part::Message, rect, screen, MARGIN, "Call me", true);
        assert_eq!(list.len(), base.len() + 1);
        let added = list.last().expect("the message is last");
        let (at, x, y) = placed_at(rect, screen, MARGIN);
        assert_eq!(added.part, Part::Message);
        assert_eq!((added.at, added.x, added.y), (at, x, y));
        assert_eq!(added.text.as_deref(), Some("Call me"));
        assert!(check(&list).is_ok());
    }

    #[test]
    fn adding_refuses_what_missing_does_not_offer_and_empty_words() {
        let screen = (1280.0, 800.0);
        let rect = Rect {
            x: 100.0,
            y: 100.0,
            w: 120.0,
            h: 40.0,
        };
        let base = classic(false);
        // The person, a part already shown, and a picture (it needs a file).
        assert_eq!(
            with_added(&base, Part::Person, rect, screen, MARGIN, "x", true),
            base
        );
        // A second clock is offered while fewer than three stand (M5.10c7d).
        let two = with_added(&base, Part::Clock, rect, screen, MARGIN, "x", true);
        assert_eq!(two.len(), base.len() + 1);
        assert!(check(&two).is_ok());
        assert_eq!(
            with_added(&base, Part::Picture, rect, screen, MARGIN, "x", true),
            base
        );
        // Words that fail the check leave the list as it was.
        assert_eq!(
            with_added(&base, Part::Message, rect, screen, MARGIN, "", true),
            base
        );
        // A battery is added only on a machine that has one.
        assert_eq!(
            with_added(&base, Part::Battery, rect, screen, MARGIN, "x", false),
            base
        );
        assert_eq!(
            with_added(&base, Part::Battery, rect, screen, MARGIN, "x", true).len(),
            base.len() + 1
        );
    }

    #[test]
    fn the_placeholder_is_a_message_the_check_takes() {
        let words = message_placeholder();
        assert_eq!(words, "Your message");
        let rect = Rect {
            x: 100.0,
            y: 100.0,
            w: 120.0,
            h: 40.0,
        };
        let list = with_added(
            &classic(true),
            Part::Message,
            rect,
            (360.0, 740.0),
            MARGIN,
            &words,
            true,
        );
        assert_eq!(list.len(), classic(true).len() + 1);
    }

    #[test]
    fn every_list_the_editor_returns_passes_the_check() {
        let mut lists: Vec<Vec<Placed>> = vec![classic(false), classic(true)];
        for name in LOOKS {
            for compact in [false, true] {
                lists.push(look_parts(name, compact).expect("a built-in look"));
            }
        }
        let rect = Rect {
            x: 100.0,
            y: 300.0,
            w: 200.0,
            h: 60.0,
        };
        let words = message_placeholder();
        for base in &lists {
            assert!(check(base).is_ok());
            for part in missing(base, true) {
                let list = with_added(base, part, rect, (1280.0, 800.0), MARGIN, &words, true);
                assert!(check(&list).is_ok(), "adding {part:?} to {base:?}");
            }
            for index in 0..base.len() {
                let list = without(base, index);
                if removable(base, index) {
                    assert_eq!(list.len(), base.len() - 1);
                    assert!(check(&list).is_ok(), "removing {index} from {base:?}");
                } else {
                    assert_eq!(&list, base);
                }
            }
        }
    }

    #[test]
    fn nobody_is_locked_out_of_the_lock_screen() {
        let list = classic(false);
        let person_at = list
            .iter()
            .position(|p| p.part == Part::Person)
            .expect("the person");
        assert!(!removable(&list, person_at));
        assert_eq!(without(&list, person_at), list);
        assert!(!removable(&list, 99));
        assert_eq!(without(&list, 99), list);
        assert!(removable(&list, 0));
        assert_eq!(without(&list, 0).len(), list.len() - 1);
    }

    #[test]
    fn two_steps_right_add_a_hundredth_each() {
        let placed = part_at(Part::Clock, Anchor::Top, 0.25);
        let nudged_right = nudged(&placed, 2, 0);
        assert!((nudged_right.x - 0.02).abs() < 1e-9);
        assert_eq!(nudged_right.y, 0.25);
        assert_eq!(nudged_right.at, Anchor::Top);
        let down = nudged(&placed, 0, -3);
        assert!((down.y - 0.22).abs() < 1e-9);
        assert_eq!(down.x, 0.0);
    }

    #[test]
    fn a_nudge_stays_within_the_screen() {
        let placed = Placed {
            x: 0.995,
            ..part_at(Part::Clock, Anchor::Top, -1.0)
        };
        assert_eq!(nudged(&placed, 1, 0).x, 1.0);
        assert_eq!(nudged(&placed, 1, -1).y, -1.0);
        assert_eq!(nudged(&placed, 0, 0), placed);
        assert_eq!(NUDGE, 0.01);
    }

    #[test]
    fn the_desktop_checks_its_own_widgets_and_keeps_the_lock_messages() {
        assert!(check_desktop(&desktop_default()).is_ok());
        assert!(check_desktop(&[]).is_ok());
        let person = [part_at(Part::Person, Anchor::Centre, 0.0)];
        assert_eq!(
            check_desktop(&person).unwrap_err(),
            "the person does not stand on the desktop; the desktop takes clock or date"
        );
        let four_clocks = vec![part_at(Part::Clock, Anchor::Top, 0.0); MOST_EACH + 1];
        assert_eq!(
            check_desktop(&four_clocks).unwrap_err(),
            "at most 3 clocks; take one out"
        );
        assert_eq!(
            check(&[part_at(Part::Clock, Anchor::Top, 0.0)]).unwrap_err(),
            "the lock screen needs the person and their password field; add { widget = \"person\", at = \"centre\" }"
        );
        assert_eq!(DESKTOP_WIDGETS, "appearance.widgets");
    }

    #[test]
    fn the_desktop_editor_adds_clocks_and_dates_only_and_keeps_a_default_unwritten() {
        use crate::widgets::Surface;
        let rect = Rect {
            x: 100.0,
            y: 100.0,
            w: 200.0,
            h: 40.0,
        };
        let screen = (1280.0, 764.0);
        let default = desktop_default();
        assert_eq!(missing_on(&default, false, Surface::Desktop), [Part::Clock]);
        assert_eq!(
            missing_on(&[], false, Surface::Desktop),
            [Part::Clock, Part::Date]
        );
        let clock = with_added_on(
            &[],
            Part::Clock,
            rect,
            screen,
            MARGIN,
            "",
            false,
            Surface::Desktop,
        );
        assert_eq!(clock.len(), 1);
        assert_eq!(clock[0].part, Part::Clock);
        assert!(check_desktop(&clock).is_ok());
        assert!(
            with_added_on(
                &[],
                Part::Message,
                rect,
                screen,
                MARGIN,
                "hi",
                false,
                Surface::Desktop
            )
            .is_empty()
        );

        assert_eq!(
            desktop_line(&default, None),
            Ok(Some(widgets_value(&default).unwrap()))
        );
        assert_eq!(desktop_line(&[], None), Ok(None));
        let machine = crate::settings::set(
            "format = 1\n",
            DESKTOP_WIDGETS,
            &widgets_value(&default).unwrap(),
        )
        .unwrap();
        assert_eq!(desktop_line(&default, Some(&machine)), Ok(None));
        assert_eq!(desktop_line(&[], Some(&machine)), Ok(Some("[]".into())));
        let person = [part_at(Part::Person, Anchor::Centre, 0.0)];
        assert!(desktop_line(&person, None).is_err());
    }

    /// A widget's colour (M5.33d): a count takes none, and a colour that is
    /// no accent, wallpaper or hex is refused with the accents named.
    #[test]
    fn a_colour_is_refused_on_a_part_that_takes_none_and_on_a_non_colour() {
        let mut count = part_at(Part::Count, Anchor::TopRight, 0.0);
        count.colour = Some("accent".into());
        let message = check(&[person(false), count]).unwrap_err();
        assert_eq!(message, "the count takes no colour, so take colour out");
        let mut clock = part_at(Part::Clock, Anchor::Top, 0.0);
        clock.colour = Some("navy".into());
        let message = check(&[person(false), clock]).unwrap_err();
        assert!(
            message.starts_with("the clock's colour \"navy\" is not a colour; choose accent, blue"),
            "{message}"
        );
        let mut clock = part_at(Part::Clock, Anchor::Top, 0.0);
        clock.colour = Some("#e8b04a".into());
        assert!(check(&[person(false), clock]).is_ok());
    }

    /// `coloured` writes a colour only where the part takes one and reads
    /// with it: the person and an unreadable clock stay as they were.
    #[test]
    fn coloured_writes_the_colour_only_where_the_part_reads_with_it() {
        let parts = vec![
            part_at(Part::Clock, Anchor::Top, 0.0),
            part_at(Part::Date, Anchor::TopLeft, 0.0),
            part_at(Part::Person, Anchor::Centre, 0.2),
        ];
        let out = coloured(&parts, Some("accent"), |p| p.part != Part::Date);
        assert_eq!(out[0].colour.as_deref(), Some("accent"));
        assert_eq!(out[1].colour, None);
        assert_eq!(out[2].colour, None);
        let automatic = coloured(&out, None, |_| true);
        assert_eq!(automatic[0].colour, None);
    }
}
