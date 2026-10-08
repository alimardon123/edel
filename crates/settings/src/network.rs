//! The Network page (roadmap M5.8a): whether the computer is connected,
//! in one plain headline at the top; the wired connection; and Wi-Fi,
//! with a switch, the networks in range with their signal and a padlock
//! for the ones that ask for a password, Connect, Disconnect and Forget.
//! The addresses and the rest only technical people read are folded away
//! under Details. Everything goes through `nmcli` (`edel::network`, which
//! the panel's quick settings will use too), run on a thread of its own
//! and never the one that draws; each row copies the `nmcli` command that
//! does the same, so the page and the command line are one level
//! (ADR-008).
//!
//! The connections and their passwords are NetworkManager's own state, not
//! lines of the settings file (ADR-006: the file never holds a secret), so
//! the page has no Reset. A password typed here goes to `nmcli` and is
//! kept by NetworkManager for the machine, never by this page. The page
//! looks at the network every few seconds while it is on screen, and only
//! then.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};
use edel::network::{self, Detail, Kind, Link, Snapshot, Summary, Wifi};

use crate::{card, icon, widgets};

const INTRO: &str = n_(
    "Connect by cable or Wi-Fi. Wi-Fi passwords are kept safely by this computer and never go \
     in your settings file.",
);

/// How often the page looks at the network while it is on screen.
const FOLLOW: u32 = 4;

/// What the headline says: the words, and whether the icon is lit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Headline {
    title: String,
    sub: String,
    on: bool,
}

/// The signal as a word.
fn signal_word(bars: u8) -> &'static str {
    match bars {
        0..=1 => tr("weak"),
        2 => tr("fair"),
        3 => tr("good"),
        _ => tr("strong"),
    }
}

/// The headline for what NetworkManager says: connected to what, or why
/// not and what to do about it.
fn headline(snapshot: &Snapshot) -> Headline {
    let (title, sub, on) = match snapshot.summary() {
        Summary::Wired { limited } => (
            tr("Connected").to_string(),
            if limited {
                tr("By network cable, but the internet is not reachable.").to_string()
            } else {
                tr("By network cable.").to_string()
            },
            true,
        ),
        Summary::Wireless { ssid, limited } => {
            let signal = snapshot
                .networks
                .iter()
                .find(|w| w.in_use)
                .map(|w| signal_word(w.bars()));
            let sub = match (limited, signal) {
                (true, _) => tr("By Wi-Fi, but the internet is not reachable.").to_string(),
                (false, Some(word)) => trf("By Wi-Fi, {signal} signal.", &[("signal", word)]),
                (false, None) => tr("By Wi-Fi.").to_string(),
            };
            (trf("Connected to {ssid}", &[("ssid", &ssid)]), sub, true)
        }
        Summary::Connecting => (
            tr("Connecting").to_string(),
            tr("This can take a few seconds.").to_string(),
            true,
        ),
        Summary::Asleep => (
            tr("Networking is off").to_string(),
            tr("Flight mode is on. Turn it off to connect.").to_string(),
            false,
        ),
        Summary::Offline => (
            tr("Not connected").to_string(),
            match (
                snapshot.wired().is_some(),
                snapshot.has_wifi(),
                snapshot.wifi_on,
            ) {
                (_, true, true) => {
                    tr("Plug in a network cable or choose a Wi-Fi network.").to_string()
                }
                (_, true, false) => {
                    tr("Turn Wi-Fi on and choose a network, or plug in a network cable.")
                        .to_string()
                }
                (true, false, _) => tr("Plug in a network cable.").to_string(),
                (false, false, _) => {
                    tr("This computer has no network device NetworkManager can use.").to_string()
                }
            },
            false,
        ),
    };
    Headline { title, sub, on }
}

/// What the wired row says about the cable.
fn wired_text(link: Link) -> &'static str {
    match link {
        Link::Connected => tr("Connected"),
        Link::Connecting => tr("Connecting"),
        Link::Disconnected => tr("Not connected"),
        Link::Unavailable => tr("Cable unplugged"),
        Link::Unmanaged => tr("Not managed"),
    }
}

/// The line under a network's name.
fn note(network: &Wifi) -> &'static str {
    if network.in_use {
        tr("Connected")
    } else if network.saved {
        tr("Saved")
    } else if network.secured {
        tr("Needs a password")
    } else {
        tr("Open network")
    }
}

/// What a network row shows, so the list is made again only when it
/// changes: the name, the bars, and whether it asks, is saved, in use.
type Shown = (String, u8, bool, bool, bool);

fn shown(snapshot: &Snapshot) -> Vec<Shown> {
    snapshot
        .networks
        .iter()
        .map(|w| (w.ssid.clone(), w.bars(), w.secured, w.saved, w.in_use))
        .collect()
}

/// The page's widgets and what it needs to change them.
struct Ui {
    glance: card::Glance,
    problem: gtk::Label,
    wired: gtk::Box,
    wired_state: gtk::Label,
    wifi: gtk::Box,
    switch: gtk::Switch,
    switch_row: widgets::Row,
    look: gtk::Button,
    list: gtk::Box,
    holder: gtk::Box,
    /// Keeps the Connect and Disconnect buttons as wide as each other.
    sizes: gtk::SizeGroup,
    fold: gtk::Expander,
    details: gtk::Box,
    snapshot: RefCell<Option<Snapshot>>,
    shown: RefCell<Vec<Shown>>,
    /// Set while the page itself moves a control, which is not a choice.
    quiet: Cell<bool>,
    /// Set while a change runs: the page waits for it before it looks again.
    busy: Cell<bool>,
    /// Whether the page has said what it shows on standard error, which
    /// desktop-test reads (it says it once, when it first has something).
    announced: Cell<bool>,
    /// Whether the problem line shows a failed change, which a look at the
    /// network must not clear.
    failed: Cell<bool>,
    /// The password row, with the name of the network it asks for.
    asking: RefCell<Option<(String, gtk::Box)>>,
    /// The network whose Forget waits for its second click.
    armed: RefCell<Option<String>>,
}

pub fn page() -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Network"), tr(INTRO));
    let glance = card::Glance::new(&content, "page-network");

    let wired = vbox();
    content.append(&wired);
    widgets::heading(&wired, tr("Wired"));
    let group = widgets::group(&wired);
    let wired_state = gtk::Label::builder().css_classes(["edel-value"]).build();
    let wired_row = widgets::row(&group, tr("Network cable"), wired_state.upcast_ref());
    wired_row.subtitle.set_visible(false);
    wired_row.reset.set_visible(false);
    wired_row.copy.set_visible(false);

    let wifi = vbox();
    content.append(&wifi);
    widgets::heading(&wifi, tr("Wi-Fi"));
    let group = widgets::group(&wifi);
    let controls = gtk::Box::builder().spacing(10).build();
    let look = widgets::action(tr("Look again"), false);
    let switch = gtk::Switch::new();
    switch.update_property(&[gtk::accessible::Property::Label(tr("Wi-Fi"))]);
    controls.append(&look);
    controls.append(&switch);
    let switch_row = widgets::row(&group, tr("Wi-Fi"), controls.upcast_ref());
    switch_row.subtitle.set_label(tr(
        "Turn it off to save power or in a place where radios are not allowed.",
    ));
    switch_row.reset.set_visible(false);
    let list = vbox();
    wifi.append(&list);
    widgets::heading(&list, tr("Networks nearby"));
    let holder = widgets::group(&list);

    let inner = vbox();
    let details = widgets::group(&inner);
    let fold = card::fold(&content, tr("Details"), &inner);

    let ui = Rc::new(Ui {
        glance,
        problem,
        wired,
        wired_state,
        wifi,
        switch,
        switch_row,
        look,
        list,
        holder,
        sizes: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
        fold,
        details,
        snapshot: RefCell::new(None),
        shown: RefCell::new(Vec::new()),
        quiet: Cell::new(false),
        busy: Cell::new(false),
        announced: Cell::new(false),
        failed: Cell::new(false),
        asking: RefCell::new(None),
        armed: RefCell::new(None),
    });
    wire(&ui);
    {
        let ui = ui.clone();
        card::while_shown(&page, FOLLOW, move || {
            // A look at the network also asks the adapters to look again,
            // so the list is not the one from when the page opened.
            refresh(&ui, false);
        });
    }
    page
}

fn vbox() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build()
}

/// Connects the controls that do not belong to a network's row.
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
                    tr("Turning Wi-Fi on").to_string()
                } else {
                    tr("Turning Wi-Fi off").to_string()
                },
                move || network::set_wifi(on),
            );
        });
    }
    {
        let ui = ui.clone();
        ui.clone().look.connect_clicked(move |_| refresh(&ui, true));
    }
    {
        let ui = ui.clone();
        ui.clone().switch_row.copy.connect_clicked(move |button| {
            let args = network::radio_args(ui.switch.is_active());
            button.clipboard().set_text(&network::command_line(&args));
            widgets::copied(button);
        });
    }
    {
        let ui = ui.clone();
        ui.clone().fold.connect_expanded_notify(move |fold| {
            if fold.is_expanded() {
                refresh(&ui, false);
            }
        });
    }
}

/// Reads the network and shows it; `rescan` also asks the adapters to look
/// again first.
fn refresh(ui: &Rc<Ui>, rescan: bool) {
    if ui.busy.get() {
        return;
    }
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let want_details = ui.fold.is_expanded();
        let read = gio::spawn_blocking(move || {
            if rescan {
                network::rescan();
            }
            (network::read(), want_details.then(network::details))
        })
        .await;
        // A change started meanwhile shows its own words.
        if ui.busy.get() {
            return;
        }
        match read {
            Ok((Ok(snapshot), details)) => {
                if !ui.failed.get() {
                    ui.problem.set_visible(false);
                }
                show(&ui, snapshot);
                if let Some(Ok(details)) = details {
                    show_details(&ui, &details);
                }
            }
            Ok((Err(e), _)) => {
                ui.problem.set_label(&format!("{e:#}"));
                ui.problem.set_visible(true);
                ui.failed.set(false);
                ui.glance
                    .show(false, tr("The network is not available"), "");
                ui.wired.set_visible(false);
                ui.wifi.set_visible(false);
            }
            Err(_) => {}
        }
    });
}

/// Runs a change on a thread of its own, saying `doing` meanwhile, then
/// shows what the network is, or why the change failed.
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
        match done {
            Ok(Err(e)) => {
                ui.problem.set_label(&format!("{e:#}"));
                ui.problem.set_visible(true);
                ui.failed.set(true);
            }
            Ok(Ok(())) => {
                if let Some((_, row)) = ui.asking.borrow_mut().take() {
                    if let Some(parent) = row.parent().and_downcast::<gtk::Box>() {
                        parent.remove(&row);
                    }
                }
            }
            Err(_) => {}
        }
        refresh(&ui, false);
    });
}

/// Shows what NetworkManager says.
fn show(ui: &Rc<Ui>, snapshot: Snapshot) {
    let line = headline(&snapshot);
    ui.glance.show(line.on, &line.title, &line.sub);
    if !ui.announced.replace(true) {
        eprintln!("edel-settings: network page shows \"{}\"", line.title);
    }

    // The wired connection, when there is a device for it.
    match snapshot.wired() {
        Some(device) => {
            ui.wired.set_visible(true);
            ui.wired_state.set_label(wired_text(device.link));
        }
        None => ui.wired.set_visible(false),
    }

    // Wi-Fi, when there is an adapter.
    ui.wifi.set_visible(snapshot.has_wifi());
    ui.quiet.set(true);
    ui.switch.set_active(snapshot.wifi_on);
    ui.quiet.set(false);
    ui.look.set_sensitive(snapshot.wifi_on);
    ui.list.set_visible(snapshot.wifi_on);

    let now = shown(&snapshot);
    let asking = ui.asking.borrow().is_some();
    if snapshot.wifi_on && !asking && *ui.shown.borrow() != now {
        *ui.shown.borrow_mut() = now;
        ui.armed.borrow_mut().take();
        *ui.snapshot.borrow_mut() = Some(snapshot);
        rebuild(ui);
    } else {
        *ui.snapshot.borrow_mut() = Some(snapshot);
    }
}

/// Makes the list of networks again.
fn rebuild(ui: &Rc<Ui>) {
    card::clear(&ui.holder);
    let snapshot = ui.snapshot.borrow().clone();
    let Some(snapshot) = snapshot else { return };
    if snapshot.networks.is_empty() {
        widgets::text_row(
            &ui.holder,
            tr("No Wi-Fi networks found. Move closer to the router, or choose Look again."),
        );
        return;
    }
    let device = snapshot
        .devices
        .iter()
        .find(|d| d.kind == Kind::Wifi && d.link == Link::Connected)
        .map(|d| d.name.clone());
    for network in &snapshot.networks {
        row(ui, network, device.clone());
    }
}

/// One network's row: its name and how it stands, its signal and padlock,
/// and the buttons that fit.
fn row(ui: &Rc<Ui>, network: &Wifi, device: Option<String>) {
    let controls = gtk::Box::builder().spacing(10).build();
    controls.append(&card::bars(network.bars(), signal_word(network.bars())));
    // The padlock's place is kept for an open network.
    let lock = icon::image("lock", 14);
    lock.add_css_class("edel-lock");
    if network.secured {
        lock.set_tooltip_text(Some(tr("Asks for a password")));
    } else {
        lock.set_opacity(0.0);
    }
    controls.append(&lock);
    let main = widgets::action(
        if network.in_use {
            tr("Disconnect")
        } else {
            tr("Connect")
        },
        !network.in_use,
    );
    // Connect and Disconnect are as wide as the wider, and Forget keeps its
    // place unseen for a network that is not saved, so the bars, the padlock
    // and the buttons line up from one row to the next.
    ui.sizes.add_widget(&main);
    controls.append(&main);
    let forget = widgets::action(tr("Forget"), false);
    if !network.saved {
        forget.set_opacity(0.0);
        forget.set_can_target(false);
        forget.set_focusable(false);
    }
    controls.append(&forget);

    let row = widgets::row(&ui.holder, &network.ssid, controls.upcast_ref());
    row.subtitle.set_label(note(network));
    row.reset.set_visible(false);

    // Copy as command: what the main button does.
    let line = if network.in_use {
        device
            .as_deref()
            .map(network::leave_args)
            .unwrap_or_default()
    } else if network.saved {
        network::up_args(&network.ssid)
    } else {
        network::join_args(&network.ssid, network.secured.then_some("PASSWORD"))
    };
    row.copy.connect_clicked(move |button| {
        button.clipboard().set_text(&network::command_line(&line));
        widgets::copied(button);
    });

    let box_ = controls.parent().and_downcast::<gtk::Box>();
    {
        let (ui, network, device) = (ui.clone(), network.clone(), device.clone());
        main.connect_clicked(move |_| {
            if network.in_use {
                if let Some(device) = device.clone() {
                    let doing = trf("Disconnecting from {ssid}", &[("ssid", &network.ssid)]);
                    act(&ui, doing, move || network::leave(&device));
                }
            } else if network.secured && !network.saved {
                if let Some(row) = &box_ {
                    ask(&ui, &network.ssid, row);
                }
            } else {
                join(&ui, &network.ssid, network.saved, None);
            }
        });
    }
    {
        let (ui, ssid) = (ui.clone(), network.ssid.clone());
        forget.connect_clicked(move |button| {
            // Two clicks, as a password is not got back.
            if ui.armed.borrow().as_deref() != Some(&ssid) {
                *ui.armed.borrow_mut() = Some(ssid.clone());
                button.set_label(tr("Click again"));
                let (ui, ssid, button) = (ui.clone(), ssid.clone(), button.downgrade());
                glib::timeout_add_seconds_local_once(5, move || {
                    if ui.armed.borrow().as_deref() == Some(&ssid) {
                        ui.armed.borrow_mut().take();
                        if let Some(button) = button.upgrade() {
                            button.set_label(tr("Forget"));
                        }
                    }
                });
                return;
            }
            ui.armed.borrow_mut().take();
            let name = ssid.clone();
            act(
                &ui,
                trf("Forgetting {ssid}", &[("ssid", &ssid)]),
                move || network::forget(&name),
            );
        });
    }
}

/// Joins `ssid`, saying so meanwhile.
fn join(ui: &Rc<Ui>, ssid: &str, saved: bool, password: Option<String>) {
    let name = ssid.to_string();
    act(
        ui,
        trf("Connecting to {ssid}", &[("ssid", ssid)]),
        move || network::join(&name, saved, password.as_deref()),
    );
}

/// Asks for the password of `ssid` in a row under the network's own.
fn ask(ui: &Rc<Ui>, ssid: &str, under: &gtk::Box) {
    if let Some((_, old)) = ui.asking.borrow_mut().take() {
        if let Some(parent) = old.parent().and_downcast::<gtk::Box>() {
            parent.remove(&old);
        }
    }
    let entry = gtk::PasswordEntry::builder()
        .placeholder_text(tr("Password"))
        .show_peek_icon(true)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .css_classes(["edel-entry"])
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(&trf(
        "Password for {ssid}",
        &[("ssid", ssid)],
    ))]);
    let join_button = widgets::action(tr("Join"), true);
    let cancel = widgets::action(tr("Cancel"), false);
    let row = gtk::Box::builder()
        .spacing(10)
        .css_classes(["edel-row"])
        .build();
    row.append(&entry);
    row.append(&cancel);
    row.append(&join_button);
    ui.holder.insert_child_after(&row, Some(under));
    *ui.asking.borrow_mut() = Some((ssid.to_string(), row.clone()));
    entry.grab_focus();

    let go = {
        let (ui, ssid, entry) = (ui.clone(), ssid.to_string(), entry.clone());
        move || {
            let password = entry.text().to_string();
            if !password.is_empty() {
                join(&ui, &ssid, false, Some(password));
            }
        }
    };
    {
        let go = go.clone();
        entry.connect_activate(move |_| go());
    }
    join_button.connect_clicked(move |_| go());
    let ui = ui.clone();
    cancel.connect_clicked(move |_| {
        if let Some((_, row)) = ui.asking.borrow_mut().take() {
            if let Some(parent) = row.parent().and_downcast::<gtk::Box>() {
                parent.remove(&row);
            }
        }
        ui.shown.borrow_mut().clear();
        refresh(&ui, false);
    });
}

/// Shows the addresses under Details.
fn show_details(ui: &Rc<Ui>, details: &[Detail]) {
    card::clear(&ui.details);
    let mut any = false;
    for detail in details {
        let name = detail.device.as_str();
        let mut value = |title: &str, value: String| {
            if !value.is_empty() {
                any = true;
                widgets::value_row(
                    &ui.details,
                    &trf("{device} {what}", &[("device", name), ("what", title)]),
                    &value,
                );
            }
        };
        value(tr("address"), detail.addresses.join(", "));
        value(tr("gateway"), detail.gateway.clone().unwrap_or_default());
        value(tr("DNS"), detail.dns.join(", "));
        value(
            tr("hardware address"),
            detail.hardware.clone().unwrap_or_default(),
        );
    }
    if !any {
        widgets::text_row(
            &ui.details,
            tr("Nothing is connected, so there are no addresses to show."),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::network::{parse_devices, parse_wifi};

    fn snapshot(state: &str, devices: &str, wifi: &str, wifi_on: bool) -> Snapshot {
        let (state, _) =
            network::parse_status(&format!("{state}:full:enabled:enabled:enabled:enabled"))
                .unwrap();
        Snapshot {
            state,
            wifi_on,
            devices: parse_devices(devices),
            networks: parse_wifi(wifi),
            saved: Vec::new(),
        }
    }

    #[test]
    fn the_headline_says_how_the_computer_is_connected() {
        let wired = snapshot(
            "connected",
            "eth0:ethernet:connected:Wired connection 1\n",
            "",
            true,
        );
        assert_eq!(
            headline(&wired),
            Headline {
                title: "Connected".into(),
                sub: "By network cable.".into(),
                on: true
            }
        );
        let wifi = snapshot(
            "connected",
            "wlan0:wifi:connected:Home\n",
            "*:Home:78:WPA2\n :Other:40:WPA2\n",
            true,
        );
        assert_eq!(
            headline(&wifi),
            Headline {
                title: "Connected to Home".into(),
                sub: "By Wi-Fi, strong signal.".into(),
                on: true
            }
        );
        let limited = snapshot(
            "connected (site only)",
            "wlan0:wifi:connected:Home\n",
            "*:Home:30:WPA2\n",
            true,
        );
        assert!(headline(&limited).sub.contains("internet is not reachable"));
    }

    #[test]
    fn the_headline_says_what_to_do_when_not_connected() {
        let none = snapshot(
            "disconnected",
            "eth0:ethernet:unavailable:\nwlan0:wifi:disconnected:\n",
            "",
            true,
        );
        let line = headline(&none);
        assert_eq!(line.title, "Not connected");
        assert_eq!(
            line.sub,
            "Plug in a network cable or choose a Wi-Fi network."
        );
        assert!(!line.on);
        let off = snapshot("disconnected", "wlan0:wifi:unavailable:\n", "", false);
        assert!(headline(&off).sub.starts_with("Turn Wi-Fi on"));
        let cable = snapshot("disconnected", "eth0:ethernet:unavailable:\n", "", false);
        assert_eq!(headline(&cable).sub, "Plug in a network cable.");
        let nothing = snapshot("disconnected", "lo:loopback:unmanaged:\n", "", false);
        assert!(headline(&nothing).sub.contains("no network device"));
        let asleep = snapshot("asleep", "eth0:ethernet:unavailable:\n", "", false);
        assert_eq!(headline(&asleep).title, "Networking is off");
        let joining = snapshot(
            "connecting",
            "wlan0:wifi:connecting (prepare):Home\n",
            "",
            true,
        );
        assert_eq!(headline(&joining).title, "Connecting");
    }

    #[test]
    fn the_wired_row_and_a_networks_line_use_plain_words() {
        assert_eq!(wired_text(Link::Unavailable), "Cable unplugged");
        assert_eq!(wired_text(Link::Connected), "Connected");
        let mut network = parse_wifi(" :Cafe:55:WPA2\n").remove(0);
        assert_eq!(note(&network), "Needs a password");
        network.secured = false;
        assert_eq!(note(&network), "Open network");
        network.saved = true;
        assert_eq!(note(&network), "Saved");
        network.in_use = true;
        assert_eq!(note(&network), "Connected");
        assert_eq!(
            [1, 2, 3, 4].map(signal_word),
            ["weak", "fair", "good", "strong"]
        );
    }

    #[test]
    fn the_list_is_made_again_only_when_a_row_would_change() {
        let a = snapshot(
            "connected",
            "wlan0:wifi:connected:Home\n",
            "*:Home:78:WPA2\n :Cafe:40:WPA2\n",
            true,
        );
        let b = snapshot(
            "connected",
            "wlan0:wifi:connected:Home\n",
            "*:Home:80:WPA2\n :Cafe:44:WPA2\n",
            true,
        );
        // A few points of signal inside the same bar are not a change.
        assert_eq!(shown(&a), shown(&b));
        let c = snapshot(
            "connected",
            "wlan0:wifi:connected:Home\n",
            "*:Home:78:WPA2\n :Cafe:60:WPA2\n",
            true,
        );
        assert_ne!(shown(&a), shown(&c));
    }
}
