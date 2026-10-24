//! The Location page (roadmap M6.14c, ADR-008): where apps may find this
//! computer. One group, three rows, each one key of the settings file and
//! each row's line the key's help from `edel::key_help`, then where its
//! value comes from, with Reset and Copy as command, as `region.rs` does.
//!
//! Location is the person's switch (`location.enabled`, on when neither file
//! says otherwise): a change is written with `Files::choose_over`, so the
//! value that applies anyway is taken out, never written (ADR-008). Place
//! (`location.place`, a city named by its time zone, the first entry being
//! the time zone's own city) and Look online (`location.online`) are the
//! machine's, for everyone: a change runs `edel settings set` or `reset` and
//! then `apply` as root through doas (`cmd::set_and_apply_as_root`), `edel`'s
//! refusal shown in the row's line. Those two rows are not greyed while
//! Location is off, as they are the machine's. The page follows the settings
//! files while it is open and runs nothing on a timer.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};
use edel::settings::{LOCATION_ENABLED, PAGES, SettingsFile, Source};

use crate::files::Files;
use crate::widgets::CONTROL_WIDTH;
use crate::widgets::{Line, exit_text, monitors, reset_words, set_words};
use crate::{cmd, rows, widgets};

/// The three keys, in the page's order.
const ENABLED: &str = LOCATION_ENABLED;
const PLACE: &str = "location.place";
const ONLINE: &str = "location.online";

/// The time zone key the place's first entry reads (`region.timezone`).
const ZONE: &str = "region.timezone";

/// The city a time zone stands for, as people read it, or none: UTC names no
/// city, and a zone with no coordinates in tzdata names none either.
fn city_of(timezone: &str) -> Option<String> {
    let mut file = SettingsFile::default();
    file.region.timezone = Some(timezone.to_string());
    edel::location::zone_of(&file)
        .filter(|zone| edel::timezones::coordinates(zone).is_some())
        .map(edel::timezones::label)
}

/// The place choice's first entry: the time zone's city, which the key
/// being absent gives (`city` is none when the zone names no city).
fn first_words(city: Option<String>) -> String {
    let city = city.unwrap_or_else(|| tr("none").to_string());
    trf("Time zone's city ({city})", &[("city", &city)])
}

/// The words of the place's change: `edel settings set` for a city, or
/// `reset` for the first entry.
fn place_words(zone: Option<&str>) -> Vec<String> {
    match zone {
        Some(zone) => set_words(PLACE, zone),
        None => reset_words(PLACE),
    }
}

/// The words of Look online's change: set to true, or reset when off.
fn online_words(on: bool) -> Vec<String> {
    if on {
        set_words(ONLINE, "true")
    } else {
        reset_words(ONLINE)
    }
}

/// The line an administrator types for `words`: the doas line, as Copy as
/// command gives it (`AS_ROOT` names the tool, written nowhere else).
fn root_line(words: &[String]) -> String {
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    format!("{} {}", edel::places::AS_ROOT, cmd::line(&args))
}

/// The key's name in the log, without its `location.` prefix.
fn short(key: &str) -> &str {
    key.strip_prefix("location.").unwrap_or(key)
}

/// Whether apps may find this computer: the person's file over the
/// machine's, on when neither says.
fn enabled_now(files: &Files) -> bool {
    files.chosen(ENABLED).as_deref() != Some("false")
}

/// Whether the machine looks online for the place: set true, off when absent.
fn online_now(files: &Files) -> bool {
    files.chosen(ONLINE).as_deref() == Some("true")
}

/// The place choice's entry for the city `now` names: its position in
/// `zones` plus one, the first entry (0) when the file names none or a
/// city the list does not hold.
fn place_at(zones: &[String], now: Option<&str>) -> usize {
    now.and_then(|zone| zones.iter().position(|known| known == zone))
        .map_or(0, |at| at + 1)
}

/// The place's value now, as the desktop reads it; none is the first entry.
fn place_now(files: &Files) -> Option<String> {
    files.chosen(PLACE)
}

/// The command Copy as command gives for `key`'s value now.
fn command_now(key: &str) -> String {
    let files = Files::here();
    match key {
        PLACE => root_line(&place_words(place_now(&files).as_deref())),
        ONLINE => root_line(&online_words(online_now(&files))),
        _ => rows::command(&[(ENABLED, enabled_now(&files).to_string())]),
    }
}

struct Ui {
    /// The page's line for a person's change that is not written.
    problem: gtk::Label,
    switch: gtk::Switch,
    enabled_line: Line,
    /// The city choice: its first entry is the time zone's city, then the
    /// zones of `zones`, in their order.
    place: Rc<widgets::Choice>,
    /// The zones the place choice offers after its first entry.
    zones: Vec<String>,
    place_line: Line,
    online: gtk::Switch,
    online_line: Line,
    /// Set while the page itself moves a control, which is no pick.
    quiet: Cell<bool>,
    /// Set while a machine key's change runs, so its controls wait.
    busy: Cell<bool>,
    /// Kept so the page follows the files while it is open.
    monitors: Vec<gio::FileMonitor>,
}

impl Ui {
    /// The row of `key`; every key of this page has one.
    fn line(&self, key: &str) -> &Line {
        match key {
            PLACE => &self.place_line,
            ONLINE => &self.online_line,
            _ => &self.enabled_line,
        }
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

    /// The zone the place choice's entry `at` names; none for the first.
    fn zone_at(&self, at: usize) -> Option<&str> {
        at.checked_sub(1)
            .and_then(|at| self.zones.get(at))
            .map(String::as_str)
    }
}

/// Shows the files' values now: the switch, the city and the online switch,
/// each row's line and the controls the machine's changes hold off.
fn update(ui: &Rc<Ui>) {
    let files = Files::here();
    ui.quiet.set(true);
    ui.switch.set_active(enabled_now(&files));
    ui.place
        .set_selected(place_at(&ui.zones, place_now(&files).as_deref()));
    ui.online.set_active(online_now(&files));
    ui.quiet.set(false);
    ui.enabled_line.show(&files.source(ENABLED));
    ui.place_line.show(&files.source(PLACE));
    ui.online_line.show(&files.source(ONLINE));
    let free = !ui.busy.get();
    ui.place.widget().set_sensitive(free);
    ui.online.set_sensitive(free);
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

/// Chooses `on` for the person's switch through `Files::choose_over`, which
/// takes it out when it is what applies anyway, and logs what it did.
fn write_enabled(ui: &Rc<Ui>, on: bool) {
    let value = on.to_string();
    match Files::here().choose_over(ENABLED, &value, Some("true")) {
        Ok(()) => {
            let written = matches!(Files::here().source(ENABLED), Source::Person(_));
            eprintln!(
                "edel-settings: location enabled {value} {}",
                if written { "written" } else { "taken out" }
            );
            ui.report(Ok(()));
        }
        Err(e) => ui.report(Err(e)),
    }
    later(ui);
}

/// Takes the person's own value of `key` out (Reset): the file's line, or,
/// for a machine key the person has no line for, the machine's value, reset
/// as root.
fn reset(ui: &Rc<Ui>, key: &'static str) {
    let person = matches!(Files::here().source(key), Source::Person(_));
    if key != ENABLED && !person {
        change_machine(ui, key, "reset", reset_words(key));
        return;
    }
    match Files::here().set(key, None) {
        Ok(()) => {
            eprintln!("edel-settings: location {} taken out", short(key));
            ui.report(Ok(()));
        }
        Err(e) => ui.report(Err(e)),
    }
    later(ui);
}

/// Runs `edel` with `words` as root, then `edel settings apply` when that
/// worked, on a thread of its own. The controls wait meanwhile; a refusal is
/// shown in the key's line, as `edel` said it. `value` is what the log names.
fn change_machine(ui: &Rc<Ui>, key: &'static str, value: &str, words: Vec<String>) {
    ui.busy.set(true);
    ui.place.widget().set_sensitive(false);
    ui.online.set_sensitive(false);
    *ui.line(key).problem.borrow_mut() = String::new();
    let value = value.to_string();
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let done = gio::spawn_blocking(move || cmd::set_and_apply_as_root(words)).await;
        let Some(ui) = weak.upgrade() else { return };
        let Ok((set, apply)) = done else {
            ui.busy.set(false);
            later(&ui);
            return;
        };
        eprintln!(
            "edel-settings: location {} {value} asked, exit {}",
            short(key),
            exit_text(set.code)
        );
        let refused = if set.ok {
            apply.and_then(|run| {
                eprintln!(
                    "edel-settings: location apply asked, exit {}",
                    exit_text(run.code)
                );
                (!run.ok).then(|| run.shown())
            })
        } else {
            Some(set.shown())
        };
        *ui.line(key).problem.borrow_mut() = refused.unwrap_or_default();
        ui.busy.set(false);
        later(&ui);
    });
}

/// The line under the page's title, a sentence of its own, as other pages
/// have (the page table's words are the command's short summary).
const INTRO: &str = n_(
    "Where apps may find this computer. The place stays on this computer unless you let it look online.",
);

/// The Location page (M6.14c): the three `location.*` rows, as the module says.
pub fn page() -> gtk::Widget {
    let words = PAGES.iter().find(|p| p.section == "location");
    let title = words.map_or("", |p| p.title);
    let (page, content, problem) = widgets::page(tr(title), tr(INTRO));
    let group = widgets::group(&content);
    let files = Files::here();

    let switch = gtk::Switch::new();
    let enabled_line = Line::new(
        &group,
        ENABLED,
        &switch.clone().upcast::<gtk::Widget>(),
        n_("Also a tile in quick settings."),
        false,
    );

    // The city choice: the time zone's city first, then tzdata's zones. A
    // place the file names that the list lacks is added at its end.
    let timezone = files
        .chosen(ZONE)
        .unwrap_or_else(|| edel::timezones::DEFAULT.to_string());
    let mut zones = edel::timezones::list_in(Path::new(edel::places::ZONEINFO_DIR));
    if let Some(zone) = place_now(&files) {
        if !zones.contains(&zone) {
            zones.push(zone);
        }
    }
    let mut labels = vec![first_words(city_of(&timezone))];
    labels.extend(zones.iter().map(|zone| edel::timezones::label(zone)));
    let place = widgets::Choice::searchable(labels);
    place.set_width(CONTROL_WIDTH);
    let place_line = Line::new(&group, PLACE, &place.widget(), "", true);

    let online = gtk::Switch::new();
    let online_line = Line::new(
        &group,
        ONLINE,
        &online.clone().upcast::<gtk::Widget>(),
        "",
        true,
    );

    let ui = Rc::new(Ui {
        problem,
        switch: switch.clone(),
        enabled_line,
        place: place.clone(),
        zones,
        place_line,
        online: online.clone(),
        online_line,
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
    ui.switch.connect_active_notify(move |switch| {
        let Some(ui) = weak.upgrade() else { return };
        if !ui.quiet.get() {
            write_enabled(&ui, switch.is_active());
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.place.connect_changed(move || {
        let Some(ui) = weak.upgrade() else { return };
        if ui.quiet.get() {
            return;
        }
        let zone = ui.zone_at(ui.place.selected()).map(String::from);
        if zone.as_deref() == place_now(&Files::here()).as_deref() {
            return;
        }
        let value = zone.clone().unwrap_or_else(|| "reset".to_string());
        change_machine(&ui, PLACE, &value, place_words(zone.as_deref()));
    });
    let weak = Rc::downgrade(&ui);
    ui.online.connect_active_notify(move |switch| {
        let Some(ui) = weak.upgrade() else { return };
        if ui.quiet.get() {
            return;
        }
        let on = switch.is_active();
        if on == online_now(&Files::here()) {
            return;
        }
        let value = if on { "true" } else { "reset" };
        change_machine(&ui, ONLINE, value, online_words(on));
    });

    for key in [ENABLED, PLACE, ONLINE] {
        let line = ui.line(key);
        let weak = Rc::downgrade(&ui);
        line.row.reset.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                reset(&ui, key);
            }
        });
        line.row.copy.connect_clicked(move |button| {
            button.clipboard().set_text(&command_now(key));
            widgets::copied(button);
        });
    }

    // The page's state lives as long as the page does: its callbacks hold
    // only weak references.
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
    fn the_rows_are_the_three_keys_in_the_pages_order() {
        let keys: Vec<&str> = rows::on_page("location").map(|row| row.key).collect();
        assert_eq!(keys, [ENABLED, PLACE, ONLINE]);
        assert_eq!(rows::title(ENABLED), "Location");
        assert_eq!(rows::title(PLACE), "Place");
        assert_eq!(rows::title(ONLINE), "Look online");
    }

    #[test]
    fn the_first_entry_names_the_time_zones_city_or_none() {
        assert_eq!(
            first_words(Some(edel::timezones::label("Asia/Tashkent"))),
            "Time zone's city (Tashkent · Asia)"
        );
        assert_eq!(first_words(None), "Time zone's city (none)");
        assert_eq!(city_of(edel::timezones::DEFAULT), None);
    }

    #[test]
    fn the_choices_write_the_words_the_command_line_does() {
        assert_eq!(
            place_words(Some("Asia/Tashkent")),
            ["settings", "set", "location.place=Asia/Tashkent"]
        );
        assert_eq!(place_words(None), ["settings", "reset", "location.place"]);
        assert_eq!(
            online_words(true),
            ["settings", "set", "location.online=true"]
        );
        assert_eq!(
            online_words(false),
            ["settings", "reset", "location.online"]
        );
    }

    #[test]
    fn the_place_is_the_entry_the_file_names_or_the_first() {
        let zones = ["UTC".to_string(), "Asia/Tashkent".to_string()];
        assert_eq!(place_at(&zones, Some("Asia/Tashkent")), 2);
        assert_eq!(place_at(&zones, None), 0);
        assert_eq!(place_at(&zones, Some("Nowhere/Else")), 0);
    }

    #[test]
    fn copy_as_command_is_the_doas_line_for_the_machine_keys() {
        assert_eq!(
            root_line(&place_words(Some("Asia/Tashkent"))),
            format!(
                "{} edel settings set location.place=Asia/Tashkent",
                edel::places::AS_ROOT
            )
        );
        assert_eq!(
            root_line(&online_words(false)),
            format!(
                "{} edel settings reset location.online",
                edel::places::AS_ROOT
            )
        );
        assert_eq!(short(PLACE), "place");
    }
}
