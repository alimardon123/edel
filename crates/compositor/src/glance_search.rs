//! The overview's search (roadmap M5.2j-b3, `docs/mockups/shell/overview.jpg`):
//! typing while the overview is open fills the field over the windows, and
//! a card under it lists what matches, best first: open windows on any
//! workspace whose title or app holds what was typed, then the apps the
//! launcher would find (`edel::apps::search`, the same ranking). Return
//! goes to the chosen window or starts the chosen app, Up and Down choose,
//! Backspace takes a letter back, and Escape empties the field before it
//! leaves. What matches is worked out once per key, not per frame.

use edel::apps::{self, App};
use smithay::desktop::Window;
use smithay::input::keyboard::xkb;
use smithay::utils::{Logical, Rectangle};

use crate::decoration::{app_id, title};
use crate::state::Edel;

/// The most results the card lists, and the most windows among them.
pub const MOST: usize = 6;
const MOST_WINDOWS: usize = 3;
/// A result's row in the card, and the gap between the field and the card.
pub const ROW: i32 = 40;
pub const CARD_GAP: i32 = 8;

/// What was typed, what matches it and which one is chosen.
#[derive(Default)]
pub struct Search {
    pub query: String,
    pub chosen: usize,
    pub found: Vec<Found>,
    /// The apps, read on the first letter typed and kept while the
    /// overview is open, so one just installed shows the next time.
    apps: Option<Vec<App>>,
}

/// One result: an open window or an app to start.
#[derive(Clone)]
pub enum Found {
    Window(Window),
    App(App),
}

impl Found {
    /// What its row says.
    pub fn name(&self) -> String {
        match self {
            Found::Window(window) => title(window),
            Found::App(app) => app.name.clone(),
        }
    }

    /// The app id its icon is found by.
    pub fn app(&self) -> String {
        match self {
            Found::Window(window) => app_id(window),
            Found::App(app) => app.id.clone(),
        }
    }

    pub fn is_window(&self) -> bool {
        matches!(self, Found::Window(_))
    }
}

/// Where the card of `rows` results lies under the search field `field`.
pub fn card(field: Rectangle<i32, Logical>, rows: usize) -> Rectangle<i32, Logical> {
    Rectangle::new(
        (field.loc.x, field.loc.y + field.size.h + CARD_GAP).into(),
        (field.size.w, rows as i32 * ROW + 2 * CARD_PAD).into(),
    )
}

/// The card's room above its first row and below its last.
pub const CARD_PAD: i32 = 6;

/// The row of `card` under `y`, if any.
pub fn row_at(card: Rectangle<i32, Logical>, rows: usize, y: f64) -> Option<usize> {
    let inside = y - f64::from(card.loc.y + CARD_PAD);
    if inside < 0.0 {
        return None;
    }
    let row = (inside / f64::from(ROW)) as usize;
    (row < rows).then_some(row)
}

impl Edel {
    /// A key pressed while the overview is open (`input.rs`): its keysym
    /// and the character it types.
    pub fn overview_key(&mut self, sym: u32, typed: Option<char>) {
        let Some(overview) = self.overview.as_mut() else {
            return;
        };
        let search = &mut overview.search;
        let before = search.query.clone();
        // The keys that act are logged by name, for CI; the letters typed
        // are not, as a search can hold anything.
        let named = match sym {
            xkb::keysyms::KEY_Escape => Some("Escape"),
            xkb::keysyms::KEY_Return | xkb::keysyms::KEY_KP_Enter => Some("Return"),
            xkb::keysyms::KEY_BackSpace => Some("BackSpace"),
            xkb::keysyms::KEY_Up => Some("Up"),
            xkb::keysyms::KEY_Down => Some("Down"),
            _ => None,
        };
        if let Some(name) = named {
            eprintln!("edel-compositor: overview key {name}");
        }
        match sym {
            xkb::keysyms::KEY_Escape => {
                if search.query.is_empty() {
                    self.leave_overview();
                    return;
                }
                search.query.clear();
            }
            xkb::keysyms::KEY_Return | xkb::keysyms::KEY_KP_Enter => {
                let chosen = search.chosen;
                self.overview_go(chosen);
                return;
            }
            xkb::keysyms::KEY_BackSpace => {
                search.query.pop();
            }
            xkb::keysyms::KEY_Down => {
                if search.chosen + 1 < search.found.len() {
                    search.chosen += 1;
                }
            }
            xkb::keysyms::KEY_Up => search.chosen = search.chosen.saturating_sub(1),
            _ => {
                if let Some(c) = typed {
                    search.query.push(c);
                }
            }
        }
        if search.query != before {
            self.overview_find();
        }
        self.dirty = true;
    }

    /// Works out what matches the field, windows first, and logs it:
    /// `overview search QUERY: N windows, M apps`.
    fn overview_find(&mut self) {
        let query = match &self.overview {
            Some(overview) => overview.search.query.trim().to_lowercase(),
            None => return,
        };
        let mut found = Vec::new();
        if !query.is_empty() {
            let mut windows: Vec<Window> = self.space.elements().cloned().collect();
            for (_, window, _) in self.desks.hidden() {
                if !windows.contains(window) {
                    windows.push(window.clone());
                }
            }
            found.extend(
                windows
                    .into_iter()
                    .filter(|w| {
                        title(w).to_lowercase().contains(&query)
                            || app_id(w).to_lowercase().contains(&query)
                    })
                    .take(MOST_WINDOWS)
                    .map(Found::Window),
            );
        }
        let Some(overview) = self.overview.as_mut() else {
            return;
        };
        let search = &mut overview.search;
        if !query.is_empty() {
            let all = search
                .apps
                .get_or_insert_with(|| apps::read_all(&apps::dirs()));
            let room = MOST - found.len();
            found.extend(
                apps::search(all, &query)
                    .into_iter()
                    .take(room)
                    .cloned()
                    .map(Found::App),
            );
        }
        let windows = found.iter().filter(|f| f.is_window()).count();
        eprintln!(
            "edel-compositor: overview search {}: {windows} windows, {} apps",
            search.query,
            found.len() - windows
        );
        search.found = found;
        search.chosen = 0;
    }

    /// Goes to result `i`: shows its window, from its workspace or from
    /// the window list, or starts its app; either leaves the overview.
    pub fn overview_go(&mut self, i: usize) {
        let Some(found) = self
            .overview
            .as_ref()
            .and_then(|o| o.search.found.get(i).cloned())
        else {
            return;
        };
        self.leave_overview();
        match found {
            Found::Window(window) => {
                let words = title(&window);
                if self.desks.minimized(&window).is_some() {
                    if !self.restore(&window) {
                        return;
                    }
                } else if let Some(desk) = self.desks.hidden_on(&window) {
                    self.switch_for(desk, &window);
                } else if self.space.elements().any(|w| *w == window) {
                    self.space.raise_element(&window, true);
                    self.focus(&window);
                } else {
                    // Closed while the overview was open.
                    return;
                }
                eprintln!("edel-compositor: overview went to window {words}");
            }
            Found::App(app) => {
                if crate::program::run(self, &apps::command(&app), &app.name) {
                    eprintln!("edel-compositor: overview started {}", app.id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_lies_under_the_field_and_its_rows_are_found_by_height() {
        let field = Rectangle::new((100, 20).into(), (360, 36).into());
        let c = card(field, 3);
        assert_eq!(c.loc.y, 20 + 36 + CARD_GAP);
        assert_eq!(c.size, (360, 3 * ROW + 2 * CARD_PAD).into());
        let top = f64::from(c.loc.y + CARD_PAD);
        assert_eq!(row_at(c, 3, top - 1.0), None);
        assert_eq!(row_at(c, 3, top), Some(0));
        assert_eq!(row_at(c, 3, top + f64::from(ROW)), Some(1));
        assert_eq!(row_at(c, 3, top + f64::from(3 * ROW)), None);
    }
}
