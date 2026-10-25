//! The Welcome (roadmap M6.12, ADR-008): the first start on a machine nobody
//! has set up. `edel-settings --welcome` opens one window, maximized, that
//! asks a page at a time: the language, the keyboard (tried in a box under
//! it), Wi-Fi when the machine has an adapter (the Network page's own
//! parts, `network::parts`), the layout (the presets as pictures) and the
//! style, then Done. The
//! questions and their order are the installer's (`edel::install::QUESTIONS`),
//! so the two ask alike. The desktop opens it by itself on the first start
//! (`edel::first_start`), when the account live logs in by itself.
//!
//! Who it is for decides two more pages (`Mode`). On a USB stick the first
//! page asks Try or Install, and Install opens the installer page. On a
//! computer nobody has an account on, the person's account (name and
//! password) and the time zone are asked where the installer asks them, and
//! Start using Edel OS makes that person with `edel settings first-boot`, as
//! root through doas, with the choices made, then restarts, so the login
//! screen asks for them.
//!
//! Each of the person's own choices is written when it is made, with
//! `Files::choose_over` as the Settings pages write theirs, so the desktop
//! follows at once and a value that applies anyway is taken out, never
//! written (ADR-008). The account and the time zone are the machine's, so
//! they wait for the end. Next writes nothing, and Skip closes the Welcome
//! writing nothing more. Each page shows what applies now when it opens, so
//! going Back shows the choice made. On a stick the Welcome leaves its stamp
//! when it closes, so it opens once there; on a computer nobody has an
//! account on it opens again at each start until it has made the person.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use edel::addon_state::{self, Facts, State};
use edel::i18n::{n_, tr, trf};
use edel::install::Question;
use edel::settings::Source;

use crate::files::{self, Files};
use crate::install;
use crate::style::Theme;
use crate::{actions, cmd, preview, region, status, widgets};

/// The settings keys the Welcome chooses, as `edel settings` names them.
const LANGUAGE_KEY: &str = "region.language";
const KEYBOARD_KEY: &str = "region.keyboard";
const ZONE_KEY: &str = "region.timezone";
const LAYOUT_KEY: &str = "layout.preset";
const STYLE_KEY: &str = "appearance.mode";

/// The person's own keys, which a new machine's first person takes as the
/// machine's at the end (the installer writes them into the machine's file).
const OWN_KEYS: [&str; 4] = [LANGUAGE_KEY, KEYBOARD_KEY, LAYOUT_KEY, STYLE_KEY];

/// The styles, as `appearance.mode` takes them, in the order shown.
const STYLES: [&str; 3] = ["light", "dark", "auto"];

/// The installer's questions the Welcome asks, each with its page and
/// whether only a new machine's Welcome asks it (the account and the time
/// zone are the machine's).
const ASKED: [(&str, Page, bool); 5] = [
    (LANGUAGE_KEY, Page::Language, false),
    (KEYBOARD_KEY, Page::Keyboard, false),
    ("user", Page::Account, true),
    (ZONE_KEY, Page::TimeZone, true),
    (LAYOUT_KEY, Page::Layout, false),
];

/// The width of the column the controls sit in, and of each control, in
/// logical pixels: a column at most this wide, centred on the page.
const COLUMN: i32 = 560;
const CONTROL: i32 = 360;

/// A group of places the log gives: its line's words and each item's name
/// and widget.
type Group = (&'static str, Vec<(String, gtk::Widget)>);

/// Who the Welcome is for, which decides its pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A system started from a USB stick: Try or Install comes first.
    Stick,
    /// A computer nobody has an account on, where live logs in by itself:
    /// the account and the time zone are asked too, and the end makes the
    /// person and restarts.
    NewMachine,
    /// A person with an account, who opened it themselves.
    Person,
}

impl Mode {
    /// The mode for a system running from a stick or not, `user` logged
    /// in: off a stick, live logs in by itself only while nobody has an
    /// account (`edel boot live`).
    pub fn of(stick: bool, user: Option<&str>) -> Mode {
        if stick {
            Mode::Stick
        } else if user == Some(edel::places::LIVE_USER) {
            Mode::NewMachine
        } else {
            Mode::Person
        }
    }

    /// This system's mode.
    fn here() -> Mode {
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let user = edel::users::current()
            .and_then(|uid| edel::first_start::name_of(&passwd, uid).map(str::to_string));
        Mode::of(edel::install::from_a_stick(), user.as_deref())
    }

    /// The name the log gives the mode.
    fn name(self) -> &'static str {
        match self {
            Mode::Stick => "stick",
            Mode::NewMachine => "new machine",
            Mode::Person => "person",
        }
    }
}

/// One page of the Welcome, in the order they show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// On a stick: Try Edel OS or Install it.
    Start,
    /// The language (`region.language`).
    Language,
    /// The keyboard layout (`region.keyboard`), tried in a box below it.
    Keyboard,
    /// The Wi-Fi networks in range, to join one; shown only when the
    /// machine has a Wi-Fi adapter. NetworkManager keeps the network and
    /// its password, never the settings file (ADR-006).
    WiFi,
    /// A new machine's first person: name and password.
    Account,
    /// A new machine's time zone (`region.timezone`).
    TimeZone,
    /// The layout (`layout.preset`), the presets as pictures.
    Layout,
    /// Light, dark or automatic (`appearance.mode`).
    Style,
    /// The last page: what was chosen stays, to change in Settings.
    Done,
}

impl Page {
    /// The name the log gives the page, `welcome page NAME`.
    pub fn name(self) -> &'static str {
        match self {
            Page::Start => "start",
            Page::Language => "language",
            Page::Keyboard => "keyboard",
            Page::WiFi => "wifi",
            Page::Account => "account",
            Page::TimeZone => "timezone",
            Page::Layout => "layout",
            Page::Style => "style",
            Page::Done => "done",
        }
    }

    /// The page's title, as people read it.
    pub fn title(self) -> &'static str {
        match self {
            Page::Start => n_("Welcome to Edel OS"),
            Page::Language => n_("Language"),
            Page::Keyboard => n_("Keyboard"),
            Page::WiFi => n_("Wi-Fi"),
            Page::Account => n_("Your account"),
            Page::TimeZone => n_("Time zone"),
            Page::Layout => n_("Layout"),
            Page::Style => n_("Style"),
            Page::Done => n_("You are all set"),
        }
    }

    /// The sentence under the title: what the page is for.
    pub fn line(self) -> &'static str {
        match self {
            Page::Start => n_(
                "Try Edel OS from this stick, or install it on this computer. Trying changes nothing on the computer.",
            ),
            Page::Language => n_("The language Edel OS speaks to you in."),
            Page::Keyboard => n_("The keys your keyboard types. Try it below."),
            Page::WiFi => n_("Join a network now, or later from Settings' Network page."),
            Page::Account => n_(
                "The name and password you log in with. You look after this computer, so you are its administrator.",
            ),
            Page::TimeZone => n_("The time zone the clock follows."),
            Page::Layout => {
                n_("How your windows, panel and dock are arranged. Change it any time in Settings.")
            }
            Page::Style => n_("Light, dark, or following the time of day."),
            Page::Done => n_("Everything you chose here is a row in Settings, to change any time."),
        }
    }

    /// Whether the page counter counts the page: not Start or Done.
    fn counted(self) -> bool {
        !matches!(self, Page::Start | Page::Done)
    }
}

/// The pages a Welcome shows for `mode`, given the installer's questions:
/// Start on a stick, then the questions the Welcome asks in the installer's
/// order (the account and the time zone only for a new machine), Wi-Fi
/// after the keyboard when the machine has an adapter (`wifi`), then Style
/// and Done.
pub fn pages(questions: &[Question], mode: Mode, wifi: bool) -> Vec<Page> {
    let mut pages = Vec::new();
    if mode == Mode::Stick {
        pages.push(Page::Start);
    }
    for asked in questions
        .iter()
        .filter_map(|question| ASKED.iter().find(|asked| asked.0 == question.key))
        .filter(|asked| !asked.2 || mode == Mode::NewMachine)
    {
        pages.push(asked.1);
        if asked.1 == Page::Keyboard && wifi {
            pages.push(Page::WiFi);
        }
    }
    pages.push(Page::Style);
    pages.push(Page::Done);
    pages
}

/// Whether this machine has Wi-Fi to join: the `network` feature, whose
/// NetworkManager joins it, and an adapter the kernel shows.
fn wifi_here() -> bool {
    edel::places::found_shared(edel::features::DIR)
        .join("network.toml")
        .exists()
        && edel::network::has_wifi_in(std::path::Path::new("/sys"))
}

/// The page counter, `2 of 4`: page `at` (from 1) of `count` pages.
pub fn counter(at: usize, count: usize) -> String {
    trf(
        "{at} of {count}",
        &[("at", &at.to_string()), ("count", &count.to_string())],
    )
}

/// The place of `mode` in [`STYLES`]: Light when it names none or another.
fn style_at(mode: &str) -> usize {
    STYLES.iter().position(|style| *style == mode).unwrap_or(0)
}

/// The account's answers checked, in the words the installer page uses: the
/// login name as `edel install` checks it, then the two passwords the same
/// and not empty. Gives the name as it will be made.
pub fn check_account(name: &str, password: &str, again: &str) -> Result<String, String> {
    let name = edel::install::user_answer(name.trim())?;
    if password.is_empty() {
        Err(tr("a password cannot be empty").to_string())
    } else if password != again {
        Err(tr("the two passwords differ; type them again").to_string())
    } else {
        Ok(name)
    }
}

/// The words of `edel settings first-boot` that make a new machine's first
/// person `name` (M6.12), with the time zone `zone` and the person's own
/// choices `own` (each key and value) and the add-ons `addons` the person
/// chose (`addons.installed+=NAME`, last), as `edel settings set` takes them;
/// the password goes on standard input. The release's time zone is left out,
/// as writers never write a default.
pub fn first_person_words(
    name: &str,
    zone: &str,
    own: &[(&str, String)],
    addons: &[&str],
) -> Vec<String> {
    let mut words: Vec<String> = actions::FIRST_PERSON
        .args
        .iter()
        .map(|word| word.to_string())
        .collect();
    words.extend(["--user".to_string(), name.to_string()]);
    words.push("--new-password-stdin".to_string());
    if zone != edel::timezones::DEFAULT {
        words.push(format!("{ZONE_KEY}={zone}"));
    }
    words.extend(own.iter().map(|(key, value)| format!("{key}={value}")));
    words.extend(
        addons
            .iter()
            .map(|addon| format!("addons.installed+={addon}")),
    );
    words
}

/// Whether the add-on `name` is one the image offers (`offered`, by name)
/// and is neither there nor on its way: not chosen (`state`).
fn free_offer(offered: &[(String, String)], state: impl Fn(&str) -> State, name: &str) -> bool {
    offered.iter().any(|(n, _)| n == name) && !state(name).chosen() && state(name) != State::BuiltIn
}

/// The language the person's file names, or the release's: the one the
/// Language page shows chosen.
fn language_now() -> String {
    Files::here()
        .chosen(LANGUAGE_KEY)
        .unwrap_or_else(|| install::LANGUAGE.to_string())
}

/// What a page built: the places it logs, the widget that takes the keys
/// when it shows (Next when none), the button Return presses (Next when
/// none), and whether the bar of Back, Skip and Next shows.
struct Built {
    groups: Vec<Group>,
    focus: Option<gtk::Widget>,
    default: Option<gtk::Button>,
    bar: bool,
}

impl Built {
    /// A page whose controls log no places, with the bar.
    fn plain(groups: Vec<Group>) -> Built {
        Built {
            groups,
            focus: None,
            default: None,
            bar: true,
        }
    }
}

/// The window and the page it shows.
struct Welcome {
    window: gtk::ApplicationWindow,
    holder: gtk::Box,
    theme: Rc<Theme>,
    mode: Mode,
    pages: Vec<Page>,
    at: Cell<usize>,
    /// The page shown and what it logs the places of, logged again when the
    /// window changes size (a preset's bar or dock gives a maximized window
    /// another area), so the places CI clicks are where they are now.
    shown: RefCell<Option<(gtk::Box, Vec<Group>)>>,
    /// What Next checks before it leaves the page, false to stay: the
    /// account page's answers.
    leave: RefCell<Option<Box<dyn Fn() -> bool>>>,
    /// A new machine's first person, name and password, once the account
    /// page has them.
    account: RefCell<Option<(String, String)>>,
    /// A new machine's time zone, once its page has shown.
    zone: RefCell<Option<String>>,
    /// Whether the end is running (`edel settings first-boot`, then the
    /// restart), when the buttons wait.
    busy: Cell<bool>,
    /// The add-on the fonts' checkbox offers, with the checkbox on: Some(NAME)
    /// when it shows and is on, else None (M7.2b).
    fonts: RefCell<Option<&'static str>>,
    /// The Language page's fonts row and its checkbox, rebuilt with the page.
    fonts_row: RefCell<Option<(gtk::Box, gtk::CheckButton)>>,
    /// The add-ons the image offers and where each stands, read once when
    /// the window opens (`edel::addon_state`, no network).
    add_ons: (Vec<(String, String)>, Facts),
}

/// Opens the Welcome window on its first page (M6.12): one window, maximized,
/// titled Welcome, which closes the app when its last page is done or skipped.
pub fn window(app: &adw::Application) {
    let mode = Mode::here();
    let welcome = Rc::new(Welcome {
        window: gtk::ApplicationWindow::builder()
            .application(app)
            .title(tr("Welcome"))
            .default_width(960)
            .default_height(640)
            .css_classes(["edel"])
            .build(),
        holder: gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .build(),
        theme: Theme::new(),
        mode,
        pages: pages(edel::install::QUESTIONS, mode, wifi_here()),
        at: Cell::new(0),
        shown: RefCell::new(None),
        leave: RefCell::new(None),
        account: RefCell::new(None),
        zone: RefCell::new(None),
        busy: Cell::new(false),
        fonts: RefCell::new(None),
        fonts_row: RefCell::new(None),
        add_ons: if mode == Mode::Stick {
            (Vec::new(), Facts::default())
        } else {
            (addon_state::offered(), Facts::read())
        },
    });
    eprintln!("edel-settings: welcome for a {}", mode.name());
    welcome.window.set_child(Some(&welcome.holder));
    welcome.window.maximize();
    // Escape is Back. The window hears the key after the page's own widgets,
    // so a popover that is open closes first. The key controller holds the
    // Welcome for as long as the window is open.
    let keys = gtk::EventControllerKey::new();
    let held = welcome.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            held.back();
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    welcome.window.add_controller(keys);
    // The surface says when its size changes, after it is made, and the
    // page's places are logged again then.
    let weak = Rc::downgrade(&welcome);
    welcome.window.connect_realize(move |window| {
        let Some(surface) = window.surface() else {
            return;
        };
        let weak = weak.clone();
        // GDK says layout at every frame that lays the surface out; only a
        // new size moves the places.
        let size = Cell::new((0, 0));
        surface.connect_layout(move |_, width, height| {
            if size.replace((width, height)) == (width, height) {
                return;
            }
            if let Some(welcome) = weak.upgrade() {
                welcome.log_again();
            }
        });
    });
    welcome.show(0);
    welcome.window.present();
}

impl Welcome {
    /// Shows page `at` (from 0): what it asks, the counter, its title and
    /// line, Back, Skip and Next, and logs `welcome page NAME`.
    fn show(self: &Rc<Self>, at: usize) {
        let Some(&page) = self.pages.get(at) else {
            return;
        };
        self.at.set(at);
        self.leave.replace(None);
        eprintln!("edel-settings: welcome page {}", page.name());
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .hexpand(true)
            .build();
        let problem = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .visible(false)
            .css_classes(["edel-error"])
            .build();
        let built = match page {
            Page::Start => self.start(&column),
            Page::Language => Built::plain(language(self, &column, &problem)),
            Page::Keyboard => Built::plain(keyboard(&column, &problem)),
            Page::WiFi => {
                crate::network::parts(&column, &column, problem.clone());
                Built::plain(Vec::new())
            }
            Page::Account => self.account_page(&column, &problem),
            Page::TimeZone => self.zone_page(&column),
            Page::Layout => Built::plain(layout(&column, &problem, &self.theme)),
            Page::Style => Built::plain(style(&column, &problem)),
            Page::Done => self.done(&column),
        };
        column.append(&problem);
        let (root, next, buttons) = self.frame(at, page, &column, built.bar, &problem);
        let mut groups = built.groups;
        if built.bar {
            groups.insert(0, ("welcome places", buttons));
        }
        *self.shown.borrow_mut() = Some((root.clone(), groups.clone()));
        log_places(&root, groups);
        while let Some(child) = self.holder.first_child() {
            self.holder.remove(&child);
        }
        self.holder.append(&root);
        // Return presses the page's own button, else Next; the keys go to
        // the page's first answer, else to that button, so Return alone
        // accepts what each page suggests.
        let default = built.default.unwrap_or(next);
        self.window.set_default_widget(Some(&default));
        let focus = built.focus.unwrap_or_else(|| default.upcast());
        GtkWindowExt::set_focus(&self.window, Some(&focus));
    }

    /// The page's frame: the counter (on the counted pages), the title and
    /// its line, the control column centred under them, and the bar of
    /// buttons at the bottom when `bar`. Gives the root, the Next button,
    /// and the buttons' places in the order the log gives them: Next, then
    /// Skip and Back where the page shows them.
    fn frame(
        self: &Rc<Self>,
        at: usize,
        page: Page,
        column: &gtk::Box,
        bar: bool,
        problem: &gtk::Label,
    ) -> (gtk::Box, gtk::Button, Vec<(String, gtk::Widget)>) {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .margin_top(32)
            .margin_bottom(24)
            .margin_start(16)
            .margin_end(16)
            .build();
        if page.counted() {
            let count = self.pages.iter().filter(|page| page.counted()).count();
            let here = self.pages[..=at]
                .iter()
                .filter(|page| page.counted())
                .count();
            root.append(
                &gtk::Label::builder()
                    .label(counter(here, count).as_str())
                    .justify(gtk::Justification::Center)
                    .css_classes(["edel-intro"])
                    .build(),
            );
        }
        root.append(
            &gtk::Label::builder()
                .label(tr(page.title()))
                .justify(gtk::Justification::Center)
                .wrap(true)
                .css_classes(["edel-hero-title"])
                .margin_top(8)
                .build(),
        );
        root.append(
            &gtk::Label::builder()
                .label(tr(page.line()))
                .justify(gtk::Justification::Center)
                .wrap(true)
                .css_classes(["edel-hero-sub"])
                .build(),
        );
        root.append(
            &adw::Clamp::builder()
                .maximum_size(COLUMN)
                .tightening_threshold(COLUMN)
                .margin_top(24)
                .vexpand(true)
                .valign(gtk::Align::Start)
                .child(column)
                .build(),
        );

        // Back never leaves the first page the Welcome asks a question on.
        let back = widgets::action(tr("Back"), false);
        back.set_visible(at > 0 && self.pages[at - 1] != Page::Start);
        let skip = widgets::action(tr("Skip"), false);
        skip.set_visible(page != Page::Done);
        let next = widgets::big_button(
            if page == Page::Done {
                tr("Start using Edel OS")
            } else {
                tr("Next")
            },
            true,
        );
        if bar {
            let row = gtk::Box::builder()
                .spacing(12)
                .margin_top(24)
                .hexpand(true)
                .build();
            row.append(&back);
            row.append(&gtk::Box::builder().hexpand(true).build());
            row.append(&skip);
            row.append(&next);
            root.append(&row);
        }

        let weak = Rc::downgrade(self);
        back.connect_clicked(move |_| {
            if let Some(welcome) = weak.upgrade() {
                welcome.back();
            }
        });
        let weak = Rc::downgrade(self);
        skip.connect_clicked(move |_| {
            if let Some(welcome) = weak.upgrade() {
                welcome.skip();
            }
        });
        let weak = Rc::downgrade(self);
        let said = problem.clone();
        next.connect_clicked(move |next| {
            if let Some(welcome) = weak.upgrade() {
                welcome.next(next, &said);
            }
        });

        let mut buttons = vec![("next".to_string(), next.clone().upcast::<gtk::Widget>())];
        if skip.is_visible() {
            buttons.push(("skip".to_string(), skip.upcast::<gtk::Widget>()));
        }
        if back.is_visible() {
            buttons.push(("back".to_string(), back.upcast::<gtk::Widget>()));
        }
        (root, next, buttons)
    }

    /// Goes to the page before, if there is one the Welcome may go back to
    /// (Back, Escape): never to Start, and never while the end runs.
    fn back(self: &Rc<Self>) {
        let at = self.at.get();
        if at > 0 && self.pages[at - 1] != Page::Start && !self.busy.get() {
            self.show(at - 1);
        }
    }

    /// Goes to the next page once the page's own check passes; on Done,
    /// Start using Edel OS ends the Welcome: a new machine's makes its
    /// first person and restarts, any other closes it.
    fn next(self: &Rc<Self>, button: &gtk::Button, problem: &gtk::Label) {
        if self.busy.get() {
            return;
        }
        let stays = self.leave.borrow().as_ref().is_some_and(|leave| !leave());
        if stays {
            return;
        }
        let at = self.at.get();
        match self.pages.get(at) {
            Some(Page::Done) if self.mode == Mode::NewMachine => self.make_person(button, problem),
            Some(Page::Done) => {
                self.ask_fonts();
                eprintln!("edel-settings: welcome done");
                self.close();
            }
            Some(_) => self.show(at + 1),
            None => {}
        }
    }

    /// Closes the Welcome at once, the page it was on logged; nothing more is
    /// written.
    fn skip(self: &Rc<Self>) {
        if self.busy.get() {
            return;
        }
        if let Some(page) = self.pages.get(self.at.get()) {
            eprintln!("edel-settings: welcome skipped at {}", page.name());
        }
        self.close();
    }

    /// Closes the window, leaving the stamp first unless the Welcome sets a
    /// new machine up: that one opens again at the next start until it has
    /// made the person (`edel::first_start`).
    fn close(&self) {
        if self.mode != Mode::NewMachine {
            if let Err(e) = edel::first_start::mark_seen() {
                eprintln!("edel-settings: welcome could not leave its stamp: {e}");
            }
        }
        self.window.close();
    }

    /// Logs the shown page's places again, once two frames agree.
    fn log_again(&self) {
        if let Some((root, groups)) = self.shown.borrow().clone() {
            log_places(&root, groups);
        }
    }

    /// The add-on the Language page offers for the language `code`: the
    /// language's fonts when the image offers them and they are not chosen or
    /// installed yet, and never on a stick (the installer adds them there).
    fn offered_font(&self, code: &str) -> Option<&'static str> {
        if self.mode == Mode::Stick {
            return None;
        }
        let name = edel::languages::font_addon(code)?;
        let (offered, facts) = &self.add_ons;
        free_offer(offered, |n| addon_state::state(n, facts), name).then_some(name)
    }

    /// Shows the fonts row under the language choice when the language now
    /// chosen has an offer, checked and in `fonts`, and hides it, with `fonts`
    /// None, when it has none (M7.2b).
    fn update_fonts(&self) {
        let offered = self.offered_font(&language_now());
        let Some((row, check)) = self.fonts_row.borrow().clone() else {
            return;
        };
        row.set_visible(offered.is_some());
        match offered {
            Some(name) => {
                // Checked by default each time it shows; the toggle logs it
                // and sets `fonts` too, so a check that was on sets it here.
                check.set_active(true);
                self.fonts.replace(Some(name));
            }
            None => {
                self.fonts.replace(None);
            }
        }
    }

    /// The fonts checkbox was toggled: `fonts` names the add-on while it is on
    /// (M7.2b), and the log says which way it went.
    fn fonts_toggled(&self, on: bool) {
        let Some(name) = self.offered_font(&language_now()) else {
            self.fonts.replace(None);
            return;
        };
        if on {
            self.fonts.replace(Some(name));
            eprintln!("edel-settings: welcome fonts {name} chosen");
        } else {
            self.fonts.replace(None);
            eprintln!("edel-settings: welcome fonts {name} not chosen");
        }
    }

    /// A person's fonts choice, made at the end (Start using Edel OS, M7.2b):
    /// `edel settings set addons.installed+=NAME` and `apply` as root, on a
    /// thread the window does not wait for. The application is held until it
    /// has run, so closing the Welcome does not stop the command.
    fn ask_fonts(&self) {
        let Some(name) = *self.fonts.borrow() else {
            return;
        };
        let Some(app) = self.window.application() else {
            return;
        };
        let hold = app.hold();
        let words = vec![
            "settings".to_string(),
            "set".to_string(),
            format!("addons.installed+={name}"),
        ];
        glib::spawn_future_local(async move {
            let done = gio::spawn_blocking(move || cmd::set_and_apply_as_root(words)).await;
            let code = match done {
                Ok((set, _)) => set.code,
                Err(_) => None,
            };
            eprintln!(
                "edel-settings: welcome fonts {name} asked, exit {}",
                crate::widgets::exit_text(code)
            );
            drop(hold);
        });
    }

    /// The stick's first page: Install Edel OS, which Return presses, and
    /// Try Edel OS, two big buttons in the column and no bar.
    fn start(self: &Rc<Self>, column: &gtk::Box) -> Built {
        let install = widgets::big_button(tr("Install Edel OS"), true);
        let try_it = widgets::big_button(tr("Try Edel OS"), false);
        for button in [&install, &try_it] {
            button.set_halign(gtk::Align::Center);
            button.set_width_request(CONTROL);
            column.append(button);
        }
        let weak = Rc::downgrade(self);
        install.connect_clicked(move |_| {
            if let Some(welcome) = weak.upgrade() {
                welcome.install();
            }
        });
        let weak = Rc::downgrade(self);
        try_it.connect_clicked(move |_| {
            if let Some(welcome) = weak.upgrade() {
                eprintln!("edel-settings: welcome try chosen");
                welcome.show(welcome.at.get() + 1);
            }
        });
        Built {
            groups: vec![(
                "welcome places",
                vec![
                    ("install".to_string(), install.clone().upcast()),
                    ("try".to_string(), try_it.upcast()),
                ],
            )],
            focus: None,
            default: Some(install),
            bar: false,
        }
    }

    /// Install Edel OS: Settings opens on its installer page (the first page
    /// on a stick, M6.6a), and the Welcome closes.
    fn install(&self) {
        eprintln!("edel-settings: welcome install chosen");
        if let Some(app) = self.window.application().and_downcast::<adw::Application>() {
            crate::window(&app, 0, false);
        }
        self.close();
    }

    /// A new machine's account page: the login name and the password twice,
    /// each Return moving to the next and the last pressing Next, which
    /// checks them and keeps them for the end.
    fn account_page(self: &Rc<Self>, column: &gtk::Box, problem: &gtk::Label) -> Built {
        let (name_now, password_now) = self.account.borrow().clone().unwrap_or_default();
        let name = gtk::Entry::builder()
            .text(name_now.as_str())
            .width_request(CONTROL)
            .halign(gtk::Align::Center)
            .css_classes(["edel-entry"])
            .build();
        let password = password_entry(&password_now);
        let again = password_entry(&password_now);
        again.set_activates_default(true);
        for (title, entry) in [
            (tr("Your username"), name.clone().upcast::<gtk::Widget>()),
            (tr("Your password"), password.clone().upcast()),
            (tr("The same password again"), again.clone().upcast()),
        ] {
            column.append(
                &gtk::Label::builder()
                    .label(title)
                    .width_request(CONTROL)
                    .halign(gtk::Align::Center)
                    .xalign(0.0)
                    .margin_top(8)
                    .css_classes(["edel-row-subtitle"])
                    .build(),
            );
            column.append(&entry);
        }
        let to = password.clone();
        name.connect_activate(move |_| {
            to.grab_focus();
        });
        let to = again.clone();
        password.connect_activate(move |_| {
            to.grab_focus();
        });
        let weak = Rc::downgrade(self);
        let (entries, problem) = ((name.clone(), password, again), problem.clone());
        *self.leave.borrow_mut() = Some(Box::new(move || {
            let Some(welcome) = weak.upgrade() else {
                return false;
            };
            let (name, password, again) = &entries;
            match check_account(&name.text(), &password.text(), &again.text()) {
                Ok(checked) => {
                    eprintln!("edel-settings: welcome account {checked} kept");
                    welcome
                        .account
                        .replace(Some((checked, password.text().to_string())));
                    true
                }
                Err(why) => {
                    problem.set_label(&why);
                    problem.set_visible(true);
                    false
                }
            }
        }));
        Built {
            groups: Vec::new(),
            focus: Some(name.upcast()),
            default: None,
            bar: true,
        }
    }

    /// A new machine's time zone page: the zones as a searchable list, the
    /// one chosen before or the language's suggestion chosen. It is the
    /// machine's, so the choice is kept for the end.
    fn zone_page(self: &Rc<Self>, column: &gtk::Box) -> Built {
        let language = Files::here()
            .chosen(LANGUAGE_KEY)
            .unwrap_or_else(|| install::LANGUAGE.to_string());
        let now = self
            .zone
            .borrow()
            .clone()
            .unwrap_or_else(|| edel::timezones::suggested(&language).to_string());
        self.zone.replace(Some(now.clone()));
        let zones = install::zone_list(&now);
        let labels = zones
            .iter()
            .map(|zone| edel::timezones::label(zone))
            .collect();
        let choice = widgets::Choice::searchable(labels);
        choice.set_selected(zones.iter().position(|zone| *zone == now).unwrap_or(0));
        choice.set_width(CONTROL);
        choice.widget().set_halign(gtk::Align::Center);
        let (weak, picked) = (Rc::downgrade(&choice), Rc::downgrade(self));
        choice.connect_changed(move || {
            let (Some(choice), Some(welcome)) = (weak.upgrade(), picked.upgrade()) else {
                return;
            };
            if let Some(zone) = zones.get(choice.selected()) {
                eprintln!("edel-settings: welcome {ZONE_KEY} {zone} chosen");
                welcome.zone.replace(Some(zone.clone()));
            }
        });
        column.append(&choice.widget());
        keep(column, choice);
        Built::plain(Vec::new())
    }

    /// The last page. A new machine's says whose account Start using Edel
    /// OS makes, and that the computer restarts once.
    fn done(self: &Rc<Self>, column: &gtk::Box) -> Built {
        let name = self.account.borrow().as_ref().map(|(name, _)| name.clone());
        if let (Mode::NewMachine, Some(name)) = (self.mode, name) {
            column.append(
                &gtk::Label::builder()
                    .label(trf(
                        "Start using Edel OS makes the account {name} and restarts the computer once. Then log in as {name} with your password.",
                        &[("name", &name)],
                    ))
                    .justify(gtk::Justification::Center)
                    .wrap(true)
                    .build(),
            );
        }
        Built::plain(Vec::new())
    }

    /// A new machine's end: `edel settings first-boot` makes the person with
    /// the time zone and the choices made, as root through doas, the
    /// password on standard input; then the computer restarts, so the login
    /// screen asks for them. A refusal shows on the page as `edel` said it,
    /// and the buttons come back.
    fn make_person(self: &Rc<Self>, button: &gtk::Button, problem: &gtk::Label) {
        let Some((name, password)) = self.account.borrow().clone() else {
            // Skipped past the account page: there is nobody to make.
            problem.set_label(tr(
                "Go back to Your account and give a name and a password.",
            ));
            problem.set_visible(true);
            return;
        };
        let zone = self
            .zone
            .borrow()
            .clone()
            .unwrap_or_else(|| edel::timezones::DEFAULT.to_string());
        let files = Files::here();
        let own: Vec<(&str, String)> = OWN_KEYS
            .iter()
            .filter(|key| matches!(files.source(key), Source::Person(_)))
            .filter_map(|key| files.chosen(key).map(|value| (*key, value)))
            .collect();
        let addons: Vec<&str> = self.fonts.borrow().iter().copied().collect();
        let words = first_person_words(&name, &zone, &own, &addons);
        self.busy.set(true);
        button.set_sensitive(false);
        button.set_label(tr("Making your account"));
        problem.set_visible(false);
        let (weak, button, problem) = (Rc::downgrade(self), button.clone(), problem.clone());
        glib::spawn_future_local(async move {
            let done = gio::spawn_blocking(move || {
                let args: Vec<&str> = words.iter().map(String::as_str).collect();
                let made = cmd::edel_as_root(&args, Some(password));
                let restart = made.ok.then(cmd::restart);
                (made, restart)
            })
            .await;
            let Some(welcome) = weak.upgrade() else {
                return;
            };
            let said = match done {
                Ok((made, restart)) => {
                    eprintln!(
                        "edel-settings: welcome first person {name}, exit {}",
                        made.code
                            .map_or("none".to_string(), |code| code.to_string())
                    );
                    match restart {
                        None => status::said(&made.shown()),
                        Some(restart) if restart.ok => {
                            eprintln!("edel-settings: welcome restart asked");
                            return;
                        }
                        Some(restart) => trf(
                            "{name} can log in now, but the computer did not restart: {why}. Restart it to log in as {name}.",
                            &[
                                ("name", &name),
                                ("why", status::unprefixed(&restart.shown())),
                            ],
                        ),
                    }
                }
                Err(_) => tr("Your account could not be made, as the command stopped without an answer. Press Start using Edel OS again.").to_string(),
            };
            welcome.busy.set(false);
            button.set_sensitive(true);
            button.set_label(tr("Start using Edel OS"));
            problem.set_label(&said);
            problem.set_visible(true);
        });
    }
}

/// A password entry with the eye that shows it, `text` in it.
fn password_entry(text: &str) -> gtk::PasswordEntry {
    gtk::PasswordEntry::builder()
        .text(text)
        .show_peek_icon(true)
        .width_request(CONTROL)
        .halign(gtk::Align::Center)
        .css_classes(["edel-entry"])
        .build()
}

/// Keeps `held` (a choice and the callbacks it holds) for as long as the
/// page's `column` lives: the page keeps only weak references to it, so it
/// would be dropped when the page is built, and its clicks would do nothing.
fn keep<T: 'static>(column: &gtk::Box, held: Rc<T>) {
    column.connect_destroy(move |_| {
        let _ = &held;
    });
}

/// Chooses `value` for `key` as the Settings pages do (`Files::choose_over`,
/// so a value that applies anyway is taken out, never written) and logs what
/// was done for CI; a refusal shows under the page's controls.
fn write(key: &str, value: &str, release: &str, problem: &gtk::Label) {
    problem.set_visible(false);
    match Files::here().choose_over(key, value, Some(release)) {
        Ok(()) => {
            let written = matches!(Files::here().source(key), Source::Person(_));
            eprintln!(
                "edel-settings: welcome {key} {value} {}",
                if written { "written" } else { "taken out" }
            );
        }
        Err(why) => {
            problem.set_label(&why);
            problem.set_visible(true);
        }
    }
}

/// The language page: the languages the image has, the one the person's
/// file names (or the release's) chosen. A pick writes `region.language`
/// and, while the keyboard still follows the language, the keyboard of the
/// new language too, as Install does.
fn language(welcome: &Rc<Welcome>, column: &gtk::Box, problem: &gtk::Label) -> Vec<Group> {
    let now = language_now();
    let (codes, labels) = region::language_offer(&now);
    let at = codes.iter().position(|code| *code == now).unwrap_or(0);
    let choice = widgets::Choice::searchable(labels);
    choice.set_selected(at);
    choice.set_width(CONTROL);
    choice.widget().set_halign(gtk::Align::Center);
    let weak = Rc::downgrade(&choice);
    let (held, problem) = (Rc::downgrade(welcome), problem.clone());
    choice.connect_changed(move || {
        let Some(choice) = weak.upgrade() else {
            return;
        };
        if let Some(code) = codes.get(choice.selected()) {
            language_picked(code, &problem);
            if let Some(welcome) = held.upgrade() {
                welcome.update_fonts();
            }
        }
    });
    // The fonts row under the choice, hidden until a language needs fonts the
    // image offers (M7.2b): a checkbox on by default and a line saying when
    // they arrive.
    let fonts = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .width_request(CONTROL)
        .halign(gtk::Align::Center)
        .margin_top(8)
        .visible(false)
        .build();
    let check = gtk::CheckButton::builder()
        .label(tr("Add the fonts this language needs"))
        .active(true)
        .halign(gtk::Align::Start)
        .build();
    let line = gtk::Label::builder()
        .label(tr(
            "They download once this computer is online and show after the next restart.",
        ))
        .wrap(true)
        .xalign(0.0)
        .css_classes(["edel-row-subtitle"])
        .build();
    fonts.append(&check);
    fonts.append(&line);
    let weak_fonts = Rc::downgrade(welcome);
    check.connect_toggled(move |check| {
        if let Some(welcome) = weak_fonts.upgrade() {
            welcome.fonts_toggled(check.is_active());
        }
    });
    welcome
        .fonts_row
        .replace(Some((fonts.clone(), check.clone())));
    column.append(&choice.widget());
    column.append(&fonts);
    keep(column, choice);
    welcome.update_fonts();
    Vec::new()
}

/// The language was picked: writes it, and the keyboard too when the
/// keyboard still has the layout the old language suggested (the person has
/// not picked one of their own), the new language's suggestion instead.
fn language_picked(code: &str, problem: &gtk::Label) {
    let before = Files::here()
        .chosen(LANGUAGE_KEY)
        .unwrap_or_else(|| install::LANGUAGE.to_string());
    if before.as_str() == code {
        return;
    }
    let keyboard = Files::here()
        .chosen(KEYBOARD_KEY)
        .unwrap_or_else(|| files::release_value(KEYBOARD_KEY));
    let old = edel::install::suggested_keyboard(&before);
    let follows = install::first_layout(&keyboard) == install::first_layout(&old);
    write(LANGUAGE_KEY, code, install::LANGUAGE, problem);
    if follows {
        let suggested = edel::install::suggested_keyboard(code);
        let layout = edel::install::keyboard_for(install::first_layout(&suggested));
        write(
            KEYBOARD_KEY,
            &layout,
            &files::release_value(KEYBOARD_KEY),
            problem,
        );
    }
}

/// The keyboard page: the layouts the rules file names, with the layout that
/// applies chosen, a pick writes `region.keyboard` as the installer does, and
/// a box under the choice tries the layout as it types.
fn keyboard(column: &gtk::Box, problem: &gtk::Label) -> Vec<Group> {
    let rules = std::fs::read_to_string(edel::keyboard::RULES).ok();
    let applied = Files::here()
        .chosen(KEYBOARD_KEY)
        .unwrap_or_else(|| files::release_value(KEYBOARD_KEY));
    let suggested = install::first_layout(&applied).to_string();
    let (values, names) = install::keyboard_offer(rules.as_deref(), &suggested);
    let at = values
        .iter()
        .position(|value| *value == suggested)
        .unwrap_or(0);
    let choice = widgets::Choice::searchable(names);
    choice.set_selected(at);
    choice.set_width(CONTROL);
    choice.widget().set_halign(gtk::Align::Center);
    let weak = Rc::downgrade(&choice);
    let problem = problem.clone();
    choice.connect_changed(move || {
        let Some(choice) = weak.upgrade() else {
            return;
        };
        if let Some(layout) = values.get(choice.selected()) {
            let value = edel::install::keyboard_for(layout);
            write(
                KEYBOARD_KEY,
                &value,
                &files::release_value(KEYBOARD_KEY),
                &problem,
            );
        }
    });
    let hint = gtk::Label::builder()
        .label(tr("Try it here:"))
        .xalign(0.0)
        .margin_top(16)
        .css_classes(["edel-row-subtitle"])
        .build();
    // Return in the box is Next, as on every page (the window's default).
    let trying = gtk::Entry::builder()
        .hexpand(true)
        .activates_default(true)
        .build();
    column.append(&choice.widget());
    column.append(&hint);
    column.append(&trying);
    keep(column, choice);
    Vec::new()
}

/// The layout page: the built-in presets as pictures, three to a line, the
/// one that applies chosen (Classic when the file names none). A card writes
/// `layout.preset`.
fn layout(column: &gtk::Box, problem: &gtk::Label, theme: &Rc<Theme>) -> Vec<Group> {
    let applied = Files::here()
        .chosen(LAYOUT_KEY)
        .unwrap_or_else(|| edel::presets::DEFAULT.to_string());
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(12)
        .row_spacing(12)
        .halign(gtk::Align::Center)
        .build();
    let mut cards: Vec<(String, gtk::ToggleButton)> = Vec::new();
    for row in edel::presets::rows(&edel::settings::SettingsFile::default()) {
        // A card's picture is its preset's, drawn from the tokens.
        let picture = preview::area(&row.base, theme).upcast::<gtk::Widget>();
        let (card, badge) = widgets::picture_card(&row.title, &picture);
        // The check shows on the chosen card only, as on the Layout page.
        badge.set_visible(row.name == applied);
        card.connect_toggled(move |card| badge.set_visible(card.is_active()));
        if let Some((_, first)) = cards.first() {
            card.set_group(Some(first));
        }
        card.set_active(row.name == applied);
        flow.append(&card);
        // The card takes the focus, not the box the flow wraps it in.
        if let Some(child) = card.parent() {
            child.set_focusable(false);
        }
        cards.push((row.name, card));
    }
    for (name, card) in &cards {
        let (name, problem) = (name.clone(), problem.clone());
        card.connect_toggled(move |card| {
            if card.is_active() {
                write(LAYOUT_KEY, &name, edel::presets::DEFAULT, &problem);
            }
        });
    }
    let items = cards
        .into_iter()
        .map(|(name, card)| (name, card.upcast::<gtk::Widget>()))
        .collect();
    column.append(&flow);
    vec![("welcome presets places", items)]
}

/// The style page: Light, Dark and Automatic, the one in use chosen (the
/// release's, Automatic, when none is set). A choice writes `appearance.mode`.
fn style(column: &gtk::Box, problem: &gtk::Label) -> Vec<Group> {
    let applied = Files::here()
        .chosen(STYLE_KEY)
        .unwrap_or_else(|| files::release_value(STYLE_KEY));
    let labels = [tr("Light"), tr("Dark"), tr("Automatic")].map(|label| label.to_string());
    let segments = widgets::Segments::new(&labels);
    segments.set_selected(style_at(&applied));
    segments.widget().set_halign(gtk::Align::Center);
    let weak = Rc::downgrade(&segments);
    let problem = problem.clone();
    segments.connect_changed(move || {
        let Some(segments) = weak.upgrade() else {
            return;
        };
        if let Some(mode) = STYLES.get(segments.selected()) {
            write(STYLE_KEY, mode, &files::release_value(STYLE_KEY), &problem);
        }
    });
    column.append(&segments.widget());
    let items = STYLES
        .iter()
        .zip(segments.buttons())
        .map(|(name, button)| (name.to_string(), button.upcast::<gtk::Widget>()))
        .collect();
    keep(column, segments);
    vec![("welcome style places", items)]
}

/// Logs where the page's places lie, once two frames agree, as one line per
/// group (`edel-settings: welcome places next X+Y+WxH, ...`) for CI, in
/// logical pixels from the window's corner. A page that has left the window
/// stops logging.
fn log_places(root: &gtk::Box, groups: Vec<Group>) {
    let last = RefCell::new(String::new());
    root.add_tick_callback(move |root, _| {
        let Some(window) = root.root().and_downcast::<gtk::Window>() else {
            return gtk::glib::ControlFlow::Break;
        };
        if !root.is_mapped() {
            return gtk::glib::ControlFlow::Continue;
        }
        let lines: Vec<String> = groups
            .iter()
            .map(|(words, items)| {
                let places: Vec<String> = items
                    .iter()
                    .map(|(name, widget)| {
                        format!("{name} {}", widgets::place(widget.compute_bounds(&window)))
                    })
                    .collect();
                format!("edel-settings: {words} {}", places.join(", "))
            })
            .collect();
        let text = lines.join("\n");
        if last.replace(text.clone()) != text {
            return gtk::glib::ControlFlow::Continue;
        }
        for line in &lines {
            eprintln!("{line}");
        }
        gtk::glib::ControlFlow::Break
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_welcome_starts_from_the_releases_style_and_keyboard() {
        // Automatic, as Appearance shows for the same file: picking Light
        // must write it, or the screen turns dark at sunset.
        assert_eq!(files::release_value(STYLE_KEY), "auto");
        assert_eq!(files::release_value(KEYBOARD_KEY), "us");
        assert_eq!(style_at(&files::release_value(STYLE_KEY)), 2);
    }

    #[test]
    fn a_person_s_welcome_asks_language_keyboard_layout_style_then_done() {
        let pages = pages(edel::install::QUESTIONS, Mode::Person, false);
        assert_eq!(
            pages,
            [
                Page::Language,
                Page::Keyboard,
                Page::Layout,
                Page::Style,
                Page::Done
            ]
        );
        let names: Vec<&str> = pages.iter().map(|page| page.name()).collect();
        assert_eq!(names, ["language", "keyboard", "layout", "style", "done"]);
    }

    #[test]
    fn a_stick_asks_try_or_install_first() {
        assert_eq!(
            pages(edel::install::QUESTIONS, Mode::Stick, false),
            [
                Page::Start,
                Page::Language,
                Page::Keyboard,
                Page::Layout,
                Page::Style,
                Page::Done
            ]
        );
    }

    #[test]
    fn a_new_machine_asks_the_account_and_the_time_zone_where_the_installer_does() {
        assert_eq!(
            pages(edel::install::QUESTIONS, Mode::NewMachine, false),
            [
                Page::Language,
                Page::Keyboard,
                Page::Account,
                Page::TimeZone,
                Page::Layout,
                Page::Style,
                Page::Done
            ]
        );
    }

    #[test]
    fn the_installer_s_order_is_followed_not_the_welcome_s_own() {
        let question = |key| Question {
            key,
            title: "",
            more: false,
        };
        let questions = [
            question("layout.preset"),
            question("user"),
            question("password"),
            question("region.keyboard"),
            question("region.language"),
        ];
        assert_eq!(
            pages(&questions, Mode::NewMachine, false),
            [
                Page::Layout,
                Page::Account,
                Page::Keyboard,
                Page::Language,
                Page::Style,
                Page::Done
            ]
        );
        assert_eq!(pages(&[], Mode::Person, false), [Page::Style, Page::Done]);
    }

    #[test]
    fn wi_fi_comes_after_the_keyboard_when_there_is_an_adapter() {
        assert_eq!(
            pages(edel::install::QUESTIONS, Mode::NewMachine, true),
            [
                Page::Language,
                Page::Keyboard,
                Page::WiFi,
                Page::Account,
                Page::TimeZone,
                Page::Layout,
                Page::Style,
                Page::Done
            ]
        );
        let stick = pages(edel::install::QUESTIONS, Mode::Stick, true);
        assert_eq!(
            stick[..4],
            [Page::Start, Page::Language, Page::Keyboard, Page::WiFi]
        );
        assert_eq!(Page::WiFi.name(), "wifi");
    }

    #[test]
    fn the_mode_is_the_stick_then_live_off_a_stick_then_a_person() {
        assert_eq!(Mode::of(true, Some("live")), Mode::Stick);
        assert_eq!(Mode::of(true, Some("ali")), Mode::Stick);
        assert_eq!(Mode::of(false, Some("live")), Mode::NewMachine);
        assert_eq!(Mode::of(false, Some("ali")), Mode::Person);
        assert_eq!(Mode::of(false, None), Mode::Person);
    }

    #[test]
    fn the_counter_counts_the_pages_but_start_and_done() {
        let pages = pages(edel::install::QUESTIONS, Mode::Stick, false);
        assert_eq!(pages.iter().filter(|page| page.counted()).count(), 4);
        assert_eq!(counter(1, 4), "1 of 4");
        assert_eq!(counter(4, 4), "4 of 4");
    }

    #[test]
    fn the_style_a_file_names_is_its_place_in_the_styles() {
        assert_eq!(style_at("light"), 0);
        assert_eq!(style_at("dark"), 1);
        assert_eq!(style_at("auto"), 2);
        assert_eq!(style_at("nonsense"), 0, "Light when it names another");
    }

    #[test]
    fn the_account_is_checked_as_the_installer_checks_it() {
        assert_eq!(check_account(" ali ", "pw", "pw"), Ok("ali".to_string()));
        assert!(check_account("root", "pw", "pw").is_err());
        assert!(check_account("live", "pw", "pw").is_err());
        assert!(check_account("Ali Baba", "pw", "pw").is_err());
        assert_eq!(
            check_account("ali", "", ""),
            Err("a password cannot be empty".to_string())
        );
        assert_eq!(
            check_account("ali", "pw", "pW"),
            Err("the two passwords differ; type them again".to_string())
        );
    }

    #[test]
    fn the_first_person_is_made_with_the_zone_and_the_own_choices_never_a_default() {
        let own = [
            ("layout.preset", "mac-like".to_string()),
            ("appearance.mode", "dark".to_string()),
        ];
        assert_eq!(
            first_person_words("ali", "Europe/Berlin", &own, &[]),
            [
                "settings",
                "first-boot",
                "--user",
                "ali",
                "--new-password-stdin",
                "region.timezone=Europe/Berlin",
                "layout.preset=mac-like",
                "appearance.mode=dark"
            ]
        );
        assert_eq!(
            first_person_words("ali", edel::timezones::DEFAULT, &[], &[]),
            [
                "settings",
                "first-boot",
                "--user",
                "ali",
                "--new-password-stdin"
            ]
        );
    }

    #[test]
    fn the_chosen_fonts_add_on_comes_last_in_the_first_person_words() {
        let own = [("appearance.mode", "dark".to_string())];
        let words = first_person_words("ali", "Asia/Tokyo", &own, &["fonts-cjk"]);
        assert_eq!(
            words.last().map(String::as_str),
            Some("addons.installed+=fonts-cjk")
        );
        assert_eq!(words[words.len() - 2], "appearance.mode=dark");
    }

    #[test]
    fn the_fonts_are_offered_only_while_not_chosen() {
        let offered = vec![("fonts-cjk".to_string(), "CJK fonts".to_string())];
        let facts = |wanted: &[&str], built_in: &[&str]| Facts {
            wanted: wanted.iter().map(|n| n.to_string()).collect(),
            built_in: built_in.iter().map(|n| n.to_string()).collect(),
            ..Facts::default()
        };
        let free = |f: Facts, name: &str| free_offer(&offered, |n| addon_state::state(n, &f), name);
        assert!(free(facts(&[], &[]), "fonts-cjk"));
        assert!(!free(facts(&["fonts-cjk"], &[]), "fonts-cjk"));
        assert!(!free(facts(&[], &["fonts-cjk"]), "fonts-cjk"));
        assert!(!free(facts(&[], &[]), "fonts-other"));
    }
}
