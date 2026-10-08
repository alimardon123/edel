//! The Updates page (roadmap M5.8c, made plain and familiar in M5.8d, at
//! Alimardon's ask of 2026-10-08): what a person expects of an updates
//! page, in the order they expect it. A card at the top says in one
//! headline whether Edel OS is up to date, an update waits, a restart is
//! due or something failed (`status.rs` decides, from what `edel status`
//! and `edel update --check` print), with big buttons under it:
//! Check for updates, then Update now, then Restart now, and Go back to
//! the version before. A card of What's new follows a check that found a
//! newer version; a small Update settings section holds the channel
//! (`updates.channel`, one line of the person's settings file like any
//! other row, with Reset and Copy as command); and Details, closed, hold
//! what technical people want: the two slots and the raw output of the
//! last command. Only there does the word slot appear.
//!
//! The page runs nothing in the background: `edel status` is read when
//! the page is shown and after each action, and every command runs on a
//! thread of its own, never the one that draws (`cmd.rs`). Installing and
//! restarting need root until `doas` arrives (M6.5): until then a click
//! that lacks the right says so in plain words and shows `edel`'s refusal
//! in the Details; the page changes nothing itself.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{tr, trf};
use edel::settings;

use crate::files::Files;
use crate::status::{self, Action, Doing, Found, Inputs, Phase, Status};
use crate::{about, cmd, rows, widgets};

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
    let (confirmed, version) = status::slot_line(value)?;
    let Some(version) = version else {
        return Some(tr("Empty").to_string());
    };
    let state = if confirmed {
        tr("confirmed good")
    } else {
        tr("not confirmed yet")
    };
    Some(trf(
        "{version}, {state}",
        &[("version", &version), ("state", state)],
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

/// How long Go back waits for its second click.
const CONFIRM: Duration = Duration::from_secs(5);

/// What the Channel row says under its title.
fn blurb(channel: &str) -> &'static str {
    match channel {
        "stable" => tr("Tested releases. Recommended."),
        "preview" => tr("The newest features first; may have rough edges."),
        _ => tr("A channel this machine was set up to follow."),
    }
}

/// What the page is and has been doing.
struct State {
    phase: Phase,
    /// `edel status`, none until it was read or when it was refused.
    status: Option<Status>,
}

/// The page's widgets and what it needs to change them.
struct Ui {
    hero: widgets::Hero,
    primary: gtk::Button,
    rollback: gtk::Button,
    news_head: gtk::Box,
    news: gtk::Box,
    facts: gtk::Box,
    last_group: gtk::Box,
    last: gtk::Label,
    problem: gtk::Label,
    holder: gtk::Box,
    choice: RefCell<Option<Rc<widgets::Choice>>>,
    names: RefCell<Vec<String>>,
    channel_row: widgets::Row,
    /// The version running (os-release), read once.
    version: Option<String>,
    /// Whether Settings runs as root (it does not until `doas`, M6.5).
    root: bool,
    state: RefCell<State>,
    /// What the leading button does now.
    action: Cell<Action>,
    /// The channel followed now.
    channel: RefCell<String>,
    /// Whether Go back waits for its second click.
    armed: Cell<bool>,
    /// Set while the page itself moves a control, which is not a choice.
    quiet: Cell<bool>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

pub fn page() -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Updates"), "");

    let hero = widgets::Hero::new(&content);
    let primary = widgets::big_button(tr("Check for updates"), true);
    let rollback = widgets::big_button("", false);
    hero.add(&primary);
    hero.add(&rollback);
    hero.show_button(&rollback, false);

    // Seam: release notes (a `notes` field in release.toml) are shown here
    // when releases carry them; today the card holds what the list says.
    let news_head = widgets::heading(&content, tr("What's new"));
    let news = widgets::group(&content);
    news_head.set_visible(false);
    news.set_visible(false);

    widgets::heading(&content, tr("Update settings"));
    let group = widgets::group(&content);
    let holder = gtk::Box::builder().build();
    let channel_row = widgets::row(&group, rows::title("updates.channel"), holder.upcast_ref());
    // Seam (ADR-008): `updates.automatic` and `updates.restart_window` are
    // keys of the table that nothing follows yet (M7.5). Until then this
    // row only says so; with M7.5 it becomes a row of the key, like Channel.
    let switch = gtk::Switch::new();
    switch.set_sensitive(false);
    let automatic = widgets::row(
        &group,
        tr("Install updates automatically"),
        switch.upcast_ref(),
    );
    automatic
        .subtitle
        .set_label(tr("Not available yet. For now, check for updates here."));
    automatic.reset.set_visible(false);
    automatic.copy.set_visible(false);

    // Details, closed: the slots, and the last command as it ran.
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .build();
    let facts = widgets::group(&inner);
    let last_group = widgets::group(&inner);
    last_group.set_visible(false);
    let last = widgets::output_row(&last_group);
    widgets::disclosure(&content, tr("Details"), &inner);

    let ui = Rc::new(Ui {
        hero,
        primary,
        rollback,
        news_head,
        news,
        facts,
        last_group,
        last,
        problem,
        holder,
        choice: RefCell::new(None),
        names: RefCell::new(Vec::new()),
        channel_row,
        version: about::os_release_field("VERSION_ID"),
        root: cmd::is_root(),
        state: RefCell::new(State {
            phase: Phase::Idle,
            status: None,
        }),
        action: Cell::new(Action::Check),
        channel: RefCell::new(String::new()),
        armed: Cell::new(false),
        quiet: Cell::new(false),
        monitors: RefCell::new(Vec::new()),
    });
    ui.update();
    ui.wire();

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
        let line = match rows::note(&source, false) {
            Some(note) => format!("{} · {note}", blurb(&current)),
            None => blurb(&current).to_string(),
        };
        self.channel_row.subtitle.set_label(&line);
        self.channel_row
            .reset
            .set_visible(rows::resettable(&source));
        // The headline names the channel followed.
        let changed = *self.channel.borrow() != current;
        *self.channel.borrow_mut() = current;
        if changed {
            self.render();
        }
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

    /// Draws the card at the top, the buttons and What's new for what the
    /// page knows now.
    fn render(&self) {
        let channel = rows::label(&self.channel.borrow());
        let view = {
            let state = self.state.borrow();
            status::view(&Inputs {
                phase: &state.phase,
                status: state.status.as_ref(),
                version: self.version.as_deref(),
                channel: &channel,
                root: self.root,
            })
        };
        self.hero
            .show(view.icon, view.busy, view.problem, &view.title, &view.sub);
        self.primary.set_label(&view.primary.label);
        self.primary.set_sensitive(!view.busy);
        self.action.set(view.primary.action);
        let command = match view.primary.action {
            Action::Check => cmd::line(&["update", "--check"]),
            Action::Install => cmd::line(&["update"]),
            Action::Restart => "reboot".to_string(),
        };
        self.primary
            .set_tooltip_text(Some(&trf("Runs {command}", &[("command", &command)])));
        match &view.rollback {
            Some(version) => {
                let label = if self.armed.get() {
                    trf(
                        "Click again to go back to {version}",
                        &[("version", version)],
                    )
                } else {
                    trf("Go back to {version}", &[("version", version)])
                };
                self.rollback.set_label(&label);
                self.rollback.set_sensitive(!view.busy);
                self.rollback.set_tooltip_text(Some(&trf(
                    "Runs {command}",
                    &[("command", &cmd::line(&["rollback"]))],
                )));
                self.hero.show_button(&self.rollback, true);
            }
            None => {
                self.armed.set(false);
                self.hero.show_button(&self.rollback, false);
            }
        }
        self.show_news(view.news.as_ref());
    }

    /// What's new: the version a check found, when it was made and its size.
    fn show_news(&self, found: Option<&Found>) {
        while let Some(child) = self.news.first_child() {
            self.news.remove(&child);
        }
        self.news_head.set_visible(found.is_some());
        self.news.set_visible(found.is_some());
        let Some(found) = found else { return };
        widgets::value_row(&self.news, tr("Version"), &found.version);
        if let Some(date) = &found.released {
            widgets::value_row(&self.news, tr("Released"), date);
        }
        if let Some(size) = found.size {
            widgets::value_row(&self.news, tr("Size"), &rows::bytes_text(size));
        }
    }

    fn set_phase(&self, phase: Phase) {
        self.state.borrow_mut().phase = phase;
        self.render();
    }

    /// Reads `edel status` off the drawing thread and shows what it says.
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
        self.state.borrow_mut().status = if status.ok {
            Status::parse(&status.out)
        } else {
            None
        };
        if !status.ok {
            // Why there is nothing to show, in `edel`'s own words.
            widgets::text_row(&self.facts, &status.shown());
        } else {
            for fact in facts(&status.out) {
                if fact.title.is_empty() {
                    widgets::text_row(&self.facts, &fact.value);
                } else {
                    widgets::value_row(&self.facts, &fact.title, &fact.value);
                }
            }
        }
        self.render();
    }

    /// Connects Reset and the buttons.
    fn wire(self: &Rc<Self>) {
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

        let ui = Rc::downgrade(self);
        self.primary.connect_clicked(move |_| {
            let Some(ui) = ui.upgrade() else { return };
            match ui.action.get() {
                Action::Check => ui.check(),
                Action::Install => ui.install(),
                Action::Restart => ui.restart(),
            }
        });
        let ui = Rc::downgrade(self);
        self.rollback.connect_clicked(move |_| {
            let Some(ui) = ui.upgrade() else { return };
            if !ui.armed.get() {
                // Going back changes which version starts next: ask twice.
                ui.armed.set(true);
                ui.render();
                let later = Rc::downgrade(&ui);
                glib::timeout_add_local_once(CONFIRM, move || {
                    if let Some(ui) = later.upgrade() {
                        ui.armed.set(false);
                        ui.render();
                    }
                });
                return;
            }
            ui.armed.set(false);
            ui.go_back();
        });
    }

    /// Runs `edel args` off the drawing thread and gives what it did, after
    /// showing it in the Details as the command line shows it.
    async fn run(self: &Rc<Self>, args: &'static [&'static str]) -> Option<cmd::Outcome> {
        let done = gio::spawn_blocking(move || cmd::edel(args)).await.ok()?;
        self.show_last(&cmd::line(args), &done);
        Some(done)
    }

    fn show_last(&self, command: &str, done: &cmd::Outcome) {
        self.last_group.set_visible(true);
        widgets::say(
            &self.last,
            &format!("$ {command}\n{}", done.shown()),
            !done.ok,
        );
    }

    /// Check for updates: `edel update --check` against the channel's list.
    fn check(self: &Rc<Self>) {
        self.set_phase(Phase::Checking);
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let phase = match ui.run(&["update", "--check"]).await {
                Some(done) if done.ok => match status::parse_check(&done.out) {
                    Some(found) if found.newer => Phase::Available(found),
                    Some(_) => Phase::UpToDate { at: now() },
                    None => failed(Doing::Check, &done),
                },
                Some(done) => failed(Doing::Check, &done),
                None => stopped(Doing::Check),
            };
            ui.set_phase(phase);
        });
    }

    /// Update now: `edel update`, which takes the channel's newest version
    /// into the other slot.
    fn install(self: &Rc<Self>) {
        let version = match &self.state.borrow().phase {
            Phase::Available(found) => Some(found.version.clone()),
            _ => None,
        };
        self.set_phase(Phase::Installing);
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let phase = match ui.run(&["update"]).await {
                Some(done) if done.ok => Phase::Installed { version },
                Some(done) => failed(Doing::Install, &done),
                None => stopped(Doing::Install),
            };
            ui.set_phase(phase);
            ui.read_status();
        });
    }

    /// Go back: `edel rollback`, which makes the version before the last
    /// update start next.
    fn go_back(self: &Rc<Self>) {
        self.set_phase(Phase::GoingBack);
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let phase = match ui.run(&["rollback"]).await {
                Some(done) if done.ok => Phase::Idle,
                Some(done) => failed(Doing::GoBack, &done),
                None => stopped(Doing::GoBack),
            };
            ui.set_phase(phase);
            ui.read_status();
        });
    }

    /// Restart now: `reboot`.
    fn restart(self: &Rc<Self>) {
        self.set_phase(Phase::Restarting);
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let Ok(done) = gio::spawn_blocking(cmd::reboot).await else {
                return ui.set_phase(stopped(Doing::Restart));
            };
            ui.show_last("reboot", &done);
            if !done.ok {
                ui.set_phase(failed(Doing::Restart, &done));
            }
        });
    }
}

/// The phase after a command failed, with what it said.
fn failed(doing: Doing, done: &cmd::Outcome) -> Phase {
    Phase::Failed {
        doing,
        message: done.shown(),
    }
}

/// The phase after a command's thread stopped without an answer.
fn stopped(doing: Doing) -> Phase {
    Phase::Failed {
        doing,
        message: tr("the command stopped without an answer; try again").to_string(),
    }
}

/// The time of day, for `Checked at 14:32`.
fn now() -> String {
    glib::DateTime::now_local()
        .ok()
        .and_then(|t| t.format("%H:%M").ok())
        .map_or_else(String::new, |t| t.to_string())
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
