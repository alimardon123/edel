//! The Updates page (roadmap M5.8c): what `edel status` reports about the
//! two slots, the channel `edel update` follows (`updates.channel`, one
//! line of the person's settings file like any other row), and two
//! buttons that run what the command line runs, `edel update --check` and
//! `edel rollback` (`cmd.rs`), each showing the command's own result or
//! its own message, so the page and the command are one level and read
//! alike (ADR-008, `docs/MESSAGES.md`).
//!
//! The page runs nothing in the background: `edel status` is read when
//! the page is shown and after a button, and every command runs on a
//! thread of its own, never the one that draws. Installing needs root
//! until `doas` arrives (M6.5): until then a click that lacks the right
//! shows `edel`'s refusal as it is, and the page changes nothing itself.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};
use edel::settings;

use crate::files::Files;
use crate::{about, cmd, rows, widgets};

const INTRO: &str = n_(
    "Which version this machine runs and which is waiting in the other slot, the channel it \
     takes new versions from, and a way to look for one or go back. A new version is written \
     to the other slot and starts at the next restart; if it does not start, the machine \
     goes back by itself.",
);

/// How long the Roll back button waits for its second click.
const CONFIRM: Duration = Duration::from_secs(5);

/// One line of `edel status`, as a row shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub title: String,
    pub value: String,
}

/// The rows for `edel status`' output: `running: A`, `order: A B`, a line
/// for each slot (`A: ok=1 try=0 /dev/vda2 2026.10.3`), `effects: lite`
/// and `logs: ...`, in plain words; a line it does not know is shown as it
/// is, with its first word as the title.
pub fn facts(status: &str) -> Vec<Fact> {
    status
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(fact)
        .collect()
}

fn fact(line: &str) -> Fact {
    let Some((key, value)) = line.split_once(':') else {
        return Fact {
            title: String::new(),
            value: line.trim().to_string(),
        };
    };
    let value = value.trim();
    let (title, value) = match key {
        "running" => (tr("Running now").to_string(), slot(value)),
        "order" => (
            tr("Starts next").to_string(),
            slot(value.split_whitespace().next().unwrap_or_default()),
        ),
        "A" | "B" => (
            trf("Slot {slot}", &[("slot", key)]),
            slot_state(value).unwrap_or_else(|| value.to_string()),
        ),
        "effects" => (tr("Effects").to_string(), crate::files::title(value)),
        "logs" => (tr("Logs").to_string(), value.to_string()),
        other => (crate::files::title(other), value.to_string()),
    };
    Fact { title, value }
}

/// `Slot A` for `A`.
fn slot(name: &str) -> String {
    trf("Slot {slot}", &[("slot", name)])
}

/// A slot's line, `ok=1 try=0 /dev/vda2 2026.10.3`, as its version and
/// whether it was confirmed good, or none when the line is another shape.
fn slot_state(value: &str) -> Option<String> {
    let words: Vec<&str> = value.split_whitespace().collect();
    let ok = words.iter().find_map(|w| w.strip_prefix("ok="))?;
    let version = words.last().filter(|_| words.len() >= 4)?;
    if *version == "empty" {
        return Some(tr("Empty").to_string());
    }
    let state = if ok == "1" {
        tr("confirmed good")
    } else {
        tr("not confirmed yet")
    };
    Some(trf(
        "{version}, {state}",
        &[("version", version), ("state", state)],
    ))
}

/// The channels the Channel row offers: the ones Edel OS publishes, then
/// the one now followed when it is another (a fleet's or a test's, set by
/// name on the command line).
fn channel_names(current: &str) -> Vec<String> {
    let mut names: Vec<String> = settings::CHANNELS.iter().map(|c| c.to_string()).collect();
    if !names.iter().any(|n| n == current) {
        names.push(current.to_string());
    }
    names
}

/// The page's widgets and what it needs to change them.
struct Ui {
    facts: gtk::Box,
    problem: gtk::Label,
    holder: gtk::Box,
    choice: RefCell<Option<Rc<widgets::Choice>>>,
    names: RefCell<Vec<String>>,
    channel_row: widgets::Row,
    check: gtk::Button,
    check_out: gtk::Label,
    rollback: gtk::Button,
    rollback_out: gtk::Label,
    /// Whether Roll back waits for its second click.
    armed: Cell<bool>,
    /// Set while the page itself moves a control, which is not a choice.
    quiet: Cell<bool>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

pub fn page() -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Updates"), tr(INTRO));

    widgets::heading(&content, tr("This system"));
    let facts = widgets::group(&content);

    widgets::heading(&content, tr("New versions"));
    let group = widgets::group(&content);
    let holder = gtk::Box::builder().build();
    let channel_row = widgets::row(&group, rows::title("updates.channel"), holder.upcast_ref());

    let check = widgets::action(tr("Check now"), false);
    let check_row = widgets::row(&group, tr("Look for a new version"), check.upcast_ref());
    check_row.subtitle.set_label(tr(
        "Asks the channel for its newest version and installs nothing.",
    ));
    check_row.reset.set_visible(false);
    let check_out = widgets::output_row(&group);

    let rollback = widgets::action(tr("Roll back"), false);
    let rollback_row = widgets::row(&group, tr("Go back a version"), rollback.upcast_ref());
    rollback_row.subtitle.set_label(tr(
        "Starts the version in the other slot at the next restart.",
    ));
    rollback_row.reset.set_visible(false);
    let rollback_out = widgets::output_row(&group);

    let ui = Rc::new(Ui {
        facts,
        problem,
        holder,
        choice: RefCell::new(None),
        names: RefCell::new(Vec::new()),
        channel_row,
        check,
        check_out,
        rollback,
        rollback_out,
        armed: Cell::new(false),
        quiet: Cell::new(false),
        monitors: RefCell::new(Vec::new()),
    });
    ui.update();
    ui.wire(&check_row, &rollback_row);

    // What the slots hold is read when the page is shown, not on a timer.
    let shown = ui.clone();
    page.connect_map(move |_| shown.read_status());

    let files = Files::here();
    let mut monitors = Vec::new();
    for path in std::iter::once(&files.machine).chain(files.person.as_ref()) {
        let file = gio::File::for_path(path);
        if let Ok(monitor) =
            file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        {
            let weak = Rc::downgrade(&ui);
            monitor.connect_changed(move |_, _, _, _| {
                if let Some(ui) = weak.upgrade() {
                    ui.update();
                }
            });
            monitors.push(monitor);
        }
    }
    *ui.monitors.borrow_mut() = monitors;
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
}

impl Ui {
    /// Shows the channel as the files say it now.
    fn update(self: &Rc<Self>) {
        let files = Files::here();
        let read =
            |path: Option<&std::path::PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok());
        let image = image_channel();
        let current = settings::channel(
            read(Some(&files.machine)).as_deref(),
            read(files.person.as_ref()).as_deref(),
            image.as_deref(),
        );
        let names = channel_names(&current);
        self.quiet.set(true);
        if *self.names.borrow() != names {
            while let Some(child) = self.holder.first_child() {
                self.holder.remove(&child);
            }
            let labels = names.iter().map(|n| rows::label(n)).collect();
            let choice = widgets::Choice::new(labels);
            let weak = Rc::downgrade(self);
            let picked = Rc::downgrade(&choice);
            choice.connect_changed(move || {
                let (Some(ui), Some(choice)) = (weak.upgrade(), picked.upgrade()) else {
                    return;
                };
                if ui.quiet.get() {
                    return;
                }
                let name = ui.names.borrow().get(choice.selected()).cloned();
                if let Some(name) = name {
                    ui.report(
                        Files::here().choose_over(
                            "updates.channel",
                            &name,
                            Some(
                                image_channel()
                                    .as_deref()
                                    .unwrap_or(settings::DEFAULT_CHANNEL),
                            ),
                        ),
                    );
                }
                ui.update();
            });
            self.holder.append(&choice.widget());
            *self.choice.borrow_mut() = Some(choice);
            *self.names.borrow_mut() = names.clone();
        }
        if let Some(choice) = self.choice.borrow().as_ref() {
            let at = names.iter().position(|n| *n == current).unwrap_or(0);
            choice.set_selected(at);
        }
        self.quiet.set(false);
        let source = files.source("updates.channel");
        self.channel_row
            .subtitle
            .set_label(&rows::describe(&source, &current));
        self.channel_row
            .reset
            .set_visible(rows::resettable(&source));
    }

    /// Shows a refusal in the page's problem line, or clears it.
    fn report(&self, result: Result<(), String>) {
        match result {
            Ok(()) => self.problem.set_visible(false),
            Err(message) => {
                self.problem.set_label(&message);
                self.problem.set_visible(true);
            }
        }
    }

    /// Reads `edel status` off the drawing thread and shows its lines.
    fn read_status(self: &Rc<Self>) {
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let Ok(status) = gio::spawn_blocking(|| cmd::edel(&["status"])).await else {
                return;
            };
            // CI reads this line to know the page ran the command.
            eprintln!(
                "edel-settings: updates page read edel status, exit {}",
                status.code.map_or("none".into(), |c| c.to_string())
            );
            ui.show_status(&status);
        });
    }

    fn show_status(&self, status: &cmd::Outcome) {
        while let Some(child) = self.facts.first_child() {
            self.facts.remove(&child);
        }
        if !status.ok {
            // Why there is nothing to show, in `edel`'s own words.
            widgets::text_row(&self.facts, &status.shown());
            return;
        }
        for fact in facts(&status.out) {
            if fact.title.is_empty() {
                widgets::text_row(&self.facts, &fact.value);
            } else {
                widgets::value_row(&self.facts, &fact.title, &fact.value);
            }
        }
    }

    /// Connects Reset, Copy as command and the two buttons.
    fn wire(self: &Rc<Self>, check_row: &widgets::Row, rollback_row: &widgets::Row) {
        let ui = Rc::downgrade(self);
        self.channel_row.reset.connect_clicked(move |_| {
            if let Some(ui) = ui.upgrade() {
                ui.report(Files::here().set("updates.channel", None));
                ui.update();
            }
        });
        let ui = Rc::downgrade(self);
        self.channel_row.copy.connect_clicked(move |button| {
            let Some(ui) = ui.upgrade() else { return };
            let at = ui.choice.borrow().as_ref().map_or(0, |c| c.selected());
            if let Some(name) = ui.names.borrow().get(at) {
                button
                    .clipboard()
                    .set_text(&rows::command(&[("updates.channel", name.clone())]));
                widgets::copied(button);
            }
        });
        copies(check_row, &["update", "--check"]);
        copies(rollback_row, &["rollback"]);

        let ui = Rc::downgrade(self);
        self.check.connect_clicked(move |_| {
            if let Some(ui) = ui.upgrade() {
                ui.run(&ui.check, &ui.check_out, &["update", "--check"]);
            }
        });
        let ui = Rc::downgrade(self);
        self.rollback.connect_clicked(move |_| {
            let Some(ui) = ui.upgrade() else { return };
            if !ui.armed.get() {
                // Going back changes which version starts next: ask twice.
                ui.armed.set(true);
                ui.rollback.set_label(tr("Click again to roll back"));
                let later = Rc::downgrade(&ui);
                glib::timeout_add_local_once(CONFIRM, move || {
                    if let Some(ui) = later.upgrade() {
                        ui.disarm();
                    }
                });
                return;
            }
            ui.disarm();
            ui.run(&ui.rollback, &ui.rollback_out, &["rollback"]);
        });
    }

    fn disarm(&self) {
        self.armed.set(false);
        self.rollback.set_label(tr("Roll back"));
    }

    /// Runs `edel args` off the drawing thread with `button` held, shows
    /// what it printed (or its message) in `out`, then reads the status
    /// again.
    fn run(self: &Rc<Self>, button: &gtk::Button, out: &gtk::Label, args: &'static [&'static str]) {
        button.set_sensitive(false);
        widgets::say(out, tr("Working..."), false);
        let (ui, button, out) = (self.clone(), button.clone(), out.clone());
        glib::spawn_future_local(async move {
            let done = gio::spawn_blocking(move || cmd::edel(args)).await;
            button.set_sensitive(true);
            match done {
                Ok(done) => widgets::say(&out, &done.shown(), !done.ok),
                Err(_) => widgets::say(&out, "", false),
            }
            ui.read_status();
        });
    }
}

/// Copy as command on an action's row copies the command it runs.
fn copies(row: &widgets::Row, args: &'static [&'static str]) {
    row.copy.connect_clicked(move |button| {
        button.clipboard().set_text(&cmd::line(args));
        widgets::copied(button);
    });
}

/// The channel this image was built for (os-release's `EDEL_CHANNEL`).
fn image_channel() -> Option<String> {
    about::os_release_field("EDEL_CHANNEL")
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "running: A\norder: A B\nA: ok=1 try=0 /dev/vda2 2026.10.3\n\
                          B: ok=0 try=2 /dev/vda3 2026.10.4\neffects: lite\n\
                          logs: the last update did not start, so the machine went back";

    fn pair(title: &str, value: &str) -> Fact {
        Fact {
            title: title.into(),
            value: value.into(),
        }
    }

    #[test]
    fn status_lines_become_rows_in_plain_words() {
        assert_eq!(
            facts(STATUS),
            [
                pair("Running now", "Slot A"),
                pair("Starts next", "Slot A"),
                pair("Slot A", "2026.10.3, confirmed good"),
                pair("Slot B", "2026.10.4, not confirmed yet"),
                pair("Effects", "Lite"),
                pair(
                    "Logs",
                    "the last update did not start, so the machine went back"
                ),
            ]
        );
    }

    #[test]
    fn an_empty_slot_and_a_strange_line_are_still_shown() {
        assert_eq!(
            facts("B: ok=0 try=0 /dev/vda3 empty\nsomething new: 1\njust words"),
            [
                pair("Slot B", "Empty"),
                pair("Something new", "1"),
                pair("", "just words"),
            ]
        );
        assert_eq!(
            facts("A: unexpected"),
            [pair("Slot A", "unexpected")],
            "a slot line of another shape is shown as it is"
        );
        assert!(facts("").is_empty());
    }

    #[test]
    fn the_channel_row_offers_the_published_channels_and_the_one_followed() {
        assert_eq!(channel_names("stable"), ["stable", "preview"]);
        assert_eq!(channel_names("preview"), ["stable", "preview"]);
        assert_eq!(channel_names("ci"), ["stable", "preview", "ci"]);
    }

    #[test]
    fn the_channel_row_is_a_key_of_the_table() {
        assert_eq!(rows::title("updates.channel"), "Channel");
    }
}
