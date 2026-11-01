//! The Add-ons page (roadmap M7.2b, ADR-007 level 2): one switch for each
//! add-on the image was built with, the extras in one group and the
//! hardware packs in Hardware support, the desktop on the System page
//! (M8.21, T2), whose switch asks before it takes the desktop away. Each
//! row's title is the
//! add-on's name and its line the add-on's summary, then where it stands
//! (`edel::addon_state`, the reading `edel addon list` shows too). A switch
//! is `addons.installed+=NAME` or `-=NAME` as root through doas
//! (`cmd::set_and_apply_as_root`, then `apply`), so the machine's list is
//! changed in one place, the way Region's and Location's machine keys are.
//! A refusal is shown in that row's line until the next change.
//!
//! The page has no Reset: the list is the machine's, and each row's Copy as
//! command gives the line that reproduces the state shown. It follows the
//! settings files and the add-ons' folders while it is open, and the page is
//! read again each time it is shown, as a pending add-on changes without a
//! file change. A hardware pack's row says whether this computer has the
//! hardware it serves (M7.11), read once when the page is made. It runs
//! nothing on a timer.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::addon_state::{self, Facts, State};
use edel::hardware;
use edel::i18n::{n_, tr};
use edel::places;
use edel::settings::PAGES;

use crate::files::Files;
use crate::widgets::{exit_text, monitors};
use crate::{actions, cmd, widgets};

/// The list the switches change, as the settings file names it.
const KEY: &str = "addons.installed";

/// The page's line under its title, a sentence of its own.
const INTRO: &str = n_(
    "Signed extras to the system, each added or taken away with one switch. A new one downloads once this computer is online and starts at the next restart.",
);

/// The words a hardware pack's row adds when this computer has hardware it
/// serves (M7.11), and when it has none (M8.21, T2).
const FOUND: &str = n_("Found in this computer");
const NOT_FOUND: &str = n_("Not found in this computer");

/// The add-on every desktop is made of, shown on the System page alone.
const DESKTOP: &str = "desktop";

/// The Add-ons page's two groups (M8.21, T2).
const EXTRAS: &str = n_("Extras");
const HARDWARE: &str = n_("Hardware support");

/// What the Desktop switch asks before it takes the desktop away.
const TAKE_DESKTOP: &str = n_("Take the desktop away?");
const TAKE_DESKTOP_DETAIL: &str = n_(
    "At the next restart this computer starts with a console only. Your files and settings stay, and turning this switch on again brings the desktop back.",
);

/// What the page says when the image has no add-on to offer.
const NONE: &str = n_("This version of Edel OS has no add-ons to choose.");

/// The words of an `edel settings set` line that adds (`+=`) or takes away
/// (`-=`) `name` from the list, as the Settings app's action names them.
pub fn words(name: &str, on: bool) -> Vec<String> {
    let op = if on { "+" } else { "-" };
    actions::SET
        .args
        .iter()
        .map(|word| word.to_string())
        .chain(std::iter::once(format!("{KEY}{op}={name}")))
        .collect()
}

/// The line Copy as command gives for `name`: the doas line that sets it as
/// the state shown says, `+=` when chosen and `-=` when not.
pub fn copy_line(name: &str, on: bool) -> String {
    let args = words(name, on);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    format!("{} {}", edel::places::AS_ROOT, cmd::line(&args))
}

/// One add-on's row: its switch, its line, and a refusal shown in it.
struct Add {
    name: String,
    summary: String,
    switch: gtk::Switch,
    row: widgets::Row,
    /// The state the files gave at the last read, for the line.
    state: Cell<State>,
    /// For a hardware pack, whether this computer has hardware it serves.
    found: Option<bool>,
    /// Whether turning the switch off asks first (the desktop).
    ask_off: bool,
    /// A refused change's message, which replaces the line until the next change.
    problem: RefCell<String>,
}

impl Add {
    /// Shows the row's line: the problem in the error colour while there is
    /// one, else the summary, then where the add-on stands.
    fn show(&self) {
        let problem = self.problem.borrow();
        if problem.is_empty() {
            self.row.subtitle.remove_css_class("edel-error");
            let state = tr(self.state.get().line());
            let line = match self.found {
                Some(true) => format!("{} · {} · {state}", self.summary, tr(FOUND)),
                Some(false) => format!("{} · {} · {state}", self.summary, tr(NOT_FOUND)),
                None => format!("{} · {state}", self.summary),
            };
            self.row.subtitle.set_label(&line);
        } else {
            self.row.subtitle.add_css_class("edel-error");
            self.row.subtitle.set_label(problem.as_str());
        }
    }
}

struct Ui {
    rows: Vec<Add>,
    /// Set while the page itself moves a switch, which is no pick.
    quiet: Cell<bool>,
    /// Set while a change runs, so the switches wait.
    busy: Cell<bool>,
    /// Kept so the page follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

/// Shows the files' state now: each switch, each row's line and whether the
/// switches are free.
fn update(ui: &Rc<Ui>) {
    let facts = Facts::read();
    ui.quiet.set(true);
    for add in &ui.rows {
        let state = addon_state::state(&add.name, &facts);
        add.state.set(state);
        add.switch.set_active(state.chosen());
        add.switch.set_sensitive(!ui.busy.get());
        add.show();
    }
    ui.quiet.set(false);
}

/// Calls `update` once the callback that asked for it has returned.
fn later(ui: &Rc<Ui>) {
    let weak = Rc::downgrade(ui);
    glib::idle_add_local_once(move || {
        if let Some(ui) = weak.upgrade() {
            update(&ui);
        }
    });
}

/// Adds or takes away the add-on at `at` as root, then applies the list,
/// on a thread of its own. The switches wait meanwhile; a refusal is shown
/// in the row's line, as `edel` said it.
fn change(ui: &Rc<Ui>, at: usize, on: bool) {
    let add = &ui.rows[at];
    let name = add.name.clone();
    let words = words(&name, on);
    ui.busy.set(true);
    for add in &ui.rows {
        add.switch.set_sensitive(false);
    }
    add.problem.borrow_mut().clear();
    add.show();
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let done = gio::spawn_blocking(move || cmd::set_and_apply_as_root(words)).await;
        let Some(ui) = weak.upgrade() else { return };
        let Ok((set, apply)) = done else {
            ui.busy.set(false);
            later(&ui);
            return;
        };
        let state = if on { "on" } else { "off" };
        eprintln!(
            "edel-settings: addons {name} {state} asked, exit {}",
            exit_text(set.code)
        );
        let refused = if set.ok {
            apply.and_then(|run| {
                eprintln!(
                    "edel-settings: addons apply asked, exit {}",
                    exit_text(run.code)
                );
                (!run.ok).then(|| run.shown())
            })
        } else {
            Some(set.shown())
        };
        if let Some(add) = ui.rows.iter().find(|add| add.name == name) {
            *add.problem.borrow_mut() = refused.unwrap_or_default();
        }
        ui.busy.set(false);
        later(&ui);
    });
}

/// Logs where each add-on's switch lies in the window, for CI to click
/// (logical pixels), as `edel-settings: addons group places NAME X+Y+WxH,
/// ...`. The view settles over a few frames, so it logs once two frames
/// agree.
fn log_places(page: &gtk::Widget, switches: Vec<(String, gtk::Switch)>, what: &'static str) {
    let last = RefCell::new(String::new());
    page.add_tick_callback(move |page, _| {
        let Some(window) = page.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !page.is_mapped() || page.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        let places: Vec<String> = switches
            .iter()
            .map(|(name, switch)| {
                format!("{name} {}", widgets::place(switch.compute_bounds(&window)))
            })
            .collect();
        let line = if places.is_empty() {
            format!("edel-settings: {what} group places none")
        } else {
            format!("edel-settings: {what} group places {}", places.join(", "))
        };
        if last.replace(line.clone()) != line {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{line}");
        glib::ControlFlow::Break
    });
}

/// The Add-ons page (M7.2b): one row for each add-on the image was built
/// with, in the order of the feature files' names, the extras in one group
/// and the hardware packs in Hardware support (M8.21, T2). The desktop is
/// left to the System page's own row ([`desktop`]).
pub fn page() -> gtk::Widget {
    let title = PAGES
        .iter()
        .find(|p| p.section == "addons")
        .map_or("", |p| p.title);
    let (page, content, _problem) = widgets::page(tr(title), tr(INTRO));
    let (packs, extras): (Vec<Offer>, Vec<Offer>) = offers()
        .into_iter()
        .filter(|offer| offer.name != DESKTOP)
        .partition(|offer| offer.pack);
    if packs.is_empty() && extras.is_empty() {
        widgets::text_row(&widgets::group(&content), tr(NONE));
    }
    let devices = if packs.is_empty() {
        Vec::new()
    } else {
        hardware::devices()
    };
    let mut rows = Vec::new();
    for (heading, offers) in [(EXTRAS, extras), (HARDWARE, packs)] {
        if offers.is_empty() {
            continue;
        }
        widgets::heading(&content, tr(heading));
        let group = widgets::group(&content);
        for offer in offers {
            let found = offer
                .pack
                .then(|| hardware::serves(&offer.hardware, &devices));
            rows.push(add(&group, offer, found, false));
        }
    }
    follow(&page, rows, "addons");
    page
}

/// The System page's Desktop group (M8.21, T2): the desktop add-on's
/// switch on its own, as taking it away leaves a console at the next
/// restart, which the switch asks about first. Nothing when the image
/// does not offer the desktop.
pub fn desktop(page: &gtk::Widget, content: &gtk::Box) {
    let Some(offer) = offers().into_iter().find(|offer| offer.name == DESKTOP) else {
        return;
    };
    widgets::heading(content, tr("Desktop"));
    let group = widgets::group(content);
    let row = add(&group, offer, None, true);
    follow(page, vec![row], "system desktop");
}

/// The add-ons the image offers, with whether each is a hardware pack.
fn offers() -> Vec<Offer> {
    addon_state::offered_features_in(std::path::Path::new(places::OFFERED_ADDONS_DIR))
        .into_iter()
        .map(|(name, feature)| Offer {
            name,
            summary: feature.summary,
            pack: !feature.hardware.is_empty(),
            hardware: feature.hardware,
        })
        .collect()
}

/// One add-on as the page offers it.
struct Offer {
    name: String,
    summary: String,
    /// Whether it is a hardware pack, matched by `hardware`.
    pack: bool,
    hardware: Vec<String>,
}

/// Puts `offer`'s row in `group`.
fn add(group: &gtk::Box, offer: Offer, found: Option<bool>, ask_off: bool) -> Add {
    let switch = gtk::Switch::new();
    let control = switch.clone().upcast::<gtk::Widget>();
    let title = if ask_off {
        tr("Use the desktop")
    } else {
        offer.name.as_str()
    };
    let row = widgets::row(group, title, &control);
    Add {
        name: offer.name,
        summary: offer.summary,
        switch,
        row,
        found,
        ask_off,
        state: Cell::new(State::Off),
        problem: RefCell::new(String::new()),
    }
}

/// Makes `rows` follow the files while `page` is open and act on their
/// switches; `what` names the rows in the places line CI reads.
fn follow(page: &gtk::Widget, rows: Vec<Add>, what: &'static str) {
    let switches: Vec<(String, gtk::Switch)> = rows
        .iter()
        .map(|add| (add.name.clone(), add.switch.clone()))
        .collect();
    let ui = Rc::new(Ui {
        rows,
        quiet: Cell::new(false),
        busy: Cell::new(false),
        monitors: monitors(&Files::here()),
    });
    update(&ui);
    for monitor in &ui.monitors {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                update(&ui);
            }
        });
    }

    for (at, add) in ui.rows.iter().enumerate() {
        let weak = Rc::downgrade(&ui);
        add.switch.connect_active_notify(move |switch| {
            let Some(ui) = weak.upgrade() else { return };
            if ui.quiet.get() {
                return;
            }
            let on = switch.is_active();
            if on || !ui.rows[at].ask_off {
                change(&ui, at, on);
                return;
            }
            // Asked first: the switch stays on until the answer is yes.
            ui.quiet.set(true);
            switch.set_active(true);
            ui.quiet.set(false);
            let weak = Rc::downgrade(&ui);
            widgets::confirm(
                switch.upcast_ref(),
                tr(TAKE_DESKTOP),
                tr(TAKE_DESKTOP_DETAIL),
                tr("Take away"),
                true,
                move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.quiet.set(true);
                        ui.rows[at].switch.set_active(false);
                        ui.quiet.set(false);
                        change(&ui, at, false);
                    }
                },
            );
        });
        add.row.reset.set_visible(false);
        let name = add.name.clone();
        let switch = add.switch.clone();
        add.row.copy.connect_clicked(move |button| {
            button
                .clipboard()
                .set_text(&copy_line(&name, switch.is_active()));
            widgets::copied(button);
        });
    }

    // Read again each time the page is shown, as a pending add-on changes
    // without a settings file changing; the places are logged once, after
    // the page is laid out.
    let weak = Rc::downgrade(&ui);
    let placed = Rc::new(Cell::new(false));
    page.connect_map(move |page| {
        if let Some(ui) = weak.upgrade() {
            update(&ui);
        }
        if !placed.replace(true) {
            let page = page.clone();
            let switches = switches.clone();
            glib::idle_add_local_once(move || log_places(&page, switches, what));
        }
    });

    // The rows' state lives as long as the page does: its callbacks hold
    // only weak references.
    page.connect_destroy(move |_| {
        let _ = &ui;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_an_add_on_writes_the_plus_word() {
        assert_eq!(
            words("orca", true),
            ["settings", "set", "addons.installed+=orca"]
        );
    }

    #[test]
    fn taking_an_add_on_away_writes_the_minus_word() {
        assert_eq!(
            words("orca", false),
            ["settings", "set", "addons.installed-=orca"]
        );
    }

    #[test]
    fn copy_as_command_gives_the_doas_line_for_the_state_shown() {
        assert_eq!(
            copy_line("orca", true),
            format!(
                "{} edel settings set addons.installed+=orca",
                edel::places::AS_ROOT
            )
        );
        assert_eq!(
            copy_line("orca", false),
            format!(
                "{} edel settings set addons.installed-=orca",
                edel::places::AS_ROOT
            )
        );
    }
}
