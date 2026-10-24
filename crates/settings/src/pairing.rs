//! Pairing with a Bluetooth device that shows or compares a code (from the
//! other projects' bugs review, 2026-10-24): for the length of one pairing,
//! Settings puts BlueZ's agent (`org.bluez.Agent1`) on the system bus with
//! GIO, registers it as able to show a code and take a yes or a no
//! (`edel::bluetooth::AGENT_CAPABILITY`), and asks BlueZ to pair from the
//! same connection, so BlueZ asks this agent: a keyboard's code to type
//! is shown, and a phone's number is shown with "Pair" and Cancel. The
//! agent answers only BlueZ (the owner of `org.bluez`), and is taken off
//! the bus when the pairing ends, so nothing of it runs in the background.
//! BlueZ's names and the words for what went wrong are `edel::bluetooth`'s.
//!
//! Seam: typing a code on this computer (`RequestPinCode`,
//! `RequestPasskey`) is refused today with `bluetooth::type_here`'s words;
//! it joins as one more `Question` answered with a prompt that has an
//! entry, and the capability `KeyboardDisplay`, with no change to the rest.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::glib::variant::ObjectPath;
use gtk::prelude::*;
use gtk::{gio, glib};

use edel::bluetooth::{self, Question};
use edel::i18n::{tr, trf};

use crate::widgets;

/// The bus's own service, which answers who owns a name.
const DBUS: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const GET_NAME_OWNER: &str = "GetNameOwner";
/// BlueZ's objects, listed in one call.
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const GET_MANAGED_OBJECTS: &str = "GetManagedObjects";
const REGISTER_AGENT: &str = "RegisterAgent";
const UNREGISTER_AGENT: &str = "UnregisterAgent";
const PAIR: &str = "Pair";
const CANCEL_PAIRING: &str = "CancelPairing";
/// The error for a method the agent does not have.
const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
/// How long a call to the bus may take, apart from Pair's own time.
const CALL_MS: i32 = 5000;

thread_local! {
    /// Whether a pairing runs: one at a time, as BlueZ has one agent per
    /// pairing from a connection.
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// How a pairing ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Paired {
    /// Paired (or paired already): trust and connect follow
    Yes,
    /// The person said no or cancelled: nothing to say
    Cancelled,
    /// Why it failed, in `edel::bluetooth`'s words
    Failed(String),
}

/// Pairs with the device at `address`, called `name`, answering BlueZ's
/// questions in prompts over `near`'s window; `finished` runs once on the
/// main thread with how it ended.
pub fn pair(
    near: &gtk::Widget,
    address: &str,
    name: &str,
    finished: impl FnOnce(Paired) + 'static,
) {
    if RUNNING.with(Cell::get) {
        finished(Paired::Failed(bluetooth::pairing_runs()));
        return;
    }
    RUNNING.with(|running| running.set(true));
    let (near, address, name) = (near.clone(), address.to_string(), name.to_string());
    glib::spawn_future_local(async move {
        let end = run(&near, &address, &name).await;
        RUNNING.with(|running| running.set(false));
        finished(end);
    });
}

/// What lives while one pairing runs.
struct Run {
    bus: gio::DBusConnection,
    /// Who `org.bluez` is on the bus: the only sender answered
    bluez: String,
    /// The device being paired, its object path
    device: String,
    address: String,
    name: String,
    near: glib::WeakRef<gtk::Widget>,
    /// The prompt on screen, if any
    shown: RefCell<Option<widgets::Prompted>>,
    /// The code the prompt on screen shows, so BlueZ's repeats of it (as
    /// keys are typed) do not open another prompt
    shown_code: RefCell<Option<String>>,
    /// Set when the person said no or cancelled
    cancelled: Cell<bool>,
    /// Set when the device asked for a code typed here
    wanted_typing: Cell<bool>,
}

/// The whole pairing, in order; every failure becomes `Paired::Failed`.
async fn run(near: &gtk::Widget, address: &str, name: &str) -> Paired {
    let bus = match gio::bus_get_future(gio::BusType::System).await {
        Ok(bus) => bus,
        Err(e) => return Paired::Failed(bluetooth::bus_failed(&e.to_string())),
    };
    let owner = bus
        .call_future(
            Some(DBUS),
            DBUS_PATH,
            DBUS,
            GET_NAME_OWNER,
            Some(&(bluetooth::BLUEZ,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            CALL_MS,
        )
        .await;
    let Some(bluez) = owner
        .ok()
        .and_then(|reply| reply.get::<(String,)>())
        .map(|(owner,)| owner)
    else {
        return Paired::Failed(bluetooth::not_running());
    };

    let objects = match bus
        .call_future(
            Some(bluetooth::BLUEZ),
            "/",
            OBJECT_MANAGER,
            GET_MANAGED_OBJECTS,
            None,
            None,
            gio::DBusCallFlags::NONE,
            CALL_MS,
        )
        .await
    {
        Ok(reply) => reply
            .child_value(0)
            .get::<HashMap<ObjectPath, HashMap<String, HashMap<String, glib::Variant>>>>(),
        Err(_) => None,
    };
    let Some(objects) = objects else {
        return Paired::Failed(bluetooth::not_running());
    };
    let paths: Vec<String> = objects
        .keys()
        .map(|path| path.as_str().to_string())
        .collect();
    let device = match bluetooth::device_path(paths.iter().map(String::as_str), address, name) {
        Ok(device) => device,
        Err(e) => return Paired::Failed(format!("{e:#}")),
    };

    let node = match gio::DBusNodeInfo::for_xml(bluetooth::AGENT_XML) {
        Ok(node) => node,
        Err(e) => return Paired::Failed(bluetooth::bus_failed(&e.to_string())),
    };
    let Some(iface) = node.lookup_interface(bluetooth::AGENT) else {
        return Paired::Failed(bluetooth::bus_failed("the agent has no interface"));
    };
    let path = match ObjectPath::try_from(bluetooth::AGENT_PATH) {
        Ok(path) => path,
        Err(e) => return Paired::Failed(bluetooth::bus_failed(&e.to_string())),
    };

    let state = Rc::new(Run {
        bus: bus.clone(),
        bluez,
        device,
        address: address.to_string(),
        name: name.to_string(),
        near: near.downgrade(),
        shown: RefCell::new(None),
        shown_code: RefCell::new(None),
        cancelled: Cell::new(false),
        wanted_typing: Cell::new(false),
    });
    let registered = {
        let state = state.clone();
        bus.register_object(bluetooth::AGENT_PATH, &iface)
            .method_call(move |_, sender, _, _, method, params, invocation| {
                answer(&state, sender, method, &params, invocation)
            })
            .build()
    };
    let id = match registered {
        Ok(id) => id,
        Err(e) => return Paired::Failed(bluetooth::bus_failed(&e.to_string())),
    };

    let agent = bus
        .call_future(
            Some(bluetooth::BLUEZ),
            bluetooth::BLUEZ_ROOT,
            bluetooth::AGENT_MANAGER,
            REGISTER_AGENT,
            Some(&(path.clone(), bluetooth::AGENT_CAPABILITY).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            CALL_MS,
        )
        .await;
    if let Err(e) = agent {
        let _ = bus.unregister_object(id);
        return Paired::Failed(bluetooth::pair_failed(&said(&e), false, name));
    }

    eprintln!("edel-settings: bluetooth pairing {address} started");
    let paired = bus
        .call_future(
            Some(bluetooth::BLUEZ),
            &state.device,
            bluetooth::DEVICE,
            PAIR,
            None,
            None,
            gio::DBusCallFlags::NONE,
            pair_ms(),
        )
        .await;

    // Always, however the pairing went: the agent leaves the bus and the
    // prompt, if one is still up, goes.
    let _ = bus
        .call_future(
            Some(bluetooth::BLUEZ),
            bluetooth::BLUEZ_ROOT,
            bluetooth::AGENT_MANAGER,
            UNREGISTER_AGENT,
            Some(&(path,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            CALL_MS,
        )
        .await;
    let _ = bus.unregister_object(id);
    dismiss_shown(&state);

    match paired {
        Ok(_) => {
            eprintln!("edel-settings: bluetooth pairing {address} paired");
            Paired::Yes
        }
        Err(e) if already_paired(&e) => {
            eprintln!("edel-settings: bluetooth pairing {address} paired");
            Paired::Yes
        }
        Err(_) if state.cancelled.get() => {
            eprintln!("edel-settings: bluetooth pairing {address} cancelled");
            Paired::Cancelled
        }
        Err(e) => {
            let words = if state.wanted_typing.get() {
                bluetooth::type_here(name, address)
            } else {
                bluetooth::pair_failed(&said(&e), e.matches(gio::IOErrorEnum::TimedOut), name)
            };
            eprintln!("edel-settings: bluetooth pairing {address} failed: {words}");
            Paired::Failed(words)
        }
    }
}

/// How long Pair may take, in milliseconds, for the bus call.
fn pair_ms() -> i32 {
    i32::try_from(bluetooth::PAIR_TIME.as_millis()).unwrap_or(i32::MAX)
}

/// What a D-Bus error said: its remote name and message, as
/// `org.bluez.Error.AuthenticationFailed: Authentication Failed`, or the
/// local error's message.
fn said(e: &glib::Error) -> String {
    match gio::DBusError::remote_error(e) {
        Some(remote) => {
            let mut copy = e.clone();
            gio::DBusError::strip_remote_error(&mut copy);
            format!("{remote}: {}", copy.message())
        }
        None => e.message().to_string(),
    }
}

/// Whether Pair says the device is paired already, which is not a failure.
fn already_paired(e: &glib::Error) -> bool {
    gio::DBusError::remote_error(e)
        .is_some_and(|remote| remote.as_str() == bluetooth::ALREADY_PAIRED)
}

/// What BlueZ asked, read from its call's method and parameters (`None`
/// for a method the agent does not have, or parameters of other types).
fn question(method: &str, params: &glib::Variant) -> Option<Question> {
    Some(match method {
        "DisplayPasskey" => {
            let (device, passkey, _entered) = params.get::<(ObjectPath, u32, u16)>()?;
            Question::Show {
                device: device.as_str().to_string(),
                code: bluetooth::passkey(passkey),
            }
        }
        "DisplayPinCode" => {
            let (device, pin) = params.get::<(ObjectPath, String)>()?;
            Question::Show {
                device: device.as_str().to_string(),
                code: pin,
            }
        }
        "RequestConfirmation" => {
            let (device, passkey) = params.get::<(ObjectPath, u32)>()?;
            Question::Confirm {
                device: device.as_str().to_string(),
                code: bluetooth::passkey(passkey),
            }
        }
        "RequestAuthorization" => {
            let (device,) = params.get::<(ObjectPath,)>()?;
            Question::Allow {
                device: device.as_str().to_string(),
            }
        }
        "AuthorizeService" => {
            let (device, _uuid) = params.get::<(ObjectPath, String)>()?;
            Question::Allow {
                device: device.as_str().to_string(),
            }
        }
        "RequestPinCode" | "RequestPasskey" => {
            let (device,) = params.get::<(ObjectPath,)>()?;
            Question::Type {
                device: device.as_str().to_string(),
            }
        }
        "Cancel" | "Release" => {
            params.get::<()>()?;
            Question::Gone
        }
        _ => return None,
    })
}

/// The device a question is about; none for `Gone`, which is about the
/// whole pairing.
fn question_device(question: &Question) -> Option<&str> {
    match question {
        Question::Show { device, .. }
        | Question::Confirm { device, .. }
        | Question::Allow { device }
        | Question::Type { device } => Some(device.as_str()),
        Question::Gone => None,
    }
}

/// Answers one of BlueZ's calls to the agent. Only `org.bluez`'s own calls
/// are answered, and only about the device being paired.
fn answer(
    run: &Rc<Run>,
    sender: Option<&str>,
    method: &str,
    params: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
) {
    if sender != Some(run.bluez.as_str()) {
        invocation.return_dbus_error(bluetooth::REJECTED, "only bluetoothd may ask");
        return;
    }
    let Some(asked) = question(method, params) else {
        invocation.return_dbus_error(UNKNOWN_METHOD, method);
        return;
    };
    if question_device(&asked).is_some_and(|device| device != run.device.as_str()) {
        invocation.return_dbus_error(bluetooth::REJECTED, "not the device being paired");
        return;
    }
    match asked {
        Question::Show { code, .. } => show_code(run, &code, invocation),
        Question::Confirm { code, .. } => ask_match(run, &code, invocation),
        Question::Allow { .. } => invocation.return_value(None),
        Question::Type { .. } => {
            run.wanted_typing.set(true);
            invocation
                .return_dbus_error(bluetooth::REJECTED, "this agent cannot take a typed code");
        }
        Question::Gone => {
            dismiss_shown(run);
            invocation.return_value(None);
        }
    }
}

/// Shows a code to type on the device, with Cancel alone. Cancelling
/// cancels the pairing.
fn show_code(run: &Rc<Run>, code: &str, invocation: gio::DBusMethodInvocation) {
    // BlueZ asks again as each key is typed; the prompt on screen stays.
    if run.shown_code.borrow().as_deref() == Some(code) {
        invocation.return_value(None);
        return;
    }
    dismiss_shown(run);
    let Some(near) = run.near.upgrade() else {
        invocation.return_value(None);
        return;
    };
    let title = trf("Pair with {name}", &[("name", run.name.as_str())]);
    let message = trf("Type this code on {name}", &[("name", run.name.as_str())]);
    let detail = trf(
        "Then press Enter on it. This closes by itself once {name} is paired.",
        &[("name", run.name.as_str())],
    );
    let spoken = spoken(code);
    let asked = {
        let state = run.clone();
        move |yes: bool| {
            if !yes {
                state.cancelled.set(true);
                *state.shown_code.borrow_mut() = None;
                cancel_pairing(&state);
            }
        }
    };
    let prompt = widgets::prompt(
        &near,
        widgets::Prompt {
            title: title.as_str(),
            message: message.as_str(),
            code: Some((code, spoken.as_str())),
            detail: detail.as_str(),
            act: None,
            danger: false,
        },
        asked,
    );
    *run.shown.borrow_mut() = Some(prompt);
    *run.shown_code.borrow_mut() = Some(code.to_string());
    eprintln!(
        "edel-settings: bluetooth pairing {} shows a code",
        run.address
    );
    invocation.return_value(None);
}

/// Asks whether the device shows the same number: Pair, or Cancel, which
/// refuses it. Cancel is the default, so a stray Return pairs nothing.
fn ask_match(run: &Rc<Run>, code: &str, invocation: gio::DBusMethodInvocation) {
    dismiss_shown(run);
    let Some(near) = run.near.upgrade() else {
        invocation.return_dbus_error(bluetooth::REJECTED, "the page is closed");
        return;
    };
    let title = trf("Pair with {name}", &[("name", run.name.as_str())]);
    let message = trf(
        "Does {name} show this number?",
        &[("name", run.name.as_str())],
    );
    let detail = trf(
        "Pair only if the numbers are the same. If they differ, choose Cancel: another device may be posing as {name}.",
        &[("name", run.name.as_str())],
    );
    let spoken = spoken(code);
    let asked = {
        let state = run.clone();
        move |yes: bool| {
            if yes {
                invocation.return_value(None);
            } else {
                state.cancelled.set(true);
                invocation.return_dbus_error(bluetooth::REJECTED, "the numbers differ");
            }
        }
    };
    let prompt = widgets::prompt(
        &near,
        widgets::Prompt {
            title: title.as_str(),
            message: message.as_str(),
            code: Some((code, spoken.as_str())),
            detail: detail.as_str(),
            act: Some(tr("Pair")),
            danger: false,
        },
        asked,
    );
    *run.shown.borrow_mut() = Some(prompt);
    eprintln!(
        "edel-settings: bluetooth pairing {} asks whether the numbers match",
        run.address
    );
}

/// Takes the prompt on screen away, if there is one, without an answer.
fn dismiss_shown(run: &Rc<Run>) {
    *run.shown_code.borrow_mut() = None;
    if let Some(shown) = run.shown.take() {
        shown.dismiss();
    }
}

/// Tells BlueZ that the person cancelled, so it stops waiting. Its answer
/// is not waited for.
fn cancel_pairing(run: &Rc<Run>) {
    let (bus, device) = (run.bus.clone(), run.device.clone());
    glib::spawn_future_local(async move {
        let _ = bus
            .call_future(
                Some(bluetooth::BLUEZ),
                &device,
                bluetooth::DEVICE,
                CANCEL_PAIRING,
                None,
                None,
                gio::DBusCallFlags::NONE,
                CALL_MS,
            )
            .await;
    });
}

/// A code read figure by figure, `123456` to `1 2 3 4 5 6`, so a screen
/// reader says each figure.
fn spoken(code: &str) -> String {
    code.chars().map(String::from).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE: &str = "/org/bluez/hci0/dev_00_11_22_33_44_55";

    fn device() -> ObjectPath {
        ObjectPath::try_from(DEVICE).unwrap()
    }

    #[test]
    fn the_agent_interface_reads() {
        let node = gio::DBusNodeInfo::for_xml(bluetooth::AGENT_XML).unwrap();
        let iface = node
            .lookup_interface(bluetooth::AGENT)
            .expect("the agent interface");
        for method in [
            "Release",
            "RequestPinCode",
            "DisplayPinCode",
            "RequestPasskey",
            "DisplayPasskey",
            "RequestConfirmation",
            "RequestAuthorization",
            "AuthorizeService",
            "Cancel",
        ] {
            assert!(iface.lookup_method(method).is_some(), "{method}");
        }
    }

    #[test]
    fn the_agent_path_is_an_object_path() {
        assert!(ObjectPath::try_from(bluetooth::AGENT_PATH).is_ok());
    }

    #[test]
    fn bluezs_questions_are_read() {
        let show = |code: &str| Question::Show {
            device: DEVICE.to_string(),
            code: code.to_string(),
        };
        assert_eq!(
            question("DisplayPasskey", &(device(), 42u32, 0u16).to_variant()),
            Some(show("000042"))
        );
        assert_eq!(
            question("DisplayPinCode", &(device(), "0000").to_variant()),
            Some(show("0000"))
        );
        assert_eq!(
            question("RequestConfirmation", &(device(), 123456u32).to_variant()),
            Some(Question::Confirm {
                device: DEVICE.to_string(),
                code: "123456".to_string(),
            })
        );
        assert_eq!(
            question("RequestAuthorization", &(device(),).to_variant()),
            Some(Question::Allow {
                device: DEVICE.to_string(),
            })
        );
        assert_eq!(
            question(
                "AuthorizeService",
                &(device(), "0000110b-0000-1000-8000-00805f9b34fb").to_variant()
            ),
            Some(Question::Allow {
                device: DEVICE.to_string(),
            })
        );
        for method in ["RequestPinCode", "RequestPasskey"] {
            assert_eq!(
                question(method, &(device(),).to_variant()),
                Some(Question::Type {
                    device: DEVICE.to_string(),
                })
            );
        }
        assert_eq!(question("Cancel", &().to_variant()), Some(Question::Gone));
        assert_eq!(question("Release", &().to_variant()), Some(Question::Gone));
        assert_eq!(question("Frobnicate", &().to_variant()), None);
        assert_eq!(question("DisplayPasskey", &("x",).to_variant()), None);
    }

    #[test]
    fn a_code_is_read_figure_by_figure() {
        assert_eq!(spoken("123456"), "1 2 3 4 5 6");
        assert_eq!(spoken("0000"), "0 0 0 0");
    }
}
