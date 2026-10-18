//! The installer page (roadmap M6.6a): Edel OS from a stick onto this
//! computer's disk, shown only while the system runs from a stick
//! (`edel::install::from_a_stick`). It asks `edel::install::QUESTIONS` in
//! their order, the questions `edel install` asks on a terminal, each
//! checked by the same function (`language_answer`, `user_answer`,
//! `hostname_answer`), so the two never ask or refuse differently.
//!
//! The keyboard is applied as soon as it is a layout this machine knows, to
//! the person's own file on the stick, so the password is typed on the
//! keyboard it will be typed on later. The disks are offered by model and
//! size (`disk_choices`, read without root), and what `edel install --list`
//! says of them, their partitions included, is shown as it says it. Show
//! the plan writes a settings file of only the answers
//! (`Answers::settings_text`) and shows `edel install DISK --plan` word for
//! word; Erase and install, pressed twice, stands for typing the disk's
//! name, and only then runs `doas edel install DISK --settings FILE --yes
//! --new-password-stdin`, the password on standard input, never on a
//! command line or in the file (ADR-006, ADR-008).
//!
//! Return moves to the next question, and on the last one shows the plan
//! and gives the keyboard to Erase and install, so the whole page works
//! from the keyboard. Seam (ADR-008): the Welcome (M6.12) asks the same
//! list on a new machine, and Use a settings file (M6.6b) skips what a file
//! answers.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};

use edel::i18n::{tr, trf};
use edel::install::{self, Answers, DiskChoice};

use crate::files::Files;
use crate::{actions, cmd, icon, widgets};

/// The language suggested when the stick has none set.
pub(crate) const LANGUAGE: &str = "en_US.UTF-8";

/// How long Erase and install waits for its second press.
const ARMED: Duration = Duration::from_secs(5);

/// Whether Settings shows the page: only while it runs from a stick.
pub fn shown() -> bool {
    install::from_a_stick()
}

/// One question of the form, so a problem lands under its own row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Language,
    Keyboard,
    TimeZone,
    User,
    Password,
    Again,
    Disk,
    Hostname,
}

/// What the person typed and chose, as text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub language: String,
    pub keyboard: String,
    pub user: String,
    pub password: String,
    pub again: String,
    /// The chosen disk's name, none when there is no disk to offer
    pub disk: Option<String>,
    /// The time zone, as `edel::timezones::answer` takes it
    pub timezone: String,
    pub hostname: String,
    pub preset: String,
    /// The profile (M7.6), a name of `edel::profiles::NAMES` or
    /// `install::NO_PROFILE`
    pub profile: String,
    /// The question keys a settings file answers already, which are not
    /// asked (M6.6b)
    pub skip: Vec<&'static str>,
    /// Whether a password is asked: not when the file names several people
    pub password_needed: bool,
}

/// The question key each field asks, as `edel::install::QUESTIONS` names it.
fn key_of(field: Field) -> &'static str {
    match field {
        Field::Language => "region.language",
        Field::Keyboard => "region.keyboard",
        Field::TimeZone => "region.timezone",
        Field::User => "user",
        Field::Password | Field::Again => "password",
        Field::Disk => "disk",
        Field::Hostname => "network.hostname",
    }
}

/// The answers and the disk when every question is answered, else each
/// field's problem in the words the terminal uses. `rules` is the keyboard
/// list (`edel::keyboard::RULES`) when the machine has it.
pub fn check(form: &Form, rules: Option<&str>) -> Result<(Answers, String), Vec<(Field, String)>> {
    let password = if !form.password_needed {
        Ok(String::new())
    } else if form.password.is_empty() {
        Err(tr("a password cannot be empty").to_string())
    } else if form.password != form.again {
        Err(tr("the two passwords differ; type them again").to_string())
    } else {
        Ok(String::new())
    };
    let disk = form
        .disk
        .clone()
        .ok_or_else(|| tr("no disk to install on").to_string());
    let answers = [
        (
            Field::Language,
            install::language_answer(form.language.trim()),
        ),
        (
            Field::Keyboard,
            edel::keyboard::normalize(form.keyboard.trim(), rules),
        ),
        (Field::User, install::user_answer(form.user.trim())),
        (
            if form.password.is_empty() {
                Field::Password
            } else {
                Field::Again
            },
            password,
        ),
        (
            Field::TimeZone,
            edel::timezones::answer(form.timezone.trim()),
        ),
        (
            Field::Hostname,
            install::hostname_answer(form.hostname.trim()),
        ),
        (Field::Disk, disk),
    ]
    // A question the file answers is not asked, so it has no answer.
    .map(|(field, answer)| {
        if form.skip.contains(&key_of(field)) {
            (field, Ok(None))
        } else {
            (field, answer.map(Some))
        }
    });
    let problems: Vec<(Field, String)> = answers
        .iter()
        .filter_map(|(field, answer)| answer.clone().err().map(|why| (*field, why)))
        .collect();
    let [language, keyboard, user, _, timezone, hostname, disk] =
        answers.map(|(_, a)| a.ok().flatten());
    if !problems.is_empty() {
        return Err(problems);
    }
    let answers = Answers {
        language,
        keyboard,
        user,
        ssh_key: None,
        timezone,
        hostname,
        preset: (!form.skip.contains(&"layout.preset")).then(|| form.preset.clone()),
        profile: (!form.skip.contains(&"system.profiles")).then(|| form.profile.clone()),
    };
    Ok((answers, disk.unwrap_or_default()))
}

/// `edel install`'s arguments for the plan of `disk` with `file`, or for
/// the install itself, given the password on standard input when
/// `password` says one was asked.
pub fn install_args(disk: &str, file: &Path, plan: bool, password: bool) -> Vec<String> {
    let mut args: Vec<String> = actions::INSTALL
        .args
        .iter()
        .map(|word| word.to_string())
        .collect();
    args.extend([
        format!("/dev/{disk}"),
        "--settings".to_string(),
        file.display().to_string(),
    ]);
    if plan {
        args.push("--plan".into());
    } else {
        args.push("--yes".into());
        if password {
            args.push("--new-password-stdin".into());
        }
    }
    args
}

/// The part of `edel install --list`'s text about the disk `name`: its line
/// and its partitions' lines under it.
pub fn disk_block<'a>(list: &'a str, name: &str) -> Option<&'a str> {
    let head = format!("/dev/{name}: ");
    list.split("\n\n")
        .map(str::trim)
        .find(|block| block.starts_with(&head))
}

/// Where the answers are written: the person's runtime folder, which only
/// they and root read.
fn answers_file() -> PathBuf {
    glib::user_runtime_dir().join("edel-install.toml")
}

/// Writes `text` where only its owner reads it.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())
}

/// A settings file `edel install --find-settings` found on another disk
/// (M6.6b).
struct Found {
    text: String,
    /// Where it was, as `edel install` says it
    place: String,
    /// How many people it names
    people: usize,
    /// The question keys it answers
    answered: Vec<&'static str>,
}

/// The disk a found file is on, from `edel install`'s "found FILE on
/// /dev/sdb1"; the whole line when it reads otherwise (another language).
fn device_of(place: &str) -> &str {
    place
        .rsplit_once(" on ")
        .map_or(place, |(_, device)| device)
}

/// The words for what a file answers, by the rows' own titles.
fn answered_words(answered: &[&str]) -> String {
    answered
        .iter()
        .filter_map(|key| match *key {
            "region.language" => Some(tr("Language")),
            "region.keyboard" => Some(tr("Keyboard")),
            "user" => Some(tr("Who logs in")),
            "network.hostname" => Some(tr("Computer name")),
            "layout.preset" => Some(tr("Layout")),
            "system.profiles" => Some(tr("Profile")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A profile's name in the Profile row: None, or the profile's title.
fn profile_title(name: &str) -> String {
    if name == install::NO_PROFILE {
        tr("None").to_string()
    } else {
        edel::profiles::title(name)
    }
}

/// The Profile row's line: what the chosen profile brings, or what a
/// profile is while none is chosen.
fn profile_line(name: &str) -> String {
    if name == install::NO_PROFILE {
        tr("Apps and add-ons for one kind of work, if you like. Change it any time in Settings.")
            .to_string()
    } else {
        edel::profiles::about(name)
    }
}

/// What the hero above the form says: the welcome, an install under way,
/// one that finished, or one that did not finish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Welcome,
    Installing,
    Installed,
    Failed,
}

/// The page's controls and its state.
struct Ui {
    hero: widgets::Hero,
    stage: Cell<Stage>,
    problem: gtk::Label,
    /// The card that offers a settings file found on another disk
    file_card: gtk::Box,
    file_line: gtk::Label,
    use_file: gtk::Button,
    stop_file: gtk::Button,
    found: RefCell<Option<Found>>,
    /// Whether the found file is used, so only the rest is asked
    using: Cell<bool>,
    /// The Layout row's choice, hidden when the file sets the preset
    preset_widget: gtk::Widget,
    /// The Profile row's choice, hidden when the file names profiles
    profile_widget: gtk::Widget,
    language: Rc<widgets::Choice>,
    /// The language codes, by the language choice's index
    languages: Vec<String>,
    keyboard: Rc<widgets::Choice>,
    /// The keyboard layouts, by the keyboard choice's index
    keyboards: RefCell<Vec<String>>,
    /// The time zone choice, by the index of `zones`
    zone: Rc<widgets::Choice>,
    /// The zones the choice lists, in its order
    zones: Vec<String>,
    user: gtk::Entry,
    password: gtk::PasswordEntry,
    again: gtk::PasswordEntry,
    hostname: gtk::Entry,
    presets: Vec<&'static str>,
    preset: Rc<widgets::Choice>,
    /// None first, then the release's profiles
    profiles: Vec<&'static str>,
    profile: Rc<widgets::Choice>,
    /// The Profile row's line: what the chosen profile brings
    profile_line: gtk::Label,
    disks: Vec<DiskChoice>,
    disk: Option<Rc<widgets::Choice>>,
    /// The line under each row: its problem, else its own words
    lines: Vec<(Field, gtk::Label)>,
    /// The problems the last check found, by field
    problems: RefCell<Vec<(Field, String)>>,
    /// The review's values: the disk, the username, the language and the
    /// keyboard, as the answers stand
    review_disk: gtk::Label,
    review_user: gtk::Label,
    review_language: gtk::Label,
    review_keyboard: gtk::Label,
    review_zone: gtk::Label,
    /// What `edel install --list` says of the chosen disk
    list: gtk::Label,
    /// The run of `edel install --list`, once it is read
    listed: RefCell<Option<cmd::Outcome>>,
    plan: gtk::Label,
    show_plan: gtk::Button,
    erase: gtk::Button,
    restart: gtk::Button,
    /// The disk the plan shown is for; None while no plan is shown
    planned: RefCell<Option<String>>,
    armed: Cell<bool>,
    /// The keyboard suggested last, so a keyboard the person typed is kept
    suggested: RefCell<String>,
    /// The computer name suggested last
    suggested_name: RefCell<String>,
    /// The time zone suggested last, so a zone the person picked is kept
    suggested_zone: RefCell<String>,
    rules: Option<String>,
}

/// The welcome the hero shows until the person does something.
fn welcome(hero: &widgets::Hero) {
    hero.show(
        "page-install",
        false,
        false,
        tr("Welcome to Edel OS"),
        tr("A few questions, then a plan to check. Nothing changes until you press Erase and install."),
    );
}

/// One row of the review: its title, then its value, which `summarize`
/// writes. Returns the value's label.
fn review_row(grid: &gtk::Grid, at: i32, title: &str) -> gtk::Label {
    let name = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .valign(gtk::Align::Start)
        .css_classes(["edel-row-subtitle"])
        .build();
    let value = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .hexpand(true)
        .build();
    grid.attach(&name, 0, at, 1, 1);
    grid.attach(&value, 1, at, 1, 1);
    value
}

/// The keyboard layouts the choice offers, as their values and as their
/// names (the rules file's description, or the value without it): the
/// rules' layouts, the layout each language suggests where the list lacks it
/// (a variant such as `uz(latin)`), so the keyboard can follow every
/// language offered, and `suggested`, sorted by name. The Welcome (M6.12)
/// offers the same list.
pub(crate) fn keyboard_offer(rules: Option<&str>, suggested: &str) -> (Vec<String>, Vec<String>) {
    let mut layouts: Vec<(String, String)> = rules.map(edel::keyboard::layouts).unwrap_or_default();
    let wanted = std::iter::once(suggested.to_string()).chain(
        edel::languages::LANGUAGES
            .iter()
            .map(|l| first_layout(&install::suggested_keyboard(l.code)).to_string()),
    );
    for name in wanted {
        if !layouts.iter().any(|(listed, _)| *listed == name) {
            // Only a layout the list knows; without the list, the suggestion.
            let text = match rules {
                Some(r) => edel::keyboard::description(r, &name),
                None => Some(name.clone()),
            };
            if let Some(text) = text {
                layouts.push((name, text));
            }
        }
    }
    layouts.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let names: Vec<String> = layouts
        .iter()
        .map(|(name, text)| {
            if text.is_empty() {
                name.clone()
            } else {
                text.clone()
            }
        })
        .collect();
    let values: Vec<String> = layouts.into_iter().map(|(name, _)| name).collect();
    (values, names)
}

/// The first layout of a keyboard value, `ru` of `ru,us`.
pub(crate) fn first_layout(keyboard: &str) -> &str {
    keyboard.split(',').next().unwrap_or(keyboard)
}

/// The zones the time zone choice lists: tzdata's, the one `now` names and
/// each language's suggestion added when the list lacks them (so a machine
/// with no zone files still offers every language's zone), sorted by their
/// labels as the list is.
pub(crate) fn zone_list(now: &str) -> Vec<String> {
    let mut zones = edel::timezones::list_in(Path::new(edel::places::ZONEINFO_DIR));
    if zones.is_empty() {
        zones.push(edel::timezones::DEFAULT.to_string());
    }
    let suggested = edel::languages::LANGUAGES
        .iter()
        .map(|l| l.zone.to_string());
    for zone in std::iter::once(now.to_string()).chain(suggested) {
        if !zones.contains(&zone) {
            zones.push(zone);
        }
    }
    zones.sort_by_key(|zone| edel::timezones::label(zone));
    zones
}

/// The width of every control at the right of a row on this page.
const CONTROL_WIDTH: i32 = 240;

/// An entry of the form, its text filled in.
fn entry(text: &str) -> gtk::Entry {
    gtk::Entry::builder()
        .text(text)
        .width_request(CONTROL_WIDTH)
        .css_classes(["edel-entry"])
        .build()
}

/// A password entry with the eye that shows it.
fn password_entry() -> gtk::PasswordEntry {
    gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .width_request(CONTROL_WIDTH)
        .css_classes(["edel-entry"])
        .build()
}

pub fn page() -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Install Edel OS"), "");
    let files = Files::here();
    let language_now = files
        .person_file()
        .region
        .language
        .unwrap_or_else(|| LANGUAGE.to_string());
    let keyboard_now = install::suggested_keyboard(&language_now);
    let mut lines = Vec::new();

    // The welcome sits on the page itself; the install changes it as it goes.
    let hero = widgets::Hero::new(&content);
    hero.set_bare();
    welcome(&hero);

    // A settings file on another disk (M6.6b), shown once one is found, 20 px
    // below the welcome.
    let file_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .visible(false)
        .margin_top(20)
        .build();
    let file_group = widgets::group(&file_card);
    // One row, as a notice reads: what was found on the left, the choice
    // on the right.
    let file_row = gtk::Box::builder()
        .spacing(12)
        .css_classes(["edel-row"])
        .build();
    let file_icon = icon::image("copy", 20);
    file_icon.set_valign(gtk::Align::Center);
    file_row.append(&file_icon);
    let file_words = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    file_words.append(
        &gtk::Label::builder()
            .label(tr("Found a settings file"))
            .xalign(0.0)
            .wrap(true)
            .build(),
    );
    let file_line = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row-subtitle"])
        .build();
    file_words.append(&file_line);
    file_row.append(&file_words);
    let stop_file = widgets::action(tr("Answer every question"), false);
    let use_file = widgets::action(tr("Use it"), true);
    for button in [&stop_file, &use_file] {
        button.set_valign(gtk::Align::Center);
        file_row.append(button);
    }
    file_group.append(&file_row);
    content.append(&file_card);

    // The languages and keyboard layouts are the library's lists, so every
    // part shows the same names.
    widgets::step_heading(&content, 1, tr("Language and keyboard"));
    let choices = widgets::group(&content);
    let mut codes: Vec<String> = edel::languages::LANGUAGES
        .iter()
        .map(|language| language.code.to_string())
        .collect();
    let mut names: Vec<String> = edel::languages::LANGUAGES
        .iter()
        .map(edel::languages::label)
        .collect();
    // A language the person's file names that the list lacks is added last.
    if !codes.contains(&language_now) {
        codes.push(language_now.clone());
        names.push(language_now.clone());
    }
    let language_at = codes.iter().position(|c| *c == language_now).unwrap_or(0);
    let language = widgets::Choice::searchable(names);
    language.set_selected(language_at);
    language.set_width(CONTROL_WIDTH);
    let row = widgets::row(&choices, tr("Language"), &language.widget());
    row.reset.set_visible(false);
    row.copy.set_visible(false);
    lines.push((Field::Language, row.subtitle));

    let rules = std::fs::read_to_string(edel::keyboard::RULES).ok();
    let suggested = first_layout(&keyboard_now).to_string();
    let (keyboard_values, keyboard_names) = keyboard_offer(rules.as_deref(), &suggested);
    let keyboard_at = keyboard_values
        .iter()
        .position(|name| *name == suggested)
        .unwrap_or(0);
    let keyboard = widgets::Choice::searchable(keyboard_names);
    keyboard.set_selected(keyboard_at);
    keyboard.set_width(CONTROL_WIDTH);
    let row = widgets::row(&choices, tr("Keyboard"), &keyboard.widget());
    row.reset.set_visible(false);
    row.copy.set_visible(false);
    lines.push((Field::Keyboard, row.subtitle));

    // The time zone follows the language until the person picks one, as the
    // keyboard does. The zones are tzdata's; the file's own zone and each
    // language's suggestion are listed too, so the choice follows any of them.
    let zone_suggested = edel::timezones::suggested(&language_now).to_string();
    let zone_now = files
        .person_file()
        .region
        .timezone
        .unwrap_or_else(|| zone_suggested.clone());
    let zones = zone_list(&zone_now);
    let zone_at = zones.iter().position(|z| *z == zone_now).unwrap_or(0);
    let zone_names: Vec<String> = zones.iter().map(|z| edel::timezones::label(z)).collect();
    let zone = widgets::Choice::searchable(zone_names);
    zone.set_selected(zone_at);
    zone.set_width(CONTROL_WIDTH);
    let row = widgets::row(&choices, tr("Time zone"), &zone.widget());
    row.reset.set_visible(false);
    row.copy.set_visible(false);
    lines.push((Field::TimeZone, row.subtitle));

    widgets::step_heading(&content, 2, tr("Your account"));
    let you = widgets::group(&content);
    let user = entry("");
    let password = password_entry();
    let again = password_entry();
    for (field, title, control) in [
        (
            Field::User,
            tr("Your username"),
            user.clone().upcast::<gtk::Widget>(),
        ),
        (
            Field::Password,
            tr("Your password"),
            password.clone().upcast(),
        ),
        (
            Field::Again,
            tr("The same password again"),
            again.clone().upcast(),
        ),
    ] {
        let row = widgets::row(&you, title, &control);
        row.reset.set_visible(false);
        row.copy.set_visible(false);
        lines.push((field, row.subtitle));
    }

    widgets::step_heading(&content, 3, tr("Where to install"));
    let disk_group = widgets::group(&content);
    let disks = install::disk_choices(Path::new("/sys"), running().as_deref());
    let disk = if disks.is_empty() {
        widgets::note_row(
            &disk_group,
            tr("No other disk was found. Plug one in, then open this page again."),
        );
        None
    } else {
        let choice = widgets::Choice::new(disks.iter().map(|d| d.title.clone()).collect());
        choice.set_width(CONTROL_WIDTH);
        let row = widgets::row(&disk_group, tr("Disk"), &choice.widget());
        row.reset.set_visible(false);
        row.copy.set_visible(false);
        lines.push((Field::Disk, row.subtitle));
        Some(choice)
    };
    let list = widgets::output_row(&disk_group);

    let more = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let more_group = widgets::group(&more);
    let hostname = entry("");
    let row = widgets::row(&more_group, tr("Computer name"), &hostname.clone().upcast());
    row.reset.set_visible(false);
    row.copy.set_visible(false);
    lines.push((Field::Hostname, row.subtitle));
    let presets: Vec<&'static str> = edel::settings::PRESETS.to_vec();
    let preset = widgets::Choice::new(presets.iter().map(|p| edel::presets::title(p)).collect());
    preset.set_width(CONTROL_WIDTH);
    let row = widgets::row(&more_group, tr("Layout"), &preset.widget());
    row.reset.set_visible(false);
    row.copy.set_visible(false);
    row.subtitle.set_label(tr(
        "How windows and panels are laid out. Change it any time in Settings.",
    ));
    // One optional profile (M7.6): None, then the release's, each the apps
    // and add-ons a kind of work needs; the file gets only its name.
    let profiles: Vec<&'static str> = std::iter::once(install::NO_PROFILE)
        .chain(edel::profiles::NAMES.iter().copied())
        .collect();
    let profile = widgets::Choice::new(profiles.iter().map(|p| profile_title(p)).collect());
    profile.set_width(CONTROL_WIDTH);
    let row = widgets::row(&more_group, tr("Profile"), &profile.widget());
    row.reset.set_visible(false);
    row.copy.set_visible(false);
    row.subtitle.set_label(&profile_line(install::NO_PROFILE));
    let profile_line_label = row.subtitle;
    widgets::disclosure(&content, tr("More options"), &more);

    widgets::step_heading(&content, 4, tr("Review and install"));
    let install_group = widgets::group(&content);
    // The answers as they stand, read before anything is erased; each change
    // writes its value again (`Ui::summarize`).
    let review = gtk::Grid::builder()
        .column_spacing(16)
        .row_spacing(6)
        .css_classes(["edel-row"])
        .build();
    let review_disk = review_row(&review, 0, tr("Install on"));
    let review_user = review_row(&review, 1, tr("Username"));
    let review_language = review_row(&review, 2, tr("Language"));
    let review_keyboard = review_row(&review, 3, tr("Keyboard"));
    let review_zone = review_row(&review, 4, tr("Time zone"));
    install_group.append(&review);
    let plan = widgets::output_row(&install_group);
    // The buttons sit centred under the plan, then a line says what the
    // install asks.
    // The row spans the card, so its line above does; the buttons sit in
    // its middle.
    let button_row = gtk::Box::builder().css_classes(["edel-row"]).build();
    let buttons = gtk::Box::builder()
        .spacing(10)
        .hexpand(true)
        .halign(gtk::Align::Center)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    button_row.append(&buttons);
    let show_plan = widgets::big_button(tr("Show the plan"), false);
    let erase = widgets::big_button(tr("Erase and install"), true);
    erase.add_css_class("destructive-action");
    erase.set_sensitive(false);
    let restart = widgets::big_button(tr("Restart now"), true);
    restart.set_visible(false);
    buttons.append(&show_plan);
    buttons.append(&erase);
    buttons.append(&restart);
    install_group.append(&button_row);
    content.append(
        &gtk::Label::builder()
            .label(tr("Erase and install asks you to press it twice."))
            .xalign(0.5)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .margin_top(10)
            .css_classes(["edel-row-subtitle"])
            .build(),
    );

    let preset_widget = preset.widget();
    let profile_widget = profile.widget();
    let ui = Rc::new(Ui {
        hero,
        stage: Cell::new(Stage::Welcome),
        problem,
        file_card,
        file_line,
        use_file,
        stop_file,
        found: RefCell::new(None),
        using: Cell::new(false),
        preset_widget,
        profile_widget,
        language,
        languages: codes,
        keyboard,
        keyboards: RefCell::new(keyboard_values),
        zone,
        zones,
        user,
        password,
        again,
        hostname,
        presets,
        preset,
        profiles,
        profile,
        profile_line: profile_line_label,
        disks,
        disk,
        lines,
        problems: RefCell::new(Vec::new()),
        review_disk,
        review_user,
        review_language,
        review_keyboard,
        review_zone,
        list,
        listed: RefCell::new(None),
        plan,
        show_plan,
        erase,
        restart,
        planned: RefCell::new(None),
        armed: Cell::new(false),
        suggested: RefCell::new(keyboard_now),
        suggested_name: RefCell::new(String::new()),
        suggested_zone: RefCell::new(zone_suggested),
        rules,
    });
    ui.connect();
    // Each row's line starts with its own words.
    ui.show_problems(&[]);
    ui.summarize();
    ui.read_list();
    ui.find_file();
    // The page holds its state while it lives; the signals hold it weakly.
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    // The first question with no suggestion takes the keyboard, after the
    // window gave it to the sidebar's row, so the page is typed into at
    // once, from the keyboard alone.
    let weak = Rc::downgrade(&ui);
    page.connect_map(move |_| {
        let weak = weak.clone();
        glib::idle_add_local_once(move || {
            if let Some(ui) = weak.upgrade() {
                // A file found already takes the keyboard to Use it.
                if ui.file_card.is_visible() && ui.use_file.is_visible() {
                    ui.use_file.grab_focus();
                } else {
                    ui.user.grab_focus();
                }
                eprintln!(
                    "edel-settings: install page shown, {} disks",
                    ui.disks.len()
                );
            }
        });
    });
    page
}

/// The disk the system runs from, which is never offered.
fn running() -> Option<String> {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    install::running_disk(&mountinfo, Path::new("/sys"))
}

impl Ui {
    fn connect(self: &Rc<Self>) {
        // The language and the keyboard are choices: nothing to type next.
        let order: Vec<gtk::Widget> = vec![
            self.user.clone().upcast(),
            self.password.clone().upcast(),
            self.again.clone().upcast(),
        ];
        // Return moves on to the next question shown; after the last it
        // shows the plan.
        for (at, widget) in order.iter().enumerate() {
            let rest: Vec<gtk::Widget> = order[at + 1..].to_vec();
            let weak = Rc::downgrade(self);
            let on_return = move || match rest.iter().find(|w| w.is_mapped()) {
                Some(next) => {
                    next.grab_focus();
                }
                None => {
                    if let Some(ui) = weak.upgrade() {
                        ui.show_the_plan();
                    }
                }
            };
            if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
                entry.connect_activate(move |_| on_return());
            } else if let Some(entry) = widget.downcast_ref::<gtk::PasswordEntry>() {
                entry.connect_activate(move |_| on_return());
            }
        }
        let changed = |ui: &Rc<Self>| {
            let weak = Rc::downgrade(ui);
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.stale();
                }
            }
        };
        for entry in [&self.user, &self.hostname] {
            let stale = changed(self);
            entry.connect_changed(move |_| stale());
        }
        for entry in [&self.password, &self.again] {
            let stale = changed(self);
            entry.connect_changed(move |_| stale());
        }
        let stale = changed(self);
        self.zone.connect_changed(stale);
        let stale = changed(self);
        self.preset.connect_changed(stale);
        let weak = Rc::downgrade(self);
        let stale = changed(self);
        self.profile.connect_changed(move || {
            if let Some(ui) = weak.upgrade() {
                ui.profile_line
                    .set_label(&profile_line(&ui.profile_value()));
            }
            stale();
        });
        if let Some(disk) = &self.disk {
            disk.connect_changed(changed(self));
        }
        let weak = Rc::downgrade(self);
        self.language.connect_changed(move || {
            if let Some(ui) = weak.upgrade() {
                ui.language_picked();
            }
        });
        // The keyboard applies as soon as the person picks one.
        let weak = Rc::downgrade(self);
        self.keyboard.connect_changed(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.apply_keyboard(&ui.keyboard_value());
            ui.refresh_keyboard_line();
            ui.stale();
        });
        if let Some(disk) = &self.disk {
            let weak = Rc::downgrade(self);
            disk.connect_changed(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.show_disk();
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.user.connect_changed(move |entry| {
            let Some(ui) = weak.upgrade() else { return };
            // The computer name follows the login name until the person
            // types one: `ali-laptop`.
            let user = entry.text();
            let suggestion = if user.is_empty() {
                String::new()
            } else {
                install::suggested_hostname(user.trim(), install::form_factor(Path::new("/sys")))
            };
            if ui.hostname.text() == *ui.suggested_name.borrow() {
                ui.hostname.set_text(&suggestion);
            }
            *ui.suggested_name.borrow_mut() = suggestion;
        });
        let weak = Rc::downgrade(self);
        self.use_file.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.set_using(true);
            }
        });
        let weak = Rc::downgrade(self);
        self.stop_file.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.set_using(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.show_plan.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.show_the_plan();
            }
        });
        let weak = Rc::downgrade(self);
        self.erase.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.erase_pressed();
            }
        });
        self.restart.connect_clicked(|button| {
            button.set_sensitive(false);
            let button = button.clone();
            glib::spawn_future_local(async move {
                if let Ok(run) = gio::spawn_blocking(cmd::restart).await {
                    if !run.ok {
                        button.set_sensitive(true);
                        eprintln!("edel-settings: install restart failed: {}", run.shown());
                    }
                }
            });
        });
    }

    /// The language code the language choice shows.
    fn language_code(&self) -> String {
        self.languages
            .get(self.language.selected())
            .cloned()
            .unwrap_or_default()
    }

    /// The layout the keyboard choice shows, such as `ru` or `uz(latin)`.
    fn keyboard_layout(&self) -> String {
        self.keyboards
            .borrow()
            .get(self.keyboard.selected())
            .cloned()
            .unwrap_or_default()
    }

    /// The keyboard the form uses: the layout, with `us` after one that types
    /// no Latin letters.
    fn keyboard_value(&self) -> String {
        install::keyboard_for(&self.keyboard_layout())
    }

    /// The time zone the choice shows, such as `Europe/Berlin`.
    fn zone_value(&self) -> String {
        self.zones
            .get(self.zone.selected())
            .cloned()
            .unwrap_or_default()
    }

    /// The language changed: the keyboard follows it while the person has not
    /// picked one of their own.
    fn language_picked(&self) {
        let zone_suggestion = edel::timezones::suggested(&self.language_code()).to_string();
        if self.zone_value() == *self.suggested_zone.borrow() {
            if let Some(at) = self.zones.iter().position(|z| *z == zone_suggestion) {
                self.zone.set_selected(at);
            }
        }
        *self.suggested_zone.borrow_mut() = zone_suggestion;
        let suggestion = install::suggested_keyboard(&self.language_code());
        if self.keyboard_value() == *self.suggested.borrow() {
            let at = self
                .keyboards
                .borrow()
                .iter()
                .position(|layout| *layout == first_layout(&suggestion));
            // A layout the list lacks leaves the choice as it is.
            if let Some(at) = at {
                self.keyboard.set_selected(at);
            }
        }
        *self.suggested.borrow_mut() = suggestion;
        self.apply_keyboard(&self.keyboard_value());
        self.refresh_keyboard_line();
        self.stale();
    }

    /// The words a row's line shows when its answer has no problem.
    fn default_line(&self, field: Field) -> &'static str {
        match field {
            Field::Keyboard if self.keyboard_value().contains(',') => tr(
                "English (US) is added, to type your username and password; Super+Space switches",
            ),
            Field::User => tr("Lowercase, such as ali. Your home folder takes this name."),
            Field::Password => tr("You type it to log in and to change the system."),
            Field::Hostname => tr("How other computers on the network see this one."),
            Field::Disk => tr("Everything on it is erased. What it holds now is listed below."),
            Field::TimeZone => tr("The clock shows this zone's time."),
            Field::Language | Field::Keyboard | Field::Again => "",
        }
    }

    /// Shows a row's line: its problem when it has one, else its own words.
    fn show_line(&self, field: Field, line: &gtk::Label) {
        let problem = self
            .problems
            .borrow()
            .iter()
            .find(|(f, _)| *f == field)
            .map(|(_, words)| words.clone());
        match problem {
            Some(words) => {
                line.set_label(&words);
                line.add_css_class("edel-error");
                line.remove_css_class("edel-warn");
            }
            None => {
                line.set_label(self.default_line(field));
                line.remove_css_class("edel-error");
                // The disk's line warns: what it holds is erased.
                if field == Field::Disk {
                    line.add_css_class("edel-warn");
                } else {
                    line.remove_css_class("edel-warn");
                }
            }
        }
    }

    /// The review's values, as the answers stand now.
    fn summarize(&self) {
        let disk = match self
            .disk
            .as_ref()
            .and_then(|d| self.disks.get(d.selected()))
        {
            Some(disk) => disk.title.clone(),
            None => tr("No disk chosen").to_string(),
        };
        let user = if self.skipped().contains(&"user") {
            tr("From the settings file").to_string()
        } else {
            let typed = self.user.text().trim().to_string();
            if typed.is_empty() {
                tr("Not given yet").to_string()
            } else {
                typed
            }
        };
        self.review_disk.set_label(&disk);
        self.review_user.set_label(&user);
        self.review_language.set_label(&self.language.chosen_text());
        self.review_keyboard.set_label(&self.keyboard.chosen_text());
        self.review_zone.set_label(&self.zone.chosen_text());
    }

    /// The keyboard's line says what a second layout is for, as the
    /// keyboard changes.
    fn refresh_keyboard_line(&self) {
        if let Some((field, line)) = self.lines.iter().find(|(f, _)| *f == Field::Keyboard) {
            self.show_line(*field, line);
        }
    }

    /// The keyboard written to the person's file once it is a layout this    /// The keyboard written to the person's file once it is a layout this
    /// machine knows, so it applies at once; `us`, the default, taken out.
    fn apply_keyboard(&self, value: &str) {
        let Ok(layouts) = edel::keyboard::normalize(value, self.rules.as_deref()) else {
            return;
        };
        let wanted = (layouts != "us").then_some(layouts.as_str());
        if Files::here().person_file().region.keyboard.as_deref() == wanted {
            return;
        }
        if let Err(why) = Files::here().set("region.keyboard", wanted) {
            self.say_problem(&why);
        }
    }

    /// The form as it stands.
    fn form(&self) -> Form {
        Form {
            language: self.language_code(),
            keyboard: self.keyboard_value(),
            user: self.user.text().to_string(),
            password: self.password.text().to_string(),
            again: self.again.text().to_string(),
            disk: self
                .disk
                .as_ref()
                .and_then(|d| self.disks.get(d.selected()))
                .map(|d| d.name.clone()),
            timezone: self.zone_value(),
            hostname: self.hostname.text().to_string(),
            preset: self.presets[self.preset.selected().min(self.presets.len() - 1)].to_string(),
            profile: self.profile_value(),
            skip: self.skipped(),
            password_needed: self.password_needed(),
        }
    }

    /// The profile chosen, `install::NO_PROFILE` for None.
    fn profile_value(&self) -> String {
        self.profiles[self.profile.selected().min(self.profiles.len() - 1)].to_string()
    }

    /// The question keys the file in use answers; none without one.
    fn skipped(&self) -> Vec<&'static str> {
        match self.found.borrow().as_ref() {
            Some(found) if self.using.get() => found.answered.clone(),
            _ => Vec::new(),
        }
    }

    /// A password is asked unless the file in use names several people,
    /// who then log in with what the file gives them.
    fn password_needed(&self) -> bool {
        !(self.using.get() && self.found.borrow().as_ref().is_some_and(|f| f.people > 1))
    }

    /// Looks for a settings file on another disk, once, and offers it.
    fn find_file(self: &Rc<Self>) {
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let done =
                gio::spawn_blocking(|| cmd::edel_as_root(&["install", "--find-settings"], None))
                    .await;
            let Ok(run) = done else { return };
            if !run.ok {
                return;
            }
            let place = run
                .err
                .lines()
                .last()
                .unwrap_or_default()
                .trim_start_matches("edel install: ")
                .to_string();
            match edel::settings::read(&run.out) {
                Ok(read) => {
                    let answered = install::answered(&read.file);
                    ui.file_line.set_label(&trf(
                        "On {device}, answering {what}. Use it, and only the rest is asked.",
                        &[
                            ("device", device_of(&place)),
                            ("what", &answered_words(&answered)),
                        ],
                    ));
                    *ui.found.borrow_mut() = Some(Found {
                        text: run.out.clone(),
                        place,
                        people: read.file.users.len(),
                        answered,
                    });
                    ui.use_file.set_visible(true);
                }
                Err(why) => {
                    ui.file_line.set_label(&trf(
                        "{place}, but it cannot be used: {why}",
                        &[
                            ("place", &crate::status::capital(&place)),
                            ("why", &format!("{why:#}")),
                        ],
                    ));
                    ui.use_file.set_visible(false);
                }
            }
            ui.stop_file.set_visible(false);
            ui.file_card.set_visible(true);
            eprintln!("edel-settings: install found a settings file");
            // Nothing typed yet: the keyboard goes to Use it.
            if ui.user.has_focus() && ui.user.text().is_empty() && ui.use_file.is_visible() {
                ui.use_file.grab_focus();
            }
        });
    }

    /// Uses the found file, asking only what it leaves open, or stops.
    fn set_using(&self, using: bool) {
        self.using.set(using);
        let skip = self.skipped();
        let password = self.password_needed();
        let rows: [(&str, gtk::Widget); 9] = [
            ("region.language", self.language.widget()),
            ("region.keyboard", self.keyboard.widget()),
            ("region.timezone", self.zone.widget()),
            ("user", self.user.clone().upcast()),
            ("password", self.password.clone().upcast()),
            ("password", self.again.clone().upcast()),
            ("network.hostname", self.hostname.clone().upcast()),
            ("layout.preset", self.preset_widget.clone()),
            ("system.profiles", self.profile_widget.clone()),
        ];
        for (key, control) in &rows {
            let shown = !skip.contains(key) && (*key != "password" || password);
            if let Some(row) = control.parent() {
                row.set_visible(shown);
            }
        }
        self.use_file.set_visible(!using);
        self.stop_file.set_visible(using);
        if let Some(found) = self.found.borrow().as_ref() {
            let words = if using {
                trf(
                    "{place}. In use: it answers {what}, so only the rest is asked.",
                    &[
                        ("place", &crate::status::capital(&found.place)),
                        ("what", &answered_words(&found.answered)),
                    ],
                )
            } else {
                trf(
                    "{place}. It answers: {what}. Use it, and only the rest is asked.",
                    &[
                        ("place", &crate::status::capital(&found.place)),
                        ("what", &answered_words(&found.answered)),
                    ],
                )
            };
            self.file_line.set_label(&words);
        }
        eprintln!("edel-settings: install file used {using}");
        self.stale();
        // The keyboard goes to the first question left that is empty.
        let order: [gtk::Widget; 3] = [
            self.user.clone().upcast(),
            self.password.clone().upcast(),
            self.again.clone().upcast(),
        ];
        let empty = |w: &gtk::Widget| {
            w.downcast_ref::<gtk::Editable>()
                .is_none_or(|e| e.text().is_empty())
        };
        match order
            .iter()
            .find(|w| w.parent().is_some_and(|r| r.is_visible()) && empty(w))
        {
            Some(next) => {
                next.grab_focus();
            }
            None => {
                self.show_plan.grab_focus();
            }
        }
    }

    fn say_problem(&self, text: &str) {
        self.problem.set_label(text);
        self.problem.set_visible(!text.is_empty());
    }

    /// An answer changed: the plan shown is no longer this one.
    fn stale(&self) {
        if self.planned.borrow_mut().take().is_some() {
            widgets::say(&self.plan, "", false);
        }
        self.erase.set_sensitive(false);
        self.disarm();
        // Once the person changes an answer after a failed install, the hero
        // goes back to the welcome.
        if self.stage.get() == Stage::Failed {
            self.stage.set(Stage::Welcome);
            welcome(&self.hero);
        }
        self.summarize();
    }

    /// The install did not finish: the hero says so, and the plan's message
    /// says why.
    fn failed(&self) {
        self.stage.set(Stage::Failed);
        self.hero.show(
            "status-problem",
            false,
            true,
            tr("The install did not finish"),
            tr("The message below says what went wrong. Fix it and press Erase and install again."),
        );
    }

    fn disarm(&self) {
        self.armed.set(false);
        self.erase.set_label(tr("Erase and install"));
    }

    /// Shows each field's problem under its row, and each other row its own
    /// words again.
    fn show_problems(&self, problems: &[(Field, String)]) {
        *self.problems.borrow_mut() = problems.to_vec();
        for (field, line) in &self.lines {
            self.show_line(*field, line);
        }
    }

    /// Checks the answers, writes them and shows `edel install --plan`.
    fn show_the_plan(self: &Rc<Self>) {
        self.say_problem("");
        self.apply_keyboard(&self.keyboard_value());
        let (answers, disk) = match check(&self.form(), self.rules.as_deref()) {
            Ok(done) => {
                self.show_problems(&[]);
                done
            }
            Err(problems) => {
                self.show_problems(&problems);
                if let Some((field, _)) = problems.first() {
                    self.focus(*field);
                }
                return;
            }
        };
        let file = answers_file();
        let text = match self.found.borrow().as_ref() {
            Some(found) if self.using.get() => install::merged(&found.text, &answers),
            _ => answers.settings_text(),
        };
        let written = text.map_err(|e| format!("{e:#}")).and_then(|text| {
            write_private(&file, &text).map_err(|e| {
                trf(
                    "could not write {path}: {why}",
                    &[
                        ("path", &file.display().to_string()),
                        ("why", &e.to_string()),
                    ],
                )
            })
        });
        if let Err(why) = written {
            self.say_problem(&why);
            return;
        }
        self.show_plan.set_sensitive(false);
        widgets::say(&self.plan, tr("Working..."), false);
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let args = install_args(&disk, &file, true, false);
            let done = gio::spawn_blocking(move || {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                cmd::edel_as_root(&args, None)
            })
            .await;
            ui.show_plan.set_sensitive(true);
            let Ok(run) = done else { return };
            widgets::say(&ui.plan, &run.shown(), !run.ok);
            eprintln!(
                "edel-settings: install plan for {disk}, exit {:?}",
                run.code
            );
            if run.ok {
                *ui.planned.borrow_mut() = Some(disk);
                ui.erase.set_sensitive(true);
                ui.erase.grab_focus();
            }
        });
    }

    /// Gives the keyboard to the row of `field`.
    fn focus(&self, field: Field) {
        match field {
            Field::Language => self.language.widget().grab_focus(),
            Field::Keyboard => self.keyboard.widget().grab_focus(),
            Field::TimeZone => self.zone.widget().grab_focus(),
            Field::User => self.user.grab_focus(),
            Field::Password => self.password.grab_focus(),
            Field::Again => self.again.grab_focus(),
            Field::Hostname => self.hostname.grab_focus(),
            Field::Disk => self.disk.as_ref().is_some_and(|d| d.widget().grab_focus()),
        };
    }

    /// The first press asks again, naming the disk, for five seconds; the
    /// second installs.
    fn erase_pressed(self: &Rc<Self>) {
        let Some(disk) = self.planned.borrow().clone() else {
            return;
        };
        if !self.armed.get() {
            self.armed.set(true);
            self.erase
                .set_label(&trf("Press again to erase {disk}", &[("disk", &disk)]));
            let weak = Rc::downgrade(self);
            glib::timeout_add_local_once(ARMED, move || {
                if let Some(ui) = weak.upgrade() {
                    ui.disarm();
                }
            });
            return;
        }
        self.disarm();
        self.erase.set_sensitive(false);
        self.show_plan.set_sensitive(false);
        widgets::say(
            &self.plan,
            &trf(
                "Installing on {disk}. This takes a few minutes; keep the stick in.",
                &[("disk", &disk)],
            ),
            false,
        );
        eprintln!("edel-settings: install started on {disk}");
        let password = self.password.text().to_string();
        self.password.set_text("");
        self.again.set_text("");
        self.stage.set(Stage::Installing);
        self.hero.show(
            "page-install",
            true,
            false,
            tr("Installing Edel OS"),
            tr("This takes a few minutes. Keep the computer on."),
        );
        let file = answers_file();
        let ui = self.clone();
        let asked = self.password_needed();
        glib::spawn_future_local(async move {
            let args = install_args(&disk, &file, false, asked);
            let done = gio::spawn_blocking(move || {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                cmd::edel_as_root(&args, asked.then_some(password))
            })
            .await;
            let _ = std::fs::remove_file(&file);
            let Ok(run) = done else {
                ui.failed();
                return;
            };
            eprintln!("edel-settings: install on {disk}, exit {:?}", run.code);
            if run.ok {
                ui.stage.set(Stage::Installed);
                ui.hero.show(
                    "status-ok",
                    false,
                    false,
                    tr("Edel OS is installed"),
                    tr("Restart, take out the USB stick, and your computer starts Edel OS."),
                );
                widgets::say(
                    &ui.plan,
                    &trf(
                        "Edel OS is installed on {disk}. Restart, and take the stick out when the screen goes dark.",
                        &[("disk", &disk)],
                    ),
                    false,
                );
                ui.erase.set_visible(false);
                ui.show_plan.set_visible(false);
                ui.restart.set_visible(true);
                ui.restart.grab_focus();
            } else {
                widgets::say(&ui.plan, &run.shown(), true);
                ui.show_plan.set_sensitive(true);
                *ui.planned.borrow_mut() = None;
                ui.failed();
            }
        });
    }

    /// Reads what `edel install --list` says of the disks, once, and shows
    /// the chosen one's part.
    fn read_list(self: &Rc<Self>) {
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let done =
                gio::spawn_blocking(|| cmd::edel_as_root(&["install", "--list"], None)).await;
            if let Ok(run) = done {
                *ui.listed.borrow_mut() = Some(run);
                ui.show_disk();
            }
        });
    }

    /// What `edel install --list` says of the chosen disk, word for word;
    /// the whole message when it failed.
    fn show_disk(&self) {
        let listed = self.listed.borrow();
        let Some(run) = listed.as_ref() else { return };
        if !run.ok {
            widgets::say(&self.list, run.shown().trim_end(), true);
            return;
        }
        let name = self.form().disk.unwrap_or_default();
        widgets::say(
            &self.list,
            disk_block(&run.out, &name).unwrap_or_default(),
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Form {
        Form {
            language: "de_DE.UTF-8".into(),
            keyboard: "de".into(),
            user: "ali".into(),
            password: "secret".into(),
            again: "secret".into(),
            disk: Some("nvme0n1".into()),
            timezone: "Europe/Berlin".into(),
            hostname: "ali-laptop".into(),
            preset: "classic".into(),
            profile: "none".into(),
            skip: Vec::new(),
            password_needed: true,
        }
    }

    #[test]
    fn the_disk_a_file_is_on_reads_from_the_found_line() {
        assert_eq!(device_of("found a file on /dev/sdb1"), "/dev/sdb1");
        assert_eq!(device_of("anderswo"), "anderswo");
    }

    #[test]
    fn a_whole_form_gives_the_answers_the_terminal_would() {
        let (answers, disk) = check(&form(), None).unwrap();
        assert_eq!(disk, "nvme0n1");
        let text = answers.settings_text().unwrap();
        assert!(text.contains("[users.ali]\nadmin = true\n"), "{text}");
        assert!(text.contains("hostname = \"ali-laptop\""), "{text}");
        assert!(text.contains("keyboard = \"de\""), "{text}");
        assert!(!text.contains("classic"), "Classic applies anyway: {text}");
        assert!(
            !text.contains("secret"),
            "the password is never in the file"
        );
    }

    #[test]
    fn each_problem_lands_under_its_own_row() {
        let bad = Form {
            user: "root".into(),
            again: "other".into(),
            disk: None,
            hostname: "ali laptop".into(),
            ..form()
        };
        let fields: Vec<Field> = check(&bad, None)
            .unwrap_err()
            .into_iter()
            .map(|p| p.0)
            .collect();
        assert_eq!(
            fields,
            [Field::User, Field::Again, Field::Hostname, Field::Disk]
        );
        let empty = Form {
            password: String::new(),
            again: String::new(),
            ..form()
        };
        let problems = check(&empty, None).unwrap_err();
        assert_eq!(
            problems,
            [(Field::Password, "a password cannot be empty".to_string())]
        );
    }

    #[test]
    fn a_question_the_file_answers_is_not_asked_nor_refused() {
        let with_file = Form {
            user: String::new(),
            hostname: "bad name".into(),
            skip: vec!["user", "network.hostname", "layout.preset"],
            ..form()
        };
        let (answers, disk) = check(&with_file, None).unwrap();
        assert_eq!(disk, "nvme0n1");
        assert_eq!(answers.user, None);
        assert_eq!(answers.hostname, None);
        assert_eq!(answers.preset, None);
        assert_eq!(answers.keyboard.as_deref(), Some("de"));
        let several = Form {
            password: String::new(),
            again: String::new(),
            password_needed: false,
            ..with_file
        };
        assert!(check(&several, None).is_ok(), "no password asked");
        let text = edel::install::merged(
            "format = 1\n[users.bea]\nadmin = true\n[network]\nhostname = \"office-1\"\n",
            &check(&several, None).unwrap().0,
        )
        .unwrap();
        assert!(
            text.contains("hostname = \"office-1\"") && text.contains("[users.bea]"),
            "{text}"
        );
    }

    #[test]
    fn the_zone_is_refused_in_the_words_of_the_terminal() {
        let bad = Form {
            timezone: "../etc".into(),
            ..form()
        };
        let fields: Vec<Field> = check(&bad, None)
            .unwrap_err()
            .into_iter()
            .map(|p| p.0)
            .collect();
        assert_eq!(fields, [Field::TimeZone]);
        let (answers, _) = check(&form(), None).unwrap();
        assert_eq!(answers.timezone.as_deref(), Some("Europe/Berlin"));
    }

    #[test]
    fn the_zone_choice_holds_the_file_zone_and_each_languages() {
        let zones = zone_list("Asia/Tashkent");
        assert!(zones.contains(&"Asia/Tashkent".to_string()));
        for language in edel::languages::LANGUAGES {
            assert!(
                zones.contains(&language.zone.to_string()),
                "{}",
                language.zone
            );
        }
        assert!(!zones.is_empty());
    }

    #[test]
    fn shows_the_chosen_disks_part_of_the_list() {
        let list = "/dev/sda: SanDisk Ultra, 32 GB (removable, running from it, so not offered)\n  sda1: EDEL-ESP (vfat, 67 MB)\n\n/dev/nvme0n1: Samsung SSD 980, 512 GB\n  nvme0n1p3: Windows (ntfs, 420 GB)\n";
        assert_eq!(
            disk_block(list, "nvme0n1"),
            Some("/dev/nvme0n1: Samsung SSD 980, 512 GB\n  nvme0n1p3: Windows (ntfs, 420 GB)")
        );
        assert_eq!(disk_block(list, "nvme0"), None, "a name is never a prefix");
    }

    #[test]
    fn the_plan_and_the_install_are_the_commands_people_type() {
        let file = Path::new("/run/user/100/edel-install.toml");
        assert_eq!(
            install_args("sda", file, true, false).join(" "),
            "install /dev/sda --settings /run/user/100/edel-install.toml --plan"
        );
        assert_eq!(
            install_args("sda", file, false, true).join(" "),
            "install /dev/sda --settings /run/user/100/edel-install.toml --yes --new-password-stdin"
        );
        assert_eq!(
            install_args("sda", file, false, false).join(" "),
            "install /dev/sda --settings /run/user/100/edel-install.toml --yes",
            "a file of several people gives no password"
        );
    }
}
