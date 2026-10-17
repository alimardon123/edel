//! A row that chooses one of a key's values (M5.9j, M5.9k, M7.8): a
//! `widgets::Choice` of the values, the value that applies without the key
//! taken out of the file rather than written, Reset and Copy as command,
//! and the line under the title that says where the value comes from. The
//! Notifications and Power pages build their choices from it, so the
//! wiring is written once (ADR-010). The values are the key table's by
//! default, or the page's own list when a key's values depend on the file
//! (the minutes the Lock screen after row offers).
//!
//! Nothing here runs on its own: the page calls [`ChoiceRow::update`] when
//! the files change and [`watch_files`] tells it when they do.

use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use edel::i18n::tr;

use crate::files::Files;
use crate::{rows, widgets};

/// One row choosing one of a key's values.
pub struct ChoiceRow {
    pub key: &'static str,
    what: &'static str,
    /// The value that applies without the key, which is taken out of the
    /// file rather than written.
    release: &'static str,
    /// The values the choice offers, in the order it lists them.
    values: Vec<String>,
    pub choice: Rc<widgets::Choice>,
    pub row: widgets::Row,
}

impl ChoiceRow {
    /// A row offering the key table's values, labelled as `rows.rs` labels
    /// them, appended to `group`.
    pub fn new(
        group: &gtk::Box,
        key: &'static str,
        what: &'static str,
        release: &'static str,
    ) -> ChoiceRow {
        let values = rows::values(key);
        let labels = values.iter().map(|v| rows::label(v)).collect();
        ChoiceRow::with_values(
            group,
            key,
            what,
            release,
            values.iter().map(|v| v.to_string()).collect(),
            labels,
        )
    }

    /// A row offering `values`, shown as `labels` (one each, in order),
    /// appended to `group`.
    pub fn with_values(
        group: &gtk::Box,
        key: &'static str,
        what: &'static str,
        release: &'static str,
        values: Vec<String>,
        labels: Vec<String>,
    ) -> ChoiceRow {
        let choice = widgets::Choice::new(labels);
        let row = widgets::row(group, rows::title(key), &choice.widget());
        ChoiceRow {
            key,
            what,
            release,
            values,
            choice,
            row,
        }
    }

    /// The value that applies now, the release's without a line.
    pub fn value(&self, files: &Files) -> String {
        files
            .chosen(self.key)
            .filter(|v| self.values.contains(v))
            .unwrap_or_else(|| self.release.to_string())
    }

    /// Shows what the files say, without calling back.
    pub fn update(&self, files: &Files) {
        let value = self.value(files);
        let at = self.values.iter().position(|v| *v == value).unwrap_or(0);
        self.choice.set_selected(at);
        let source = files.source(self.key);
        let line = match rows::note(&source, false) {
            Some(note) => format!("{} · {note}", tr(self.what)),
            None => tr(self.what).to_string(),
        };
        self.row.subtitle.set_label(&line);
        self.row.reset.set_visible(rows::resettable(&source));
    }

    /// Wires what a person does with the row: a pick writes its value
    /// (taken out when it is the release's), Reset takes the key out and
    /// Copy as command copies the line that sets it. `report` shows why a
    /// change was not written, and `after` runs once a change is made, to
    /// show the files again. Both should hold their page weakly, so that
    /// the row does not keep the page alive.
    pub fn connect(
        self: &Rc<Self>,
        report: impl Fn(Result<(), String>) + 'static,
        after: impl Fn() + 'static,
    ) {
        let report = Rc::new(report);
        let after = Rc::new(after);
        let weak = Rc::downgrade(self);
        let (report_pick, after_pick) = (report.clone(), after.clone());
        self.choice.connect_changed(move || {
            let Some(row) = weak.upgrade() else { return };
            // The choice offers `values` only, so the pick is one of them.
            let value = row.values[row.choice.selected()].clone();
            report_pick(Files::here().choose_over(row.key, &value, Some(row.release)));
            after_pick();
        });
        let weak = Rc::downgrade(self);
        let (report_reset, after_reset) = (report, after);
        self.row.reset.connect_clicked(move |_| {
            let Some(row) = weak.upgrade() else { return };
            report_reset(Files::here().set(row.key, None));
            after_reset();
        });
        let weak = Rc::downgrade(self);
        self.row.copy.connect_clicked(move |button| {
            let Some(row) = weak.upgrade() else { return };
            let value = row.value(&Files::here());
            button
                .clipboard()
                .set_text(&rows::command(&[(row.key, value)]));
            widgets::copied(button);
        });
    }
}

/// Shows why a change was not written, or takes an old message away.
pub fn report(problem: &gtk::Label, result: Result<(), String>) {
    match result {
        Ok(()) => problem.set_visible(false),
        Err(e) => {
            problem.set_label(&e);
            problem.set_visible(true);
        }
    }
}

/// The monitors of the machine's settings file and the person's, so a page
/// follows both while it is open. Keep them as long as the page.
pub fn watch_files(files: &Files) -> Vec<gio::FileMonitor> {
    std::iter::once(&files.machine)
        .chain(files.person.as_ref())
        .filter_map(|path| {
            gio::File::for_path(path)
                .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                .ok()
        })
        .collect()
}
