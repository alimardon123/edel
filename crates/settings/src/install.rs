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

use edel::i18n::{n_, tr, trf};
use edel::install::{self, Answers, DiskChoice};

use crate::files::Files;
use crate::{cmd, widgets};

const INTRO: &str = n_(
    "Put Edel OS on this computer's disk. Nothing changes until you press Erase and install, and every answer can be changed later in Settings.",
);

/// The language suggested when the stick has none set.
const LANGUAGE: &str = "en_US.UTF-8";

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
    pub hostname: String,
    pub preset: String,
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
    let [language, keyboard, user, _, hostname, disk] = answers.map(|(_, a)| a.ok().flatten());
    if !problems.is_empty() {
        return Err(problems);
    }
    let answers = Answers {
        language,
        keyboard,
        user,
        ssh_key: None,
        timezone: None,
        hostname,
        preset: (!form.skip.contains(&"layout.preset")).then(|| form.preset.clone()),
    };
    Ok((answers, disk.unwrap_or_default()))
}

/// `edel install`'s arguments for the plan of `disk` with `file`, or for
/// the install itself, given the password on standard input when
/// `password` says one was asked.
pub fn install_args(disk: &str, file: &Path, plan: bool, password: bool) -> Vec<String> {
    let mut args = vec![
        "install".to_string(),
        format!("/dev/{disk}"),
        "--settings".to_string(),
        file.display().to_string(),
    ];
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

/// `text` with a capital first letter, as a sentence starts.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
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
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The page's controls and its state.
struct Ui {
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
    language: gtk::Entry,
    keyboard: gtk::Entry,
    user: gtk::Entry,
    password: gtk::PasswordEntry,
    again: gtk::PasswordEntry,
    hostname: gtk::Entry,
    presets: Vec<&'static str>,
    preset: Rc<widgets::Choice>,
    disks: Vec<DiskChoice>,
    disk: Option<Rc<widgets::Choice>>,
    /// The line under each row, for its problem
    lines: Vec<(Field, gtk::Label)>,
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
    rules: Option<String>,
}

/// An entry of the form, its text filled in.
fn entry(text: &str) -> gtk::Entry {
    gtk::Entry::builder()
        .text(text)
        .width_chars(18)
        .css_classes(["edel-entry"])
        .build()
}

/// A password entry with the eye that shows it.
fn password_entry() -> gtk::PasswordEntry {
    gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .width_chars(18)
        .css_classes(["edel-entry"])
        .build()
}

pub fn page() -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Install Edel OS"), tr(INTRO));
    let files = Files::here();
    let language_now = files
        .person_file()
        .region
        .language
        .unwrap_or_else(|| LANGUAGE.to_string());
    let keyboard_now = install::suggested_keyboard(&language_now);
    let mut lines = Vec::new();

    // A settings file on another disk (M6.6b), shown once one is found.
    let file_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .visible(false)
        .build();
    widgets::heading(&file_card, tr("A settings file"));
    let file_group = widgets::group(&file_card);
    let file_line = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row"])
        .build();
    file_group.append(&file_line);
    let file_buttons = gtk::Box::builder()
        .spacing(8)
        .css_classes(["edel-row"])
        .build();
    file_buttons.append(&gtk::Box::builder().hexpand(true).build());
    let stop_file = widgets::action(tr("Answer every question"), false);
    let use_file = widgets::action(tr("Use it"), true);
    file_buttons.append(&stop_file);
    file_buttons.append(&use_file);
    file_group.append(&file_buttons);
    content.append(&file_card);

    widgets::heading(&content, tr("You"));
    let you = widgets::group(&content);
    let language = entry(&language_now);
    let keyboard = entry(&keyboard_now);
    let user = entry("");
    let password = password_entry();
    let again = password_entry();
    for (field, title, control) in [
        (
            Field::Language,
            tr("Language"),
            language.clone().upcast::<gtk::Widget>(),
        ),
        (Field::Keyboard, tr("Keyboard"), keyboard.clone().upcast()),
        (Field::User, tr("Your login name"), user.clone().upcast()),
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

    widgets::heading(&content, tr("Disk"));
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
        let row = widgets::row(&disk_group, tr("Disk to install on"), &choice.widget());
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
    let row = widgets::row(&more_group, tr("Layout"), &preset.widget());
    row.reset.set_visible(false);
    row.copy.set_visible(false);
    widgets::disclosure(&content, tr("More options"), &more);

    widgets::heading(&content, tr("Plan"));
    let plan_group = widgets::group(&content);
    widgets::note_row(
        &plan_group,
        tr("The plan says what is erased and what is written. Erase and install asks twice."),
    );
    let plan = widgets::output_row(&plan_group);
    // The row spans the card, so its line above does; the buttons sit at
    // its end.
    let buttons = gtk::Box::builder()
        .spacing(8)
        .css_classes(["edel-row"])
        .build();
    buttons.append(&gtk::Box::builder().hexpand(true).build());
    let show_plan = widgets::action(tr("Show the plan"), false);
    let erase = widgets::action(tr("Erase and install"), true);
    erase.add_css_class("destructive-action");
    erase.set_sensitive(false);
    let restart = widgets::action(tr("Restart now"), true);
    restart.set_visible(false);
    buttons.append(&show_plan);
    buttons.append(&erase);
    buttons.append(&restart);
    plan_group.append(&buttons);

    let preset_widget = preset.widget();
    let ui = Rc::new(Ui {
        problem,
        file_card,
        file_line,
        use_file,
        stop_file,
        found: RefCell::new(None),
        using: Cell::new(false),
        preset_widget,
        language,
        keyboard,
        user,
        password,
        again,
        hostname,
        presets,
        preset,
        disks,
        disk,
        lines,
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
        rules: std::fs::read_to_string(edel::keyboard::RULES).ok(),
    });
    ui.connect();
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
        let order: Vec<gtk::Widget> = vec![
            self.language.clone().upcast(),
            self.keyboard.clone().upcast(),
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
        for entry in [&self.language, &self.keyboard, &self.user, &self.hostname] {
            let stale = changed(self);
            entry.connect_changed(move |_| stale());
        }
        for entry in [&self.password, &self.again] {
            let stale = changed(self);
            entry.connect_changed(move |_| stale());
        }
        let stale = changed(self);
        self.preset.connect_changed(stale);
        if let Some(disk) = &self.disk {
            disk.connect_changed(changed(self));
        }
        let weak = Rc::downgrade(self);
        self.language.connect_changed(move |entry| {
            let Some(ui) = weak.upgrade() else { return };
            // The keyboard follows the language until the person types one.
            let suggestion = install::suggested_keyboard(entry.text().trim());
            if ui.keyboard.text() == *ui.suggested.borrow() {
                ui.keyboard.set_text(&suggestion);
            }
            *ui.suggested.borrow_mut() = suggestion;
        });
        // The keyboard applies once the person leaves its row, not at
        // each letter, which could pass through another layout's name.
        let weak = Rc::downgrade(self);
        let left = gtk::EventControllerFocus::new();
        left.connect_leave(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.apply_keyboard(ui.keyboard.text().trim());
            }
        });
        self.keyboard.add_controller(left);
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

    /// The keyboard written to the person's file once it is a layout this
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
            language: self.language.text().to_string(),
            keyboard: self.keyboard.text().to_string(),
            user: self.user.text().to_string(),
            password: self.password.text().to_string(),
            again: self.again.text().to_string(),
            disk: self
                .disk
                .as_ref()
                .and_then(|d| self.disks.get(d.selected()))
                .map(|d| d.name.clone()),
            hostname: self.hostname.text().to_string(),
            preset: self.presets[self.preset.selected().min(self.presets.len() - 1)].to_string(),
            skip: self.skipped(),
            password_needed: self.password_needed(),
        }
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
                        "{place}. It answers: {what}. Use it, and only the rest is asked.",
                        &[
                            ("place", &capitalized(&place)),
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
                            ("place", &capitalized(&place)),
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
        let rows: [(&str, gtk::Widget); 7] = [
            ("region.language", self.language.clone().upcast()),
            ("region.keyboard", self.keyboard.clone().upcast()),
            ("user", self.user.clone().upcast()),
            ("password", self.password.clone().upcast()),
            ("password", self.again.clone().upcast()),
            ("network.hostname", self.hostname.clone().upcast()),
            ("layout.preset", self.preset_widget.clone()),
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
                        ("place", &capitalized(&found.place)),
                        ("what", &answered_words(&found.answered)),
                    ],
                )
            } else {
                trf(
                    "{place}. It answers: {what}. Use it, and only the rest is asked.",
                    &[
                        ("place", &capitalized(&found.place)),
                        ("what", &answered_words(&found.answered)),
                    ],
                )
            };
            self.file_line.set_label(&words);
        }
        eprintln!("edel-settings: install file used {using}");
        self.stale();
        // The keyboard goes to the first question left that is empty.
        let order: [gtk::Widget; 5] = [
            self.language.clone().upcast(),
            self.keyboard.clone().upcast(),
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
    }

    fn disarm(&self) {
        self.armed.set(false);
        self.erase.set_label(tr("Erase and install"));
    }

    /// Shows each field's problem under its row, and clears the others.
    fn show_problems(&self, problems: &[(Field, String)]) {
        for (field, line) in &self.lines {
            let words = problems
                .iter()
                .find(|(f, _)| f == field)
                .map_or("", |(_, w)| w.as_str());
            line.set_label(words);
            if words.is_empty() {
                line.remove_css_class("edel-error");
            } else {
                line.add_css_class("edel-error");
            }
        }
    }

    /// Checks the answers, writes them and shows `edel install --plan`.
    fn show_the_plan(self: &Rc<Self>) {
        self.say_problem("");
        self.apply_keyboard(self.keyboard.text().trim());
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
            Field::Language => self.language.grab_focus(),
            Field::Keyboard => self.keyboard.grab_focus(),
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
            let Ok(run) = done else { return };
            eprintln!("edel-settings: install on {disk}, exit {:?}", run.code);
            if run.ok {
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
            hostname: "ali-laptop".into(),
            preset: "classic".into(),
            skip: Vec::new(),
            password_needed: true,
        }
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
