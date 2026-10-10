//! The login screen's row of people (M5.10c, `docs/mockups/shell/greeter.jpg`):
//! the machine's people side by side, the chosen one large in the middle
//! with its name under it in white, the others smaller at its sides with
//! quieter names, and "Other" at the end, a disc with a plus, for typing a
//! name. Plain data, layout and drawing, tested without a display; the
//! greeter (`greeter.rs`) chooses with the arrows and clicks. Only the chosen one and at most
//! `MOST_EACH_SIDE` neighbours on each side are drawn, so a long list
//! scrolls in as the choice moves. The disc and its colours are `lock`'s,
//! one copy for the lock screen's person and for each person here.

use edel::i18n::tr;
use edel::tokens::Tokens;
use tiny_skia::Pixmap;

use crate::lock;
use crate::paint::{Face, Text, fill, mix};

/// A person of the row: their login name, and the name shown (their full
/// name, else the login).
#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    pub login: String,
    pub shown: String,
}

/// The row: the people in order, and which one is chosen. `chosen` may
/// equal `people.len()`, which is "Other".
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub people: Vec<Person>,
    pub chosen: usize,
}

/// The chosen disc's diameter, logical pixels (the mockup's large disc).
pub const CHOSEN: f32 = 88.0;
/// The others' diameter.
pub const SIDE: f32 = 64.0;
/// Centre to centre between neighbours.
pub const STEP: f32 = 118.0;
/// Between a small disc and its name.
pub const OTHER_NAME_GAP: f32 = 10.0;
/// The chosen disc's name sits this far under its disc, so its top lands
/// on the lock screen's name line: half of `CHOSEN` plus this equals the
/// name's top in `lock::layout` (`AVATAR` plus 12 px), as the mockup's
/// name sits close under the disc.
pub const CHOSEN_NAME_GAP: f32 = 12.0;
/// At most this many neighbours are drawn on each side of the chosen one.
pub const MOST_EACH_SIDE: usize = 2;

/// The margin no disc may cross, a screen's edge to the disc.
const EDGE: f32 = 16.0;
/// How far a point may lie off a disc and still hit it.
const SLOP: f32 = 6.0;
/// How much higher the others sit than the chosen one, as the mockup has
/// them (M5.10c).
const RAISE: f32 = 8.0;
/// The plus on "Other": each arm's length and thickness.
const PLUS_LONG: f32 = 18.0;
const PLUS_THICK: f32 = 2.0;
/// The names' sizes; the others' names are fitted in a step less 12 px.
const CHOSEN_NAME: f32 = 18.0;
const SMALL_NAME: f32 = 13.0;
const NAME_ROOM: f32 = STEP - 12.0;

/// One entry of the row drawn at a place: its index (`people.len()` is
/// Other), the centre and the diameter, logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub index: usize,
    pub centre: (f32, f32),
    pub diameter: f32,
}

/// The people of a login list, in the order given, each shown by their
/// full name, else their login.
pub fn from_users(list: &[edel::users::Person]) -> Vec<Person> {
    list.iter()
        .map(|p| Person {
            login: p.name.clone(),
            shown: p.shown().to_string(),
        })
        .collect()
}

/// Where each entry of `row` is drawn on a screen `w` wide, its chosen
/// disc's centre at `centre_y`, in index order. The small ones sit higher
/// so their tops line up with the chosen one's. An entry whose disc would
/// cross the screen's edge is left out.
pub fn spots(row: &Row, w: f32, centre_y: f32) -> Vec<Spot> {
    let last = row.people.len();
    let chosen = row.chosen.min(last);
    let small_y = centre_y - RAISE;
    let mut all = vec![(chosen, 0.0_f32, CHOSEN, centre_y)];
    for k in 1..=MOST_EACH_SIDE {
        let dx = k as f32 * STEP;
        if chosen >= k {
            all.push((chosen - k, -dx, SIDE, small_y));
        }
        if chosen + k <= last {
            all.push((chosen + k, dx, SIDE, small_y));
        }
    }
    let mut out: Vec<Spot> = all
        .into_iter()
        .filter_map(|(index, dx, diameter, y)| {
            let x = w / 2.0 + dx;
            let fits = x - diameter / 2.0 >= EDGE && x + diameter / 2.0 <= w - EDGE;
            fits.then_some(Spot {
                index,
                centre: (x, y),
                diameter,
            })
        })
        .collect();
    out.sort_by_key(|s| s.index);
    out
}

/// The index of the entry whose disc holds the point, or lies within
/// `SLOP` of it.
pub fn hit(spots: &[Spot], x: f32, y: f32) -> Option<usize> {
    spots
        .iter()
        .find(|s| {
            let (dx, dy) = (x - s.centre.0, y - s.centre.1);
            dx * dx + dy * dy <= (s.diameter / 2.0 + SLOP).powi(2)
        })
        .map(|s| s.index)
}

/// The chosen index after a step of `by` (left is negative), clamped to
/// the people and Other: no wrapping round the ends.
pub fn step(row: &Row, by: i32) -> usize {
    let most = row.people.len() as i64;
    (row.chosen.min(row.people.len()) as i64 + by as i64).clamp(0, most) as usize
}

/// The log line CI reads to click the people: each entry's index, its
/// box (`X+Y+WxH`, logical pixels, rounded) and its login, Other written
/// as `other`, joined with ", " in index order.
pub fn places(spots: &[Spot], row: &Row) -> String {
    let boxed = |s: &Spot| {
        let (x, y) = (s.centre.0 - s.diameter / 2.0, s.centre.1 - s.diameter / 2.0);
        format!("{x:.0}+{y:.0}+{d:.0}x{d:.0}", d = s.diameter)
    };
    spots
        .iter()
        .map(|s| match row.people.get(s.index) {
            Some(person) => format!("person{} {} {}", s.index, boxed(s), person.login),
            None => format!("other {}", boxed(s)),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Draws the row `spots` lay out, on a pixmap drawn at scale `s`: a disc
/// per person (the others quieter), Other's glass disc with its plus, and
/// each name under its disc.
pub fn paint(
    pixmap: &mut Pixmap,
    row: &Row,
    spots: &[Spot],
    tokens: &Tokens,
    text: &mut Text,
    s: f32,
) {
    let chosen = row.chosen.min(row.people.len());
    for spot in spots {
        let (cx, cy) = spot.centre;
        let r = spot.diameter / 2.0;
        let is_chosen = spot.index == chosen;
        let person = row.people.get(spot.index);
        match person {
            Some(person) => {
                let initial: String = person
                    .shown
                    .chars()
                    .take(1)
                    .flat_map(char::to_uppercase)
                    .collect();
                lock::disc(
                    pixmap,
                    &initial,
                    (cx, cy, r),
                    r * 0.8,
                    !is_chosen,
                    tokens,
                    text,
                    s,
                );
            }
            None => {
                lock::glass(
                    pixmap,
                    ((cx - r) * s, (cy - r) * s, 2.0 * r * s, 2.0 * r * s),
                    r * s,
                    s,
                    tokens,
                );
                let ink = tokens.backdrop_text;
                let (long, thick) = (PLUS_LONG * s, PLUS_THICK * s);
                fill(
                    pixmap,
                    (cx * s) - long / 2.0,
                    (cy * s) - thick / 2.0,
                    long,
                    thick,
                    0.0,
                    ink,
                );
                fill(
                    pixmap,
                    (cx * s) - thick / 2.0,
                    (cy * s) - long / 2.0,
                    thick,
                    long,
                    0.0,
                    ink,
                );
            }
        }
        let name = match person {
            Some(person) => person.shown.as_str(),
            None => tr("Other"),
        };
        let (size, face, ink, top) = if is_chosen {
            (
                CHOSEN_NAME,
                Face::SEMIBOLD,
                tokens.backdrop_text,
                cy + r + CHOSEN_NAME_GAP,
            )
        } else {
            (
                SMALL_NAME,
                Face::MEDIUM,
                mix(tokens.backdrop_text, tokens.lock_backdrop_deep, 0.3),
                cy + r + OTHER_NAME_GAP,
            )
        };
        let mut line = text.fit_in(name, size * s, NAME_ROOM * s, face);
        let x = cx * s - line.width / 2.0;
        text.draw(pixmap, &mut line, x, top * s, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(names: &[&str], chosen: usize) -> Row {
        Row {
            people: names
                .iter()
                .map(|n| Person {
                    login: n.to_string(),
                    shown: n.to_string(),
                })
                .collect(),
            chosen,
        }
    }

    fn indexes(spots: &[Spot]) -> Vec<usize> {
        spots.iter().map(|s| s.index).collect()
    }

    #[test]
    fn one_person_is_chosen_in_the_middle_and_other_sits_at_her_side() {
        let s = spots(&row(&["ci"], 0), 1280.0, 460.0);
        assert_eq!(
            s,
            vec![
                Spot {
                    index: 0,
                    centre: (640.0, 460.0),
                    diameter: CHOSEN,
                },
                Spot {
                    index: 1,
                    // 640 + STEP (118), and 460 - RAISE (8).
                    centre: (758.0, 452.0),
                    diameter: SIDE,
                },
            ]
        );
    }

    #[test]
    fn three_people_with_the_middle_one_chosen_show_all_of_them_and_other() {
        let s = spots(&row(&["a", "b", "c"], 1), 1280.0, 460.0);
        let xs: Vec<(usize, f32)> = s.iter().map(|s| (s.index, s.centre.0)).collect();
        assert_eq!(xs, vec![(0, 522.0), (1, 640.0), (2, 758.0), (3, 876.0)]);
        assert_eq!(s[1].diameter, CHOSEN);
        assert_eq!(s[1].centre.1, 460.0);
        assert!(
            s.iter()
                .filter(|s| s.index != 1)
                .all(|s| s.diameter == SIDE)
        );
    }

    #[test]
    fn at_most_two_neighbours_show_each_side() {
        let five = row(&["a", "b", "c", "d", "e"], 0);
        assert_eq!(indexes(&spots(&five, 1280.0, 460.0)), vec![0, 1, 2]);
        let far = row(&["a", "b", "c", "d", "e"], 3);
        assert_eq!(indexes(&spots(&far, 1280.0, 460.0)), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn spots_that_would_leave_a_narrow_screen_are_dropped() {
        // 360 wide: the disc at 298 with a 64 px side reaches 330, and the
        // one at 416 would leave the screen.
        let r = row(&["a", "b", "c", "d", "e"], 0);
        assert_eq!(indexes(&spots(&r, 360.0, 400.0)), vec![0, 1]);
        let three = row(&["a", "b", "c"], 1);
        assert_eq!(indexes(&spots(&three, 360.0, 400.0)), vec![0, 1, 2]);
    }

    #[test]
    fn hit_finds_each_middle_and_nothing_far_away() {
        let s = spots(&row(&["a", "b", "c"], 1), 1280.0, 460.0);
        for spot in &s {
            assert_eq!(hit(&s, spot.centre.0, spot.centre.1), Some(spot.index));
        }
        assert_eq!(hit(&s, 10.0, 10.0), None);
        // The chosen disc's radius is 44 (CHOSEN / 2). Six px past its edge
        // (640 + 44 + 6 = 690) still hits it, seven px past does not.
        assert_eq!(hit(&s, 640.0 + 44.0 + 6.0, 460.0), Some(1));
        assert_eq!(hit(&s, 640.0 + 44.0 + 7.0, 460.0), None);
    }

    #[test]
    fn step_clamps_at_both_ends_and_takes_other_too() {
        let r = row(&["a", "b"], 0);
        assert_eq!(step(&r, -1), 0);
        assert_eq!(step(&r, 1), 1);
        assert_eq!(step(&r, 5), 2);
        let last = row(&["a", "b"], 2);
        assert_eq!(step(&last, 1), 2);
        assert_eq!(step(&last, -9), 0);
    }

    #[test]
    fn the_places_line_names_the_person_and_other() {
        let r = row(&["ci"], 0);
        let s = spots(&r, 1280.0, 460.0);
        // The chosen disc: 640 - 44, 460 - 44, 88 across. The other: 758 - 32,
        // 452 - 32, 64 across.
        assert_eq!(
            places(&s, &r),
            "person0 596+416+88x88 ci, other 726+420+64x64"
        );
    }

    #[test]
    fn the_row_of_people_draws_on_the_backdrop() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let mut pixmap = Pixmap::new(1280, 800).unwrap();
        lock::backdrop(&mut pixmap, &tokens);
        let r = row(&["ali", "mira"], 0);
        let s = spots(&r, 1280.0, 460.0);
        assert_eq!(indexes(&s), vec![0, 1, 2]);
        paint(&mut pixmap, &r, &s, &tokens, &mut text, 1.0);
        // The chosen disc, 8 px inside its left edge (640 - 44 + 8 = 604), is
        // the accent: bluer than red, as the lock screen's disc is.
        let disc = pixmap.pixel(604, 460).unwrap();
        assert!(disc.blue() > disc.red(), "{disc:?}");
        if let Ok(dir) = std::env::var("EDEL_LOCK_PNG") {
            pixmap.save_png(format!("{dir}/people.png")).unwrap();
        }
    }
}
