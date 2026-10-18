//! Battery care on the Power page (roadmap M7.16e): a group under the
//! battery's card with one switch, `power.battery_care`, which stops charging
//! at 80 percent where the laptop offers a limit, and a Charge fully once
//! button for a trip (`edel power charge-full`, `actions::CHARGE_FULL`). The
//! limit is the machine's, for everyone, so a change runs `edel settings set`
//! or `reset` and then `apply` as root through doas, as the Location page's
//! machine keys do (`cmd::set_and_apply_as_root`); apply sets the limit.
//!
//! A computer with no battery shows no group. A laptop whose firmware offers
//! no limit shows the switch off and greyed, and says so plainly. The group
//! reads the kernel's files through `edel::battery_care` when the page looks
//! at UPower, and the settings files while the page is open; nothing runs on
//! a timer of its own.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::battery_care;
use edel::i18n::{n_, tr};
use edel::settings::BATTERY_CARE;

use crate::files::Files;
use crate::widgets::{self, Line, exit_text, monitors, reset_words, set_words};
use crate::{actions, cmd};

/// What the row says when the laptop's firmware offers no limit.
const NO_LIMIT: &str = n_("This laptop offers no charge limit, so it always charges to 100%.");

/// What the Charge fully once row says.
const ONCE: &str = n_("For a trip: charges to 100% until it is unplugged, or restarts.");

/// The group's widgets and state.
pub struct Care {
    heading: gtk::Box,
    group: gtk::Box,
    switch: gtk::Switch,
    line: Line,
    /// The Charge fully once row and its button.
    once: gtk::Widget,
    button: gtk::Button,
    /// Set while the group itself moves the switch, which is no pick.
    quiet: Cell<bool>,
    /// Set while a change runs as root, so the switch waits.
    busy: Cell<bool>,
    /// Kept so the group follows the files while the page is open.
    monitors: Vec<gio::FileMonitor>,
}

/// Whether the machine's settings ask for battery care; off when absent.
fn on_now(files: &Files) -> bool {
    files.chosen(BATTERY_CARE).as_deref() == Some("true")
}

/// The words of the switch's change: set to true, or reset when off.
fn words(on: bool) -> Vec<String> {
    if on {
        set_words(BATTERY_CARE, "true")
    } else {
        reset_words(BATTERY_CARE)
    }
}

/// The row a [`widgets::Row`] lives in, to show or hide it whole.
fn row_of(row: &widgets::Row) -> gtk::Widget {
    row.subtitle
        .parent()
        .and_then(|words| words.parent())
        .unwrap_or_else(|| row.subtitle.clone().upcast())
}

/// Builds the group, hidden until [`Care::update`] finds a battery, and
/// appends it to `content`.
pub fn group(content: &gtk::Box) -> Rc<Care> {
    let heading = widgets::heading(content, tr("Battery"));
    let group = widgets::group(content);
    let switch = gtk::Switch::new();
    let line = Line::new(
        &group,
        BATTERY_CARE,
        &switch.clone().upcast::<gtk::Widget>(),
        "",
        true,
    );
    let button = widgets::action(tr("Charge fully"), false);
    let once = widgets::row(&group, tr("Charge fully once"), &button.clone().upcast());
    once.subtitle.set_label(tr(ONCE));
    once.reset.set_visible(false);
    once.copy.set_visible(false);
    let files = Files::here();
    let care = Rc::new(Care {
        heading,
        group,
        switch,
        line,
        once: row_of(&once),
        button,
        quiet: Cell::new(false),
        busy: Cell::new(false),
        monitors: monitors(&files),
    });
    care.update();
    for monitor in &care.monitors {
        let weak = Rc::downgrade(&care);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(care) = weak.upgrade() {
                care.update();
            }
        });
    }
    let weak = Rc::downgrade(&care);
    care.switch.connect_active_notify(move |switch| {
        let Some(care) = weak.upgrade() else { return };
        let on = switch.is_active();
        if care.quiet.get() || on == on_now(&Files::here()) {
            return;
        }
        change(&care, words(on));
    });
    let weak = Rc::downgrade(&care);
    care.line.row.reset.connect_clicked(move |_| {
        if let Some(care) = weak.upgrade() {
            change(&care, words(false));
        }
    });
    care.line.row.copy.connect_clicked(move |button| {
        let words = words(on_now(&Files::here()));
        let args: Vec<&str> = words.iter().map(String::as_str).collect();
        let line = format!("{} {}", edel::places::AS_ROOT, cmd::line(&args));
        button.clipboard().set_text(&line);
        widgets::copied(button);
    });
    let weak = Rc::downgrade(&care);
    care.button.connect_clicked(move |button| {
        let Some(care) = weak.upgrade() else { return };
        button.set_sensitive(false);
        let weak = Rc::downgrade(&care);
        glib::spawn_future_local(async move {
            let done = gio::spawn_blocking(|| cmd::edel(actions::CHARGE_FULL.args)).await;
            let Some(care) = weak.upgrade() else { return };
            if let Ok(run) = done {
                eprintln!(
                    "edel-settings: charge fully once asked, exit {}",
                    exit_text(run.code)
                );
                *care.line.problem.borrow_mut() = if run.ok { String::new() } else { run.shown() };
            }
            care.update();
        });
    });
    care
}

impl Care {
    /// Shows the battery's and the files' state now: the group only with a
    /// battery, the switch greyed where no limit is offered, and Charge fully
    /// once while care is on, greyed while a full charge is pending.
    pub fn update(&self) {
        let dir = Path::new(battery_care::POWER_SUPPLY);
        let battery = !battery_care::batteries(dir).is_empty();
        self.heading.set_visible(battery);
        self.group.set_visible(battery);
        if !battery {
            return;
        }
        let offered = battery_care::offered(dir);
        let files = Files::here();
        let on = on_now(&files) && offered;
        self.quiet.set(true);
        self.switch.set_active(on);
        self.quiet.set(false);
        self.switch.set_sensitive(offered && !self.busy.get());
        self.line.show(&files.source(BATTERY_CARE));
        if !offered {
            self.line.row.subtitle.set_label(tr(NO_LIMIT));
        }
        let pending = Path::new(edel::places::CHARGE_FULL_ONCE).exists();
        self.once.set_visible(on);
        self.button.set_sensitive(!pending);
        self.button.set_label(if pending {
            tr("Charging fully")
        } else {
            tr("Charge fully")
        });
    }
}

/// Runs `edel` with `words` as root, then `edel settings apply`, on a thread
/// of its own; the switch waits meanwhile, and a refusal is shown in the
/// row's line as `edel` said it.
fn change(care: &Rc<Care>, words: Vec<String>) {
    care.busy.set(true);
    care.switch.set_sensitive(false);
    *care.line.problem.borrow_mut() = String::new();
    let weak = Rc::downgrade(care);
    glib::spawn_future_local(async move {
        let done = gio::spawn_blocking(move || cmd::set_and_apply_as_root(words)).await;
        let Some(care) = weak.upgrade() else { return };
        if let Ok((set, apply)) = done {
            eprintln!(
                "edel-settings: battery care asked, exit {}",
                exit_text(set.code)
            );
            let refused = if set.ok {
                apply.and_then(|run| (!run.ok).then(|| run.shown()))
            } else {
                Some(set.shown())
            };
            *care.line.problem.borrow_mut() = refused.unwrap_or_default();
        }
        care.busy.set(false);
        care.update();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_sets_the_key_or_resets_it() {
        assert_eq!(words(true), ["settings", "set", "power.battery_care=true"]);
        assert_eq!(words(false), ["settings", "reset", "power.battery_care"]);
        assert_eq!(crate::rows::title(BATTERY_CARE), "Battery care");
    }
}
