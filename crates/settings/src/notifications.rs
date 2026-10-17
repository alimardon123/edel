//! The Notifications page (roadmap M5.9b): Do not disturb, the key
//! `notifications.do_not_disturb`, which keeps banners away while the
//! notifications still collect in the list the panel's clock opens; Where
//! notifications appear, `notifications.position` (M5.9j); and Unread
//! badge, `notifications.badge`, what the notifications button shows
//! (M5.9k); and Notification list, `notifications.list`, where the list
//! opens, by its button or along a side, as three pictures (M5.9l). Each
//! is the same line of the person's settings file as `edel
//! settings set` (and Do not disturb as quick settings' tile and the
//! centre's switch), so each shows what the others chose: the page follows
//! the files while it is open. shell-ui follows them at once. Nothing runs
//! here but the file monitors.

use std::cell::Cell;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use edel::i18n::{n_, tr};
use edel::settings::{
    DO_NOT_DISTURB, NOTIFICATION_BADGE, NOTIFICATION_LIST, NOTIFICATION_POSITION,
};

use crate::choice_row::{self, ChoiceRow};
use crate::files::Files;
use crate::style::Theme;
use crate::{preview, rows, widgets};

const INTRO: &str = n_("Banners, do not disturb, and the list the clock keeps.");

/// What the row's line says under its title.
const WHAT: &str = n_("Banners stay away; notifications still collect in the list the clock opens");

/// What the Notification list row says under its title.
const LIST_WHAT: &str = n_("Where the list opens, from the clock or the bell");

/// The row that chooses where the notification list opens (M5.9l), as a
/// picture for each place. Each card is `(place, card, its check)`.
struct ListRow {
    cards: Vec<(&'static str, gtk::ToggleButton, gtk::Box)>,
    row: widgets::Row,
}

impl ListRow {
    fn new(group: &gtk::Box, theme: &Rc<Theme>) -> ListRow {
        let pictures = gtk::Box::builder()
            .spacing(10)
            .valign(gtk::Align::Center)
            .build();
        let mut cards: Vec<(&'static str, gtk::ToggleButton, gtk::Box)> = Vec::new();
        for place in rows::values(NOTIFICATION_LIST) {
            let picture = preview::list_area(theme, place);
            let (card, badge) = widgets::picture_card(&list_title(place), &picture.upcast());
            if let Some((_, first, _)) = cards.first() {
                card.set_group(Some(first));
            }
            pictures.append(&card);
            cards.push((place, card, badge));
        }
        let row = widgets::row(group, rows::title(NOTIFICATION_LIST), &pictures.upcast());
        ListRow { cards, row }
    }

    /// The place that applies now, the button's without a line.
    fn value(files: &Files) -> String {
        files
            .chosen(NOTIFICATION_LIST)
            .filter(|v| rows::values(NOTIFICATION_LIST).contains(&v.as_str()))
            .unwrap_or_else(|| "button".to_string())
    }

    /// Shows what the files say, without calling back.
    fn update(&self, files: &Files) {
        let value = ListRow::value(files);
        for (place, card, badge) in &self.cards {
            card.set_active(*place == value);
            badge.set_visible(*place == value);
        }
        let source = files.source(NOTIFICATION_LIST);
        let what = tr(LIST_WHAT);
        let line = match rows::note(&source, false) {
            Some(note) => format!("{what} · {note}"),
            None => what.to_string(),
        };
        self.row.subtitle.set_label(&line);
        self.row.reset.set_visible(rows::resettable(&source));
    }
}

/// The name of a place's picture card (M5.9l).
fn list_title(place: &str) -> String {
    let word = match place {
        "left" => n_("Left side"),
        "right" => n_("Right side"),
        _ => n_("By the button"),
    };
    tr(word).to_string()
}

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

struct Ui {
    switch: gtk::Switch,
    row: widgets::Row,
    choices: Vec<Rc<ChoiceRow>>,
    list: ListRow,
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
        self.list.update(&files);
        self.quiet.set(false);
        let source = files.source(DO_NOT_DISTURB);
        let line = match rows::note(&source, false) {
            Some(note) => format!("{} · {note}", tr(WHAT)),
            None => tr(WHAT).to_string(),
        };
        self.row.subtitle.set_label(&line);
        self.row.reset.set_visible(rows::resettable(&source));
    }

    fn report(&self, result: Result<(), String>) {
        choice_row::report(&self.problem, result);
    }
}

pub fn page(theme: &Rc<Theme>) -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Notifications"), tr(INTRO));
    let group = widgets::group(&content);
    let switch = gtk::Switch::new();
    let row = widgets::row(
        &group,
        rows::title(DO_NOT_DISTURB),
        &switch.clone().upcast::<gtk::Widget>(),
    );
    let choices = CHOICES
        .map(|(key, what, release)| Rc::new(ChoiceRow::new(&group, key, what, release)))
        .into();
    let list = ListRow::new(&group, theme);
    log_places(&page, &list);
    let files = Files::here();
    let monitors = choice_row::watch_files(&files);
    let ui = Rc::new(Ui {
        switch,
        row,
        choices,
        list,
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
    for (place, card, _) in &ui.list.cards {
        let weak = Rc::downgrade(&ui);
        let place = *place;
        card.connect_toggled(move |button| {
            // A card's check comes and goes with the one chosen; only the
            // one turned on is a choice.
            if !button.is_active() {
                return;
            }
            let Some(ui) = weak.upgrade() else { return };
            if !ui.quiet.get() {
                // The button's place is what applies without the key, so
                // it is taken out, not written.
                ui.report(Files::here().choose_over(NOTIFICATION_LIST, place, Some("button")));
                ui.update();
            }
        });
    }
    let weak = Rc::downgrade(&ui);
    ui.list.row.reset.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(Files::here().set(NOTIFICATION_LIST, None));
            ui.update();
        }
    });
    ui.list.row.copy.connect_clicked(move |button| {
        let value = ListRow::value(&Files::here());
        button
            .clipboard()
            .set_text(&rows::command(&[(NOTIFICATION_LIST, value)]));
        widgets::copied(button);
    });
    for choice in &ui.choices {
        let (weak_report, weak_after) = (Rc::downgrade(&ui), Rc::downgrade(&ui));
        choice.connect(
            move |result| {
                if let Some(ui) = weak_report.upgrade() {
                    ui.report(result);
                }
            },
            move || {
                if let Some(ui) = weak_after.upgrade() {
                    ui.update();
                }
            },
        );
    }
    // The page's state lives as long as the page does.
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
}

/// Logs where the notification list's three pictures and its Reset lie in
/// the window, once the page is laid out, for CI to click (logical pixels),
/// as `edel-settings: notification list group places button P, left P,
/// right P, reset R`. Logged once two frames agree, as `layout.rs` does.
fn log_places(page: &gtk::Widget, list: &ListRow) {
    let cards: Vec<(&'static str, gtk::ToggleButton)> = list
        .cards
        .iter()
        .map(|(place, card, _)| (*place, card.clone()))
        .collect();
    let reset = list.row.reset.clone();
    let last = std::cell::RefCell::new(String::new());
    page.add_tick_callback(move |page, _| {
        let Some(window) = page.root().and_downcast::<gtk::Window>() else {
            return gtk::glib::ControlFlow::Continue;
        };
        if !page.is_mapped() || page.width() == 0 {
            return gtk::glib::ControlFlow::Continue;
        }
        let places: Vec<String> = cards
            .iter()
            .map(|(place, card)| {
                format!("{place} {}", widgets::place(card.compute_bounds(&window)))
            })
            .collect();
        let line = format!(
            "edel-settings: notification list group places {}, reset {}",
            places.join(", "),
            widgets::place(reset.compute_bounds(&window)),
        );
        if last.replace(line.clone()) != line {
            return gtk::glib::ControlFlow::Continue;
        }
        eprintln!("{line}");
        gtk::glib::ControlFlow::Break
    });
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
        assert_eq!(rows::title(NOTIFICATION_LIST), "Notification list");
        assert_eq!(
            rows::command(&[(NOTIFICATION_LIST, "right".into())]),
            "edel settings set notifications.list=right"
        );
        assert_eq!(list_title("button"), "By the button");
        assert_eq!(list_title("left"), "Left side");
        assert_eq!(list_title("right"), "Right side");
        assert_eq!(rows::on_page("notifications").count(), 4);
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
