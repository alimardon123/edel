//! The lock and login screens as parts in places (M5.10c5, ADR-008's same
//! names decision): the data model, its check, the default arrangements and
//! the one pure layout function. No drawing here: shell-ui draws the parts,
//! and the settings file holds the person's list as `lock_screen.parts`
//! ([`crate::settings`]). Each part stands at an anchor, the edge or the
//! middle of the screen, with an offset from it, so one arrangement fits a
//! phone, a laptop and a 4K screen.

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

/// One part in its place: its anchor and an offset from it, each a share of the
/// screen's width (`x`) and height (`y`), from -1 to 1; a message's words and a
/// picture's path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    /// Which part this is
    pub part: Part,
    /// Where it stands
    pub at: Anchor,
    /// An offset across the screen, a share of its width, from -1 to 1
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x: f32,
    /// An offset down the screen, a share of its height, from -1 to 1
    #[serde(default, skip_serializing_if = "is_zero")]
    pub y: f32,
    /// A message's words, 1 to 120 characters on one line
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A picture's file, an absolute path ending `.png`, `.jpg` or `.jpeg`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

fn is_zero(value: &f32) -> bool {
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
    }
    Ok(())
}

/// A part at `at` with an offset down the screen of `y` (and none across).
fn part_at(part: Part, at: Anchor, y: f32) -> Placed {
    Placed {
        part,
        at,
        x: 0.0,
        y,
        text: None,
        path: None,
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
            part_at(Part::Date, Anchor::Top, 0.14),
            part_at(Part::Clock, Anchor::Top, 0.14),
            part_at(Part::Count, Anchor::Top, 0.14),
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
                    x: p.x,
                    y: p.y,
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
}
