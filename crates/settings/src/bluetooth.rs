//! The Bluetooth page (roadmap M5.8a): whether Bluetooth is on, in one
//! plain headline at the top with its switch; the devices paired with this
//! computer, each with Connect or Disconnect and Remove; and a search for
//! devices nearby, each with Pair. A computer with no Bluetooth says so
//! and shows nothing else. Everything goes through `bluetoothctl`
//! (`edel::bluetooth`, which the panel's quick settings will use too), run
//! on a thread of its own and never the one that draws; each row copies
//! the `bluetoothctl` command that does the same, so the page and the
//! command line are one level (ADR-008).
//!
//! The pairings are BlueZ's own state and their keys are secrets, so
//! neither is a line of the settings file (ADR-006) and the page has no
//! Reset. The page looks at Bluetooth every few seconds while it is on
//! screen, and only then; a search runs only when a person asks for it.
//!
//! Seam (ADR-011): a device is whatever BlueZ lists, so a paired phone's
//! or a lent keyboard's entry is one more row, and pairing with a passcode
//! (a keyboard that shows a number to type) joins later as another answer
//! to the agent, with no change to the rest of the page.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::bluetooth::{self, Device, Kind, Snapshot};
use edel::i18n::{n_, tr, trf};

use crate::{card, widgets};

const INTRO: &str = n_(
    "Connect headphones, speakers, a mouse or a keyboard. Pairings are kept safely by this \
     computer and never go in your settings file.",
);

/// How often the page looks at Bluetooth while it is on screen.
const FOLLOW: u32 = 4;

/// How often the page looks while a search runs, so devices appear as
/// they are found.
const FOLLOW_SEARCH: u32 = 2;

/// What the headline says: the words, and whether the icon is lit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Headline {
    title: String,
    sub: String,
    on: bool,
}

/// The headline: whether this computer has Bluetooth, whether it is on,
/// and what it is connected to.
fn headline(snapshot: &Snapshot) -> Headline {
    let Some(adapter) = &snapshot.adapter else {
        return Headline {
            title: tr("This computer has no Bluetooth").to_string(),
            sub: tr("If you plug in a Bluetooth adapter, it will show up here.").to_string(),
            on: false,
        };
    };
    if !adapter.powered {
        return Headline {
            title: tr("Bluetooth is off").to_string(),
            sub: tr("Turn it on to connect headphones, a mouse or a keyboard.").to_string(),
            on: false,
        };
    }
    let connected: Vec<&str> = snapshot
        .devices
        .iter()
        .filter(|d| d.connected)
        .map(|d| d.name.as_str())
        .collect();
    let sub = if connected.is_empty() {
        tr("Not connected to any device.").to_string()
    } else {
        trf(
            "Connected to {devices}.",
            &[("devices", &connected.join(", "))],
        )
    };
    Headline {
        title: tr("Bluetooth is on").to_string(),
        sub,
        on: true,
    }
}

/// What kind of thing a device is, in a word.
fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Audio => tr("Headphones or speaker"),
        Kind::Keyboard => tr("Keyboard"),
        Kind::Pointer => tr("Mouse"),
        Kind::Controller => tr("Game controller"),
        Kind::Phone => tr("Phone"),
        Kind::Computer => tr("Computer"),
        Kind::Other => tr("Device"),
    }
}

/// The line under a device's name.
fn note(device: &Device) -> String {
    if !device.paired {
        return kind_word(device.kind).to_string();
    }
    let state = if device.connected {
        tr("connected")
    } else {
        tr("not connected")
    };
    trf(
        "{kind}, {state}",
        &[("kind", kind_word(device.kind)), ("state", state)],
    )
}

/// What a device row shows, so the lists are made again only when one
/// would change.
type Shown = (String, String, bool, bool, bool, Kind);

fn shown(snapshot: &Snapshot) -> Vec<Shown> {
    snapshot
        .devices
        .iter()
        .map(|d| {
            (
                d.address.clone(),
                d.name.clone(),
                d.paired,
                d.connected,
                d.trusted,
                d.kind,
            )
        })
        .collect()
}

/// The page's widgets and what it needs to change them.
struct Ui {
    glance: card::Glance,
    switch: gtk::Switch,
    problem: gtk::Label,
    body: gtk::Box,
    mine: gtk::Box,
    nearby: gtk::Box,
    search: gtk::Button,
    spinner: gtk::Spinner,
    found: gtk::Box,
    details: gtk::Box,
    snapshot: RefCell<Option<Snapshot>>,
    shown: RefCell<Vec<Shown>>,
    /// Set while the page itself moves a control, which is not a choice.
    quiet: Cell<bool>,
    /// Set while a change runs: the page waits for it before it looks again.
    busy: Cell<bool>,
    /// Set while a search runs.
    searching: Cell<bool>,
    /// Whether a search has finished since the page opened, so "nothing
    /// found" is only said after one.
    searched: Cell<bool>,
    /// Whether the page has said what it shows on standard error, which
    /// desktop-test reads (it says it once, when it first has something).
    announced: Cell<bool>,
    /// Whether the problem line shows a failed change, which a look at
    /// Bluetooth must not clear.
    failed: Cell<bool>,
    /// The device whose Remove waits for its second click.
    armed: RefCell<Option<String>>,
}

pub fn page() -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Bluetooth"), tr(INTRO));
    let glance = card::Glance::new(&content, "page-bluetooth");
    let switch = gtk::Switch::new();
    switch.update_property(&[gtk::accessible::Property::Label(tr("Bluetooth"))]);
    glance.aside.append(&switch);

    let body = vbox();
    content.append(&body);
    widgets::heading(&body, tr("Your devices"));
    let mine = widgets::group(&body);

    let nearby = vbox();
    body.append(&nearby);
    widgets::heading(&nearby, tr("Nearby devices"));
    let group = widgets::group(&nearby);
    let controls = gtk::Box::builder().spacing(10).build();
    let spinner = gtk::Spinner::new();
    spinner.set_visible(false);
    let search = widgets::action(tr("Search"), true);
    controls.append(&spinner);
    controls.append(&search);
    let search_row = widgets::row(&group, tr("Search for devices"), controls.upcast_ref());
    search_row
        .subtitle
        .set_label(tr("Put the device in pairing mode first, then search."));
    search_row.reset.set_visible(false);
    let found = widgets::group(&nearby);
    found.set_margin_top(10);
    found.set_visible(false);
    {
        search_row.copy.connect_clicked(move |button| {
            button
                .clipboard()
                .set_text(&bluetooth::command_line(&bluetooth::scan_args()));
            widgets::copied(button);
        });
    }

    let inner = vbox();
    let details = widgets::group(&inner);
    card::fold(&body, tr("Details"), &inner);

    let ui = Rc::new(Ui {
        glance,
        switch,
        problem,
        body,
        mine,
        nearby,
        search,
        spinner,
        found,
        details,
        snapshot: RefCell::new(None),
        shown: RefCell::new(Vec::new()),
        quiet: Cell::new(false),
        busy: Cell::new(false),
        searching: Cell::new(false),
        searched: Cell::new(false),
        announced: Cell::new(false),
        failed: Cell::new(false),
        armed: RefCell::new(None),
    });
    wire(&ui);
    {
        let ui = ui.clone();
        card::while_shown(&page, FOLLOW, move || refresh(&ui));
    }
    page
}

fn vbox() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build()
}

/// Connects the switch and the search button.
fn wire(ui: &Rc<Ui>) {
    {
        let ui = ui.clone();
        ui.clone().switch.connect_active_notify(move |switch| {
            if ui.quiet.get() {
                return;
            }
            let on = switch.is_active();
            act(
                &ui,
                if on {
                    tr("Turning Bluetooth on").to_string()
                } else {
                    tr("Turning Bluetooth off").to_string()
                },
                move || bluetooth::set_power(on),
            );
        });
    }
    {
        let ui = ui.clone();
        ui.clone().search.connect_clicked(move |_| search(&ui));
    }
}

/// Looks for devices nearby for as long as `bluetooth::SCAN_SECONDS`, and
/// shows each as it is found.
fn search(ui: &Rc<Ui>) {
    if ui.searching.get() || ui.busy.get() {
        return;
    }
    ui.searching.set(true);
    ui.failed.set(false);
    ui.problem.set_visible(false);
    ui.search.set_sensitive(false);
    ui.search.set_label(tr("Searching"));
    ui.spinner.set_visible(true);
    ui.spinner.set_spinning(true);
    {
        // Devices appear while the search runs.
        let ui = ui.clone();
        glib::timeout_add_seconds_local(FOLLOW_SEARCH, move || {
            if !ui.searching.get() {
                return glib::ControlFlow::Break;
            }
            refresh(&ui);
            glib::ControlFlow::Continue
        });
    }
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let done = gio::spawn_blocking(bluetooth::scan).await;
        ui.searching.set(false);
        ui.searched.set(true);
        ui.search.set_sensitive(true);
        ui.search.set_label(tr("Search"));
        ui.spinner.set_spinning(false);
        ui.spinner.set_visible(false);
        if let Ok(Err(e)) = done {
            ui.problem.set_label(&format!("{e:#}"));
            ui.problem.set_visible(true);
            ui.failed.set(true);
        }
        ui.shown.borrow_mut().clear();
        refresh(&ui);
    });
}

/// Reads Bluetooth and shows it.
fn refresh(ui: &Rc<Ui>) {
    if ui.busy.get() {
        return;
    }
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let read = gio::spawn_blocking(bluetooth::read).await;
        // A change started meanwhile shows its own words.
        if ui.busy.get() {
            return;
        }
        match read {
            Ok(Ok(snapshot)) => {
                if !ui.failed.get() {
                    ui.problem.set_visible(false);
                }
                show(&ui, snapshot);
            }
            Ok(Err(e)) => {
                ui.problem.set_label(&format!("{e:#}"));
                ui.problem.set_visible(true);
                ui.failed.set(false);
                ui.glance.show(false, tr("Bluetooth is not available"), "");
                ui.switch.set_visible(false);
                ui.body.set_visible(false);
            }
            Err(_) => {}
        }
    });
}

/// Runs a change on a thread of its own, saying `doing` meanwhile, then
/// shows what Bluetooth is, or why the change failed.
fn act<E: std::fmt::Display + Send + 'static>(
    ui: &Rc<Ui>,
    doing: String,
    work: impl FnOnce() -> Result<(), E> + Send + 'static,
) {
    ui.busy.set(true);
    ui.failed.set(false);
    ui.problem.set_visible(false);
    ui.glance
        .show(true, &doing, tr("This can take a few seconds."));
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let done = gio::spawn_blocking(work).await;
        ui.busy.set(false);
        if let Ok(Err(e)) = done {
            ui.problem.set_label(&format!("{e:#}"));
            ui.problem.set_visible(true);
            ui.failed.set(true);
        }
        ui.shown.borrow_mut().clear();
        refresh(&ui);
    });
}

/// Shows what BlueZ says.
fn show(ui: &Rc<Ui>, snapshot: Snapshot) {
    let line = headline(&snapshot);
    ui.glance.show(line.on, &line.title, &line.sub);
    if !ui.announced.replace(true) {
        eprintln!("edel-settings: bluetooth page shows \"{}\"", line.title);
    }
    let present = snapshot.adapter.is_some();
    let powered = snapshot.adapter.as_ref().is_some_and(|a| a.powered);
    ui.switch.set_visible(present);
    ui.quiet.set(true);
    ui.switch.set_active(powered);
    ui.quiet.set(false);
    // With no Bluetooth the page says so and shows nothing else.
    ui.body.set_visible(present);
    ui.nearby.set_visible(powered);
    show_details(ui, &snapshot);
    let now = shown(&snapshot);
    let changed = *ui.shown.borrow() != now;
    *ui.snapshot.borrow_mut() = Some(snapshot);
    if changed {
        *ui.shown.borrow_mut() = now;
        ui.armed.borrow_mut().take();
        rebuild(ui);
    }
}

/// Makes the two lists of devices again.
fn rebuild(ui: &Rc<Ui>) {
    card::clear(&ui.mine);
    card::clear(&ui.found);
    let snapshot = ui.snapshot.borrow().clone();
    let Some(snapshot) = snapshot else { return };
    let powered = snapshot.adapter.as_ref().is_some_and(|a| a.powered);
    let (paired, nearby): (Vec<&Device>, Vec<&Device>) =
        snapshot.devices.iter().partition(|d| d.paired);
    if paired.is_empty() {
        widgets::text_row(
            &ui.mine,
            tr("No devices yet. Choose Search below, with the device in pairing mode."),
        );
    }
    for device in paired {
        paired_row(ui, device, powered);
    }
    ui.found
        .set_visible(!nearby.is_empty() || ui.searched.get());
    if nearby.is_empty() && ui.searched.get() {
        widgets::text_row(
            &ui.found,
            tr("Nothing found nearby. Put the device in pairing mode and search again."),
        );
    }
    for device in nearby {
        nearby_row(ui, device);
    }
}

/// A paired device: Connect or Disconnect, and Remove.
fn paired_row(ui: &Rc<Ui>, device: &Device, powered: bool) {
    let controls = gtk::Box::builder().spacing(10).build();
    let main = widgets::action(
        if device.connected {
            tr("Disconnect")
        } else {
            tr("Connect")
        },
        !device.connected,
    );
    main.set_sensitive(powered);
    controls.append(&main);
    let remove = widgets::action(tr("Remove"), false);
    controls.append(&remove);
    let row = widgets::row(&ui.mine, &device.name, controls.upcast_ref());
    row.subtitle.set_label(&note(device));
    row.reset.set_visible(false);

    let command = if device.connected {
        "disconnect"
    } else {
        "connect"
    };
    let line = bluetooth::command_line(&bluetooth::device_args(command, &device.address));
    row.copy.connect_clicked(move |button| {
        button.clipboard().set_text(&line);
        widgets::copied(button);
    });
    {
        let (ui, device) = (ui.clone(), device.clone());
        main.connect_clicked(move |_| {
            let (address, name) = (device.address.clone(), device.name.clone());
            if device.connected {
                act(
                    &ui,
                    trf("Disconnecting from {name}", &[("name", &name)]),
                    move || bluetooth::disconnect(&address, &name),
                );
            } else {
                act(
                    &ui,
                    trf("Connecting to {name}", &[("name", &name)]),
                    move || bluetooth::connect(&address, &name),
                );
            }
        });
    }
    {
        let (ui, device) = (ui.clone(), device.clone());
        remove.connect_clicked(move |button| {
            // Two clicks, as a pairing is not got back without pairing again.
            if ui.armed.borrow().as_deref() != Some(&device.address) {
                *ui.armed.borrow_mut() = Some(device.address.clone());
                button.set_label(tr("Click again"));
                let (ui, address, button) =
                    (ui.clone(), device.address.clone(), button.downgrade());
                glib::timeout_add_seconds_local_once(5, move || {
                    if ui.armed.borrow().as_deref() == Some(&address) {
                        ui.armed.borrow_mut().take();
                        if let Some(button) = button.upgrade() {
                            button.set_label(tr("Remove"));
                        }
                    }
                });
                return;
            }
            ui.armed.borrow_mut().take();
            let (address, name) = (device.address.clone(), device.name.clone());
            act(
                &ui,
                trf("Removing {name}", &[("name", &device.name)]),
                move || bluetooth::remove(&address, &name),
            );
        });
    }
}

/// A device found nearby: Pair.
fn nearby_row(ui: &Rc<Ui>, device: &Device) {
    let controls = gtk::Box::builder().spacing(10).build();
    let pair = widgets::action(tr("Pair"), true);
    controls.append(&pair);
    let row = widgets::row(&ui.found, &device.name, controls.upcast_ref());
    row.subtitle.set_label(&note(device));
    row.reset.set_visible(false);
    let line = bluetooth::command_line(&bluetooth::pair_args(&device.address));
    row.copy.connect_clicked(move |button| {
        button.clipboard().set_text(&line);
        widgets::copied(button);
    });
    let (ui, device) = (ui.clone(), device.clone());
    pair.connect_clicked(move |_| {
        let (address, name) = (device.address.clone(), device.name.clone());
        act(
            &ui,
            trf("Pairing with {name}", &[("name", &device.name)]),
            move || bluetooth::pair(&address, &name),
        );
    });
}

/// Shows the adapter under Details.
fn show_details(ui: &Rc<Ui>, snapshot: &Snapshot) {
    card::clear(&ui.details);
    if let Some(adapter) = &snapshot.adapter {
        widgets::value_row(&ui.details, tr("Called"), &adapter.name);
        widgets::value_row(&ui.details, tr("Address"), &adapter.address);
    }
    widgets::text_row(
        &ui.details,
        tr(
            "Keyboards that ask you to type a code on them cannot be paired here yet; pair them with bluetoothctl.",
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::bluetooth::Adapter;

    fn device(name: &str, paired: bool, connected: bool, kind: Kind) -> Device {
        Device {
            address: "00:11:22:33:44:55".into(),
            name: name.into(),
            kind,
            paired,
            connected,
            trusted: paired,
        }
    }

    fn snapshot(powered: bool, devices: Vec<Device>) -> Snapshot {
        Snapshot {
            adapter: Some(Adapter {
                address: "A4:C3:F0:12:34:56".into(),
                name: "edel".into(),
                powered,
                scanning: false,
            }),
            devices,
        }
    }

    #[test]
    fn a_computer_without_bluetooth_says_so() {
        let line = headline(&Snapshot {
            adapter: None,
            devices: Vec::new(),
        });
        assert_eq!(line.title, "This computer has no Bluetooth");
        assert!(!line.on);
    }

    #[test]
    fn the_headline_says_whether_bluetooth_is_on_and_what_it_holds() {
        let off = headline(&snapshot(false, Vec::new()));
        assert_eq!((off.title.as_str(), off.on), ("Bluetooth is off", false));
        let idle = headline(&snapshot(
            true,
            vec![device("Mouse", true, false, Kind::Pointer)],
        ));
        assert_eq!(idle.title, "Bluetooth is on");
        assert_eq!(idle.sub, "Not connected to any device.");
        let busy = headline(&snapshot(
            true,
            vec![
                device("Headphones", true, true, Kind::Audio),
                device("Keyboard", true, true, Kind::Keyboard),
                device("Mouse", true, false, Kind::Pointer),
            ],
        ));
        assert_eq!(busy.sub, "Connected to Headphones, Keyboard.");
        assert!(busy.on);
    }

    #[test]
    fn a_device_line_says_what_it_is_and_how_it_stands() {
        assert_eq!(
            note(&device("A", true, true, Kind::Audio)),
            "Headphones or speaker, connected"
        );
        assert_eq!(
            note(&device("B", true, false, Kind::Pointer)),
            "Mouse, not connected"
        );
        assert_eq!(note(&device("C", false, false, Kind::Phone)), "Phone");
        assert_eq!(note(&device("D", false, false, Kind::Other)), "Device");
    }

    #[test]
    fn the_lists_are_made_again_only_when_a_device_would_change() {
        let a = snapshot(true, vec![device("Mouse", true, false, Kind::Pointer)]);
        let same = snapshot(true, vec![device("Mouse", true, false, Kind::Pointer)]);
        let connected = snapshot(true, vec![device("Mouse", true, true, Kind::Pointer)]);
        assert_eq!(shown(&a), shown(&same));
        assert_ne!(shown(&a), shown(&connected));
    }
}
