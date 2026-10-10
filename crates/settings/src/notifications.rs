//! The Notifications page (roadmap M5.9b): Do not disturb, the key
//! `notifications.do_not_disturb`, which keeps banners away while the
//! notifications still collect in the list the panel's clock opens; Where
//! notifications appear, `notifications.position` (M5.9j); and Unread
//! badge, `notifications.badge`, what the notifications button shows
//! (M5.9k). Each is the same line of the person's settings file as `edel
//! settings set` (and Do not disturb as quick settings' tile and the
//! centre's switch), so each shows what the others chose: the page follows
//! the files while it is open. shell-ui follows them at once. Nothing runs
//! here but the file monitors.

use std::cell::Cell;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use edel::i18n::{n_, tr};
use edel::settings::{DO_NOT_DISTURB, NOTIFICATION_BADGE, NOTIFICATION_POSITION};

use crate::files::Files;
use crate::{rows, widgets};

const INTRO: &str =
    n_("Banners appear when an app has something to say, and the clock keeps them all.");

/// What the row's line says under its title.
const WHAT: &str = n_("Banners stay away; notifications still collect in the list the clock opens");

/// The rows that choose one of a key's values: the key, what the row says
/// under its title, and the value that applies without the key, which is
/// taken out of the file rather than written.
const CHOICES: [(&str, &str, &str); 2] = [
    (
        NOTIFICATION_POSITION,
        n_("Automatic puts them by the status area; a banner keeps clear of panels"),
        "auto",
    ),
    (
        NOTIFICATION_BADGE,
        n_("What the notifications button shows while some are unread"),
        "dot",
    ),
];

/// One row choosing one of a key's values, from [`CHOICES`].
struct ChoiceRow {
    key: &'static str,
    what: &'static str,
    release: &'static str,
    choice: Rc<widgets::Choice>,
    row: widgets::Row,
}

impl ChoiceRow {
    fn new(
        group: &gtk::Box,
        (key, what, release): (&'static str, &'static str, &'static str),
    ) -> ChoiceRow {
        let labels = rows::values(key).iter().map(|v| rows::label(v)).collect();
        let choice = widgets::Choice::new(labels);
        let row = widgets::row(group, rows::title(key), &choice.widget());
        ChoiceRow {
            key,
            what,
            release,
            choice,
            row,
        }
    }

    /// The value that applies now, the release's without a line.
    fn value(&self, files: &Files) -> String {
        files
            .chosen(self.key)
            .filter(|v| rows::values(self.key).contains(&v.as_str()))
            .unwrap_or_else(|| self.release.to_string())
    }

    /// Shows what the files say, without calling back.
    fn update(&self, files: &Files) {
        let value = self.value(files);
        let at = rows::values(self.key)
            .iter()
            .position(|v| *v == value)
            .unwrap_or(0);
        self.choice.set_selected(at);
        let source = files.source(self.key);
        let line = match rows::note(&source, false) {
            Some(note) => format!("{} · {note}", tr(self.what)),
            None => tr(self.what).to_string(),
        };
        self.row.subtitle.set_label(&line);
        self.row.reset.set_visible(rows::resettable(&source));
    }
}

struct Ui {
    switch: gtk::Switch,
    row: widgets::Row,
    choices: Vec<ChoiceRow>,
    problem: gtk::Label,
    /// Set while the page itself moves the switch, so that is no choice.
    quiet: Cell<bool>,
    /// Kept so the page follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

impl Ui {
    /// Shows what the files say now: each control, and where its value
    /// comes from.
    fn update(&self) {
        let files = Files::here();
        self.quiet.set(true);
        self.switch.set_active(files.do_not_disturb());
        for choice in &self.choices {
            choice.update(&files);
        }
        self.quiet.set(false);
        let source = files.source(DO_NOT_DISTURB);
        let line = match rows::note(&source, false) {
            Some(note) => format!("{} · {note}", tr(WHAT)),
            None => tr(WHAT).to_string(),
        };
        self.row.subtitle.set_label(&line);
        self.row.reset.set_visible(rows::resettable(&source));
    }

    /// Shows why a change was not written, or takes an old message away.
    fn report(&self, result: Result<(), String>) {
        match result {
            Ok(()) => self.problem.set_visible(false),
            Err(e) => {
                self.problem.set_label(&e);
                self.problem.set_visible(true);
            }
        }
    }
}

pub fn page() -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Notifications"), tr(INTRO));
    let group = widgets::group(&content);
    let switch = gtk::Switch::new();
    let row = widgets::row(
        &group,
        rows::title(DO_NOT_DISTURB),
        &switch.clone().upcast::<gtk::Widget>(),
    );
    let choices = CHOICES.map(|c| ChoiceRow::new(&group, c)).into();
    let files = Files::here();
    let monitors = std::iter::once(&files.machine)
        .chain(files.person.as_ref())
        .filter_map(|path| {
            gio::File::for_path(path)
                .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                .ok()
        })
        .collect();
    let ui = Rc::new(Ui {
        switch,
        row,
        choices,
        problem,
        quiet: Cell::new(false),
        monitors,
    });
    ui.update();
    for monitor in &ui.monitors {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                ui.update();
            }
        });
    }
    let weak = Rc::downgrade(&ui);
    ui.switch.connect_active_notify(move |switch| {
        let Some(ui) = weak.upgrade() else { return };
        if !ui.quiet.get() {
            // Off is what applies without the key, so it is taken out.
            let wanted = switch.is_active().to_string();
            ui.report(Files::here().choose_over(DO_NOT_DISTURB, &wanted, Some("false")));
            ui.update();
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.row.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(Files::here().set(DO_NOT_DISTURB, None));
            ui.update();
        }
    });
    ui.row.copy.connect_clicked(move |button| {
        let value = Files::here().do_not_disturb().to_string();
        button
            .clipboard()
            .set_text(&rows::command(&[(DO_NOT_DISTURB, value)]));
        widgets::copied(button);
    });
    for (i, choice) in ui.choices.iter().enumerate() {
        let weak = Rc::downgrade(&ui);
        choice.choice.connect_changed(move || {
            let Some(ui) = weak.upgrade() else { return };
            if !ui.quiet.get() {
                // What applies without the key is taken out, not written.
                let c = &ui.choices[i];
                let value = rows::values(c.key)[c.choice.selected()];
                ui.report(Files::here().choose_over(c.key, value, Some(c.release)));
                ui.update();
            }
        });
        let weak = Rc::downgrade(&ui);
        choice.row.reset.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.report(Files::here().set(ui.choices[i].key, None));
                ui.update();
            }
        });
        let weak = Rc::downgrade(&ui);
        choice.row.copy.connect_clicked(move |button| {
            let Some(ui) = weak.upgrade() else { return };
            let c = &ui.choices[i];
            let value = c.value(&Files::here());
            button
                .clipboard()
                .set_text(&rows::command(&[(c.key, value)]));
            widgets::copied(button);
        });
    }
    // The page's state lives as long as the page does.
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_is_the_key_and_reads_as_the_command_does() {
        assert_eq!(rows::title(DO_NOT_DISTURB), "Do not disturb");
        assert_eq!(
            rows::command(&[(DO_NOT_DISTURB, "true".into())]),
            "edel settings set notifications.do_not_disturb=true"
        );
        assert_eq!(
            rows::title(NOTIFICATION_POSITION),
            "Where notifications appear"
        );
        assert_eq!(
            rows::command(&[(NOTIFICATION_POSITION, "top-right".into())]),
            "edel settings set notifications.position=top-right"
        );
        assert_eq!(rows::label("top-right"), "Top right");
        assert_eq!(rows::label("auto"), "Automatic");
        assert_eq!(rows::title(NOTIFICATION_BADGE), "Unread badge");
        assert_eq!(rows::label("count"), "Count");
        assert_eq!(rows::on_page("notifications").count(), 3);
    }

    #[test]
    fn each_choice_rows_release_value_is_one_of_its_keys_values() {
        for (key, _, release) in CHOICES {
            assert!(rows::values(key).contains(&release), "{key}: {release}");
            assert_eq!(
                rows::values(key)[0],
                release,
                "{key} lists its default first"
            );
        }
    }
}
