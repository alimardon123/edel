//! The Shortcuts page (roadmap M5.13b, ADR-008): one row per action of
//! `edel::shortcuts::ACTIONS` with its keys as keycaps, a Change button
//! that opens a small dialog taking the next chord, and Reset and Copy as
//! command as every row has. The dialog asks the compositor for the keys
//! (GTK's `inhibit_system_shortcuts`, which the compositor's keyboard
//! shortcuts inhibit answers), so Super+W reaches it and not the overview.
//! A chord another action holds is shown as a conflict with Use anyway,
//! which writes both lines in one write: the other action's cleared, this
//! action's chord set. Every write is the settings file's own line, as
//! `edel settings set` writes it (`Files::write_shortcuts`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};
use edel::shortcuts::{ACTIONS, Action, GROUPS};

use crate::files::Files;
use crate::{rows, widgets};

const INTRO: &str = n_("The keys for each action. Change one and press the keys you want.");

/// The title of the one closed row that holds the 18 numbered workspace
/// keys, the first action of which is `go_to_workspace_1`.
const NUMBERED: &str = n_("All 18 workspace keys");
const FIRST_NUMBERED: &str = "go_to_workspace_1";

/// What the dialog asks for, with the way out of it written beside the keys.
const ASK: &str =
    n_("Hold Super, Ctrl, Alt or Shift with a key. Escape cancels; Backspace clears the action.");

/// One action's row: its keycaps and Change button, and the line under
/// the row that shows a refused value.
struct Line {
    action: &'static Action,
    key: &'static str,
    row: widgets::Row,
    place: gtk::Box,
    change: gtk::Button,
    error: gtk::Label,
    /// The closed row this action sits in, for the numbered keys.
    fold: Option<gtk::Expander>,
}

struct Ui {
    lines: Vec<Line>,
    monitors: Vec<gtk::gio::FileMonitor>,
}

impl Ui {
    /// Shows what the files say: the resolved keys, the source line and
    /// whether Reset applies. Calls back into nothing.
    fn update(&self) {
        let files = Files::here();
        let (resolved, _) = edel::shortcuts::resolve(&files.shortcut_table());
        for line in &self.lines {
            let keys = resolved
                .iter()
                .find(|(a, _)| a.name == line.action.name)
                .and_then(|(_, keys)| keys.as_ref().map(ToString::to_string));
            show_keys(&line.place, keys.as_deref());
            let source = files.source(line.key);
            let what = tr(line.action.what);
            let line_text = match rows::note(&source, false) {
                Some(note) => format!("{what} · {note}"),
                None => what.to_string(),
            };
            line.row.subtitle.set_label(&line_text);
            line.row.reset.set_visible(rows::resettable(&source));
        }
    }

    /// Shows a refused value under its row, or hides the message when
    /// the write went through.
    fn report(&self, index: usize, result: Result<(), String>) {
        let line = &self.lines[index];
        match result {
            Ok(()) => line.error.set_visible(false),
            Err(e) => {
                line.error.set_label(&e);
                line.error.set_visible(true);
            }
        }
        self.update();
    }
}

/// The keycaps of `keys` in `place`, or the word None for an action that
/// has no keys, so a row never looks empty.
fn show_keys(place: &gtk::Box, keys: Option<&str>) {
    widgets::show_keys(place, keys);
    if keys.is_none() {
        let none = gtk::Label::builder()
            .label(tr("None"))
            .css_classes(["edel-source"])
            .build();
        place.append(&none);
        place.set_visible(true);
    }
}

pub fn page() -> gtk::Widget {
    let (page, content, _problem) = widgets::page(tr("Shortcuts"), tr(INTRO));
    let mut lines = Vec::new();
    for section in GROUPS {
        widgets::heading(&content, tr(section.title()));
        let group = widgets::group(&content);
        // The rows that go in the closed row, once it is made.
        let mut holder = group.clone();
        let mut fold = None;
        for action in ACTIONS.iter().filter(|a| a.group == section) {
            let Some(key) = rows::row_named(&format!("shortcuts.{}", action.name)) else {
                continue;
            };
            if action.name == FIRST_NUMBERED {
                let inside = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .css_classes(["edel-group"])
                    .build();
                fold = Some(numbered_fold(&group, &inside));
                holder = inside;
            }
            let place = gtk::Box::builder()
                .spacing(3)
                .valign(gtk::Align::Center)
                .visible(false)
                .build();
            let change = widgets::action(tr("Change"), false);
            let control = gtk::Box::builder().spacing(10).build();
            control.append(&place);
            control.append(&change);
            let row = widgets::row(&holder, rows::title(key), &control.upcast());
            let error = gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .visible(false)
                .css_classes(["edel-error"])
                .build();
            holder.append(&error);
            lines.push(Line {
                action,
                key,
                row,
                place,
                change,
                error,
                fold: fold.clone(),
            });
        }
    }
    let monitors = std::iter::once(&Files::here().machine)
        .chain(Files::here().person.as_ref())
        .filter_map(|path| {
            gtk::gio::File::for_path(path)
                .monitor_file(
                    gtk::gio::FileMonitorFlags::WATCH_MOVES,
                    gtk::gio::Cancellable::NONE,
                )
                .ok()
        })
        .collect();
    let ui = Rc::new(Ui { lines, monitors });
    ui.update();
    for monitor in &ui.monitors {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                ui.update();
            }
        });
    }
    for (index, line) in ui.lines.iter().enumerate() {
        let weak = Rc::downgrade(&ui);
        line.change.connect_clicked(move |button| {
            if let Some(ui) = weak.upgrade() {
                capture(button, Rc::downgrade(&ui), index);
            }
        });
        let weak = Rc::downgrade(&ui);
        let key = line.key;
        line.row.reset.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.report(index, Files::here().set(key, None));
            }
        });
        let name = line.action.name;
        line.row.copy.connect_clicked(move |button| {
            let keys = Files::here().shortcut(name).unwrap_or_default();
            button.clipboard().set_text(&rows::command(&[(key, keys)]));
            widgets::copied(button);
        });
    }
    log_places(&page, &ui);
    // The callbacks hold the page's state weakly; the page holds it.
    page.connect_destroy(move |_| {
        let _ = &ui;
    });
    page.upcast()
}

/// The closed row that holds the numbered workspace keys: its title and
/// arrow, with `inside` (the keys' own card) open under it. It sits in
/// the Workspaces card, after the rows before it.
fn numbered_fold(group: &gtk::Box, inside: &gtk::Box) -> gtk::Expander {
    let fold = gtk::Expander::builder()
        .label(tr(NUMBERED))
        .child(inside)
        .css_classes(["edel-row"])
        .build();
    group.append(&fold);
    fold
}

/// Logs where each Change button lies in the window, for CI to click, as
/// `edel-settings: shortcuts group places ACTION X+Y+WxH, ...` once two
/// frames agree (as the other groups do). Asked for by a row's key, the
/// page first scrolls that row to the top and gives the keyboard to its
/// Change button.
fn log_places(page: &gtk::Widget, ui: &Rc<Ui>) {
    let weak = Rc::downgrade(ui);
    let asked_done = Cell::new(false);
    let last = RefCell::new(String::new());
    let logged = RefCell::new(String::new());
    page.add_tick_callback(move |page, _| {
        let Some(window) = page.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        let Some(ui) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if !page.is_mapped() || page.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        if !asked_done.get() {
            // A key asked for inside the closed row opens that row first,
            // and waits until its Change button is shown to scroll to it.
            if let Some(line) = widgets::asked_row()
                .and_then(|asked| ui.lines.iter().find(|line| line.key == asked))
            {
                if let Some(fold) = &line.fold {
                    fold.set_expanded(true);
                }
                if !line.change.is_mapped() {
                    return glib::ControlFlow::Continue;
                }
                widgets::scroll_to_top(line.change.upcast_ref());
                widgets::take_asked(page, &line.change);
            }
            asked_done.set(true);
            return glib::ControlFlow::Continue;
        }
        // A Change button in a closed row is not mapped, so it has no place
        // yet and is left out.
        let places: Vec<String> = ui
            .lines
            .iter()
            .filter(|line| line.change.is_mapped())
            .map(|line| {
                format!(
                    "{} {}",
                    line.action.name,
                    widgets::place(line.change.compute_bounds(&window))
                )
            })
            .collect();
        let now = places.join(", ");
        let stable = *last.borrow() == now;
        *last.borrow_mut() = now.clone();
        if stable && *logged.borrow() != now {
            eprintln!("edel-settings: shortcuts group places {now}");
            *logged.borrow_mut() = now;
        }
        glib::ControlFlow::Continue
    });
}

/// The keys a chord of these modifiers and this one key is written as, as
/// `edel::shortcuts` reads them, or why not.
pub fn chord(logo: bool, ctrl: bool, alt: bool, shift: bool, key: &str) -> Result<String, String> {
    let mut parts: Vec<&str> = Vec::new();
    for (held, name) in [
        (logo, "Super"),
        (ctrl, "Ctrl"),
        (alt, "Alt"),
        (shift, "Shift"),
    ] {
        if held {
            parts.push(name);
        }
    }
    parts.push(key);
    let text = parts.join("+");
    match edel::shortcuts::parse(&text)? {
        Some(keys) => Ok(keys.to_string()),
        None => Err(text),
    }
}

/// A key's name as `edel::shortcuts` spells it: a letter or digit as
/// itself in capitals, anything else as GDK names it (`Return`, `F5`).
pub fn key_text(name: &str) -> String {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => c.to_ascii_uppercase().to_string(),
        _ => name.to_string(),
    }
}

/// Whether a key name is a modifier, which takes no chord of its own.
pub fn is_modifier(name: &str) -> bool {
    [
        "Shift_",
        "Control_",
        "Alt_",
        "Super_",
        "Meta_",
        "Hyper_",
        "ISO_Level",
        "Caps_Lock",
        "Num_Lock",
        "Scroll_Lock",
        "Mode_switch",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

/// A chord the dialog took and waits to write on the key's release.
#[derive(Clone)]
struct Pending {
    keycode: u32,
    chord: String,
}

/// A chord another action holds, waiting for Use anyway.
#[derive(Clone)]
struct Clash {
    chord: String,
    other: &'static Action,
}

/// The Change dialog for the action at `index`: it takes the next chord
/// with a modifier, shows it as keycaps and writes it on release, or
/// shows the conflict first. Escape cancels; Backspace alone clears.
fn capture(near: &gtk::Button, ui: Weak<Ui>, index: usize) {
    let Some(ui_now) = ui.upgrade() else { return };
    let action = ui_now.lines[index].action;
    let key = ui_now.lines[index].key;
    let window = gtk::Window::builder()
        .title(tr("Change shortcut"))
        .modal(true)
        .resizable(false)
        .default_width(380)
        .build();
    if let Some(parent) = near.root().and_downcast::<gtk::Window>() {
        window.set_transient_for(Some(&parent));
    }
    let prompt = gtk::Label::builder()
        .label(trf(
            "Press the new keys for {action}",
            &[("action", &rows::plain(key))],
        ))
        .xalign(0.0)
        .wrap(true)
        .build();
    let preview = gtk::Box::builder()
        .spacing(3)
        .halign(gtk::Align::Center)
        .build();
    let hint = gtk::Label::builder()
        .label(tr(ASK))
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-source"])
        .build();
    let status = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .css_classes(["edel-error"])
        .build();
    let cancel = widgets::action(tr("Cancel"), false);
    let use_anyway = widgets::action(tr("Use anyway"), true);
    use_anyway.set_visible(false);
    let buttons = gtk::Box::builder()
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    buttons.append(&cancel);
    buttons.append(&use_anyway);
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(20)
        .margin_end(20)
        .build();
    body.append(&prompt);
    body.append(&preview);
    body.append(&hint);
    body.append(&status);
    body.append(&buttons);
    window.set_child(Some(&body));
    window.update_property(&[gtk::accessible::Property::Label(&rows::plain(key))]);

    let pending: Rc<RefCell<Option<Pending>>> = Rc::new(RefCell::new(None));
    let clash: Rc<RefCell<Option<Clash>>> = Rc::new(RefCell::new(None));
    let held = Rc::new(RefCell::new(String::new()));

    let keys = gtk::EventControllerKey::new();
    {
        let (pending, clash, preview, status, window, held) = (
            pending.clone(),
            clash.clone(),
            preview.clone(),
            status.clone(),
            window.clone(),
            held.clone(),
        );
        keys.connect_key_pressed(move |_, keyval, keycode, state| {
            let Some(name) = keyval.name().map(|n| n.to_string()) else {
                return glib::Propagation::Stop;
            };
            let (logo, ctrl, alt, shift) = (
                state.contains(gdk::ModifierType::SUPER_MASK),
                state.contains(gdk::ModifierType::CONTROL_MASK),
                state.contains(gdk::ModifierType::ALT_MASK),
                state.contains(gdk::ModifierType::SHIFT_MASK),
            );
            if is_modifier(&name) {
                // Shows the modifiers held, as they are pressed.
                let mods: Vec<&str> = [
                    (logo, "Super"),
                    (ctrl, "Ctrl"),
                    (alt, "Alt"),
                    (shift, "Shift"),
                ]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, n)| *n)
                .collect();
                *held.borrow_mut() = mods.join("+");
                if pending.borrow().is_none() {
                    widgets::show_keys(&preview, Some(&held.borrow()));
                }
                return glib::Propagation::Stop;
            }
            let no_modifier = !(logo || ctrl || alt || shift);
            *clash.borrow_mut() = None;
            status.set_visible(false);
            if no_modifier && name == "Escape" {
                window.close();
                return glib::Propagation::Stop;
            }
            if no_modifier && name == "BackSpace" {
                *pending.borrow_mut() = Some(Pending {
                    keycode,
                    chord: String::new(),
                });
                widgets::show_keys(&preview, None);
                return glib::Propagation::Stop;
            }
            if no_modifier {
                *pending.borrow_mut() = None;
                widgets::show_keys(&preview, None);
                status.set_label(tr(ASK));
                status.set_visible(true);
                return glib::Propagation::Stop;
            }
            match chord(logo, ctrl, alt, shift, &key_text(&name)) {
                Ok(text) => {
                    widgets::show_keys(&preview, Some(&text));
                    *pending.borrow_mut() = Some(Pending {
                        keycode,
                        chord: text,
                    });
                }
                Err(why) => {
                    *pending.borrow_mut() = None;
                    widgets::show_keys(&preview, None);
                    status.set_label(&why);
                    status.set_visible(true);
                }
            }
            glib::Propagation::Stop
        });
    }
    {
        let (pending, window) = (pending.clone(), window.clone());
        let (ui_weak, preview) = (ui.clone(), preview.clone());
        let (status, clash, use_anyway) = (status.clone(), clash.clone(), use_anyway.clone());
        keys.connect_key_released(move |_, _keyval, keycode, _state| {
            let Some(next) = pending.borrow().clone().filter(|p| p.keycode == keycode) else {
                return;
            };
            *pending.borrow_mut() = None;
            let Some(ui) = ui_weak.upgrade() else { return };
            let files = Files::here();
            let table = files.shortcut_table();
            if !next.chord.is_empty() {
                let keys = edel::shortcuts::parse(&next.chord).ok().flatten();
                if let Some(other) =
                    keys.and_then(|keys| edel::shortcuts::clash(&table, action.name, &keys))
                {
                    widgets::show_keys(&preview, Some(&next.chord));
                    status.set_label(&trf(
                        "Already used by {action}",
                        &[("action", &rows::plain(&format!("shortcuts.{}", other.name)))],
                    ));
                    status.set_visible(true);
                    use_anyway.set_visible(true);
                    *clash.borrow_mut() = Some(Clash {
                        chord: next.chord.clone(),
                        other,
                    });
                    return;
                }
            }
            let line = files.shortcut_line(action.name, &next.chord);
            let edits = vec![(key.to_string(), line)];
            finish(&window, &ui, index, &edits);
        });
    }
    {
        let (clash, window) = (clash.clone(), window.clone());
        let ui_weak = ui.clone();
        use_anyway.connect_clicked(move |_| {
            let Some(conflict) = clash.borrow().clone() else {
                return;
            };
            let Some(ui) = ui_weak.upgrade() else { return };
            let files = Files::here();
            let other_key = format!("shortcuts.{}", conflict.other.name);
            let edits = vec![
                // The holder is cleared first, so the chord may move to it.
                (other_key, Some(String::new())),
                (
                    key.to_string(),
                    files.shortcut_line(action.name, &conflict.chord),
                ),
            ];
            finish(&window, &ui, index, &edits);
        });
    }
    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
    }
    window.add_controller(keys);
    window.connect_map(|window| {
        if let Some(toplevel) = window
            .surface()
            .and_then(|s| s.downcast::<gdk::Toplevel>().ok())
        {
            toplevel.inhibit_system_shortcuts(None::<gdk::Event>);
        }
    });
    window.connect_close_request(|window| {
        if let Some(toplevel) = window
            .surface()
            .and_then(|s| s.downcast::<gdk::Toplevel>().ok())
        {
            toplevel.restore_system_shortcuts();
        }
        glib::Propagation::Proceed
    });
    log_dialog_places(&window, &use_anyway, &cancel);
    window.present();
    prompt.grab_focus();
}

/// Writes `edits` in one write and logs each line, then closes the dialog;
/// a refused write is shown under the row instead.
fn finish(window: &gtk::Window, ui: &Rc<Ui>, index: usize, edits: &[(String, Option<String>)]) {
    for (key, value) in edits {
        let name = key.trim_start_matches("shortcuts.");
        match value {
            Some(value) if value.is_empty() => {
                eprintln!("edel-settings: shortcut {name} none written");
            }
            Some(value) => eprintln!("edel-settings: shortcut {name} {value} written"),
            None => eprintln!("edel-settings: shortcut {name} taken out"),
        }
    }
    let result = Files::here().write_shortcuts(edits);
    ui.report(index, result);
    window.close();
}

/// Logs where the dialog's Use anyway and Cancel lie once they are shown
/// and two frames agree, as `edel-settings: shortcut dialog places use
/// X+Y+WxH, cancel X+Y+WxH` (CI clicks them).
fn log_dialog_places(window: &gtk::Window, use_anyway: &gtk::Button, cancel: &gtk::Button) {
    let (use_anyway, cancel) = (use_anyway.clone(), cancel.clone());
    let last = RefCell::new(String::new());
    window.add_tick_callback(move |window, _| {
        if !use_anyway.is_visible() || !window.is_mapped() {
            return glib::ControlFlow::Continue;
        }
        let line = format!(
            "use {}, cancel {}",
            widgets::place(use_anyway.compute_bounds(window)),
            widgets::place(cancel.compute_bounds(window)),
        );
        let stable = *last.borrow() == line;
        if stable {
            eprintln!("edel-settings: shortcut dialog places {line}");
            return glib::ControlFlow::Break;
        }
        *last.borrow_mut() = line;
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_captured_chord_is_written_the_way_the_file_reads_it() {
        assert_eq!(chord(true, false, false, false, "W").unwrap(), "Super+W");
        assert_eq!(
            chord(true, true, true, true, "F5").unwrap(),
            "Super+Ctrl+Alt+Shift+F5"
        );
        assert_eq!(chord(false, true, true, false, "T").unwrap(), "Ctrl+Alt+T");
        assert!(chord(true, false, false, false, "Hyper").is_err());
    }

    #[test]
    fn key_names_read_as_edel_settings_spells_them() {
        assert_eq!(key_text("w"), "W");
        assert_eq!(key_text("1"), "1");
        assert_eq!(key_text("Page_Up"), "Page_Up");
        assert_eq!(key_text("Return"), "Return");
        assert!(is_modifier("Super_L") && is_modifier("Shift_R"));
        assert!(is_modifier("ISO_Level3_Shift"));
        assert!(!is_modifier("w") && !is_modifier("Return"));
    }

    #[test]
    fn every_action_has_a_row_to_search_and_change() {
        for action in ACTIONS {
            let key = format!("shortcuts.{}", action.name);
            assert_eq!(
                rows::row_named(&key),
                Some(rows::ROWS.iter().find(|r| r.key == key).unwrap().key),
                "{key} has its row"
            );
        }
        assert_eq!(
            rows::on_page("shortcuts").count(),
            ACTIONS.len(),
            "one row per action"
        );
    }
}
