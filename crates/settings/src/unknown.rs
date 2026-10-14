//! The System page's group of what this release did not understand (roadmap
//! M6.11, ADR-008): each line of the person's settings file and of this
//! machine's that `edel::settings::read` left out, a key this release does
//! not know or a value of the wrong kind, with Remove on the person's own
//! lines, so a file a newer release wrote is never kept wrong without a
//! word. `entries` is the pure part, tested without GTK; `group` follows
//! both files while the page lives. Settings writes only the person's file,
//! so the machine's lines get no Remove, only Copy as command, which gives
//! the administrator's `edel settings reset` line.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use edel::i18n::{tr, trf};
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::files::Files;
use crate::{actions, cmd, widgets};

/// One line a settings file holds that this release did not understand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The key as the file writes it, such as `appearance.acent`; empty when
    /// the whole file could not be read.
    pub key: String,
    /// Why, in `edel::settings`' own words.
    pub message: String,
    /// Whether it is in the person's own file (true) or the machine's (false).
    pub mine: bool,
}

/// What the person's file and the machine's file hold that this release did
/// not understand, the person's first. A file that is absent is None.
pub fn entries(person: Option<&str>, machine: Option<&str>) -> Vec<Entry> {
    let mut found = Vec::new();
    for (text, mine) in [(person, true), (machine, false)] {
        let Some(text) = text else { continue };
        match edel::settings::read(text) {
            Ok(read) => found.extend(read.problems.into_iter().map(|problem| Entry {
                key: problem.key,
                message: problem.message,
                mine,
            })),
            Err(why) => found.push(Entry {
                key: String::new(),
                message: format!("{why:#}"),
                mine,
            }),
        }
    }
    found
}

/// The group's state, kept by the group's own destroy closure. The widgets
/// are held weakly, so the page and this state never keep each other alive.
struct Ui {
    heading: glib::WeakRef<gtk::Box>,
    group: glib::WeakRef<gtk::Box>,
    /// Counts the builds, so a place logged for an earlier build is left out.
    generation: Rc<Cell<u64>>,
    /// Kept so the group follows both files while it lives.
    monitors: Vec<gio::FileMonitor>,
}

impl Ui {
    /// Builds the rows again from what the files say now, and logs for CI
    /// the keys the person's own lines name.
    fn rebuild(self: &Rc<Self>) {
        let (Some(heading), Some(group)) = (self.heading.upgrade(), self.group.upgrade()) else {
            return;
        };
        let built = self.generation.get() + 1;
        self.generation.set(built);
        while let Some(child) = group.first_child() {
            group.remove(&child);
        }
        let files = Files::here();
        let person = read_file(files.person.as_deref());
        let machine = read_file(Some(files.machine.as_path()));
        let found = entries(person.as_deref(), machine.as_deref());
        heading.set_visible(!found.is_empty());
        group.set_visible(!found.is_empty());

        let mut own: Vec<String> = Vec::new();
        let mut first_remove: Option<(String, gtk::Button)> = None;
        for entry in &found {
            let title = if entry.key.is_empty() {
                tr("The whole file").to_string()
            } else {
                entry.key.clone()
            };
            let remove =
                (entry.mine && !entry.key.is_empty()).then(|| widgets::action(tr("Remove"), false));
            let control: gtk::Widget = match &remove {
                Some(button) => button.clone().upcast(),
                None => gtk::Box::new(gtk::Orientation::Horizontal, 0).upcast(),
            };
            let row = widgets::row(&group, &title, &control);
            let where_ = if entry.mine {
                tr("In your settings file.")
            } else {
                tr("In this machine's settings file.")
            };
            row.subtitle.set_label(&trf(
                "{message} {where}",
                &[("message", &entry.message), ("where", where_)],
            ));
            row.reset.set_visible(false);
            if entry.key.is_empty() {
                row.copy.set_visible(false);
            } else {
                // The machine's line is the administrator's to reset.
                let args: Vec<&str> = actions::RESET
                    .args
                    .iter()
                    .copied()
                    .chain([entry.key.as_str()])
                    .collect();
                let line = cmd::line(&args);
                let text = if entry.mine {
                    line
                } else {
                    format!("{} {line}", edel::places::AS_ROOT)
                };
                row.copy.connect_clicked(move |button| {
                    button.clipboard().set_text(&text);
                    widgets::copied(button);
                });
            }
            if let Some(button) = remove {
                button.update_property(&[gtk::accessible::Property::Label(&trf(
                    "Remove {key}",
                    &[("key", &entry.key)],
                ))]);
                let weak = Rc::downgrade(self);
                let (key, subtitle) = (entry.key.clone(), row.subtitle.clone());
                button.connect_clicked(move |_| {
                    if let Some(ui) = weak.upgrade() {
                        ui.remove(&key, &subtitle);
                    }
                });
                own.push(entry.key.clone());
                if first_remove.is_none() {
                    first_remove = Some((entry.key.clone(), button));
                }
            }
        }

        let keys = if own.is_empty() {
            "nothing".to_string()
        } else {
            own.join(", ")
        };
        eprintln!("edel-settings: system not understood {keys}");
        if let Some((key, button)) = first_remove {
            log_places(&group, button, key, Rc::clone(&self.generation), built);
        }
    }

    /// Takes `key` out of the person's file, as `edel settings reset` does,
    /// then builds the list again; a refusal is shown in the row's line.
    fn remove(self: &Rc<Self>, key: &str, subtitle: &gtk::Label) {
        match Files::here().set(key, None) {
            Ok(()) => self.rebuild(),
            Err(why) => widgets::say(subtitle, &why, true),
        }
    }
}

/// The text of a file, None when it is absent or cannot be read.
fn read_file(path: Option<&Path>) -> Option<String> {
    path.and_then(|path| std::fs::read_to_string(path).ok())
}

/// The group under `content` (the System page's), its heading and the
/// rows of what this release did not understand. The heading and the group
/// are hidden when there is nothing to show.
pub fn group(content: &gtk::Box) {
    let heading = widgets::heading(content, tr("Not understood by this release"));
    let group = widgets::group(content);
    let files = Files::here();
    let monitors = std::iter::once(files.machine.as_path())
        .chain(files.person.as_deref())
        .filter_map(|path| {
            gio::File::for_path(path)
                .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                .ok()
        })
        .collect();
    let ui = Rc::new(Ui {
        heading: heading.downgrade(),
        group: group.downgrade(),
        generation: Rc::new(Cell::new(0)),
        monitors,
    });
    ui.rebuild();
    for monitor in &ui.monitors {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                ui.rebuild();
            }
        });
    }
    // The state lives as long as the group does.
    group.connect_destroy(move |_| {
        let _ = &ui;
    });
}

/// Logs where the first Remove button lies in the window, once the group is
/// laid out, for CI to click (logical pixels), as `edel-settings: system
/// group places remove KEY X+Y+WxH`. Logged once two frames agree, as the
/// other groups do; a build that came after this one leaves it out.
fn log_places(
    group: &gtk::Box,
    remove: gtk::Button,
    key: String,
    generation: Rc<Cell<u64>>,
    built: u64,
) {
    let last = RefCell::new(String::new());
    group.add_tick_callback(move |group, _| {
        if generation.get() != built {
            return glib::ControlFlow::Break;
        }
        let Some(window) = group.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !group.is_mapped() || group.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        let line = format!(
            "edel-settings: system group places remove {key} {}",
            widgets::place(remove.compute_bounds(&window)),
        );
        if last.replace(line.clone()) != line {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{line}");
        glib::ControlFlow::Break
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_key_in_the_persons_file_is_listed_as_theirs() {
        let found = entries(Some("format = 1\n[appearance]\nacent = \"red\"\n"), None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "appearance.acent");
        assert!(found[0].mine);
        assert_eq!(
            found[0].message,
            "unknown key; did you mean appearance.accent?"
        );
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_listed() {
        let found = entries(Some("format = 1\n[layout]\ntiling = \"yes\"\n"), None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "layout.tiling");
        assert!(found[0].mine);
        assert_eq!(found[0].message, "expected true or false, not \"yes\"");
    }

    #[test]
    fn a_file_that_is_not_toml_is_listed_with_no_key() {
        let found = entries(Some("format = [\n"), None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "");
        assert!(found[0].message.starts_with("the file is not valid TOML"));
    }

    #[test]
    fn the_persons_entries_come_before_the_machines() {
        let found = entries(
            Some("format = 1\n[appearance]\nacent = \"red\"\n"),
            Some("format = 1\n[layout]\ntiling = \"yes\"\n"),
        );
        let keys: Vec<(&str, bool)> = found.iter().map(|e| (e.key.as_str(), e.mine)).collect();
        assert_eq!(
            keys,
            vec![("appearance.acent", true), ("layout.tiling", false)]
        );
    }

    #[test]
    fn a_good_file_and_no_file_list_nothing() {
        assert!(entries(Some("format = 1\n"), None).is_empty());
        assert!(entries(None, None).is_empty());
    }
}
