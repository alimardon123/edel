//! The lock and login screens as parts in places (M5.10c5, ADR-008's same
//! names decision): the data model, its check, the default arrangements and
//! the one pure layout function. No drawing here: shell-ui draws the parts,
//! and the settings file holds the person's list as `lock_screen.parts`
//! ([`crate::settings`]). Each part stands at an anchor, the edge or the
//! middle of the screen, with an offset from it, so one arrangement fits a
//! phone, a laptop and a 4K screen.

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
    /// The name the file writes the part by, the same as serde's.
    pub fn name(self) -> &'static str {
        match self {
            Part::Clock => "clock",
            Part::Date => "date",
            Part::Count => "count",
            Part::Player => "player",
            Part::Person => "person",
            Part::Message => "message",
            Part::Picture => "picture",
            Part::Battery => "battery",
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

/// One part in its place: its anchor and an offset from it, each a share of the
/// screen's width (`x`) and height (`y`), from -1 to 1; a message's words, a
/// picture's path and the options of a clock or a date.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    /// Which part this is
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
    /// `digital`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<Face>,
    /// A clock's hours, 12 or 24; absent is 24
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
    /// year whatever order is written. Absent is weekday, day and month.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<Vec<DatePart>>,
    /// The date's style: `long`, `short` or `numeric`; absent is `long`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<DateStyle>,
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
                "the lock screen needs the person and their password field; add { part = \"person\", at = \"centre\" }",
            )
            .into());
        }
        1 => {}
        _ => return Err(tr("the person stands once; take out the second").into()),
    }
    for part in [
        Part::Clock,
        Part::Date,
        Part::Count,
        Part::Player,
        Part::Battery,
    ] {
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
            _ if p.text.is_some() => {
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
            _ if p.path.is_some() => {
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

/// The clock's and the date's options: each refused on the other part, and
/// the clock's hours and weight and the date's `show` checked. The message
/// names the part and says what to do.
fn check_options(p: &Placed) -> Result<(), String> {
    let part = p.part.name();
    if p.part != Part::Clock {
        if p.face.is_some() {
            return Err(trf(
                "the {part} has a face; only the clock has one, so take face out",
                &[("part", part)],
            ));
        }
        if p.hours.is_some() {
            return Err(trf(
                "the {part} has hours; only the clock has them, so take hours out",
                &[("part", part)],
            ));
        }
        if p.size.is_some() {
            return Err(trf(
                "the {part} has a size; only the clock has one, so take size out",
                &[("part", part)],
            ));
        }
        if p.weight.is_some() {
            return Err(trf(
                "the {part} has a weight; only the clock has one, so take weight out",
                &[("part", part)],
            ));
        }
    }
    if p.part != Part::Date {
        if p.show.is_some() {
            return Err(trf(
                "the {part} has show; only the date has it, so take show out",
                &[("part", part)],
            ));
        }
        if p.style.is_some() {
            return Err(trf(
                "the {part} has a style; only the date has one, so take style out",
                &[("part", part)],
            ));
        }
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
        check(&look.parts).map_err(|why| {
            trf(
                "the look {name} does not check: {why}",
                &[("name", &shown), ("why", &why)],
            )
        })?;
    }
    Ok(())
}

/// What the lock screen shows (M5.10c5c), from [`applied`].
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub parts: Vec<Placed>,
    pub background: Background,
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
    let parts = match (&lock.parts, list) {
        (Some(parts), _) => applying(Some(parts), compact),
        (None, Some(list)) => applying(Some(&list), compact),
        (None, None) => classic(compact),
    };
    Applied {
        parts,
        background: lock
            .background
            .as_deref()
            .and_then(parse_background)
            .unwrap_or_else(|| look_background(look)),
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

#[cfg(test)]
mod tests {
    use super::*;

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
        for part in [
            Part::Clock,
            Part::Date,
            Part::Count,
            Part::Player,
            Part::Person,
            Part::Message,
            Part::Picture,
            Part::Battery,
        ] {
            let value = toml::Value::try_from(part).unwrap();
            assert_eq!(value, toml::Value::String(part.name().into()));
        }
        for anchor in ANCHORS {
            let value = toml::Value::try_from(anchor).unwrap();
            assert_eq!(value, toml::Value::String(anchor.name().into()));
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
            (vec![], "add { part = \"person\", at = \"centre\" }"),
            (
                vec![person.clone(), person.clone()],
                "the person stands once; take out the second",
            ),
            (
                vec![
                    person.clone(),
                    part_at(Part::Clock, Anchor::Top, 0.0),
                    part_at(Part::Clock, Anchor::Top, 0.1),
                ],
                "the clock stands once",
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

    /// A settings file's lock screen, as the tables `[[lock_screen.parts]]`.
    #[derive(Deserialize, Serialize)]
    struct Doc {
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
        let text = "[[parts]]\npart = \"person\"\nat = \"centre\"\n\n\
            [[parts]]\npart = \"clock\"\nat = \"top\"\ny = 0.14\nface = \"analog\"\n\
            hours = 12\nsize = \"medium\"\nweight = 300\n\n\
            [[parts]]\npart = \"date\"\nat = \"top\"\nshow = [\"day\", \"month\"]\n\
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
        for option in ["face", "hours", "size", "weight", "show", "style"] {
            assert!(!text.contains(option), "{option} written: {text}");
        }
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
}
