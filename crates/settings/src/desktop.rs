//! The Appearance page's Desktop group (M8.21 part 21j1, M5.33c): one switch
//! for `appearance.widgets`, the clock and the date over the wallpaper and
//! under the windows. On writes the default arrangement,
//! `lock_screen::desktop_default`, through `widgets_value`; off takes the
//! person's line out, and writes `[]` only where the machine's file still
//! gives a list, since the release gives none. The switch follows the files,
//! its row has Reset and Copy as command, and each change logs one line for
//! CI. The editor that moves the widgets is 21j2.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;

use edel::i18n::{tr, trf};
use edel::lock_screen::{DESKTOP_WIDGETS, desktop_default, widgets_value};
use edel::settings::{self, Source};

use crate::files::Files;
use crate::{rows, widgets};

/// The line under the switch's title: what the desktop shows with it on.
const LINE: &str = "The clock and the date over the wallpaper, under the windows";

/// The Desktop group on the Appearance page.
pub struct Group {
    /// The group's source line, Reset and Copy as command.
    source: widgets::Source,
    /// The page's line for a problem.
    problem: gtk::Label,
    switch: gtk::Switch,
    row: widgets::Row,
    /// Set while `show` moves the switch, so that is no choice.
    quiet: Cell<bool>,
}

impl Group {
    /// Shows what the files say now: the switch, on when the desktop's list
    /// is not empty, the row's line with where its value comes from, and the
    /// group's source line.
    pub fn show(&self) {
        let files = Files::here();
        let (machine, person) = files.texts();
        let list = settings::desktop_widgets_in(machine.as_deref(), Some(person.as_str()));
        self.quiet.set(true);
        self.switch.set_active(!list.is_empty());
        self.quiet.set(false);
        let source = files.source(DESKTOP_WIDGETS);
        let mut text = vec![tr(LINE).to_string()];
        if let Some(note) = rows::note(&source, false) {
            text.push(note.to_string());
        }
        if !list.is_empty() && list != desktop_default() {
            text.push(tr("your own arrangement").to_string());
        }
        self.row.subtitle.set_label(&text.join(" · "));
        self.row.reset.set_visible(rows::resettable(&source));
        self.source
            .label
            .set_label(&match rows::note(&source, false) {
                Some(note) => note.to_string(),
                None => trf("Automatic ({current})", &[("current", tr("the release"))]),
            });
        self.source
            .reset
            .set_visible(matches!(source, Source::Person(_)));
    }

    /// The `edel settings set` line for the list the desktop shows now.
    fn command(&self) -> String {
        let files = Files::here();
        let (machine, person) = files.texts();
        let list = settings::desktop_widgets_in(machine.as_deref(), Some(person.as_str()));
        rows::command(&[(DESKTOP_WIDGETS, widgets_value(&list).unwrap_or_default())])
    }

    /// Shows a refused write in the problem line, or hides the line when the
    /// write went through, then shows the group again.
    fn report(&self, result: Result<(), String>) {
        match result {
            Ok(()) => self.problem.set_visible(false),
            Err(e) => {
                self.problem.set_label(&e);
                self.problem.set_visible(true);
            }
        }
        self.show();
    }
}

/// Turns the desktop's widgets on: the default arrangement is written.
fn turn_on(files: &Files) -> Result<(), String> {
    files.set(DESKTOP_WIDGETS, Some(&widgets_value(&desktop_default())?))
}

/// Turns the desktop's widgets off: the person's line goes, and `[]` is
/// written only where the machine's file still gives a list.
fn turn_off(files: &Files) -> Result<(), String> {
    let machine_lists = settings::read(&files.machine_text())
        .ok()
        .and_then(|read| read.file.appearance.widgets)
        .is_some();
    files.set(DESKTOP_WIDGETS, machine_lists.then_some("[]"))
}

/// Builds the Desktop group in `content`, after the Motion group: the section
/// with its source line, then one card with the switch's row.
pub fn group(content: &gtk::Box, problem: &gtk::Label) -> Rc<Group> {
    let source = widgets::section(content, tr("Desktop"));
    let card = widgets::group(content);
    let switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
    let row = widgets::row(
        &card,
        rows::title(DESKTOP_WIDGETS),
        &switch.clone().upcast::<gtk::Widget>(),
    );
    let group = Rc::new(Group {
        source,
        problem: problem.clone(),
        switch,
        row,
        quiet: Cell::new(false),
    });
    wire(&group);
    group.show();
    group
}

/// Connects the switch, the row's Reset and Copy as command and the group's.
fn wire(group: &Rc<Group>) {
    let weak = Rc::downgrade(group);
    group.switch.connect_active_notify(move |switch| {
        let Some(group) = weak.upgrade() else { return };
        if group.quiet.get() {
            return;
        }
        let on = switch.is_active();
        let result = if on {
            turn_on(&Files::here())
        } else {
            turn_off(&Files::here())
        };
        if result.is_ok() {
            eprintln!(
                "edel-settings: desktop widgets {}",
                if on { "on" } else { "off" }
            );
        }
        group.report(result);
    });
    let weak = Rc::downgrade(group);
    group.row.reset.connect_clicked(move |_| {
        if let Some(group) = weak.upgrade() {
            group.report(Files::here().set(DESKTOP_WIDGETS, None));
        }
    });
    let weak = Rc::downgrade(group);
    group.row.copy.connect_clicked(move |button| {
        let Some(group) = weak.upgrade() else { return };
        button.clipboard().set_text(&group.command());
        widgets::copied(button);
    });
    let weak = Rc::downgrade(group);
    group.source.reset.connect_clicked(move |_| {
        if let Some(group) = weak.upgrade() {
            group.report(Files::here().set(DESKTOP_WIDGETS, None));
        }
    });
    let weak = Rc::downgrade(group);
    group.source.copy.connect_clicked(move |button| {
        let Some(group) = weak.upgrade() else { return };
        button.clipboard().set_text(&group.command());
        widgets::copied(button);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default arrangement is written as the switch writes it and reads
    /// back equal, through the file's reader and the desktop's.
    #[test]
    fn the_default_arrangement_reads_back_as_it_is_written() {
        let value = widgets_value(&desktop_default()).unwrap();
        let text = format!("format = 1\n[appearance]\nwidgets = {value}\n");
        let read = settings::read(&text).unwrap();
        assert_eq!(read.file.appearance.widgets, Some(desktop_default()));
        assert_eq!(
            settings::desktop_widgets_in(None, Some(&text)),
            desktop_default()
        );
    }
}
