//! The Layout page's Panels group (M5.31d): a picture of the panels that
//! apply, a row whose button opens shell-ui's panel editor (the same editor
//! as a panel's right-click menu), and the section's Reset and Copy as
//! command, which follow `panels.list` in the person's file (ADR-008). The
//! editor writes the file itself, as it always has; this page only asks for
//! it, over the session bus (`edel::panel_edit::SHELL_BUS`), on a thread of
//! GIO's, so it never waits for the desktop.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

use edel::apps;
use edel::i18n::{tr, trf};
use edel::panel_edit::{self, EDIT_PANELS, PANELS, PINNED, SHELL_BUS, SHELL_INTERFACE, SHELL_PATH};
use edel::presets::Panel;
use edel::settings::Source;

use crate::files::Files;
use crate::style::Theme;
use crate::{preview, rows, widgets};

/// How long the bus may take to answer the call, in milliseconds.
const WAIT_MS: i32 = 2000;

/// The Panels group: its picture, its Edit panels button and the section's
/// Reset and Copy as command.
pub struct Card {
    /// The panels that apply, as the picture draws them.
    shown: Rc<RefCell<Vec<Panel>>>,
    picture: gtk::DrawingArea,
    /// Where `panels.list` comes from, with Reset and Copy as command.
    source: widgets::Source,
    edit: gtk::Button,
    /// The apps the apps widget pins, by name, and their row: where
    /// `panels.pinned` comes from, with Reset and Copy as command (M5.31d).
    pins: gtk::Label,
    pins_row: widgets::Row,
    problem: gtk::Label,
}

/// Appends the Panels section to `content`: its heading, then the group.
/// Call [`Card::show`] once the page is built and whenever a file changes.
pub fn card(content: &gtk::Box, theme: &Rc<Theme>, problem: &gtk::Label) -> Rc<Card> {
    let source = widgets::section(content, rows::title(PANELS));
    let group = widgets::group(content);
    widgets::note_row(&group, tr(rows::PANELS_WHAT));
    let shown = Rc::new(RefCell::new(Vec::new()));
    let picture = preview::panels_area(theme, shown.clone());
    picture.update_property(&[gtk::accessible::Property::Label(tr(
        "The panels as they apply now",
    ))]);
    let frame = gtk::Box::builder()
        .css_classes(["edel-row", "edel-preview-row"])
        .build();
    frame.append(&picture);
    group.append(&frame);
    let edit = widgets::action(tr("Edit panels"), true);
    let control = edit.clone().upcast::<gtk::Widget>();
    let row = widgets::row(
        &group,
        tr("Drag widgets, change a panel's style and size"),
        &control,
    );
    row.subtitle.set_label(tr("Or right-click a panel"));
    row.reset.set_visible(false);
    row.copy.set_visible(false);
    // The row names its control after the row; the button keeps its own
    // name, which is what a screen reader should read.
    edit.update_property(&[gtk::accessible::Property::Label(tr("Edit panels"))]);
    // The pinned apps: the drag on the panel changes them, so the row only
    // shows them (M5.31d).
    let pins = gtk::Label::builder()
        .xalign(1.0)
        .wrap(true)
        .css_classes(["edel-source"])
        .build();
    let pins_row = widgets::row(&group, rows::title(PINNED), &pins.clone().upcast());
    pins_row.subtitle.set_label(tr(
        "Drag an app's icon on the panel to pin, move or unpin it",
    ));
    let card = Rc::new(Card {
        shown,
        picture,
        source,
        edit,
        pins,
        pins_row,
        problem: problem.clone(),
    });
    card.wire();
    card.log_places();
    card
}

impl Card {
    /// The button that opens the panel editor, which the page gives the
    /// keyboard to when it was asked for by name (`--page panels.list`).
    pub fn edit(&self) -> gtk::Button {
        self.edit.clone()
    }

    /// Shows the panels that apply and where `panels.list` comes from.
    /// `preset` is the preset the desktop uses, which the panels come from
    /// when the person has none of their own.
    pub fn show(&self, files: &Files, preset: &str) {
        *self.shown.borrow_mut() = files.panels();
        self.picture.queue_draw();
        let source = files.source(PANELS);
        let title = edel::presets::title(preset);
        self.source
            .label
            .set_label(&rows::describe(&source, &title));
        self.source.reset.set_visible(rows::resettable(&source));
        self.show_pins(files);
    }

    /// Shows the apps pinned, by name, and where `panels.pinned` comes from;
    /// the row's line says what the drag does, then the source (M5.31d).
    fn show_pins(&self, files: &Files) {
        let names = names_of(&files.pins(), &apps::read_all(&apps::dirs()));
        let shown = if names.is_empty() {
            tr("None").to_string()
        } else {
            names.join(", ")
        };
        self.pins.set_label(&shown);
        let source = files.source(PINNED);
        let hint = tr("Drag an app's icon on the panel to pin, move or unpin it");
        let line = match rows::note(&source, true) {
            Some(note) => format!("{hint} · {note}"),
            None => hint.to_string(),
        };
        self.pins_row.subtitle.set_label(&line);
        self.pins_row.reset.set_visible(rows::resettable(&source));
    }

    /// Connects Reset, Copy as command and the Edit panels button, and the
    /// pinned apps' Reset and Copy as command.
    fn wire(&self) {
        let problem = self.problem.clone();
        self.source.reset.connect_clicked(move |_| {
            report(&problem, Files::here().set(PANELS, None));
        });
        let problem = self.problem.clone();
        self.pins_row
            .reset
            .connect_clicked(move |_| report(&problem, Files::here().set(PINNED, None)));
        self.pins_row.copy.connect_clicked(move |button| {
            button.clipboard().set_text(&copy_pins_command());
            widgets::copied(button);
        });
        let problem = self.problem.clone();
        self.source
            .copy
            .connect_clicked(move |button| match copy_command() {
                Ok(line) => {
                    button.clipboard().set_text(&line);
                    widgets::copied(button);
                }
                Err(e) => report(&problem, Err(e)),
            });
        let problem = self.problem.clone();
        self.edit.connect_clicked(move |_| {
            let problem = problem.clone();
            glib::spawn_future_local(async move {
                let asked = gio::spawn_blocking(ask_editor)
                    .await
                    .unwrap_or_else(|_| Err(tr("the call to the desktop stopped").to_string()));
                match asked {
                    Ok(()) => {
                        eprintln!("edel-settings: asked the panels to be edited");
                        report(&problem, Ok(()));
                    }
                    Err(why) => report(
                        &problem,
                        Err(trf(
                            "The desktop's panels are not running, so they cannot be edited now:\n{why}",
                            &[("why", &why)],
                        )),
                    ),
                }
            });
        });
    }

    /// Logs where the Edit panels button and Reset lie in the window, once
    /// the page is laid out, for CI to click (logical pixels). Asked for by
    /// name, the page first scrolls the group to the top of its view, as the
    /// group lies below the fold of a small window; the places are read the
    /// frame after that.
    fn log_places(&self) {
        let reset = self.source.reset.clone();
        let scrolled = Cell::new(false);
        self.edit.add_tick_callback(move |edit, _| {
            let Some(window) = edit.root().and_downcast::<gtk::Window>() else {
                return glib::ControlFlow::Continue;
            };
            if !edit.is_mapped() || edit.width() == 0 {
                return glib::ControlFlow::Continue;
            }
            if !scrolled.replace(true) {
                if widgets::asked_row() == Some(PANELS) {
                    widgets::scroll_to_top(reset.upcast_ref());
                }
                return glib::ControlFlow::Continue;
            }
            eprintln!(
                "edel-settings: panels group places edit {}, reset {}",
                widgets::place(edit.compute_bounds(&window)),
                widgets::place(reset.compute_bounds(&window)),
            );
            glib::ControlFlow::Break
        });
    }
}

/// Shows why a change was not written, or takes an old message away.
fn report(problem: &gtk::Label, result: Result<(), String>) {
    match result {
        Ok(()) => problem.set_visible(false),
        Err(e) => {
            problem.set_label(&e);
            problem.set_visible(true);
        }
    }
}

/// The command Copy as command gives: `edel settings reset` when the
/// person has a line of their own, else the `set` line for the panels that
/// apply now (ADR-008).
fn copy_command() -> Result<String, String> {
    let files = Files::here();
    match files.source(PANELS) {
        Source::Person(_) => Ok(format!("edel settings reset {PANELS}")),
        _ => {
            let value = panel_edit::value(&files.panels()).map_err(|e| format!("{e:#}"))?;
            Ok(rows::command(&[(PANELS, value)]))
        }
    }
}

/// The apps the pins name, as Settings shows them in the list's order: each
/// app's name when it is installed, else the pin as it is written (M5.31d).
pub fn names_of(pins: &[String], installed: &[apps::App]) -> Vec<String> {
    pins.iter()
        .map(|pin| apps::pinned(installed, pin).map_or_else(|| pin.clone(), |app| app.name.clone()))
        .collect()
}

/// The command Copy as command gives for the pinned apps: `edel settings
/// reset panels.pinned` when the person has a list of their own, else the
/// `set` line for the list that applies now (ADR-008).
fn copy_pins_command() -> String {
    let files = Files::here();
    match files.source(PINNED) {
        Source::Person(_) => format!("edel settings reset {PINNED}"),
        _ => rows::command(&[(PINNED, panel_edit::pins_value(&files.pins()))]),
    }
}

/// Asks shell-ui to open its panel editor over the session bus, as a
/// panel's right-click menu does. Blocking, so it runs on GIO's thread; with
/// no desktop it fails with the bus's own words.
fn ask_editor() -> Result<(), String> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    bus.call_sync(
        Some(SHELL_BUS),
        SHELL_PATH,
        SHELL_INTERFACE,
        EDIT_PANELS,
        None,
        None,
        gio::DBusCallFlags::NONE,
        WAIT_MS,
        gio::Cancellable::NONE,
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_apps_are_named_as_settings_knows_them() {
        let installed = vec![apps::parse(
            "[Desktop Entry]\nType=Application\nName=Foot\nExec=foot\nCategories=TerminalEmulator;\n",
        )
        .unwrap()];
        let pins = ["terminal".to_string(), "gone".to_string()];
        assert_eq!(names_of(&pins, &installed), ["Foot", "gone"]);
        assert!(names_of(&[], &installed).is_empty());
    }
}
