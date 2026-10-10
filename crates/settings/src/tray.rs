//! The Layout page's Tray card (M5.9h, the fifth round's
//! `docs/mockups/shell/tray.jpg`): each app the tray knows, in the panel or
//! behind the arrow, the same choice dragging makes on the panel, written to
//! `layout.tray_in_panel` with the rule `edel::settings` holds
//! (`tray_list_with`, `tray_value`). The apps are the names in the key and
//! the items with an icon now, read from the session bus with GIO on a
//! thread of GIO's, so the page never waits for them; no bus or no watcher
//! lists only the names the key gives. Like the rest of the crate's pages,
//! every GTK call is here or in `widgets.rs` (ADR-004).

use std::cell::RefCell;
use std::cmp::Ordering;
use std::path::Path;
use std::rc::{Rc, Weak};

use gtk::gio;
use gtk::glib::{self, prelude::ToVariant};
use gtk::prelude::*;

use edel::i18n::{tr, trf};
use edel::settings::{self, TRAY_IN_PANEL};

use crate::files::Files;
use crate::widgets;

/// The watcher the tray's items register with, its path and the interface
/// its properties are read from.
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
/// An item's interface, whose `Id` and `Title` the card reads.
const ITEM: &str = "org.kde.StatusNotifierItem";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// How long one bus call may wait, in milliseconds.
const WAIT_MS: i32 = 2000;
/// The choice that keeps an app in the panel; the other is behind the arrow.
const IN_PANEL: usize = 0;

/// An app the card lists: `app` is what the key names (the item's `Id`,
/// else its title), `title` what people read (its title, else its `Id`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub app: String,
    pub title: String,
}

/// An id's bus name and object path: `:1.42/StatusNotifierItem` is
/// (`:1.42`, `/StatusNotifierItem`). The same split as shell-ui's, which
/// reads the same registrations.
pub fn parse_id(id: &str) -> Option<(&str, &str)> {
    let at = id.find('/')?;
    let (name, path) = id.split_at(at);
    (!name.is_empty() && path.len() > 1).then_some((name, path))
}

/// The apps in the card's order: the kept ones first, in the key's order,
/// then the rest by title (ignoring case), then by name for a tie.
pub fn order(mut apps: Vec<App>, kept: &[String]) -> Vec<App> {
    let rank = |app: &App| kept.iter().position(|k| *k == app.app);
    apps.sort_by(|a, b| match (rank(a), rank(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a
            .title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.app.cmp(&b.app)),
    });
    apps
}

/// The line that says what the card lists, as CI reads it:
/// `edel-settings: tray card lists A (in the panel), B (behind the arrow)`,
/// or `... lists no app`. Each entry is an app and whether it is in the panel.
pub fn listing(rows: &[(String, bool)]) -> String {
    if rows.is_empty() {
        return "edel-settings: tray card lists no app".to_string();
    }
    let parts: Vec<String> = rows
        .iter()
        .map(|(app, in_panel)| {
            let side = if *in_panel {
                "in the panel"
            } else {
                "behind the arrow"
            };
            format!("{app} ({side})")
        })
        .collect();
    format!("edel-settings: tray card lists {}", parts.join(", "))
}

/// The text of a file that is there, if it is.
fn read_text(path: Option<&Path>) -> Option<String> {
    path.and_then(|p| std::fs::read_to_string(p).ok())
}

/// One property of `interface` at `service` and `path`, the value inside
/// the reply; none when the bus does not answer.
fn property(
    bus: &gio::DBusConnection,
    service: &str,
    path: &str,
    interface: &str,
    name: &str,
) -> Option<glib::Variant> {
    let args = (interface, name).to_variant();
    let reply = bus
        .call_sync(
            Some(service),
            path,
            PROPERTIES,
            "Get",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            WAIT_MS,
            gio::Cancellable::NONE,
        )
        .ok()?;
    reply.child_value(0).as_variant()
}

/// A text property of a tray item, when it is not empty.
fn text_of(bus: &gio::DBusConnection, service: &str, path: &str, name: &str) -> Option<String> {
    property(bus, service, path, ITEM, name)?
        .get::<String>()
        .filter(|s| !s.is_empty())
}

/// The apps with a tray icon now, read from the session bus: the watcher's
/// registered items and each item's `Id` and `Title`. Blocking, so it runs
/// on GIO's thread; no bus, no watcher or no item gives what could be read.
pub fn read_apps() -> Vec<App> {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return Vec::new();
    };
    let items = property(
        &bus,
        WATCHER,
        WATCHER_PATH,
        WATCHER,
        "RegisteredStatusNotifierItems",
    )
    .and_then(|v| v.get::<Vec<String>>());
    let Some(items) = items else {
        return Vec::new();
    };
    let mut apps: Vec<App> = Vec::new();
    for id in items {
        let Some((service, path)) = parse_id(&id) else {
            continue;
        };
        let ident = text_of(&bus, service, path, "Id");
        let title = text_of(&bus, service, path, "Title");
        let Some(app) = ident.or_else(|| title.clone()) else {
            continue;
        };
        let title = title.unwrap_or_else(|| app.clone());
        if !apps.iter().any(|a| a.app == app) {
            apps.push(App { app, title });
        }
    }
    apps
}

/// One listed app's row, with the title it was built with.
struct Shown {
    app: String,
    title: String,
    choice: Rc<widgets::Choice>,
}

/// The card: its group under the heading, and what it shows.
pub struct Card {
    group: gtk::Box,
    problem: gtk::Label,
    /// The apps the tray shows now, as the bus last said.
    apps: RefCell<Vec<App>>,
    /// The key's list as the files say it now.
    kept: RefCell<Vec<String>>,
    /// The rows as they were built.
    shown: RefCell<Vec<Shown>>,
    /// The listing last logged, so a change is logged once.
    logged: RefCell<String>,
}

/// Appends the Tray card to `content`: its heading and group. Call
/// [`Card::show`] and then [`Card::refresh`] once the page is built.
pub fn card(content: &gtk::Box, problem: &gtk::Label) -> Rc<Card> {
    widgets::heading(content, tr("Tray"));
    let group = widgets::group(content);
    Rc::new(Card {
        group,
        problem: problem.clone(),
        apps: RefCell::new(Vec::new()),
        kept: RefCell::new(Vec::new()),
        shown: RefCell::new(Vec::new()),
        logged: RefCell::new(String::new()),
    })
}

impl Card {
    /// Reads the apps from the bus again, off the main thread, and shows
    /// them. Called when the page opens.
    pub fn refresh(self: &Rc<Self>) {
        let card = Rc::clone(self);
        glib::spawn_future_local(async move {
            if let Ok(apps) = gio::spawn_blocking(read_apps).await {
                *card.apps.borrow_mut() = apps;
                card.redraw();
            }
        });
    }

    /// Reads the key from the files and shows it. Called whenever a file
    /// changes, so a change by the panel's drag or `edel settings set`
    /// shows here too.
    pub fn show(self: &Rc<Self>) {
        let files = Files::here();
        let machine = read_text(Some(files.machine.as_path()));
        let person = read_text(files.person.as_deref());
        let kept = settings::texts(TRAY_IN_PANEL, machine.as_deref(), person.as_deref())
            .unwrap_or_default();
        *self.kept.borrow_mut() = kept;
        self.redraw();
    }

    /// The apps to list: the ones the tray shows now, then the ones the key
    /// names that the tray does not show (listed by their key name).
    fn wanted(&self) -> Vec<App> {
        let mut apps = self.apps.borrow().clone();
        for name in self.kept.borrow().iter() {
            if !apps.iter().any(|a| &a.app == name) {
                apps.push(App {
                    app: name.clone(),
                    title: name.clone(),
                });
            }
        }
        apps
    }

    /// Shows the apps in order. The rows are built again only when the
    /// apps or their titles change; otherwise their choices follow the key.
    fn redraw(self: &Rc<Self>) {
        let kept = self.kept.borrow().clone();
        let wanted = order(self.wanted(), &kept);
        let same = {
            let shown = self.shown.borrow();
            shown.len() == wanted.len()
                && shown
                    .iter()
                    .zip(&wanted)
                    .all(|(s, a)| s.app == a.app && s.title == a.title)
        };
        if same {
            for s in self.shown.borrow().iter() {
                s.choice.set_selected(side(&kept, &s.app));
            }
        } else {
            self.build(&wanted, &kept);
        }
        self.log();
    }

    /// Builds one row per app in `apps`, replacing the rows shown. Focus on
    /// an app's row moves to the same app's new row.
    fn build(self: &Rc<Self>, apps: &[App], kept: &[String]) {
        let focused = self
            .shown
            .borrow()
            .iter()
            .find(|s| s.choice.widget().has_focus())
            .map(|s| s.app.clone());
        while let Some(child) = self.group.first_child() {
            self.group.remove(&child);
        }
        self.shown.borrow_mut().clear();
        if apps.is_empty() {
            widgets::note_row(&self.group, tr("No app has put an icon in the tray yet"));
            return;
        }
        let mut shown = Vec::new();
        for app in apps {
            let labels = vec![
                tr("In the panel").to_string(),
                tr("Behind the arrow").to_string(),
            ];
            let choice = widgets::Choice::new(labels);
            choice.set_selected(side(kept, &app.app));
            let row = widgets::row(&self.group, &app.title, &choice.widget());
            if app.title != app.app {
                row.subtitle
                    .set_label(&trf("Its icon's name: {app}", &[("app", &app.app)]));
            } else {
                row.subtitle.set_visible(false);
            }
            // The key is the card's; no Reset or command per app.
            row.reset.set_visible(false);
            row.copy.set_visible(false);
            let (name, weak): (String, Weak<Card>) = (app.app.clone(), Rc::downgrade(self));
            choice.connect_changed(move || {
                if let Some(card) = weak.upgrade() {
                    card.choose(&name);
                }
            });
            shown.push(Shown {
                app: app.app.clone(),
                title: app.title.clone(),
                choice,
            });
        }
        *self.shown.borrow_mut() = shown;
        if let Some(app) = focused {
            if let Some(s) = self.shown.borrow().iter().find(|s| s.app == app) {
                s.choice.widget().grab_focus();
            }
        }
    }

    /// A choice a person made: `app` is kept in the panel or put behind the
    /// arrow, by the rule the panel's drag follows, and written to the
    /// person's file. The card follows the file when it changes.
    fn choose(&self, app: &str) {
        let keep = self
            .shown
            .borrow()
            .iter()
            .find(|s| s.app == app)
            .map(|s| s.choice.selected() == IN_PANEL);
        let Some(keep) = keep else {
            return;
        };
        let files = Files::here();
        let machine = read_text(Some(files.machine.as_path()));
        let person = read_text(files.person.as_deref());
        let current = settings::texts(TRAY_IN_PANEL, machine.as_deref(), person.as_deref())
            .unwrap_or_default();
        let list = settings::tray_list_with(&current, app, keep);
        let value = settings::tray_value(&list, machine.as_deref());
        match files.set(TRAY_IN_PANEL, value.as_deref()) {
            Ok(()) => self.problem.set_visible(false),
            Err(e) => {
                self.problem.set_label(&e);
                self.problem.set_visible(true);
            }
        }
    }

    /// Logs what the card lists, once for each change of it.
    fn log(&self) {
        let rows: Vec<(String, bool)> = self
            .shown
            .borrow()
            .iter()
            .map(|s| (s.app.clone(), s.choice.selected() == IN_PANEL))
            .collect();
        let line = listing(&rows);
        if *self.logged.borrow() != line {
            eprintln!("{line}");
            *self.logged.borrow_mut() = line;
        }
    }
}

/// The choice for `app`: in the panel when the key keeps it there.
fn side(kept: &[String], app: &str) -> usize {
    if kept.iter().any(|k| k == app) {
        IN_PANEL
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(app: &str, title: &str) -> App {
        App {
            app: app.to_string(),
            title: title.to_string(),
        }
    }

    #[test]
    fn a_registered_id_splits_into_its_bus_name_and_path() {
        assert_eq!(
            parse_id(":1.42/StatusNotifierItem"),
            Some((":1.42", "/StatusNotifierItem"))
        );
        assert_eq!(
            parse_id("org.example.App/Tray/1"),
            Some(("org.example.App", "/Tray/1"))
        );
        // A bus name alone has no path to ask, and an empty name neither.
        assert_eq!(parse_id("org.example.App"), None);
        assert_eq!(parse_id("/StatusNotifierItem"), None);
        assert_eq!(parse_id("org.example.App/"), None);
    }

    #[test]
    fn kept_apps_come_first_in_the_keys_order_then_the_rest_by_title() {
        let apps = vec![app("a", "Zed"), app("b", "alpha"), app("c", "Beta")];
        let kept = vec!["c".to_string(), "a".to_string()];
        let ranked: Vec<String> = order(apps, &kept).into_iter().map(|a| a.app).collect();
        assert_eq!(ranked, ["c", "a", "b"]);
        // With nothing kept, the titles decide, whatever their case.
        let apps = vec![app("a", "Zed"), app("b", "alpha"), app("c", "Beta")];
        let by_title: Vec<String> = order(apps, &[]).into_iter().map(|a| a.app).collect();
        assert_eq!(by_title, ["b", "c", "a"]);
    }

    #[test]
    fn the_log_line_names_each_app_and_its_side() {
        assert_eq!(
            listing(&[
                ("nm-applet".to_string(), true),
                ("edel-testclient".to_string(), false),
            ]),
            "edel-settings: tray card lists nm-applet (in the panel), edel-testclient (behind the arrow)"
        );
        assert_eq!(listing(&[]), "edel-settings: tray card lists no app");
    }

    #[test]
    fn a_kept_app_chooses_the_panel_and_any_other_the_arrow() {
        let kept = vec!["nm-applet".to_string()];
        assert_eq!(side(&kept, "nm-applet"), IN_PANEL);
        assert_eq!(side(&kept, "blueman"), 1);
    }
}
