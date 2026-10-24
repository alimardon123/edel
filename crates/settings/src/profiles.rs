//! The System page's Profile group (roadmap M7.6): one row, the machine's
//! `system.profiles`, as a choice of None and the release's profiles
//! (`edel::profiles`), each the apps and add-ons a kind of work needs. The
//! key is the machine's, for everyone, as the Location page's place is: a
//! change runs `edel settings set` or `reset` and then `apply` as root
//! through doas (`cmd::set_and_apply_as_root`), `edel`'s refusal shown in
//! the row's line. Only the profile's name is written, never its lines, so
//! a profile a release renews reaches the machine (ADR-008). A file that
//! names several profiles shows the first; choosing writes one.

use std::cell::Cell;
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{tr, trf};

use crate::files::Files;
use crate::widgets::CONTROL_WIDTH;
use crate::widgets::{Line, exit_text, monitors, reset_words, set_words};
use crate::{cmd, widgets};

/// The key the row writes.
const KEY: &str = "system.profiles";

/// The choice's entries: None (no name), then the release's profiles.
fn names() -> Vec<Option<&'static str>> {
    std::iter::once(None)
        .chain(edel::profiles::NAMES.iter().copied().map(Some))
        .collect()
}

/// An entry as people read it.
fn title(name: Option<&str>) -> String {
    name.map_or_else(|| tr("None").to_string(), edel::profiles::title)
}

/// The first profile the files name, as the machine applies it.
fn now(files: &Files) -> Option<String> {
    let value = files.chosen(KEY)?;
    let list: toml::Value = toml::from_str(&format!("v = {value}")).ok()?;
    list.get("v")?
        .as_array()?
        .first()?
        .as_str()
        .map(String::from)
}

/// The words of a change: `set` for a profile, `reset` for None.
fn words(name: Option<&str>) -> Vec<String> {
    match name {
        Some(name) => set_words(KEY, &format!("[\"{name}\"]")),
        None => reset_words(KEY),
    }
}

/// The line an administrator types for `words`, as Copy as command gives it.
fn root_line(words: &[String]) -> String {
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    format!("{} {}", edel::places::AS_ROOT, cmd::line(&args))
}

struct Ui {
    choice: Rc<widgets::Choice>,
    line: Line,
    /// What the chosen profile brings, a line under the row
    brings: gtk::Label,
    /// Set while the page itself moves the choice, which is no pick.
    quiet: Cell<bool>,
    /// Set while a change runs, so the choice waits.
    busy: Cell<bool>,
    /// Kept so the group follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

/// Shows the files' value now: the choice, its line and what it brings.
fn update(ui: &Rc<Ui>) {
    let files = Files::here();
    let chosen = now(&files);
    let at = names()
        .iter()
        .position(|n| n.map(String::from) == chosen)
        .unwrap_or(0);
    ui.quiet.set(true);
    ui.choice.set_selected(at);
    ui.quiet.set(false);
    ui.line.show(&files.source(KEY));
    // What the profile brings, then what the file leaves out of it (M7.6c):
    // "Steam, and game mode for every fullscreen game; without Steam".
    let without = edel::settings::read(&files.machine_text())
        .ok()
        .and_then(|read| edel::profiles::without_words(&read.file));
    let brings = chosen.as_deref().map(|name| match &without {
        Some(without) => trf(
            "{brings}; {without}",
            &[
                ("brings", &edel::profiles::about(name)),
                ("without", without),
            ],
        ),
        None => edel::profiles::about(name),
    });
    ui.brings.set_visible(brings.is_some());
    ui.brings.set_label(&brings.unwrap_or_default());
    ui.choice.widget().set_sensitive(!ui.busy.get());
}

/// Calls `update` once the callback that asked for it has returned.
fn later(ui: &Rc<Ui>) {
    let weak = Rc::downgrade(ui);
    glib::idle_add_local_once(move || {
        if let Some(ui) = weak.upgrade() {
            update(&ui);
        }
    });
}

/// Runs `edel` with `words` as root, then `edel settings apply` when that
/// worked, on a thread of its own; a refusal shows in the row's line.
fn change(ui: &Rc<Ui>, value: String, words: Vec<String>) {
    ui.busy.set(true);
    ui.choice.widget().set_sensitive(false);
    *ui.line.problem.borrow_mut() = String::new();
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let done = gio::spawn_blocking(move || cmd::set_and_apply_as_root(words)).await;
        let Some(ui) = weak.upgrade() else { return };
        let refused = match done {
            Ok((set, apply)) => {
                eprintln!(
                    "edel-settings: system profiles {value} asked, exit {}",
                    exit_text(set.code)
                );
                if set.ok {
                    apply.and_then(|run| (!run.ok).then(|| run.shown()))
                } else {
                    Some(set.shown())
                }
            }
            Err(_) => None,
        };
        *ui.line.problem.borrow_mut() = refused.unwrap_or_default();
        ui.busy.set(false);
        later(&ui);
    });
}

/// The Profile heading and its card, on `content`.
pub fn group(content: &gtk::Box) {
    widgets::heading(content, tr("What this computer is for"));
    let group = widgets::group(content);
    let entries = names();
    let choice = widgets::Choice::new(entries.iter().map(|n| title(*n)).collect());
    choice.set_width(CONTROL_WIDTH);
    let line = Line::new(&group, KEY, &choice.widget(), "", true);
    // What the chosen profile brings, a line of the card under the row, as
    // `widgets::text_row` draws one; hidden for None.
    let brings = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row", "edel-row-subtitle"])
        .build();
    group.append(&brings);
    let files = Files::here();
    let ui = Rc::new(Ui {
        choice: choice.clone(),
        line,
        brings,
        quiet: Cell::new(false),
        busy: Cell::new(false),
        monitors: monitors(&files),
    });
    update(&ui);
    for monitor in &ui.monitors {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                update(&ui);
            }
        });
    }
    let weak = Rc::downgrade(&ui);
    choice.connect_changed(move || {
        let Some(ui) = weak.upgrade() else { return };
        if ui.quiet.get() {
            return;
        }
        let name = names().get(ui.choice.selected()).copied().flatten();
        if name.map(String::from) == now(&Files::here()) {
            return;
        }
        change(&ui, name.unwrap_or("reset").to_string(), words(name));
    });
    let weak = Rc::downgrade(&ui);
    ui.line.row.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            change(&ui, "reset".into(), words(None));
        }
    });
    ui.line.row.copy.connect_clicked(|button| {
        let name = now(&Files::here());
        button
            .clipboard()
            .set_text(&root_line(&words(name.as_deref())));
        widgets::copied(button);
    });
    // The group's state lives as long as the card does: its callbacks hold
    // only weak references.
    let keep = ui.clone();
    group.connect_destroy(move |_| {
        let _ = &keep;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_is_none_then_every_profile() {
        let entries = names();
        assert_eq!(entries[0], None);
        assert_eq!(entries.len(), edel::profiles::NAMES.len() + 1);
        assert_eq!(title(Some("gaming")), "Gaming");
        assert_eq!(title(None), "None");
    }

    #[test]
    fn a_choice_writes_the_words_the_command_line_does() {
        assert_eq!(
            words(Some("office")),
            ["settings", "set", "system.profiles=[\"office\"]"]
        );
        assert_eq!(words(None), ["settings", "reset", "system.profiles"]);
        assert!(root_line(&words(None)).starts_with(edel::places::AS_ROOT));
    }
}
