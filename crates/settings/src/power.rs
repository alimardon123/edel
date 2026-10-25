//! The Power page (roadmap M5.8b, M5.32): what a battery page says
//! everywhere, in the order people look for it. A card at the top, as the
//! board `docs/mockups/settings/power-updates-users.jpg` draws it: the
//! battery, drawn with how full it is; the charge in large figures and
//! what the battery is doing under them; its health at the right; and the
//! last day as twelve bars, two hours each, the last one now. A computer
//! with no battery says `Plugged in` and nothing more. The batteries of
//! other things (a wireless mouse) follow when UPower lists any. Details,
//! closed, holds what technical people read: UPower's own dump, as
//! `upower --dump` prints it.
//!
//! Everything comes from UPower (`edel::power`, which the panel's battery
//! will use too), asked on a thread of its own and never the one that
//! draws, every few seconds while the page is on screen and never when it
//! is not (no service runs for what nobody uses). The day's chart asks
//! for the battery's history at most once a minute. The charge is the
//! battery's own state, not a line of the settings file, so the status has
//! no Reset.
//!
//! The `[power]` keys (M7.8) are a group under Other batteries: when the
//! lid closes, the power button, how long before the screen locks, and how
//! hard the computer works on battery. Each is a row of `rows.rs` written
//! to the person's file as the other pages' rows are (`choice_row.rs`), and
//! a test here holds the page to the table, so a key cannot go without a
//! row. The lid and the power button act in shell-ui and the compositor, the
//! screen locks in shell-ui, and the profile is set by `edel power profile`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};
use edel::power::{self, Bar, Device, Sample, Snapshot, State};
use edel::settings::{
    GAME_MODE, GAME_MODE_DEFAULT, LID_CLOSE, LID_CLOSE_DEFAULT, LOCK_AFTER_MINUTES,
    LOCK_AFTER_MINUTES_DEFAULT_TEXT, ON_BATTERY, ON_BATTERY_DEFAULT, POWER_BUTTON,
    POWER_BUTTON_DEFAULT, SAVER, SAVER_DEFAULT, SCREEN_OFF_MINUTES,
    SCREEN_OFF_MINUTES_DEFAULT_TEXT, SLEEP_AFTER_MINUTES, SLEEP_AFTER_MINUTES_DEFAULT_TEXT,
};

use crate::choice_row::{self, ChoiceRow};
use crate::files::Files;
use crate::shortcuts::keys_of;
use crate::{battery_care, card, rows, widgets};

const INTRO: &str = n_("The battery, the lid and the power button.");

/// What the Game mode row says under its title, with the keys the desktop
/// follows for the Game mode action.
const GAME_MODE_WHAT: &str =
    n_("A fullscreen game gets variable refresh; {keys} turns it on or off");

/// How often the page looks at UPower while it is on screen, in seconds.
const FOLLOW: u32 = 5;

/// How often the battery's history is asked for, in seconds: a day's
/// chart does not change faster than that.
const HISTORY_EVERY: u64 = 60;

/// The battery drawing in logical pixels, as style.rs draws its outline
/// (`.edel-battery`, a border of 2 px): the outside, the inside's margin
/// and the chart's tallest bar, which a full charge reaches.
const BODY_WIDTH: i32 = 58;
const BODY_HEIGHT: i32 = 30;
const BORDER: i32 = 2;
const GAP: i32 = 3;
const BAR_HEIGHT: i32 = 56;
const BAR_MIN: i32 = 4;

/// The card at the top: the words, and whether its icon is lit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Headline {
    title: String,
    sub: String,
    on: bool,
}

/// The title: the charge as a percent when the battery reports one, else
/// the word for what it is.
fn title(percent: Option<u32>, word: &str) -> String {
    match percent {
        Some(p) => trf("{percent}%", &[("percent", &p.to_string())]),
        None => word.to_string(),
    }
}

/// What the card says about `snapshot`.
fn headline(snapshot: &Snapshot) -> Headline {
    let Some(battery) = snapshot.battery() else {
        // No battery: a desktop, a server, a virtual machine. Plugged in,
        // which is all there is to say, unless the daemon says otherwise.
        return if snapshot.plugged_in() {
            Headline {
                title: tr("Plugged in").to_string(),
                sub: tr("This computer has no battery.").to_string(),
                on: true,
            }
        } else {
            Headline {
                title: tr("Running on battery").to_string(),
                sub: tr("The battery's charge is not reported.").to_string(),
                on: true,
            }
        };
    };
    let percent = battery.percent_whole();
    let time = battery.minutes.map(power::duration_text);
    let plugged = snapshot.plugged_in();
    let (title, sub) = match battery.state {
        State::Charging => (
            title(percent, tr("Battery")),
            match &time {
                Some(t) => trf("Charging · full in {time}", &[("time", t)]),
                None => tr("Charging").to_string(),
            },
        ),
        State::FullyCharged => (
            title(percent, tr("Fully charged")),
            tr("Fully charged · plugged in").to_string(),
        ),
        State::PendingCharge => (
            title(percent, tr("Battery")),
            tr("Plugged in · not charging").to_string(),
        ),
        State::Empty => (
            title(percent, tr("Battery empty")),
            tr("Battery empty · plug in the charger").to_string(),
        ),
        State::Discharging => (
            title(percent, tr("Battery")),
            match &time {
                Some(t) => trf("On battery · {time} left", &[("time", t)]),
                None => tr("On battery").to_string(),
            },
        ),
        State::PendingDischarge | State::Unknown => (
            title(percent, tr("Battery")),
            if plugged {
                tr("Plugged in").to_string()
            } else {
                tr("On battery").to_string()
            },
        ),
    };
    Headline {
        title,
        sub,
        on: true,
    }
}

/// The class of the battery's fill (the page's rule, as the board draws
/// it): green when it is plugged in or above 20 percent, amber from 11 to
/// 20 percent on battery, red at 10 percent or less on battery.
fn fill_class(percent: u32, plugged: bool) -> &'static str {
    if plugged || percent > 20 {
        "edel-ok"
    } else if percent > 10 {
        "edel-low"
    } else {
        "edel-empty"
    }
}

/// The fill's width in logical pixels: `percent` of the inside.
fn fill_width(percent: u32) -> i32 {
    (BODY_WIDTH - 2 * (BORDER + GAP)) * percent.min(100) as i32 / 100
}

/// A bar's height in logical pixels for `percent`, never under the least.
fn bar_height(percent: f64) -> i32 {
    ((percent * f64::from(BAR_HEIGHT) / 100.0).round() as i32).max(BAR_MIN)
}

/// The seconds since 1970 now (0 when the clock is before it).
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The card's widgets and what they show.
struct Card {
    root: gtk::Box,
    drawing: gtk::Box,
    fill: gtk::Box,
    title: gtk::Label,
    sub: gtk::Label,
    health: gtk::Box,
    health_value: gtk::Label,
    /// How many times it has been charged through, under its health,
    /// when the battery says (M7.16e)
    health_cycles: gtk::Label,
    chart: gtk::Box,
    bars: gtk::Box,
}

/// The minutes the Lock screen after row offers; the file's own minutes
/// join them when they are none of these (M7.8).
const MINUTES: &[&str] = &["0", "1", "2", "5", "10", "15", "30"];

/// The values the Lock screen after row offers: [`MINUTES`], and the file's
/// minutes in order when they are none of those.
fn minute_values(chosen: Option<&str>) -> Vec<String> {
    let mut values: Vec<String> = MINUTES.iter().map(|m| m.to_string()).collect();
    if let Some(minutes) = chosen.and_then(|v| v.parse::<u32>().ok()) {
        let text = minutes.to_string();
        if !values.contains(&text) {
            values.push(text);
            values.sort_by_key(|v| v.parse::<u32>().unwrap_or(0));
        }
    }
    values
}

/// The minutes the three minute rows offer, in their order: the Lock screen
/// after row, Screen off after and Sleep on battery after (M7.16b).
fn minutes_of(files: &Files) -> Vec<Vec<String>> {
    [LOCK_AFTER_MINUTES, SCREEN_OFF_MINUTES, SLEEP_AFTER_MINUTES]
        .iter()
        .map(|key| minute_values(files.chosen(key).as_deref()))
        .collect()
}

/// What the Lock screen after row shows for a value of minutes: Never for
/// 0, else the number of minutes.
fn minute_label(value: &str) -> String {
    match value.parse::<u32>() {
        Ok(0) => tr("Never").to_string(),
        Ok(1) => trf("{n} minute", &[("n", value)]),
        Ok(_) => trf("{n} minutes", &[("n", value)]),
        Err(_) => value.to_string(),
    }
}

/// The line under the Game mode row: its words with the keys the desktop
/// follows for the action, then where its value comes from.
fn game_mode_line(files: &Files) -> String {
    let what = trf(
        GAME_MODE_WHAT,
        &[("keys", &keys_of(files, "toggle_game_mode"))],
    );
    match rows::note(&files.source(GAME_MODE), false) {
        Some(note) => format!("{what} · {note}"),
        None => what,
    }
}

/// The group under Other batteries with the lid, the power button, the
/// screen lock, the screen off and sleep times and the battery's profile
/// (M7.8, M7.16b). The minute rows list the file's own minutes too, so the
/// rows are built again when those change; the page follows both settings
/// files while it is open.
struct Lid {
    group: gtk::Box,
    problem: gtk::Label,
    rows: RefCell<Vec<Rc<ChoiceRow>>>,
    /// The minutes each minute row offers, as [`minutes_of`] gives them.
    minutes: RefCell<Vec<Vec<String>>>,
    /// Kept so the page follows the files while it is open.
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

impl Lid {
    /// The group, built for the files as they are now and following them.
    fn new(group: gtk::Box, problem: gtk::Label) -> Rc<Lid> {
        let lid = Rc::new(Lid {
            group,
            problem,
            rows: RefCell::new(Vec::new()),
            minutes: RefCell::new(Vec::new()),
            monitors: RefCell::new(Vec::new()),
        });
        let monitors = choice_row::watch_files(&Files::here());
        for monitor in &monitors {
            let weak = Rc::downgrade(&lid);
            monitor.connect_changed(move |_, _, _, _| {
                if let Some(lid) = weak.upgrade() {
                    lid.update();
                }
            });
        }
        *lid.monitors.borrow_mut() = monitors;
        lid.update();
        lid
    }

    /// Shows what the files say, and builds the rows again when the minutes
    /// the Lock screen after row offers have changed.
    fn update(self: &Rc<Self>) {
        let files = Files::here();
        let minutes = minutes_of(&files);
        if minutes != *self.minutes.borrow() {
            self.build(minutes);
        }
        for row in self.rows.borrow().iter() {
            row.update(&files);
            // The row writes its template's words; this sets them with the keys.
            if row.key == GAME_MODE {
                row.row.subtitle.set_label(&game_mode_line(&files));
            }
        }
    }

    /// Builds the rows in the group, each wired to the files.
    fn build(self: &Rc<Self>, minutes: Vec<Vec<String>>) {
        card::clear(&self.group);
        let labels =
            |values: &[String]| -> Vec<String> { values.iter().map(|m| minute_label(m)).collect() };
        let specs = vec![
            ChoiceRow::new(
                &self.group,
                LID_CLOSE,
                n_("Suspend locks first; with another screen connected, nothing happens"),
                LID_CLOSE_DEFAULT,
            ),
            ChoiceRow::new(
                &self.group,
                POWER_BUTTON,
                n_("Ask opens the lock screen's power menu"),
                POWER_BUTTON_DEFAULT,
            ),
            ChoiceRow::with_values(
                &self.group,
                LOCK_AFTER_MINUTES,
                n_("Without any input; a playing video keeps the screen on"),
                LOCK_AFTER_MINUTES_DEFAULT_TEXT,
                minutes[0].clone(),
                labels(&minutes[0]),
            ),
            ChoiceRow::with_values(
                &self.group,
                SCREEN_OFF_MINUTES,
                n_("Without any input; the backlight dims 30 seconds before"),
                SCREEN_OFF_MINUTES_DEFAULT_TEXT,
                minutes[1].clone(),
                labels(&minutes[1]),
            ),
            ChoiceRow::with_values(
                &self.group,
                SLEEP_AFTER_MINUTES,
                n_("On battery, without any input; a playing video keeps it awake"),
                SLEEP_AFTER_MINUTES_DEFAULT_TEXT,
                minutes[2].clone(),
                labels(&minutes[2]),
            ),
            ChoiceRow::new(
                &self.group,
                ON_BATTERY,
                n_("Plugged in, it runs at Balanced, or Performance when chosen here"),
                ON_BATTERY_DEFAULT,
            ),
            ChoiceRow::new(
                &self.group,
                SAVER,
                n_(
                    "Lighter effects and the power-saver profile; Automatic turns it on at 20 percent on battery",
                ),
                SAVER_DEFAULT,
            ),
            ChoiceRow::new(&self.group, GAME_MODE, GAME_MODE_WHAT, GAME_MODE_DEFAULT),
        ];
        let rows: Vec<Rc<ChoiceRow>> = specs
            .into_iter()
            .map(|row| {
                let row = Rc::new(row);
                row.connect(self.report(), self.after());
                row
            })
            .collect();
        *self.rows.borrow_mut() = rows;
        *self.minutes.borrow_mut() = minutes;
    }

    /// Shows a change's refusal in the page's line.
    fn report(&self) -> impl Fn(Result<(), String>) + 'static {
        let problem = self.problem.clone();
        move |result| choice_row::report(&problem, result)
    }

    /// Shows the files again after a change. Later, not now: the change is
    /// picked in a row that a rebuild would take away.
    fn after(self: &Rc<Self>) -> impl Fn() + 'static {
        let weak = Rc::downgrade(self);
        move || {
            let weak = weak.clone();
            glib::idle_add_local_once(move || {
                if let Some(lid) = weak.upgrade() {
                    lid.update();
                }
            });
        }
    }
}

/// The page's widgets and what it needs to change them.
struct Ui {
    card: Card,
    /// Battery care's group under the card (M7.16e).
    care: Rc<battery_care::Care>,
    problem: gtk::Label,
    others_head: gtk::Box,
    others: gtk::Box,
    raw: gtk::Label,
    /// The other batteries as last shown, so the list is made again only
    /// when one would change.
    shown: RefCell<Vec<(String, u32)>>,
    /// Whether the page has said what it shows on standard error, which
    /// desktop-test reads (once, when it first has something).
    announced: Cell<bool>,
    /// Whether the chart has been shown, for the same reason.
    chart_announced: Cell<bool>,
    /// When the battery's history was last asked for, in seconds since
    /// 1970; the chart keeps its bars between asks.
    history_at: Cell<Option<u64>>,
}

pub fn page() -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Power"), tr(INTRO));
    let card = battery_card(&content);
    let care = battery_care::group(&content);

    let others_head = widgets::heading(&content, tr("Other batteries"));
    let others = widgets::group(&content);
    others_head.set_visible(false);
    others.set_visible(false);

    widgets::heading(&content, tr("Behaviour"));
    let lid = Lid::new(widgets::group(&content), problem.clone());

    // Details, closed: UPower's own words.
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let details = widgets::group(&inner);
    let raw = widgets::output_row(&details);
    card::fold(&content, tr("Details"), &inner);

    let ui = Rc::new(Ui {
        card,
        care,
        problem,
        others_head,
        others,
        raw,
        shown: RefCell::new(Vec::new()),
        announced: Cell::new(false),
        chart_announced: Cell::new(false),
        history_at: Cell::new(None),
    });
    card::while_shown(&page, FOLLOW, move || refresh(&ui));
    // The group's rows and the files' monitors live as long as the page.
    page.connect_destroy(move |_| {
        let _ = &lid;
    });
    page
}

/// Builds the card at the head of the page and appends it to `content`.
fn battery_card(content: &gtk::Box) -> Card {
    let body = gtk::Box::builder()
        .width_request(BODY_WIDTH)
        .height_request(BODY_HEIGHT)
        .valign(gtk::Align::Center)
        .css_classes(["edel-battery"])
        .build();
    let fill = gtk::Box::builder()
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Fill)
        .margin_start(GAP)
        .margin_end(GAP)
        .margin_top(GAP)
        .margin_bottom(GAP)
        .css_classes(["edel-power-fill"])
        .build();
    body.append(&fill);
    let nub = gtk::Box::builder()
        .width_request(3)
        .height_request(12)
        .valign(gtk::Align::Center)
        .css_classes(["edel-battery-nub"])
        .build();
    let drawing = gtk::Box::builder().valign(gtk::Align::Center).build();
    drawing.append(&body);
    drawing.append(&nub);

    let title = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["edel-power-percent"])
        .build();
    let sub = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-power-sub"])
        .build();
    let words = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    words.append(&title);
    words.append(&sub);

    let health_title = gtk::Label::builder()
        .label(tr("Battery health"))
        .xalign(1.0)
        .css_classes(["edel-power-dim"])
        .build();
    let health_value = gtk::Label::builder()
        .xalign(1.0)
        .css_classes(["edel-power-health"])
        .build();
    let health_cycles = gtk::Label::builder()
        .xalign(1.0)
        .visible(false)
        .css_classes(["edel-power-dim"])
        .build();
    let health = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .build();
    health.append(&health_title);
    health.append(&health_value);
    health.append(&health_cycles);

    let top = gtk::Box::builder().spacing(18).build();
    top.append(&drawing);
    top.append(&words);
    top.append(&health);

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(18)
        .css_classes(["edel-power-card"])
        .build();
    root.append(&top);

    // A narrow card puts the health under the charge, not at its end; the
    // card's own width decides, as the window's does for the sidebar.
    let (narrow_top, narrow_words, narrow_health) = (top.clone(), words.clone(), health.clone());
    let labels = (
        health_title.clone(),
        health_value.clone(),
        health_cycles.clone(),
    );
    let folded = widgets::folding(&root, move |narrow| {
        narrow_health.unparent();
        // Right-aligned at the card's end, left-aligned under the words.
        let (align, justify) = if narrow {
            (gtk::Align::Start, 0.0)
        } else {
            (gtk::Align::End, 1.0)
        };
        narrow_health.set_halign(align);
        labels.0.set_xalign(justify);
        labels.1.set_xalign(justify);
        labels.2.set_xalign(justify);
        if narrow {
            narrow_words.append(&narrow_health);
        } else {
            narrow_top.append(&narrow_health);
        }
    });

    let bars = gtk::Box::builder()
        .spacing(4)
        .homogeneous(true)
        .height_request(BAR_HEIGHT)
        .valign(gtk::Align::End)
        .build();
    bars.update_property(&[gtk::accessible::Property::Label(tr(
        "The battery over the last 24 hours",
    ))]);
    let axis = gtk::Box::builder().spacing(8).build();
    let ago = gtk::Label::builder()
        .label(tr("24 hours ago"))
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["edel-power-axis"])
        .build();
    let now = gtk::Label::builder()
        .label(tr("Now"))
        .xalign(1.0)
        .css_classes(["edel-power-axis"])
        .build();
    axis.append(&ago);
    axis.append(&now);
    let chart = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(18)
        .visible(false)
        .build();
    chart.append(&bars);
    chart.append(&axis);

    root.append(&chart);
    content.append(&folded);
    Card {
        root,
        drawing,
        fill,
        title,
        sub,
        health,
        health_value,
        health_cycles,
        chart,
        bars,
    }
}

impl Card {
    /// Shows the headline, the battery (when there is one), its health
    /// and charge cycles (when they are known) and what the chart holds:
    /// none without a battery. A screen reader hears the headline and the
    /// line as one sentence.
    fn show(
        &self,
        line: &Headline,
        battery: Option<&Device>,
        plugged: bool,
        health: Option<u32>,
        cycles: Option<u32>,
    ) {
        self.title.set_label(&line.title);
        self.sub.set_label(&line.sub);
        self.sub.set_visible(!line.sub.is_empty());
        let said = if line.sub.is_empty() {
            line.title.clone()
        } else {
            format!("{}. {}", line.title, line.sub)
        };
        self.root
            .update_property(&[gtk::accessible::Property::Label(&said)]);
        let percent = battery.and_then(Device::percent_whole);
        self.drawing.set_visible(percent.is_some());
        for class in ["edel-ok", "edel-low", "edel-empty"] {
            self.fill.remove_css_class(class);
        }
        if let Some(percent) = percent {
            self.fill.set_width_request(fill_width(percent));
            self.fill.add_css_class(fill_class(percent, plugged));
        }
        let health = health.filter(|_| battery.is_some());
        self.health.set_visible(health.is_some());
        if let Some(health) = health {
            self.health_value
                .set_label(&trf("{percent}%", &[("percent", &health.to_string())]));
        }
        let cycles = cycles.filter(|_| health.is_some());
        self.health_cycles.set_visible(cycles.is_some());
        if let Some(cycles) = cycles {
            self.health_cycles
                .set_label(&trf("{count} cycles", &[("count", &cycles.to_string())]));
        }
        if battery.is_none() {
            self.chart.set_visible(false);
        }
    }
}

/// One bar of the day's chart: its height, its colour by class, and the
/// last one solid, green when the battery is charging, else the accent.
fn bar_widget(slot: Option<Bar>, last: bool) -> gtk::Box {
    let bar = gtk::Box::builder()
        .valign(gtk::Align::End)
        .css_classes(["edel-day-bar"])
        .build();
    match slot {
        None => {
            bar.add_css_class("edel-gap");
            bar.set_size_request(-1, BAR_MIN);
        }
        Some(slot) => {
            bar.set_size_request(-1, bar_height(slot.percent));
            if slot.charging {
                bar.add_css_class("edel-charging");
            } else if slot.percent < 20.0 {
                bar.add_css_class("edel-low");
            }
            if last {
                bar.add_css_class("edel-now");
            }
        }
    }
    bar
}

/// The day's history of the battery at `path` as the chart's bars, or
/// none when it cannot be read: the chart is then left out, and the reason
/// goes to standard error, not to the page.
fn day_of(path: &str) -> Option<Vec<Sample>> {
    match power::history(path) {
        Ok(samples) => Some(samples),
        Err(e) => {
            eprintln!("edel-settings: power chart left out: {e:#}");
            None
        }
    }
}

/// Asks UPower and shows what it says; the battery's history follows
/// when it is due.
fn refresh(ui: &Rc<Ui>) {
    let ui = ui.clone();
    let now = now_secs();
    let due = match ui.history_at.get() {
        None => true,
        Some(at) => now.saturating_sub(at) >= HISTORY_EVERY,
    };
    glib::spawn_future_local(async move {
        let read = gio::spawn_blocking(move || {
            let snapshot = power::read();
            // Its history on this thread too, when it is due and there is a battery.
            let history = match (&snapshot, due) {
                (Ok(snapshot), true) => Some(snapshot.battery().and_then(|b| day_of(&b.path))),
                _ => None,
            };
            (snapshot, history)
        })
        .await;
        match read {
            Ok((Ok(snapshot), history)) => {
                ui.problem.set_visible(false);
                if let Some(samples) = history {
                    ui.history_at.set(Some(now));
                    let bars = samples
                        .map(|samples| power::day_bars(&samples, now))
                        .unwrap_or_default();
                    ui.show_chart(&bars);
                }
                ui.show(&snapshot);
            }
            Ok((Err(e), _)) => {
                ui.problem.set_label(&format!("{e:#}"));
                ui.problem.set_visible(true);
                ui.card.show(
                    &Headline {
                        title: tr("Power information is not available").to_string(),
                        sub: String::new(),
                        on: false,
                    },
                    None,
                    false,
                    None,
                    None,
                );
                ui.show_chart(&[]);
                ui.others_head.set_visible(false);
                ui.others.set_visible(false);
                if !ui.announced.replace(true) {
                    eprintln!("edel-settings: power page shows nothing: {e:#}");
                }
            }
            Err(_) => {}
        }
    });
}

impl Ui {
    fn show(&self, snapshot: &Snapshot) {
        self.care.update();
        let line = headline(snapshot);
        self.card.show(
            &line,
            snapshot.battery(),
            snapshot.plugged_in(),
            snapshot.health(),
            snapshot.cycles(),
        );
        if !self.announced.replace(true) {
            // CI reads this line to know what the page showed.
            eprintln!("edel-settings: power page shows \"{}\"", line.title);
        }
        let others: Vec<(String, u32)> = snapshot
            .others()
            .map(|d| {
                let name = if d.name.is_empty() {
                    d.kind.clone()
                } else {
                    d.name.clone()
                };
                (name, d.percent_whole().unwrap_or(0))
            })
            .collect();
        self.others_head.set_visible(!others.is_empty());
        self.others.set_visible(!others.is_empty());
        if *self.shown.borrow() != others {
            card::clear(&self.others);
            for (name, percent) in &others {
                widgets::fact_row(&self.others, name, &format!("{percent}%"));
            }
            *self.shown.borrow_mut() = others;
        }
        widgets::say(
            &self.raw,
            &format!("$ {}\n{}", power::command_line(&["--dump"]), snapshot.raw),
            false,
        );
    }

    /// Shows the day's chart when two of its slots have a reading, with a
    /// bar per slot; a day with less is left out.
    fn show_chart(&self, bars: &[Option<Bar>]) {
        let filled = bars.iter().filter(|bar| bar.is_some()).count();
        card::clear(&self.card.bars);
        let last = bars.len().saturating_sub(1);
        for (at, slot) in bars.iter().enumerate() {
            self.card.bars.append(&bar_widget(*slot, at == last));
        }
        self.card.chart.set_visible(filled >= 2);
        if filled >= 2 && !self.chart_announced.replace(true) {
            // CI reads this line to know the chart showed.
            eprintln!(
                "edel-settings: power page shows a day of {} bars",
                power::BARS
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows;

    fn dump(battery: &str, daemon: &str) -> Snapshot {
        power::parse(&format!(
            "Device: /org/freedesktop/UPower/devices/DisplayDevice\n  battery\n    present:             yes\n{battery}\nDaemon:\n  on-battery:      {daemon}\n"
        ))
    }

    fn says(snapshot: &Snapshot) -> (String, String) {
        let line = headline(snapshot);
        (line.title, line.sub)
    }

    #[test]
    fn a_computer_with_no_battery_is_plugged_in_and_says_so_plainly() {
        let vm = power::parse(
            "Device: /org/freedesktop/UPower/devices/DisplayDevice\n  battery\n    present:             no\n    state:               unknown\n    percentage:          0%\n\nDaemon:\n  on-battery:      no\n",
        );
        assert_eq!(
            says(&vm),
            ("Plugged in".into(), "This computer has no battery.".into())
        );
        assert_eq!(says(&power::parse("")).0, "Plugged in");
    }

    #[test]
    fn a_battery_in_use_says_how_much_and_how_long() {
        let on_battery = dump(
            "    state:               discharging\n    time to empty:       5.4 hours\n    percentage:          82%\n",
            "yes",
        );
        assert_eq!(
            says(&on_battery),
            ("82%".into(), "On battery · 5 hours 25 minutes left".into())
        );
        let no_time = dump(
            "    state:               discharging\n    percentage:          9%\n",
            "yes",
        );
        assert_eq!(says(&no_time), ("9%".into(), "On battery".into()));
    }

    #[test]
    fn a_battery_on_the_charger_says_it_charges_and_when_it_is_full() {
        let charging = dump(
            "    state:               charging\n    time to full:        41.2 minutes\n    percentage:          55%\n",
            "no",
        );
        assert_eq!(
            says(&charging),
            ("55%".into(), "Charging · full in 41 minutes".into())
        );
        let no_time = dump(
            "    state:               charging\n    percentage:          40%\n",
            "no",
        );
        assert_eq!(says(&no_time), ("40%".into(), "Charging".into()));
        let full = dump(
            "    state:               fully-charged\n    percentage:          100%\n",
            "no",
        );
        assert_eq!(
            says(&full),
            ("100%".into(), "Fully charged · plugged in".into())
        );
        let held = dump(
            "    state:               pending-charge\n    percentage:          80%\n",
            "no",
        );
        assert_eq!(
            says(&held),
            ("80%".into(), "Plugged in · not charging".into())
        );
    }

    #[test]
    fn an_empty_battery_says_so_and_asks_for_the_charger() {
        let empty = dump(
            "    state:               empty\n    percentage:          0%\n",
            "yes",
        );
        assert_eq!(
            says(&empty),
            ("0%".into(), "Battery empty · plug in the charger".into())
        );
        let no_percent = dump("    state:               empty\n", "yes");
        assert_eq!(says(&no_percent).0, "Battery empty");
    }

    #[test]
    fn a_battery_with_no_charge_reported_still_gets_a_headline() {
        let full = dump("    state:               fully-charged\n", "no");
        assert_eq!(
            says(&full),
            ("Fully charged".into(), "Fully charged · plugged in".into())
        );
        let charging = dump("    state:               charging\n", "no");
        assert_eq!(says(&charging), ("Battery".into(), "Charging".into()));
    }

    #[test]
    fn a_battery_in_a_state_nobody_knows_still_gets_a_headline() {
        let odd = dump(
            "    state:               warming-up\n    percentage:          64%\n",
            "yes",
        );
        assert_eq!(says(&odd), ("64%".into(), "On battery".into()));
        let plugged = dump("    state:               warming-up\n", "no");
        assert_eq!(says(&plugged), ("Battery".into(), "Plugged in".into()));
    }

    #[test]
    fn the_fill_is_green_on_the_charger_or_above_20_amber_then_red() {
        assert_eq!(fill_class(82, false), "edel-ok");
        assert_eq!(fill_class(21, false), "edel-ok");
        assert_eq!(fill_class(20, false), "edel-low");
        assert_eq!(fill_class(11, false), "edel-low");
        assert_eq!(fill_class(10, false), "edel-empty");
        assert_eq!(fill_class(0, false), "edel-empty");
        assert_eq!(fill_class(5, true), "edel-ok", "plugged in is never red");
    }

    #[test]
    fn the_fill_and_the_bars_follow_their_percent() {
        assert_eq!(fill_width(0), 0);
        assert_eq!(fill_width(50), 24, "half of the 48 px inside");
        assert_eq!(fill_width(100), 48);
        assert_eq!(fill_width(250), 48, "never wider than the inside");
        assert_eq!(bar_height(100.0), BAR_HEIGHT);
        assert_eq!(bar_height(0.5), BAR_MIN, "never under the least");
        assert_eq!(bar_height(50.0), 28);
    }

    #[test]
    fn every_power_key_is_supported_and_has_a_row_on_this_page() {
        let on_page: Vec<&str> = rows::on_page("power").map(|row| row.key).collect();
        for key in edel::settings::KEYS
            .iter()
            .filter(|k| k.path.starts_with("power."))
        {
            assert!(key.supported, "{} is not supported yet", key.path);
            assert!(
                on_page.contains(&key.path),
                "{} has no row on the Power page",
                key.path
            );
        }
        assert!(
            on_page.iter().all(|key| key.starts_with("power.")),
            "{on_page:?} are not power keys"
        );
    }

    #[test]
    fn the_power_rows_have_their_titles_and_words() {
        assert_eq!(rows::title(LID_CLOSE), "When the lid closes");
        assert_eq!(rows::title(POWER_BUTTON), "Power button");
        assert_eq!(rows::title(LOCK_AFTER_MINUTES), "Lock the screen after");
        assert_eq!(rows::title(SCREEN_OFF_MINUTES), "Screen off after");
        assert_eq!(rows::title(SLEEP_AFTER_MINUTES), "Sleep on battery after");
        assert_eq!(rows::title(ON_BATTERY), "On battery");
        assert_eq!(rows::title(SAVER), "Battery saver");
        assert_eq!(rows::title(GAME_MODE), "Game mode");
        assert_eq!(rows::title(edel::settings::BATTERY_CARE), "Battery care");
        assert_eq!(rows::label("plugged-in"), "When plugged in");
        assert_eq!(rows::label("poweroff"), "Shut down");
        assert_eq!(rows::label("nothing"), "Do nothing");
        assert_eq!(rows::label("ask"), "Ask");
        assert_eq!(rows::label("lock"), "Lock the screen");
        assert_eq!(rows::label("power-saver"), "Power saver");
        assert_eq!(rows::label("suspend"), "Suspend");
        for (key, release) in [
            (LID_CLOSE, LID_CLOSE_DEFAULT),
            (POWER_BUTTON, POWER_BUTTON_DEFAULT),
            (ON_BATTERY, ON_BATTERY_DEFAULT),
            (SAVER, SAVER_DEFAULT),
            (GAME_MODE, GAME_MODE_DEFAULT),
        ] {
            assert!(rows::values(key).contains(&release), "{key}: {release}");
        }
    }

    #[test]
    fn the_lock_after_row_offers_the_minutes_and_the_files_own() {
        assert_eq!(minute_values(None), ["0", "1", "2", "5", "10", "15", "30"]);
        assert_eq!(minute_values(Some("5")), minute_values(None));
        assert_eq!(
            minute_values(Some("7")),
            ["0", "1", "2", "5", "7", "10", "15", "30"],
            "a value of the file that is none of them joins them, in order"
        );
        assert_eq!(minute_values(Some("90")).last().unwrap(), "90");
        assert_eq!(minute_values(Some("soon")), minute_values(None));
        assert!(
            MINUTES.contains(&LOCK_AFTER_MINUTES_DEFAULT_TEXT),
            "the release's minutes are one the row offers"
        );
        assert_eq!(
            LOCK_AFTER_MINUTES_DEFAULT_TEXT.parse::<u32>().ok(),
            Some(edel::settings::LOCK_AFTER_MINUTES_DEFAULT)
        );
    }

    #[test]
    fn the_lock_after_row_says_never_or_the_minutes() {
        assert_eq!(minute_label("0"), "Never");
        assert_eq!(minute_label("1"), "1 minute");
        assert_eq!(minute_label("2"), "2 minutes");
        assert_eq!(minute_label("30"), "30 minutes");
    }
}
