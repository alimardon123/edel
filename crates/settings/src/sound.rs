//! The Sound page (roadmap M5.7b): where sound plays and what listens,
//! and how loud, for the output and the input. The devices and their
//! volume are the sound system's own live state, read and set through
//! `wpctl` (`edel::sound`, which the panel's volume will use too), not
//! lines of the settings file: WirePlumber remembers each person's choice
//! of device and volume, so a key would only be a second copy that could
//! disagree. Each row copies the `wpctl` command that does the same, so the
//! page and the command line are one level (ADR-008). The list is whatever
//! PipeWire reports, not only local cards (ADR-011). The page follows the
//! sound system while it is open, so a change made elsewhere shows here.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr};
use edel::sound::{self, Device, Devices, Kind};

use crate::widgets;

const INTRO: &str = n_(
    "Choose where sound plays and what listens, and how loud. This is the sound system's own \
     state, kept for you by the machine, so it is not a line of your settings file; each row \
     can copy the wpctl command that does the same.",
);

/// How often the page looks at the sound system while it is open.
const FOLLOW: u32 = 2;

/// How long after a person's change the page leaves their control alone,
/// so a refresh never moves a slider under their hand.
const HOLD: Duration = Duration::from_secs(3);

/// How long after the last move of a slider the volume is set, so a drag
/// is one command, not a hundred.
const SETTLE: Duration = Duration::from_millis(120);

/// One side of the page: the output or the input.
struct Side {
    kind: Kind,
    /// The heading and the card, hidden when PipeWire lists no such device.
    shown: gtk::Box,
    /// Holds the list of devices, which is made again when they change.
    holder: gtk::Box,
    choice: RefCell<Option<Rc<widgets::Choice>>>,
    scale: gtk::Scale,
    percent: gtk::Label,
    mute: gtk::Switch,
    /// The devices the list shows, in its order.
    devices: RefCell<Vec<Device>>,
    /// The row of the device list, for its Copy as command.
    choose_row: widgets::Row,
    volume_row: widgets::Row,
    mute_row: widgets::Row,
    /// Set while the page itself moves a control, so that is not a choice.
    quiet: Cell<bool>,
    /// When a person last changed this side.
    touched: Cell<Option<Instant>>,
    /// The pending volume command, until the slider settles.
    pending: RefCell<Option<glib::SourceId>>,
}

/// The page's widgets.
struct Ui {
    /// The page itself, which may have been asked for by name.
    page: gtk::Widget,
    sides: [Side; 2],
    /// Said when no sound device is listed at all.
    nothing: gtk::Box,
    problem: gtk::Label,
}

pub fn page() -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Sound"), tr(INTRO));
    let output = side(
        &content,
        Kind::Sink,
        [
            tr("Output"),
            tr("Output device"),
            tr("Where sound plays"),
            tr("Volume"),
            tr("Mute"),
        ],
    );
    let input = side(
        &content,
        Kind::Source,
        [
            tr("Input"),
            tr("Input device"),
            tr("What listens"),
            tr("Level"),
            tr("Mute"),
        ],
    );
    let nothing = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .visible(false)
        .build();
    widgets::heading(&nothing, tr("Devices"));
    let group = widgets::group(&nothing);
    widgets::text_row(&group, sound::no_devices());
    content.append(&nothing);
    let ui = Rc::new(Ui {
        page: page.clone(),
        sides: [output, input],
        nothing,
        problem,
    });
    for side in &ui.sides {
        wire(&ui, side);
    }
    // Follow the sound system while the page is on screen, and only then.
    let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    {
        let (ui, timer) = (ui.clone(), timer.clone());
        page.connect_map(move |_| {
            refresh(&ui);
            let ui = ui.clone();
            let id = glib::timeout_add_seconds_local(FOLLOW, move || {
                refresh(&ui);
                glib::ControlFlow::Continue
            });
            if let Some(old) = timer.borrow_mut().replace(id) {
                old.remove();
            }
        });
    }
    page.connect_unmap(move |_| {
        if let Some(id) = timer.borrow_mut().take() {
            id.remove();
        }
    });
    page
}

/// The heading and card of one side, with its rows: the device, the volume
/// and the mute switch.
fn side(content: &gtk::Box, kind: Kind, words: [&str; 5]) -> Side {
    let [heading, device, what, volume, mute] = words;
    let shown = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .visible(false)
        .build();
    widgets::heading(&shown, heading);
    let group = widgets::group(&shown);
    content.append(&shown);

    let holder = gtk::Box::builder().build();
    let choose_row = widgets::row(&group, device, holder.upcast_ref());
    choose_row.subtitle.set_label(what);

    let scale = gtk::Scale::with_range(
        gtk::Orientation::Horizontal,
        0.0,
        f64::from(sound::MOST_PERCENT),
        5.0,
    );
    scale.set_draw_value(false);
    scale.set_width_request(190);
    scale.set_hexpand(true);
    scale.update_property(&[gtk::accessible::Property::Label(volume)]);
    let percent = gtk::Label::builder()
        .width_chars(4)
        .xalign(1.0)
        .css_classes(["edel-value"])
        .build();
    let control = gtk::Box::builder().spacing(10).build();
    control.append(&scale);
    control.append(&percent);
    let volume_row = widgets::row(&group, volume, control.upcast_ref());

    let switch = gtk::Switch::new();
    let mute_row = widgets::row(&group, mute, switch.upcast_ref());
    // There is nothing to take out of a file: the sound system has the
    // value, so Reset has nothing to do here.
    for row in [&choose_row, &volume_row, &mute_row] {
        row.reset.set_visible(false);
    }
    // Only the device row has a line under its title.
    volume_row.subtitle.set_visible(false);
    mute_row.subtitle.set_visible(false);
    Side {
        kind,
        shown,
        holder,
        choice: RefCell::new(None),
        scale,
        percent,
        mute: switch,
        devices: RefCell::new(Vec::new()),
        choose_row,
        volume_row,
        mute_row,
        quiet: Cell::new(false),
        touched: Cell::new(None),
        pending: RefCell::new(None),
    }
}

/// Connects a side's controls to the sound system and its copy buttons to
/// the commands.
fn wire(ui: &Rc<Ui>, side: &Side) {
    let at = side.kind as usize;
    // The slider: a percent label at once, the command once it settles.
    {
        let ui = ui.clone();
        side.scale.connect_value_changed(move |scale| {
            let side = &ui.sides[at];
            let percent = scale.value().round() as u32;
            side.percent.set_label(&format!("{percent}%"));
            if side.quiet.get() {
                return;
            }
            side.touched.set(Some(Instant::now()));
            if let Some(old) = side.pending.borrow_mut().take() {
                old.remove();
            }
            let later = ui.clone();
            let id = glib::timeout_add_local_once(SETTLE, move || {
                let side = &later.sides[at];
                side.pending.borrow_mut().take();
                if let Some(device) = side.current() {
                    let target = device.id.to_string();
                    run(&later, move || sound::set_volume(&target, percent));
                }
            });
            *side.pending.borrow_mut() = Some(id);
        });
    }
    {
        let ui = ui.clone();
        side.mute.connect_active_notify(move |switch| {
            let side = &ui.sides[at];
            if side.quiet.get() {
                return;
            }
            side.touched.set(Some(Instant::now()));
            if let Some(device) = side.current() {
                let (target, muted) = (device.id.to_string(), switch.is_active());
                run(&ui, move || sound::set_mute(&target, muted));
            }
        });
    }
    // Copy as command: what a row does now, as wpctl takes it.
    {
        let ui = ui.clone();
        side.volume_row.copy.connect_clicked(move |button| {
            let side = &ui.sides[at];
            let percent = side.scale.value().round() as u32;
            let args = sound::volume_args(side.kind.default_name(), percent);
            button.clipboard().set_text(&sound::command_line(&args));
            widgets::copied(button);
        });
    }
    {
        let ui = ui.clone();
        side.mute_row.copy.connect_clicked(move |button| {
            let side = &ui.sides[at];
            let args = sound::mute_args(side.kind.default_name(), side.mute.is_active());
            button.clipboard().set_text(&sound::command_line(&args));
            widgets::copied(button);
        });
    }
    {
        let ui = ui.clone();
        side.choose_row.copy.connect_clicked(move |button| {
            let side = &ui.sides[at];
            if let Some(device) = side.current() {
                let args = sound::default_args(device.id);
                button.clipboard().set_text(&sound::command_line(&args));
                widgets::copied(button);
            }
        });
    }
}

impl Side {
    /// The device the list shows as chosen.
    fn current(&self) -> Option<Device> {
        let at = self.choice.borrow().as_ref().map_or(0, |c| c.selected());
        self.devices.borrow().get(at).cloned()
    }
}

/// Reads the sound system and shows it.
fn refresh(ui: &Rc<Ui>) {
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        match gio::spawn_blocking(sound::read).await {
            Ok(Ok(devices)) => {
                ui.problem.set_visible(false);
                show(&ui, &devices);
            }
            Ok(Err(e)) => {
                ui.problem.set_label(&format!("{e:#}"));
                ui.problem.set_visible(true);
                show(&ui, &Devices::default());
            }
            Err(_) => {}
        }
    });
}

/// Runs a change on a thread of its own, then shows what the sound system
/// says, or why it could not.
fn run<E: std::fmt::Display + Send + 'static>(
    ui: &Rc<Ui>,
    change: impl FnOnce() -> Result<(), E> + Send + 'static,
) {
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        match gio::spawn_blocking(change).await {
            Ok(Ok(())) => ui.problem.set_visible(false),
            Ok(Err(e)) => {
                ui.problem.set_label(&format!("{e:#}"));
                ui.problem.set_visible(true);
            }
            Err(_) => {}
        }
        refresh(&ui);
    });
}

/// Shows `devices`: each side's list with the one in use chosen, its
/// volume and mute, or nothing for a side with no device.
fn show(ui: &Rc<Ui>, devices: &Devices) {
    for side in &ui.sides {
        let list = devices.of(side.kind);
        side.shown.set_visible(!list.is_empty());
        side.quiet.set(true);
        let same = side
            .devices
            .borrow()
            .iter()
            .map(|d| (d.id, &d.name))
            .eq(list.iter().map(|d| (d.id, &d.name)));
        let chosen = list.iter().position(|d| d.default).unwrap_or(0);
        if !same {
            side.rebuild(ui, list);
        }
        *side.devices.borrow_mut() = list.to_vec();
        // A person's own change settles before the page speaks again.
        let held = side.touched.get().is_some_and(|t| t.elapsed() < HOLD);
        if !held {
            if let Some(choice) = side.choice.borrow().as_ref() {
                choice.set_selected(chosen);
            }
            if let Some(device) = list.get(chosen) {
                let volume = device.volume.unwrap_or(0).min(sound::MOST_PERCENT);
                side.scale.set_value(f64::from(volume));
                side.scale.set_sensitive(device.volume.is_some());
                side.mute.set_active(device.muted);
            }
        }
        side.quiet.set(false);
    }
    ui.nothing
        .set_visible(devices.sinks.is_empty() && devices.sources.is_empty());
    // Opened by name, the page gives the keyboard to the volume slider.
    if ui.sides[0].shown.is_visible() {
        widgets::take_asked(&ui.page, &ui.sides[0].scale);
    }
}

impl Side {
    /// Makes the list of devices again, as PipeWire lists them now.
    fn rebuild(&self, ui: &Rc<Ui>, list: &[Device]) {
        while let Some(child) = self.holder.first_child() {
            self.holder.remove(&child);
        }
        if list.is_empty() {
            *self.choice.borrow_mut() = None;
            return;
        }
        let choice = widgets::Choice::new(list.iter().map(|d| d.name.clone()).collect());
        let at = self.kind as usize;
        let weak = Rc::downgrade(ui);
        let picked = Rc::downgrade(&choice);
        choice.connect_changed(move || {
            let (Some(ui), Some(choice)) = (weak.upgrade(), picked.upgrade()) else {
                return;
            };
            let side = &ui.sides[at];
            let Some(device) = side.devices.borrow().get(choice.selected()).cloned() else {
                return;
            };
            // The new device's own volume shows once it is the one in use.
            side.touched.set(None);
            run(&ui, move || sound::set_default(device.id));
        });
        self.holder.append(&choice.widget());
        *self.choice.borrow_mut() = Some(choice);
    }
}
