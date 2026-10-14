//! The Welcome (roadmap M6.12, part 1, ADR-008): the first start on a
//! machine nobody has set up. `edel-settings --welcome` opens one window,
//! maximized, that asks the person's own questions a page at a time: the
//! language, the keyboard (tried in a box under it), the layout (the presets
//! as pictures) and the style, then Done. The questions and their order are
//! the installer's (`edel::install::QUESTIONS`), so the two ask alike.
//!
//! Each choice is written when it is made, with `Files::choose_over` as the
//! Settings pages write theirs, so the desktop follows at once and a value
//! that applies anyway is taken out, never written (ADR-008). Next writes
//! nothing, and Skip closes the Welcome writing nothing more. Each page shows
//! what applies now when it opens, so going Back shows the choice made. Part
//! 2 (the machine's questions, Wi-Fi, the account, Try or Install) is not
//! here yet.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use edel::i18n::{n_, tr, trf};
use edel::install::Question;
use edel::settings::Source;

use crate::files::Files;
use crate::install;
use crate::style::Theme;
use crate::{preview, region, widgets};

/// The settings keys the Welcome chooses, as `edel settings` names them.
const LANGUAGE_KEY: &str = "region.language";
const KEYBOARD_KEY: &str = "region.keyboard";
const LAYOUT_KEY: &str = "layout.preset";
const STYLE_KEY: &str = "appearance.mode";

/// The keyboard layout the release gives (`region.keyboard`).
const RELEASE_KEYBOARD: &str = "us";
/// The style the release gives (`appearance.mode`).
const RELEASE_STYLE: &str = "light";
/// The styles, as `appearance.mode` takes them, in the order shown.
const STYLES: [&str; 3] = ["light", "dark", "auto"];

/// The questions the Welcome asks, the person's own, each with its page.
const ASKED: [(&str, Page); 3] = [
    (LANGUAGE_KEY, Page::Language),
    (KEYBOARD_KEY, Page::Keyboard),
    (LAYOUT_KEY, Page::Layout),
];

/// The width of the column the controls sit in, and of each control, in
/// logical pixels: a column at most this wide, centred on the page.
const COLUMN: i32 = 560;
const CONTROL: i32 = 360;

/// A group of places the log gives: its line's words and each item's name
/// and widget.
type Group = (&'static str, Vec<(String, gtk::Widget)>);

/// One page of the Welcome, in the order they show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// The language (`region.language`).
    Language,
    /// The keyboard layout (`region.keyboard`), tried in a box below it.
    Keyboard,
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
            Page::Language => "language",
            Page::Keyboard => "keyboard",
            Page::Layout => "layout",
            Page::Style => "style",
            Page::Done => "done",
        }
    }

    /// The page's title, as people read it.
    pub fn title(self) -> &'static str {
        match self {
            Page::Language => n_("Language"),
            Page::Keyboard => n_("Keyboard"),
            Page::Layout => n_("Layout"),
            Page::Style => n_("Style"),
            Page::Done => n_("You are all set"),
        }
    }

    /// The sentence under the title: what the page is for.
    pub fn line(self) -> &'static str {
        match self {
            Page::Language => n_("The language Edel OS speaks to you in."),
            Page::Keyboard => n_("The keys your keyboard types. Try it below."),
            Page::Layout => {
                n_("How your windows, panel and dock are arranged. Change it any time in Settings.")
            }
            Page::Style => n_("Light, dark, or following the time of day."),
            Page::Done => n_("Everything you chose here is a row in Settings, to change any time."),
        }
    }
}

/// The pages a Welcome shows, given the installer's questions: the person's
/// own, in the installer's order, then Style and Done.
pub fn pages(questions: &[Question]) -> Vec<Page> {
    let mut pages: Vec<Page> = welcome_keys(questions)
        .into_iter()
        .filter_map(|key| {
            ASKED
                .iter()
                .find(|asked| asked.0 == key)
                .map(|asked| asked.1)
        })
        .collect();
    pages.push(Page::Style);
    pages.push(Page::Done);
    pages
}

/// The keys of `questions` the Welcome asks, in the installer's order: the
/// person's own (`region.language`, `region.keyboard` and `layout.preset`).
/// The machine's questions are left out until part 2.
pub fn welcome_keys(questions: &[Question]) -> Vec<&'static str> {
    questions
        .iter()
        .filter_map(|question| {
            ASKED
                .iter()
                .find(|asked| asked.0 == question.key)
                .map(|asked| asked.0)
        })
        .collect()
}

/// The pages counted in the page counter: all but Done.
fn counted(pages: &[Page]) -> usize {
    pages.iter().filter(|page| **page != Page::Done).count()
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

/// The window and the page it shows.
struct Welcome {
    window: gtk::ApplicationWindow,
    holder: gtk::Box,
    theme: Rc<Theme>,
    pages: Vec<Page>,
    at: Cell<usize>,
    /// The page shown and what it logs the places of, logged again when the
    /// window changes size (a preset's bar or dock gives a maximized window
    /// another area), so the places CI clicks are where they are now.
    shown: RefCell<Option<(gtk::Box, Vec<Group>)>>,
}

/// Opens the Welcome window on its first page (M6.12): one window, maximized,
/// titled Welcome, which closes the app when its last page is done or skipped.
pub fn window(app: &adw::Application) {
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
        pages: pages(edel::install::QUESTIONS),
        at: Cell::new(0),
        shown: RefCell::new(None),
    });
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
        let mut groups = match page {
            Page::Language => language(&column, &problem),
            Page::Keyboard => keyboard(&column, &problem),
            Page::Layout => layout(&column, &problem, &self.theme),
            Page::Style => style(&column, &problem),
            Page::Done => Vec::new(),
        };
        column.append(&problem);
        let (root, next, buttons) = self.frame(at, page, &column);
        groups.insert(0, ("welcome places", buttons));
        *self.shown.borrow_mut() = Some((root.clone(), groups.clone()));
        log_places(&root, groups);
        while let Some(child) = self.holder.first_child() {
            self.holder.remove(&child);
        }
        self.holder.append(&root);
        self.window.set_default_widget(Some(&next));
    }

    /// The page's frame: the counter (not on Done), the title and its line,
    /// the control column centred under them, and the buttons at the bottom.
    /// Gives the root, the Next button, and the buttons' places in the order
    /// the log gives them: Next, then Skip and Back where the page shows them.
    fn frame(
        self: &Rc<Self>,
        at: usize,
        page: Page,
        column: &gtk::Box,
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
        if page != Page::Done {
            let count = counter(at + 1, counted(&self.pages));
            root.append(
                &gtk::Label::builder()
                    .label(count.as_str())
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

        let back = widgets::action(tr("Back"), false);
        back.set_visible(at > 0);
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
        let bar = gtk::Box::builder()
            .spacing(12)
            .margin_top(24)
            .hexpand(true)
            .build();
        bar.append(&back);
        bar.append(&gtk::Box::builder().hexpand(true).build());
        bar.append(&skip);
        bar.append(&next);
        root.append(&bar);

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
        next.connect_clicked(move |_| {
            if let Some(welcome) = weak.upgrade() {
                welcome.next();
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

    /// Goes to the page before, if there is one (Back, Escape).
    fn back(self: &Rc<Self>) {
        let at = self.at.get();
        if at > 0 {
            self.show(at - 1);
        }
    }

    /// Goes to the next page; on Done, Start using Edel OS logs the end and
    /// closes the Welcome.
    fn next(self: &Rc<Self>) {
        let at = self.at.get();
        match self.pages.get(at) {
            Some(Page::Done) => {
                eprintln!("edel-settings: welcome done");
                self.window.close();
            }
            Some(_) => self.show(at + 1),
            None => {}
        }
    }

    /// Closes the Welcome at once, the page it was on logged; nothing more is
    /// written.
    fn skip(self: &Rc<Self>) {
        if let Some(page) = self.pages.get(self.at.get()) {
            eprintln!("edel-settings: welcome skipped at {}", page.name());
        }
        self.window.close();
    }

    /// Logs the shown page's places again, once two frames agree.
    fn log_again(&self) {
        if let Some((root, groups)) = self.shown.borrow().clone() {
            log_places(&root, groups);
        }
    }
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
fn language(column: &gtk::Box, problem: &gtk::Label) -> Vec<Group> {
    let now = Files::here()
        .chosen(LANGUAGE_KEY)
        .unwrap_or_else(|| install::LANGUAGE.to_string());
    let (codes, labels) = region::language_offer(&now);
    let at = codes.iter().position(|code| *code == now).unwrap_or(0);
    let choice = widgets::Choice::searchable(labels);
    choice.set_selected(at);
    choice.set_width(CONTROL);
    choice.widget().set_halign(gtk::Align::Center);
    let weak = Rc::downgrade(&choice);
    let problem = problem.clone();
    choice.connect_changed(move || {
        let Some(choice) = weak.upgrade() else {
            return;
        };
        if let Some(code) = codes.get(choice.selected()) {
            language_picked(code, &problem);
        }
    });
    column.append(&choice.widget());
    keep(column, choice);
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
        .unwrap_or_else(|| RELEASE_KEYBOARD.to_string());
    let old = edel::install::suggested_keyboard(&before);
    let follows = install::first_layout(&keyboard) == install::first_layout(&old);
    write(LANGUAGE_KEY, code, install::LANGUAGE, problem);
    if follows {
        let suggested = edel::install::suggested_keyboard(code);
        let layout = edel::install::keyboard_for(install::first_layout(&suggested));
        write(KEYBOARD_KEY, &layout, RELEASE_KEYBOARD, problem);
    }
}

/// The keyboard page: the layouts the rules file names, with the layout that
/// applies chosen, a pick writes `region.keyboard` as the installer does, and
/// a box under the choice tries the layout as it types.
fn keyboard(column: &gtk::Box, problem: &gtk::Label) -> Vec<Group> {
    let rules = std::fs::read_to_string(edel::keyboard::RULES).ok();
    let applied = Files::here()
        .chosen(KEYBOARD_KEY)
        .unwrap_or_else(|| RELEASE_KEYBOARD.to_string());
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
            write(KEYBOARD_KEY, &value, RELEASE_KEYBOARD, &problem);
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

/// The style page: Light, Dark and Automatic, the one in use chosen (Light
/// when none is set). A choice writes `appearance.mode`.
fn style(column: &gtk::Box, problem: &gtk::Label) -> Vec<Group> {
    let applied = Files::here()
        .chosen(STYLE_KEY)
        .unwrap_or_else(|| RELEASE_STYLE.to_string());
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
            write(STYLE_KEY, mode, RELEASE_STYLE, &problem);
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
    fn the_welcome_asks_the_persons_own_questions_in_the_installers_order() {
        assert_eq!(
            welcome_keys(edel::install::QUESTIONS),
            ["region.language", "region.keyboard", "layout.preset"]
        );
        // The installer's order is followed, not the Welcome's own list.
        let questions = [
            Question {
                key: "layout.preset",
                title: "Layout",
                more: true,
            },
            Question {
                key: "user",
                title: "Your username",
                more: false,
            },
            Question {
                key: "region.keyboard",
                title: "Keyboard",
                more: false,
            },
            Question {
                key: "region.language",
                title: "Language",
                more: false,
            },
        ];
        assert_eq!(
            welcome_keys(&questions),
            ["layout.preset", "region.keyboard", "region.language"]
        );
        assert!(welcome_keys(&[]).is_empty());
    }

    #[test]
    fn the_pages_are_language_keyboard_layout_style_then_done() {
        let pages = pages(edel::install::QUESTIONS);
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
    fn the_counter_counts_the_pages_before_done() {
        let pages = pages(edel::install::QUESTIONS);
        assert_eq!(counted(&pages), 4);
        assert_eq!(counter(1, counted(&pages)), "1 of 4");
        assert_eq!(counter(4, counted(&pages)), "4 of 4");
    }

    #[test]
    fn the_style_a_file_names_is_its_place_in_the_styles() {
        assert_eq!(style_at("light"), 0);
        assert_eq!(style_at("dark"), 1);
        assert_eq!(style_at("auto"), 2);
        assert_eq!(style_at("nonsense"), 0, "Light when it names another");
    }
}
