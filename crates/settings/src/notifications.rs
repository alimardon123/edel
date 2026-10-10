//! The Notifications page (roadmap M5.9b): Do not disturb, the key
//! `notifications.do_not_disturb`, which keeps banners away while the
//! notifications still collect in the list the panel's clock opens, and
//! Where notifications appear, `notifications.position` (M5.9j). Each is
//! the same line of the person's settings file as `edel settings set`
//! (and, for the first, quick settings' tile and the centre's switch), so each shows
//! what the others chose: the page follows the files while it is open.
//! shell-ui reads the keys at each notification, so a change applies to
//! the next one. Nothing runs here but the file monitors.

use std::cell::Cell;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use edel::i18n::{n_, tr};
use edel::settings::{DO_NOT_DISTURB, NOTIFICATION_POSITION};

use crate::files::Files;
use crate::{rows, widgets};

const INTRO: &str = n_(
    "Banners appear when an app has something to say, and the clock keeps them all.",
);

/// What the row's line says under its title.
const WHAT: &str = n_("Banners stay away; notifications still collect in the list the clock opens");

/// What the place's row says under its title.
const PLACE_WHAT: &str =
    n_("Automatic puts them by the status area; a banner keeps clear of panels");

/// The place that applies without the key.
const AUTO: &str = "auto";

struct Ui {
    switch: gtk::Switch,
    row: widgets::Row,
    place: Rc<widgets::Choice>,
    place_row: widgets::Row,
    problem: gtk::Label,
    /// Set while the page itself moves the switch, so that is no choice.
    quiet: Cell<bool>,
    /// Kept so the page follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

impl Ui {
    /// Shows what the files say now: the switch, and where it comes from.
    fn update(&self) {
        let files = Files::here();
        self.quiet.set(true);
        self.switch.set_active(files.do_not_disturb());
        self.quiet.set(false);
        let source = files.source(DO_NOT_DISTURB);
        let line = match rows::note(&source, false) {
            Some(note) => format!("{} · {note}", tr(WHAT)),
            None => tr(WHAT).to_string(),
        };
        self.row.subtitle.set_label(&line);
        self.row.reset.set_visible(rows::resettable(&source));
        let values = rows::values(NOTIFICATION_POSITION);
        let chosen = files.chosen(NOTIFICATION_POSITION);
        let at = chosen
            .as_deref()
            .and_then(|v| values.iter().position(|w| *w == v))
            .unwrap_or(0);
        self.quiet.set(true);
        self.place.set_selected(at);
        self.quiet.set(false);
        let source = files.source(NOTIFICATION_POSITION);
        let line = match rows::note(&source, false) {
            Some(note) => format!("{} · {note}", tr(PLACE_WHAT)),
            None => tr(PLACE_WHAT).to_string(),
        };
        self.place_row.subtitle.set_label(&line);
        self.place_row.reset.set_visible(rows::resettable(&source));
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
    let values = rows::values(NOTIFICATION_POSITION);
    let place = widgets::Choice::new(values.iter().map(|v| rows::label(v)).collect());
    let place_row = widgets::row(&group, rows::title(NOTIFICATION_POSITION), &place.widget());
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
        place,
        place_row,
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
    let weak = Rc::downgrade(&ui);
    ui.place.connect_changed(move || {
        let Some(ui) = weak.upgrade() else { return };
        if !ui.quiet.get() {
            // Automatic is what applies without the key, so it is taken out.
            let value = rows::values(NOTIFICATION_POSITION)[ui.place.selected()];
            ui.report(Files::here().choose_over(NOTIFICATION_POSITION, value, Some(AUTO)));
            ui.update();
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.place_row.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(Files::here().set(NOTIFICATION_POSITION, None));
            ui.update();
        }
    });
    ui.place_row.copy.connect_clicked(move |button| {
        let value = Files::here()
            .chosen(NOTIFICATION_POSITION)
            .unwrap_or_else(|| AUTO.to_string());
        button
            .clipboard()
            .set_text(&rows::command(&[(NOTIFICATION_POSITION, value)]));
        widgets::copied(button);
    });
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
        assert_eq!(rows::label(AUTO), "Automatic");
        assert_eq!(rows::on_page("notifications").count(), 2);
    }
}
