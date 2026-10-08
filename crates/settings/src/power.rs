//! The Power page (roadmap M5.8b): what a battery page says everywhere,
//! in the order people look for it. A card at the top tells in one plain
//! headline how full the battery is and whether the computer is plugged in
//! and charging, with the time left under it; a computer with no battery
//! says `Plugged in` and nothing more. The batteries of other things (a
//! wireless mouse) follow when UPower lists any. Power settings come next,
//! and Details, closed, holds what technical people read: UPower's own
//! dump, as `upower --dump` prints it.
//!
//! Everything comes from UPower (`edel::power`, which the panel's battery
//! will use too), asked on a thread of its own and never the one that
//! draws, every few seconds while the page is on screen and never when it
//! is not (no service runs for what nobody uses). The charge is the
//! battery's own state, not a line of the settings file, so the status has
//! no Reset.
//!
//! The four settings below it are keys of the table that nothing follows
//! yet (`power.lid_close`, `power.lock_after_minutes`, `power.power_button`
//! and `power.on_battery`, M7.8, which needs the logind-free sleep and the
//! screen lock to exist), so each row only says so, as the Updates page's
//! automatic updates did. Seam (ADR-008): with M7.8 they become rows of
//! `rows.rs` like Channel, written with `Files::choose`, and a test here
//! fails the day the table supports one, so the row cannot be forgotten.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};
use edel::power::{self, Snapshot, State};

use crate::{card, widgets};

const INTRO: &str = n_("How much battery is left, and what the lid and the power button do.");

/// How often the page looks at UPower while it is on screen, in seconds.
const FOLLOW: u32 = 5;

/// A setting of this page that no release follows yet: its key, its title,
/// and what it will do, in plain words.
const LATER: &[(&str, &str, &str)] = &[
    (
        "power.lid_close",
        n_("When the lid closes"),
        n_("Not available yet. Sleep, lock, do nothing or shut down."),
    ),
    (
        "power.lock_after_minutes",
        n_("Lock the screen after"),
        n_("Not available yet. Lock when the computer sits unused."),
    ),
    (
        "power.power_button",
        n_("Power button"),
        n_("Not available yet. What pressing the power button does."),
    ),
    (
        "power.on_battery",
        n_("On battery"),
        n_("Not available yet. Save power or run at full speed."),
    ),
];

/// The card at the top: the words, and whether its icon is lit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Headline {
    title: String,
    sub: String,
    on: bool,
}

/// What the card says about `snapshot`.
fn headline(snapshot: &Snapshot) -> Headline {
    let Some(battery) = snapshot.battery() else {
        // No battery: a desktop, a server, a virtual machine. Plugged in,
        // which is all there is to say, unless the daemon says otherwise.
        return if snapshot.plugged_in() {
            Headline {
                title: tr("Plugged in").to_string(),
                sub: tr("This computer has no battery.").to_string(),
                on: true,
            }
        } else {
            Headline {
                title: tr("Running on battery").to_string(),
                sub: tr("The battery's charge is not reported.").to_string(),
                on: true,
            }
        };
    };
    let percent = battery.percent_whole();
    let at = |percent: Option<u32>| match percent {
        Some(p) => trf("Battery at {percent}%", &[("percent", &p.to_string())]),
        None => tr("Battery").to_string(),
    };
    let time = battery.minutes.map(power::duration_text);
    let plugged = snapshot.plugged_in();
    let (title, sub) = match battery.state {
        State::Charging => (
            at(percent),
            match &time {
                Some(t) => trf(
                    "Plugged in and charging. About {time} until full.",
                    &[("time", t)],
                ),
                None => tr("Plugged in and charging.").to_string(),
            },
        ),
        State::FullyCharged => (
            tr("Fully charged").to_string(),
            tr("Plugged in.").to_string(),
        ),
        State::PendingCharge => (
            at(percent),
            tr("Plugged in, not charging at the moment.").to_string(),
        ),
        State::Empty => (
            tr("Battery empty").to_string(),
            tr("Plug in the charger to keep working.").to_string(),
        ),
        State::Discharging => (
            at(percent),
            match &time {
                Some(t) => trf("On battery. About {time} left.", &[("time", t)]),
                None => tr("On battery.").to_string(),
            },
        ),
        State::PendingDischarge | State::Unknown => (
            at(percent),
            if plugged {
                tr("Plugged in.").to_string()
            } else {
                tr("On battery.").to_string()
            },
        ),
    };
    Headline {
        title,
        sub,
        on: true,
    }
}

/// The page's widgets and what it needs to change them.
struct Ui {
    glance: card::Glance,
    problem: gtk::Label,
    others_head: gtk::Box,
    others: gtk::Box,
    raw: gtk::Label,
    /// The other batteries as last shown, so the list is made again only
    /// when one would change.
    shown: RefCell<Vec<(String, u32)>>,
    /// Whether the page has said what it shows on standard error, which
    /// desktop-test reads (once, when it first has something).
    announced: Cell<bool>,
}

pub fn page() -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Power"), tr(INTRO));
    let glance = card::Glance::new(&content, "page-power");
    glance.show(false, tr("Looking at the battery"), "");

    let others_head = widgets::heading(&content, tr("Other batteries"));
    let others = widgets::group(&content);
    others_head.set_visible(false);
    others.set_visible(false);

    widgets::heading(&content, tr("Power settings"));
    let group = widgets::group(&content);
    for (_, title, blurb) in LATER {
        later_row(&group, tr(title), tr(blurb));
    }

    // Details, closed: UPower's own words.
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let details = widgets::group(&inner);
    let raw = widgets::output_row(&details);
    card::fold(&content, tr("Details"), &inner);

    let ui = Rc::new(Ui {
        glance,
        problem,
        others_head,
        others,
        raw,
        shown: RefCell::new(Vec::new()),
        announced: Cell::new(false),
    });
    card::while_shown(&page, FOLLOW, move || refresh(&ui));
    page
}

/// A setting nothing follows yet: its title, a control that cannot be
/// used and the line that says so.
fn later_row(group: &gtk::Box, title: &str, blurb: &str) {
    let choice = widgets::Choice::new(vec![tr("Automatic").to_string()]);
    let control = choice.widget();
    control.set_sensitive(false);
    let row = widgets::row(group, title, &control);
    row.subtitle.set_label(blurb);
    row.reset.set_visible(false);
    row.copy.set_visible(false);
}

/// Asks UPower and shows what it says.
fn refresh(ui: &Rc<Ui>) {
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        match gio::spawn_blocking(power::read).await {
            Ok(Ok(snapshot)) => {
                ui.problem.set_visible(false);
                ui.show(&snapshot);
            }
            Ok(Err(e)) => {
                ui.problem.set_label(&format!("{e:#}"));
                ui.problem.set_visible(true);
                ui.glance
                    .show(false, tr("Power information is not available"), "");
                ui.others_head.set_visible(false);
                ui.others.set_visible(false);
                if !ui.announced.replace(true) {
                    eprintln!("edel-settings: power page shows nothing: {e:#}");
                }
            }
            Err(_) => {}
        }
    });
}

impl Ui {
    fn show(&self, snapshot: &Snapshot) {
        let line = headline(snapshot);
        self.glance.show(line.on, &line.title, &line.sub);
        if !self.announced.replace(true) {
            // CI reads this line to know what the page showed.
            eprintln!("edel-settings: power page shows \"{}\"", line.title);
        }
        let others: Vec<(String, u32)> = snapshot
            .others()
            .map(|d| {
                let name = if d.name.is_empty() {
                    d.kind.clone()
                } else {
                    d.name.clone()
                };
                (name, d.percent_whole().unwrap_or(0))
            })
            .collect();
        self.others_head.set_visible(!others.is_empty());
        self.others.set_visible(!others.is_empty());
        if *self.shown.borrow() != others {
            card::clear(&self.others);
            for (name, percent) in &others {
                widgets::fact_row(&self.others, name, &format!("{percent}%"));
            }
            *self.shown.borrow_mut() = others;
        }
        widgets::say(
            &self.raw,
            &format!("$ {}\n{}", power::command_line(&["--dump"]), snapshot.raw),
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(battery: &str, daemon: &str) -> Snapshot {
        power::parse(&format!(
            "Device: /org/freedesktop/UPower/devices/DisplayDevice\n  battery\n    present:             yes\n{battery}\nDaemon:\n  on-battery:      {daemon}\n"
        ))
    }

    fn says(snapshot: &Snapshot) -> (String, String) {
        let line = headline(snapshot);
        (line.title, line.sub)
    }

    #[test]
    fn a_computer_with_no_battery_is_plugged_in_and_says_so_plainly() {
        let vm = power::parse(
            "Device: /org/freedesktop/UPower/devices/DisplayDevice\n  battery\n    present:             no\n    state:               unknown\n    percentage:          0%\n\nDaemon:\n  on-battery:      no\n",
        );
        assert_eq!(
            says(&vm),
            ("Plugged in".into(), "This computer has no battery.".into())
        );
        assert_eq!(says(&power::parse("")).0, "Plugged in");
    }

    #[test]
    fn a_battery_in_use_says_how_much_and_how_long() {
        let on_battery = dump(
            "    state:               discharging\n    time to empty:       5.4 hours\n    percentage:          82%\n",
            "yes",
        );
        assert_eq!(
            says(&on_battery),
            (
                "Battery at 82%".into(),
                "On battery. About 5 hours 25 minutes left.".into()
            )
        );
        let no_time = dump(
            "    state:               discharging\n    percentage:          9%\n",
            "yes",
        );
        assert_eq!(says(&no_time).1, "On battery.");
    }

    #[test]
    fn a_battery_on_the_charger_says_it_charges_and_when_it_is_full() {
        let charging = dump(
            "    state:               charging\n    time to full:        41.2 minutes\n    percentage:          55%\n",
            "no",
        );
        assert_eq!(
            says(&charging),
            (
                "Battery at 55%".into(),
                "Plugged in and charging. About 41 minutes until full.".into()
            )
        );
        let full = dump(
            "    state:               fully-charged\n    percentage:          100%\n",
            "no",
        );
        assert_eq!(says(&full), ("Fully charged".into(), "Plugged in.".into()));
        let held = dump(
            "    state:               pending-charge\n    percentage:          80%\n",
            "no",
        );
        assert_eq!(
            says(&held),
            (
                "Battery at 80%".into(),
                "Plugged in, not charging at the moment.".into()
            )
        );
        let empty = dump(
            "    state:               empty\n    percentage:          0%\n",
            "yes",
        );
        assert_eq!(says(&empty).0, "Battery empty");
    }

    #[test]
    fn a_battery_in_a_state_nobody_knows_still_gets_a_headline() {
        let odd = dump(
            "    state:               warming-up\n    percentage:          64%\n",
            "yes",
        );
        assert_eq!(says(&odd), ("Battery at 64%".into(), "On battery.".into()));
        let plugged = dump("    state:               warming-up\n", "no");
        assert_eq!(says(&plugged), ("Battery".into(), "Plugged in.".into()));
    }

    #[test]
    fn the_rows_that_wait_are_exactly_the_power_keys_nothing_follows_yet() {
        let table: Vec<&edel::settings::Key> = edel::settings::KEYS
            .iter()
            .filter(|k| k.path.starts_with("power."))
            .collect();
        let rows: Vec<&str> = LATER.iter().map(|(key, _, _)| *key).collect();
        assert_eq!(
            table.iter().map(|k| k.path).collect::<Vec<_>>(),
            rows,
            "every power key has a row here, in the table's order"
        );
        for key in table {
            assert!(
                !key.supported,
                "{} is followed now: make it a row of rows.rs, written with Files::choose, and take it out of LATER",
                key.path
            );
        }
    }
}
