//! The notification centre's month (M5.9b): the grid of days a calendar
//! draws, weeks starting on Monday as the mockups have it, six weeks
//! always so the card never changes height as months pass, the days of
//! the months either side dimmed and today marked. Plain dates, from
//! `jiff` (already the clock's), tested without a display: October 2026
//! starts on a Thursday and February 2028 has 29 days.

use edel::i18n::tr;
use jiff::civil::Date;

/// A month of a year.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Month {
    pub year: i16,
    pub month: i8,
}

/// A day: its year, month and day of the month.
pub type Ymd = (i16, i8, i8);

/// One cell of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Day {
    /// The day of its month, 1 to 31
    pub day: i8,
    /// Whether it is in the month shown, else the month before or after.
    pub inside: bool,
    pub today: bool,
}

/// The weeks a grid shows.
pub const WEEKS: usize = 6;

impl Month {
    /// The month `today` is in.
    pub fn of(today: Ymd) -> Month {
        Month {
            year: today.0,
            month: today.1,
        }
    }

    /// The month `by` months from this one, negative before it; years
    /// keep to what dates hold (-9999 to 9999).
    pub fn step(self, by: i32) -> Month {
        let index = i32::from(self.year) * 12 + i32::from(self.month) - 1 + by;
        let year = index.div_euclid(12).clamp(-9999, 9999);
        Month {
            year: year as i16,
            month: (index.rem_euclid(12) + 1) as i8,
        }
    }

    fn first(self) -> Option<Date> {
        Date::new(self.year, self.month, 1).ok()
    }

    /// The days this month has, 28 to 31.
    pub fn days(self) -> i8 {
        self.first().map_or(30, |d| d.days_in_month())
    }

    /// How many days after Monday the first falls: Monday 0, Sunday 6.
    pub fn offset(self) -> usize {
        self.first()
            .map_or(0, |d| d.weekday().to_monday_zero_offset() as usize)
    }

    /// The month's name and year as a heading: `October 2026`.
    pub fn title(self) -> String {
        format!("{} {}", name(self.month), self.year)
    }
}

/// The days of `month` as six weeks of seven, Monday first, with today
/// (when it is in view) marked.
pub fn grid(month: Month, today: Option<Ymd>) -> [[Day; 7]; WEEKS] {
    let before = month.step(-1);
    let (here, behind) = (month.days(), before.days());
    let offset = month.offset();
    let mut weeks = [[Day {
        day: 1,
        inside: false,
        today: false,
    }; 7]; WEEKS];
    for (i, cell) in weeks.iter_mut().flatten().enumerate() {
        let n = i as i32 - offset as i32 + 1;
        let (day, inside, year_month) = if n < 1 {
            ((behind as i32 + n) as i8, false, before)
        } else if n > i32::from(here) {
            ((n - i32::from(here)) as i8, false, month.step(1))
        } else {
            (n as i8, true, month)
        };
        *cell = Day {
            day,
            inside,
            today: today == Some((year_month.year, year_month.month, day)),
        };
    }
    weeks
}

/// Today, by the machine's clock and zone.
pub fn today() -> Ymd {
    let now = jiff::Zoned::now();
    (now.year(), now.month(), now.day())
}

/// A month's name, 1 to 12, in the person's language.
pub fn name(month: i8) -> &'static str {
    match month {
        1 => tr("January"),
        2 => tr("February"),
        3 => tr("March"),
        4 => tr("April"),
        5 => tr("May"),
        6 => tr("June"),
        7 => tr("July"),
        8 => tr("August"),
        9 => tr("September"),
        10 => tr("October"),
        11 => tr("November"),
        _ => tr("December"),
    }
}

/// The letter a weekday's column is headed by, `i` 0 for Monday: the
/// first letter of the weekday's name, so a translation of the names
/// gives the letters.
pub fn initial(i: usize) -> String {
    let day = match i {
        0 => tr("Monday"),
        1 => tr("Tuesday"),
        2 => tr("Wednesday"),
        3 => tr("Thursday"),
        4 => tr("Friday"),
        5 => tr("Saturday"),
        _ => tr("Sunday"),
    };
    day.chars().take(1).collect::<String>().to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OCT_2026: Month = Month {
        year: 2026,
        month: 10,
    };

    fn row(week: &[Day; 7]) -> Vec<i8> {
        week.iter().map(|d| d.day).collect()
    }

    #[test]
    fn october_2026_starts_on_a_thursday_and_has_31_days() {
        assert_eq!(OCT_2026.offset(), 3, "Monday is 0, Thursday 3");
        assert_eq!(OCT_2026.days(), 31);
        let g = grid(OCT_2026, Some((2026, 10, 3)));
        // The mockup: 28 29 30 of September, then 1 and 2 and the 3rd, a Saturday.
        assert_eq!(row(&g[0]), [28, 29, 30, 1, 2, 3, 4]);
        assert_eq!(row(&g[4]), [26, 27, 28, 29, 30, 31, 1]);
        assert_eq!(row(&g[5]), [2, 3, 4, 5, 6, 7, 8]);
        let inside: Vec<bool> = g[0].iter().map(|d| d.inside).collect();
        assert_eq!(inside, [false, false, false, true, true, true, true]);
        // Only the 3rd is today.
        let todays: Vec<(usize, usize)> = (0..WEEKS)
            .flat_map(|w| (0..7).map(move |d| (w, d)))
            .filter(|&(w, d)| g[w][d].today)
            .collect();
        assert_eq!(todays, [(0, 5)]);
    }

    #[test]
    fn february_2028_has_29_days() {
        let feb = Month {
            year: 2028,
            month: 2,
        };
        assert_eq!(feb.days(), 29);
        assert_eq!(feb.offset(), 1, "the 1st is a Tuesday");
        let g = grid(feb, None);
        assert_eq!(row(&g[0]), [31, 1, 2, 3, 4, 5, 6]);
        assert_eq!(row(&g[4]), [28, 29, 1, 2, 3, 4, 5]);
        let inside: Vec<bool> = g[4].iter().map(|d| d.inside).collect();
        assert_eq!(inside, [true, true, false, false, false, false, false]);
        assert!(g.iter().flatten().all(|d| !d.today));
        assert_eq!(
            Month {
                year: 2027,
                month: 2
            }
            .days(),
            28
        );
    }

    #[test]
    fn a_month_that_starts_on_monday_is_never_preceded_by_a_week() {
        // June 2026 starts on a Monday.
        let june = Month {
            year: 2026,
            month: 6,
        };
        assert_eq!(june.offset(), 0);
        let g = grid(june, None);
        assert_eq!(row(&g[0]), [1, 2, 3, 4, 5, 6, 7]);
        assert!(g[0].iter().all(|d| d.inside));
        // And six weeks always: the last two rows hold the next month.
        assert_eq!(row(&g[4]), [29, 30, 1, 2, 3, 4, 5]);
        assert_eq!(row(&g[5]), [6, 7, 8, 9, 10, 11, 12]);
        assert!(g[5].iter().all(|d| !d.inside));
    }

    #[test]
    fn today_shows_in_the_dimmed_days_of_the_next_month_too() {
        let g = grid(OCT_2026, Some((2026, 11, 1)));
        assert!(g[4][6].today && !g[4][6].inside);
        // Not the 1st of October, whose number is the same.
        assert!(!g[0][3].today);
    }

    #[test]
    fn months_step_across_years_both_ways() {
        assert_eq!(
            OCT_2026.step(1),
            Month {
                year: 2026,
                month: 11
            }
        );
        assert_eq!(
            OCT_2026.step(3),
            Month {
                year: 2027,
                month: 1
            }
        );
        assert_eq!(
            OCT_2026.step(-10),
            Month {
                year: 2025,
                month: 12
            }
        );
        assert_eq!(OCT_2026.step(0), OCT_2026);
        assert_eq!(
            OCT_2026.step(12 * 30 + 2),
            Month {
                year: 2056,
                month: 12
            }
        );
        assert_eq!(Month::of((2026, 10, 3)), OCT_2026);
    }

    #[test]
    fn the_heading_and_the_weekday_letters_read_as_the_mockups_have_them() {
        assert_eq!(OCT_2026.title(), "October 2026");
        let letters: Vec<String> = (0..7).map(initial).collect();
        assert_eq!(letters, ["M", "T", "W", "T", "F", "S", "S"]);
        assert_eq!(name(1), "January");
        assert_eq!(name(12), "December");
    }
}
