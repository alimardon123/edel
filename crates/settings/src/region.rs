//! The Region page (roadmap M6.11, ADR-008): the language the desktop and
//! Settings are read in, the keyboard layouts, the input method, the time
//! zone and the servers the clock is set from. One row for each `[region]`
//! key, in that order; each row's line is the key's help from
//! `edel::key_help`, then where its value comes from, with Reset and Copy as
//! command.
//!
//! The language, the layouts and the input method are the person's: each
//! change is one line of the person's file, written with
//! `Files::choose_over`, so a value that applies anyway is taken out, never
//! written (ADR-008). The time zone and the time servers are the machine's,
//! which `edel settings apply` sets as root: a change runs `edel settings
//! set` and then `apply` through doas (`cmd::edel_as_root`), and `edel`
//! refuses it for anyone who is not an administrator, its message shown in
//! the row's line. The page follows the files while it is open and runs
//! nothing on a timer.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};
use edel::settings::{PAGES, Source};

use crate::files::{self, Files};
use crate::install::{self, LANGUAGE as ENGLISH};
use crate::{actions, cmd, rows, widgets};

/// The five keys, in the page's order.
const LANGUAGE: &str = "region.language";
const KEYBOARD: &str = "region.keyboard";
const INPUT: &str = "region.input_method";
const ZONE: &str = "region.timezone";
const SERVERS: &str = "region.time_servers";

/// The keyboard layout the release gives (`region.keyboard`).
const RELEASE_KEYBOARD: &str = "us";
/// The input method the release runs (`region.input_method`).
const RELEASE_INPUT: &str = "none";
/// The one server the clock is set from when no list is set.
const RELEASE_SERVER: &str = "pool.ntp.org";

/// The width of the controls at the right of a row, as Install draws them.
const CONTROL_WIDTH: i32 = 240;

/// Whether `key` is the machine's (the clock's), set as root, not the
/// person's file.
fn is_machine(key: &str) -> bool {
    key == ZONE || key == SERVERS
}

/// The layouts `value` names, in order: `de,us` is `de` then `us`.
fn layouts_of(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|layout| !layout.is_empty())
        .map(String::from)
        .collect()
}

/// `list` without the entry at `at`.
fn without(list: &[String], at: usize) -> Vec<String> {
    list.iter()
        .enumerate()
        .filter(|(i, _)| *i != at)
        .map(|(_, value)| value.clone())
        .collect()
}

/// The layouts xkb's list offers that `list` does not hold yet, each with
/// the name people read for it (its own name when the list gives none), in
/// the list's order.
fn offered_layouts(all: &[(String, String)], list: &[String]) -> Vec<(String, String)> {
    all.iter()
        .filter(|(name, _)| !list.contains(name))
        .map(|(name, text)| {
            let label = if text.is_empty() { name } else { text };
            (name.clone(), label.clone())
        })
        .collect()
}

/// A TOML list of texts, as `Files::chosen` gives an array (`["a", "b"]`);
/// none when it is not one.
fn list_of(text: &str) -> Option<Vec<String>> {
    let table: toml::Table = toml::from_str(&format!("list = {text}")).ok()?;
    table
        .get("list")?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(String::from))
        .collect()
}

/// The language choice's codes and labels: the languages the image has,
/// with the code the person's file names added last when the list lacks it
/// (Install does the same).
fn language_offer(now: &str) -> (Vec<String>, Vec<String>) {
    let mut codes: Vec<String> = edel::languages::LANGUAGES
        .iter()
        .map(|language| language.code.to_string())
        .collect();
    let mut labels: Vec<String> = edel::languages::LANGUAGES
        .iter()
        .map(edel::languages::label)
        .collect();
    if !codes.iter().any(|code| code == now) {
        codes.push(now.to_string());
        labels.push(now.to_string());
    }
    (codes, labels)
}

/// What the log says the page shows: the values that apply.
fn shown_words(language: &str, keyboard: &str, input: &str, zone: &str) -> String {
    format!("language {language}, keyboard {keyboard}, input method {input}, time zone {zone}")
}

/// The words of an `edel settings set KEY=VALUE` line for a machine key,
/// the Settings action's words first.
fn set_words(key: &str, value: &str) -> Vec<String> {
    actions::SET
        .args
        .iter()
        .map(|word| word.to_string())
        .chain(std::iter::once(format!("{key}={value}")))
        .collect()
}

/// The words of an `edel settings reset KEY` for a machine key.
fn reset_words(key: &str) -> Vec<String> {
    actions::RESET
        .args
        .iter()
        .map(|word| word.to_string())
        .chain(std::iter::once(key.to_string()))
        .collect()
}

/// The line an administrator types for a machine key: the doas line, as
/// Copy as command gives it (`AS_ROOT` names the tool, written nowhere else).
fn root_command(pairs: &[(&str, String)]) -> String {
    format!("{} {}", edel::places::AS_ROOT, rows::command(pairs))
}

/// An exit status as the log shows it; `none` when the command did not run.
fn exit_text(code: Option<i32>) -> String {
    code.map_or_else(|| "none".to_string(), |code| code.to_string())
}

fn language_now(files: &Files) -> String {
    files
        .chosen(LANGUAGE)
        .unwrap_or_else(|| ENGLISH.to_string())
}

fn layouts_now(files: &Files) -> Vec<String> {
    layouts_of(
        &files
            .chosen(KEYBOARD)
            .unwrap_or_else(|| RELEASE_KEYBOARD.to_string()),
    )
}

fn input_now(files: &Files) -> String {
    files
        .chosen(INPUT)
        .unwrap_or_else(|| RELEASE_INPUT.to_string())
}

fn zone_now(files: &Files) -> String {
    files
        .chosen(ZONE)
        .unwrap_or_else(|| edel::timezones::DEFAULT.to_string())
}

fn servers_now(files: &Files) -> Vec<String> {
    files
        .chosen(SERVERS)
        .and_then(|text| list_of(&text))
        .unwrap_or_else(|| vec![RELEASE_SERVER.to_string()])
}

/// The log's line of what applies now.
fn shown_now(files: &Files) -> String {
    shown_words(
        &language_now(files),
        &layouts_now(files).join(","),
        &input_now(files),
        &zone_now(files),
    )
}

/// The command Copy as command gives for `key`'s value now.
fn command_now(key: &str) -> String {
    let files = Files::here();
    match key {
        LANGUAGE => rows::command(&[(LANGUAGE, language_now(&files))]),
        KEYBOARD => rows::command(&[(KEYBOARD, layouts_now(&files).join(","))]),
        INPUT => rows::command(&[(INPUT, input_now(&files))]),
        ZONE => root_command(&[(ZONE, zone_now(&files))]),
        _ => root_command(&[(SERVERS, files::list_text(&servers_now(&files)))]),
    }
}

/// One row's parts: its row, the key's words and the note of its value, and
/// the message of a change the machine refused, which replaces the line
/// until the next change.
struct Line {
    row: widgets::Row,
    /// The key's help (`edel::key_help`), marked for translation.
    help: &'static str,
    /// A second sentence under the help, or nothing.
    extra: &'static str,
    /// Whether the key is the machine's, so its Reset shows when it is set
    /// anywhere but the release.
    machine: bool,
    problem: RefCell<String>,
}

impl Line {
    fn new(
        group: &gtk::Box,
        key: &str,
        control: &gtk::Widget,
        extra: &'static str,
        machine: bool,
    ) -> Line {
        Line {
            row: widgets::row(group, rows::title(key), control),
            help: edel::key_help::find(key).map_or("", |line| line.help),
            extra,
            machine,
            problem: RefCell::new(String::new()),
        }
    }

    /// The line's words: the help, the extra sentence, then where the value
    /// comes from (`Set by this machine`, `Your choice`, or nothing).
    fn words(&self, source: &Source) -> String {
        let mut text = [self.help, self.extra]
            .into_iter()
            .filter(|part| !part.is_empty())
            .map(tr)
            .collect::<Vec<&str>>()
            .join(" ");
        if let Some(note) = rows::note(source, false) {
            text = if text.is_empty() {
                note.to_string()
            } else {
                format!("{text} · {note}")
            };
        }
        text
    }

    /// Shows the line and whether Reset is there: the words, or a refused
    /// change's message in the problem colour.
    fn show(&self, source: &Source) {
        let problem = self.problem.borrow();
        if problem.is_empty() {
            self.row.subtitle.remove_css_class("edel-error");
            self.row.subtitle.set_label(&self.words(source));
        } else {
            self.row.subtitle.add_css_class("edel-error");
            self.row.subtitle.set_label(problem.as_str());
        }
        let reset = if self.machine {
            !matches!(source, Source::Release)
        } else {
            rows::resettable(source)
        };
        self.row.reset.set_visible(reset);
    }
}

struct Ui {
    /// The page's line for a person's change that is refused.
    problem: gtk::Label,
    /// The codes the language choice offers, in its order.
    languages: Vec<String>,
    language: Rc<widgets::Choice>,
    language_line: Line,
    /// Every layout xkb's list gives, as (name, words).
    layouts: Vec<(String, String)>,
    /// xkb's list of layouts, read once, when the machine has it.
    rules: Option<String>,
    keyboard_line: Line,
    /// The layouts' rows and the Add row, rebuilt as the list changes.
    keyboard_rows: gtk::Box,
    input: Rc<widgets::Segments>,
    input_line: Line,
    /// The zones the time zone choice offers, in its order.
    zones: Vec<String>,
    zone: Rc<widgets::Choice>,
    zone_line: Line,
    servers_line: Line,
    /// The servers' rows and the Add row, rebuilt as the list changes.
    servers_rows: gtk::Box,
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
            KEYBOARD => &self.keyboard_line,
            INPUT => &self.input_line,
            ZONE => &self.zone_line,
            SERVERS => &self.servers_line,
            _ => &self.language_line,
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
}

/// Shows the files' values now: each control, each row's line and the two
/// lists. Called after each change, once the callback that asked for it has
/// returned, and whenever a settings file changes.
fn update(ui: &Rc<Ui>) {
    let files = Files::here();
    ui.quiet.set(true);
    if let Some(at) = ui
        .languages
        .iter()
        .position(|code| *code == language_now(&files))
    {
        ui.language.set_selected(at);
    }
    if let Some(at) = rows::values(INPUT)
        .iter()
        .position(|value| *value == input_now(&files))
    {
        ui.input.set_selected(at);
    }
    if let Some(at) = ui.zones.iter().position(|zone| *zone == zone_now(&files)) {
        ui.zone.set_selected(at);
    }
    fill_keyboard(ui, &layouts_now(&files));
    fill_servers(ui, &servers_now(&files));
    ui.quiet.set(false);
    ui.language_line.show(&files.source(LANGUAGE));
    ui.keyboard_line.show(&files.source(KEYBOARD));
    ui.input_line.show(&files.source(INPUT));
    ui.zone_line.show(&files.source(ZONE));
    ui.servers_line.show(&files.source(SERVERS));
    let free = !ui.busy.get();
    ui.zone.widget().set_sensitive(free);
    ui.servers_rows.set_sensitive(free);
}

/// Calls `update` once the callback that asked for it has returned: the
/// rows it rebuilds are the ones answering the click.
fn later(ui: &Rc<Ui>) {
    let weak = Rc::downgrade(ui);
    glib::idle_add_local_once(move || {
        if let Some(ui) = weak.upgrade() {
            update(&ui);
        }
    });
}

fn clear(parent: &gtk::Box) {
    while let Some(child) = parent.first_child() {
        parent.remove(&child);
    }
}

fn list_row() -> gtk::Box {
    gtk::Box::builder()
        .spacing(10)
        .css_classes(["edel-row"])
        .build()
}

fn list_title(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .hexpand(true)
        .wrap(true)
        .css_classes(["edel-row-title"])
        .build()
}

/// Rebuilds the keyboard's rows: one per layout with Remove (insensitive
/// for the only one), then the Add a layout row.
fn fill_keyboard(ui: &Rc<Ui>, list: &[String]) {
    clear(&ui.keyboard_rows);
    for at in 0..list.len() {
        ui.keyboard_rows.append(&layout_row(ui, list, at));
    }
    ui.keyboard_rows.append(&add_layout_row(ui, list));
}

fn layout_row(ui: &Rc<Ui>, list: &[String], at: usize) -> gtk::Box {
    let name = &list[at];
    let label = ui
        .rules
        .as_deref()
        .and_then(|rules| edel::keyboard::description(rules, name))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| name.clone());
    let row = list_row();
    let remove = widgets::action(tr("Remove"), false);
    remove.update_property(&[gtk::accessible::Property::Label(&trf(
        "Remove {layout}",
        &[("layout", &label)],
    ))]);
    remove.set_sensitive(list.len() > 1);
    let rest = without(list, at);
    let weak = Rc::downgrade(ui);
    remove.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            write_layouts(&ui, rest.clone());
        }
    });
    row.append(&list_title(&label));
    row.append(&remove);
    row
}

/// The Add a layout row: a choice of the layouts not listed, and Add, which
/// writes the list with the chosen one at its end. Insensitive at the most
/// layouts Edel OS holds, `edel::keyboard::MOST_LAYOUTS`.
fn add_layout_row(ui: &Rc<Ui>, list: &[String]) -> gtk::Box {
    let row = list_row();
    let offered = offered_layouts(&ui.layouts, list);
    let labels: Vec<String> = offered.iter().map(|(_, text)| text.clone()).collect();
    let names: Vec<String> = offered.into_iter().map(|(name, _)| name).collect();
    let choice = widgets::Choice::searchable(labels);
    choice.set_width(CONTROL_WIDTH);
    let add = widgets::action(tr("Add"), true);
    let full = list.len() >= edel::keyboard::MOST_LAYOUTS;
    let open = !full && !names.is_empty();
    choice.widget().set_sensitive(open);
    add.set_sensitive(open);
    if full {
        add.set_tooltip_text(Some(&trf(
            "At most {most} layouts",
            &[("most", &edel::keyboard::MOST_LAYOUTS.to_string())],
        )));
    }
    row.append(&list_title(tr("Add a layout")));
    row.append(&choice.widget());
    row.append(&add);
    let weak = Rc::downgrade(ui);
    let list = list.to_vec();
    add.connect_clicked(move |_| {
        let (Some(ui), Some(name)) = (weak.upgrade(), names.get(choice.selected())) else {
            return;
        };
        let mut next = list.clone();
        next.push(name.clone());
        write_layouts(&ui, next);
    });
    row
}

/// Rebuilds the servers' rows: one per host with Remove, then Add a server.
fn fill_servers(ui: &Rc<Ui>, list: &[String]) {
    clear(&ui.servers_rows);
    for (at, host) in list.iter().enumerate() {
        ui.servers_rows.append(&server_row(ui, list, at, host));
    }
    ui.servers_rows.append(&add_server_row(ui, list));
}

fn server_row(ui: &Rc<Ui>, list: &[String], at: usize, host: &str) -> gtk::Box {
    let row = list_row();
    let remove = widgets::action(tr("Remove"), false);
    remove.update_property(&[gtk::accessible::Property::Label(&trf(
        "Remove {server}",
        &[("server", host)],
    ))]);
    let rest = without(list, at);
    let weak = Rc::downgrade(ui);
    remove.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            write_servers(&ui, rest.clone());
        }
    });
    row.append(&list_title(host));
    row.append(&remove);
    row
}

/// The Add a server row: an entry and Add, which writes the list with the
/// host typed at its end. Insensitive at the most host names a list holds.
fn add_server_row(ui: &Rc<Ui>, list: &[String]) -> gtk::Box {
    let row = list_row();
    let entry = gtk::Entry::builder()
        .placeholder_text(tr("such as pool.ntp.org"))
        .hexpand(true)
        .build();
    let add = widgets::action(tr("Add"), true);
    let full = list.len() >= edel::settings::MOST_HOSTS;
    entry.set_sensitive(!full);
    add.set_sensitive(!full);
    if full {
        add.set_tooltip_text(Some(&trf(
            "At most {most} host names",
            &[("most", &edel::settings::MOST_HOSTS.to_string())],
        )));
    }
    row.append(&list_title(tr("Add a server")));
    row.append(&entry);
    row.append(&add);
    let weak = Rc::downgrade(ui);
    let list = list.to_vec();
    add.connect_clicked(move |_| {
        let host = entry.text().trim().to_string();
        if host.is_empty() {
            return;
        }
        if let Some(ui) = weak.upgrade() {
            let mut next = list.clone();
            next.push(host);
            write_servers(&ui, next);
        }
    });
    row
}

/// Writes the person's whole layout list, checked against xkb's list when
/// the machine has it (`edel::keyboard::normalize`).
fn write_layouts(ui: &Rc<Ui>, next: Vec<String>) {
    match edel::keyboard::normalize(&next.join(","), ui.rules.as_deref()) {
        Ok(value) => write_person(ui, KEYBOARD, &value, RELEASE_KEYBOARD),
        Err(e) => {
            ui.report(Err(e));
            later(ui);
        }
    }
}

/// Sets the machine's server list, or takes it out when none is left, which
/// is the release's servers (`edel` refuses an empty list).
fn write_servers(ui: &Rc<Ui>, next: Vec<String>) {
    if next.is_empty() {
        change_machine(ui, SERVERS, reset_words(SERVERS));
    } else {
        change_machine(ui, SERVERS, set_words(SERVERS, &files::list_text(&next)));
    }
}

/// Chooses `value` for a person's key through `Files::choose_over`, which
/// takes it out when it is what applies anyway, and logs what it did.
fn write_person(ui: &Rc<Ui>, key: &'static str, value: &str, release: &str) {
    *ui.line(key).problem.borrow_mut() = String::new();
    match Files::here().choose_over(key, value, Some(release)) {
        Ok(()) => {
            let written = matches!(Files::here().source(key), Source::Person(_));
            eprintln!(
                "edel-settings: region {key} {value} {}",
                if written { "written" } else { "taken out" }
            );
            ui.report(Ok(()));
        }
        Err(e) => ui.report(Err(e)),
    }
    later(ui);
}

/// Takes the person's own value of `key` out (Reset): the file's line, or,
/// for a machine key the person has no line for, the machine's value,
/// reset as root.
fn reset(ui: &Rc<Ui>, key: &'static str) {
    let person = matches!(Files::here().source(key), Source::Person(_));
    if is_machine(key) && !person {
        change_machine(ui, key, reset_words(key));
        return;
    }
    *ui.line(key).problem.borrow_mut() = String::new();
    match Files::here().set(key, None) {
        Ok(()) => {
            eprintln!("edel-settings: region {key} taken out");
            ui.report(Ok(()));
        }
        Err(e) => ui.report(Err(e)),
    }
    later(ui);
}

/// Runs `edel` with `words` as root, then `edel settings apply` when that
/// worked, so the clock follows, on a thread of its own. The controls wait
/// meanwhile; a refusal is shown in the key's line, as `edel` said it.
fn change_machine(ui: &Rc<Ui>, key: &'static str, words: Vec<String>) {
    ui.busy.set(true);
    ui.zone.widget().set_sensitive(false);
    ui.servers_rows.set_sensitive(false);
    *ui.line(key).problem.borrow_mut() = String::new();
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let done = gio::spawn_blocking(move || {
            let args: Vec<&str> = words.iter().map(String::as_str).collect();
            let set = cmd::edel_as_root(&args, None);
            let apply = set.ok.then(|| cmd::edel_as_root(actions::APPLY.args, None));
            (set, apply)
        })
        .await;
        let Some(ui) = weak.upgrade() else { return };
        let Ok((set, apply)) = done else {
            ui.busy.set(false);
            later(&ui);
            return;
        };
        eprintln!(
            "edel-settings: region {key} asked, exit {}",
            exit_text(set.code)
        );
        let refused = if set.ok {
            apply.and_then(|run| {
                eprintln!(
                    "edel-settings: region apply asked, exit {}",
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

/// Logs what the page shows, once it is on screen and two frames agree, for
/// CI (`edel-settings: region page shows ...`).
fn log_shown(page: &gtk::Widget) {
    let last = RefCell::new(String::new());
    page.add_tick_callback(move |page, _| {
        if !page.is_mapped() {
            return glib::ControlFlow::Continue;
        }
        let text = shown_now(&Files::here());
        if last.replace(text.clone()) != text {
            return glib::ControlFlow::Continue;
        }
        eprintln!("edel-settings: region page shows {text}");
        glib::ControlFlow::Break
    });
}

/// The settings files this page follows: the machine's and the person's.
fn monitors(files: &Files) -> Vec<gio::FileMonitor> {
    std::iter::once(&files.machine)
        .chain(files.person.as_ref())
        .filter_map(|path| {
            gio::File::for_path(path)
                .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                .ok()
        })
        .collect()
}

/// The Region page (M6.11): the five `[region]` rows, as the module says.
pub fn page() -> gtk::Widget {
    let words = PAGES.iter().find(|p| p.section == "region");
    let (title, about) = words.map_or(("", ""), |p| (p.title, p.about));
    let (page, content, problem) = widgets::page(tr(title), tr(about));
    let group = widgets::group(&content);
    let files = Files::here();
    let rules = std::fs::read_to_string(edel::keyboard::RULES).ok();
    let layouts = rules
        .as_deref()
        .map(edel::keyboard::layouts)
        .unwrap_or_default();

    let (languages, labels) = language_offer(&language_now(&files));
    let language = widgets::Choice::searchable(labels);
    language.set_width(CONTROL_WIDTH);
    let language_line = Line::new(
        &group,
        LANGUAGE,
        &language.widget(),
        n_("Apps show the new language when they next open."),
        false,
    );

    // The keyboard's row has no control of its own: its layouts follow it.
    let empty = gtk::Box::new(gtk::Orientation::Horizontal, 0).upcast::<gtk::Widget>();
    let keyboard_line = Line::new(&group, KEYBOARD, &empty, "", false);
    let keyboard_rows = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    group.append(&keyboard_rows);

    let input_labels: Vec<String> = rows::values(INPUT)
        .iter()
        .map(|value| rows::label(value))
        .collect();
    let input = widgets::Segments::new(&input_labels);
    let input_line = Line::new(&group, INPUT, &input.widget(), "", false);

    let zones = install::zone_list(&zone_now(&files));
    let zone = widgets::Choice::searchable(
        zones
            .iter()
            .map(|zone| edel::timezones::label(zone))
            .collect(),
    );
    zone.set_width(CONTROL_WIDTH);
    let zone_line = Line::new(&group, ZONE, &zone.widget(), "", true);

    let servers_line = Line::new(&group, SERVERS, &empty, "", true);
    let servers_rows = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    group.append(&servers_rows);

    let ui = Rc::new(Ui {
        problem,
        languages,
        language: language.clone(),
        language_line,
        layouts,
        rules,
        keyboard_line,
        keyboard_rows,
        input: input.clone(),
        input_line,
        zones,
        zone: zone.clone(),
        zone_line,
        servers_line,
        servers_rows,
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
    ui.language.connect_changed(move || {
        let Some(ui) = weak.upgrade() else { return };
        if ui.quiet.get() {
            return;
        }
        if let Some(code) = ui.languages.get(ui.language.selected()).cloned() {
            write_person(&ui, LANGUAGE, &code, ENGLISH);
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.input.connect_changed(move || {
        let Some(ui) = weak.upgrade() else { return };
        if ui.quiet.get() {
            return;
        }
        if let Some(value) = rows::values(INPUT).get(ui.input.selected()) {
            write_person(&ui, INPUT, value, RELEASE_INPUT);
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.zone.connect_changed(move || {
        let Some(ui) = weak.upgrade() else { return };
        if ui.quiet.get() {
            return;
        }
        let Some(zone) = ui.zones.get(ui.zone.selected()) else {
            return;
        };
        if *zone != zone_now(&Files::here()) {
            change_machine(&ui, ZONE, set_words(ZONE, zone));
        }
    });

    for key in [LANGUAGE, KEYBOARD, INPUT, ZONE, SERVERS] {
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

    log_shown(&page);
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
    fn the_rows_are_the_five_keys_in_the_pages_order() {
        let keys: Vec<&str> = rows::on_page("region").map(|row| row.key).collect();
        assert_eq!(keys, [LANGUAGE, KEYBOARD, INPUT, ZONE, SERVERS]);
        assert_eq!(rows::title(LANGUAGE), "Language");
        assert_eq!(rows::title(KEYBOARD), "Keyboard layouts");
        assert_eq!(rows::title(INPUT), "Input method");
        assert_eq!(rows::title(ZONE), "Time zone");
        assert_eq!(rows::title(SERVERS), "Time servers");
    }

    #[test]
    fn the_person_keeps_three_keys_and_the_machine_two() {
        assert!(!is_machine(LANGUAGE) && !is_machine(KEYBOARD) && !is_machine(INPUT));
        assert!(is_machine(ZONE) && is_machine(SERVERS));
    }

    #[test]
    fn the_layout_list_reads_and_writes_as_the_key_takes_it() {
        assert_eq!(layouts_of("de,us"), ["de", "us"]);
        assert_eq!(layouts_of(" us , "), ["us"]);
        let list = layouts_of("de,us");
        assert_eq!(without(&list, 0), ["us"]);
        assert_eq!(without(&list, 1), ["de"]);
        let all = vec![
            ("de".to_string(), "German".to_string()),
            ("us".to_string(), String::new()),
            ("fr".to_string(), "French".to_string()),
        ];
        assert_eq!(
            offered_layouts(&all, &list),
            [("fr".to_string(), "French".to_string())]
        );
        assert_eq!(
            edel::keyboard::normalize(&list.join(","), None),
            Ok("de,us".to_string())
        );
    }

    #[test]
    fn the_servers_read_back_from_the_list_a_file_gives() {
        assert_eq!(
            list_of(r#"["pool.ntp.org", "b.example"]"#),
            Some(vec!["pool.ntp.org".to_string(), "b.example".to_string()])
        );
        assert_eq!(list_of("pool.ntp.org"), None);
        assert_eq!(list_of("[1]"), None);
    }

    #[test]
    fn the_language_choice_adds_a_code_the_list_lacks() {
        let (codes, labels) = language_offer("en_US.UTF-8");
        assert_eq!(codes.len(), edel::languages::LANGUAGES.len());
        assert_eq!(codes.len(), labels.len());
        let (codes, labels) = language_offer("xx_XX.UTF-8");
        assert_eq!(codes.last().map(String::as_str), Some("xx_XX.UTF-8"));
        assert_eq!(labels.last().map(String::as_str), Some("xx_XX.UTF-8"));
    }

    #[test]
    fn the_log_names_what_applies() {
        assert_eq!(
            shown_words("en_US.UTF-8", "us", "none", "UTC"),
            "language en_US.UTF-8, keyboard us, input method none, time zone UTC"
        );
        assert_eq!(exit_text(Some(3)), "3");
        assert_eq!(exit_text(None), "none");
    }

    #[test]
    fn a_machine_change_is_the_settings_command_and_apply_as_root() {
        assert_eq!(
            set_words(ZONE, "Europe/London"),
            ["settings", "set", "region.timezone=Europe/London"]
        );
        assert_eq!(
            reset_words(SERVERS),
            ["settings", "reset", "region.time_servers"]
        );
        assert_eq!(
            root_command(&[(ZONE, "Europe/London".to_string())]),
            format!(
                "{} edel settings set region.timezone=Europe/London",
                edel::places::AS_ROOT
            )
        );
        assert_eq!(actions::APPLY.args, ["settings", "apply"]);
    }

    #[test]
    fn the_release_values_are_the_key_table_defaults() {
        let quoted = |key: &str| {
            let default = edel::key_help::find(key).map_or("", |line| line.default);
            default.trim_matches('"').to_string()
        };
        assert_eq!(quoted(KEYBOARD), RELEASE_KEYBOARD);
        assert_eq!(quoted(INPUT), RELEASE_INPUT);
        assert_eq!(quoted(ZONE), edel::timezones::DEFAULT);
        assert_eq!(
            list_of(edel::key_help::find(SERVERS).map_or("", |line| line.default)),
            Some(vec![RELEASE_SERVER.to_string()])
        );
        assert!(rows::values(INPUT).contains(&RELEASE_INPUT));
    }
}
