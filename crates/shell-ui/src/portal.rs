//! The settings portal's backend (M5.5a): shell-ui serves
//! `org.freedesktop.impl.portal.Settings` on the session's D-Bus as
//! `org.freedesktop.impl.portal.desktop.edel`, and xdg-desktop-portal,
//! which D-Bus starts when an app first asks, passes on what it says: the
//! colour scheme and the accent, from the design tokens, so GTK,
//! libadwaita and Flatpak apps follow the desktop's look, and says them
//! again whenever shell-ui starts (M5.5c) and when they change (M6.17). It also serves
//! `org.freedesktop.impl.portal.Access` (M6.3): the screenshot portal asks
//! through it before an app may take a picture, and shell-ui draws the
//! dialog itself (`access_card.rs`). It also serves
//! `org.freedesktop.impl.portal.Screenshot` (M8.21 part 21f), which shell-ui
//! answers itself (`screenshot.rs`), so no grim and no `/tmp` file. It is no
//! process of its own: zbus answers on a thread of its own, and nothing runs
//! while no app asks.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use edel::panel_edit::{SHELL_BUS, SHELL_PATH};
use edel::tokens::{Colour, Tokens};
use smithay_client_toolkit::reexports::calloop::channel;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::access::{self, Ask};
use crate::shell_bus::{self, Edit, ShellBus};
use edel::i18n::tr;

/// The name xdg-desktop-portal finds the backend by, from
/// `/usr/share/xdg-desktop-portal/portals/edel.portal`.
pub const NAME: &str = "org.freedesktop.impl.portal.desktop.edel";
const PATH: &str = "/org/freedesktop/portal/desktop";
const APPEARANCE: &str = "org.freedesktop.appearance";
/// GTK's and libadwaita's namespace for the fonts, which Flatpak and GTK
/// apps read through the portal (M5.12b5).
const INTERFACE: &str = "org.gnome.desktop.interface";
/// GNOME's namespace for the window manager's preferences: the side and the
/// buttons of a header bar, which GTK and libadwaita apps read for their own
/// bars (M6.16). `button-layout` is GNOME's own format, which
/// `edel::presets::gnome_button_layout` writes.
const WM: &str = "org.gnome.desktop.wm.preferences";
const BUTTON_LAYOUT: &str = "button-layout";
/// The icon theme apps draw their glyphs from (M6.16): the `Edel` theme of
/// `features/shell/usr/share/icons/Edel`, our window glyphs under the names
/// GTK asks for, which step 3 of M6.16 ships.
const ICON_THEME: &str = "Edel";
/// A pixel is three quarters of a point, as GTK sizes its text (M5.12b5).
const POINTS_PER_PIXEL: f64 = 0.75;

/// What the portal says, by namespace and key.
type Settings = HashMap<String, HashMap<String, OwnedValue>>;

/// The settings from `tokens`: `color-scheme` prefers dark (1) when the
/// background is dark, else light (2); `accent-color` is the accent as
/// three numbers from 0 to 1; `contrast` is high (1) when `appearance.contrast`
/// is high, else normal (0) (M6.18). `layout` is the header bars' button
/// layout in GNOME's form (M6.16), which apps' own bars follow.
pub fn settings(tokens: &Tokens, layout: &str, high: bool) -> Settings {
    let scheme: u32 = if lightness(tokens.background) < 0.5 {
        1
    } else {
        2
    };
    let accent = tokens.accent;
    let accent = (
        f64::from(accent.r),
        f64::from(accent.g),
        f64::from(accent.b),
    );
    let mut appearance = HashMap::new();
    put(&mut appearance, "color-scheme", Value::from(scheme));
    put(&mut appearance, "accent-color", Value::from(accent));
    let contrast: u32 = if high { 1 } else { 0 };
    put(&mut appearance, "contrast", Value::from(contrast));
    // The text (M5.12b5): the fonts with their sizes in points, as GTK reads
    // `font-name`, and the text's scale as the desktop's percent.
    let points = (f64::from(tokens.text_size) * POINTS_PER_PIXEL).round();
    let mut interface = HashMap::new();
    put(
        &mut interface,
        "font-name",
        Value::from(format!("{} {points}", tokens.font)),
    );
    put(
        &mut interface,
        "monospace-font-name",
        Value::from(format!("{} {points}", tokens.monospace)),
    );
    put(
        &mut interface,
        "text-scaling-factor",
        Value::from(f64::from(tokens.text_percent) / 100.0),
    );
    put(&mut interface, "icon-theme", Value::from(ICON_THEME));
    let mut wm = HashMap::new();
    put(&mut wm, BUTTON_LAYOUT, Value::from(layout));
    HashMap::from([
        (APPEARANCE.to_string(), appearance),
        (INTERFACE.to_string(), interface),
        (WM.to_string(), wm),
    ])
}

/// Adds `key` to a namespace's keys, when the value can be sent.
fn put(keys: &mut HashMap<String, OwnedValue>, key: &str, value: Value) {
    if let Ok(value) = OwnedValue::try_from(value) {
        keys.insert(key.to_string(), value);
    }
}

/// A colour's relative lightness, 0 for black to 1 for white.
fn lightness(c: Colour) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

/// The namespaces `wanted` asks for: all for none, else each one named,
/// or every one starting with what comes before a final `*`.
fn chosen(settings: &Settings, wanted: &[String]) -> Settings {
    let fits = |namespace: &str| {
        wanted.is_empty()
            || wanted.iter().any(|w| match w.strip_suffix('*') {
                Some(prefix) => namespace.starts_with(prefix),
                None => namespace == w,
            })
    };
    settings
        .iter()
        .filter(|(namespace, _)| fits(namespace))
        .map(|(namespace, keys)| {
            let keys = keys
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), v.try_clone().ok()?)))
                .collect();
            (namespace.clone(), keys)
        })
        .collect()
}

/// The Access portal's backend (M6.3): the question a screenshot asks the
/// person, through the event loop's dialog. Only `AccessDialog` is served;
/// the request object the portal makes is its own, so this does not export
/// `org.freedesktop.impl.portal.Request`, and the portal ends the request
/// itself.
struct Asking {
    edits: channel::Sender<Edit>,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Access")]
impl Asking {
    /// Asks the person whether `app_id` may do what `title`, `subtitle` and
    /// `body` say, and answers 0 for allowed, 1 for denied and 2 when the
    /// question ended without an answer. `options` may name `deny_label` and
    /// `grant_label`; `choices` and `modal` are ignored, as the dialog has two
    /// buttons and is always modal. The `handle` and `parent_window` are not
    /// used: the dialog is shell-ui's own, on the screen the panels lie on.
    #[allow(clippy::too_many_arguments)]
    async fn access_dialog(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        subtitle: String,
        body: String,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let _ = (&handle, &parent_window);
        let deny = text_option(&options, "deny_label").unwrap_or_else(|| tr("Deny").to_string());
        let grant = text_option(&options, "grant_label").unwrap_or_else(|| tr("Allow").to_string());
        let answer = ask(&self.edits, app_id, title, subtitle, body, deny, grant).await;
        (answer, HashMap::new())
    }
}

/// Asks the person through the event loop's dialog and waits for the answer
/// (M6.3, M8.21 part 21f): the dialog names `app_id`, or "An app" when it is
/// empty, and shows the portal's `title`, `subtitle` and `body` with the two
/// labels. The answer is `access::ALLOW`, `DENY` or `CANCEL`.
async fn ask(
    edits: &channel::Sender<Edit>,
    app_id: String,
    title: String,
    subtitle: String,
    body: String,
    deny: String,
    grant: String,
) -> u32 {
    let app = if app_id.is_empty() {
        tr("An app").to_string()
    } else {
        app_id
    };
    let (answer, reply) = shell_bus::answer();
    let ask = Ask {
        app,
        title,
        subtitle,
        body,
        deny,
        grant,
        answer,
    };
    if edits.send(Edit::Access(ask)).is_err() {
        return access::CANCEL;
    }
    reply.wait().await
}

/// The Screenshot portal's backend (M8.21 part 21f): shell-ui takes the
/// picture itself (`screenshot.rs`), so no other program reads the screens
/// and nothing is written to `/tmp`. Only `Screenshot` and `PickColor` are
/// served; the request object is the portal's own, as the Access dialog's is.
struct Shooting {
    edits: channel::Sender<Edit>,
    /// The pictures taken since shell-ui started, so each file's name is
    /// its own even when two come in the same millisecond.
    taken: AtomicU64,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl Shooting {
    /// Takes a picture of every screen into a PNG in the person's runtime
    /// folder and answers 0 with its `file://` address, or 1 when the person
    /// denied it, or 2 when it did not happen. The portal asks the person
    /// first only for an interactive request or an older portal (`must_ask`);
    /// otherwise its permission store has answered already.
    async fn screenshot(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let _ = (&handle, &parent_window);
        if must_ask(&options) {
            let deny = tr("Deny").to_string();
            let grant = tr("Allow").to_string();
            let answer = ask(
                &self.edits,
                app_id,
                String::new(),
                String::new(),
                String::new(),
                deny,
                grant,
            )
            .await;
            if answer != access::ALLOW {
                let code = if answer == access::DENY { 1 } else { 2 };
                return (code, HashMap::new());
            }
        }
        let Some(dir) = edel::places::screenshots_dir() else {
            eprintln!(
                "edel-shell-ui: {}",
                crate::messages::screenshot_not_saved("XDG_RUNTIME_DIR is not set")
            );
            return (2, HashMap::new());
        };
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let n = self.taken.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("screenshot-{millis}-{n}.png"));
        let (answer, reply) = shell_bus::answer();
        let edit = Edit::Screenshot {
            path: path.clone(),
            answer,
        };
        if self.edits.send(edit).is_err() {
            return (2, HashMap::new());
        }
        if reply.wait().await != access::ALLOW {
            return (2, HashMap::new());
        }
        let uri = format!("file://{}", path.display());
        let Ok(value) = OwnedValue::try_from(Value::from(uri)) else {
            return (2, HashMap::new());
        };
        (0, HashMap::from([("uri".to_string(), value)]))
    }

    /// The colour picker is not built yet, so the request ends without a
    /// colour (M8.21 part 21f).
    async fn pick_color(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let _ = (&handle, &app_id, &parent_window, &options);
        (2, HashMap::new())
    }

    /// The interface's version: 2 makes the portal ask the person through
    /// Access first and pass `permission_store_checked` (M8.21 part 21f).
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }
}

/// Whether the screenshot portal's request must be asked of the person
/// first (M8.21 part 21f): an interactive request always is, even when the
/// portal's store has the answer, and a request the store has not answered
/// (no `permission_store_checked`) is too.
fn must_ask(options: &HashMap<String, OwnedValue>) -> bool {
    bool_option(options, "interactive") == Some(true)
        || bool_option(options, "permission_store_checked") != Some(true)
}

/// Tells the session bus the compositor's Wayland display (M6.3), as other
/// desktops do at their start: the bus started before the compositor made
/// its socket, so a service it starts for an app, such as the screenshot
/// portal's backend, would otherwise not know which display to reach.
fn tell_display(connection: &zbus::blocking::Connection) {
    let Ok(display) = std::env::var("WAYLAND_DISPLAY") else {
        return;
    };
    let told = connection.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "UpdateActivationEnvironment",
        &HashMap::from([("WAYLAND_DISPLAY", display.as_str())]),
    );
    match told {
        Ok(_) => eprintln!("edel-shell-ui: told D-Bus the Wayland display {display}"),
        Err(e) => eprintln!("edel-shell-ui: {}", crate::messages::display_not_told(e)),
    }
}

/// The text of a string option, when the portal gave a non-empty one.
fn text_option(options: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let value = options.get(key)?.try_clone().ok()?;
    String::try_from(value).ok().filter(|text| !text.is_empty())
}

/// The value of a boolean option (M8.21 part 21f), when the portal gave one.
fn bool_option(options: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    let value = options.get(key)?.try_clone().ok()?;
    bool::try_from(value).ok()
}

/// The portal's own error for a setting it does not have.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.portal.Error")]
enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    NotFound(String),
}

struct Backend {
    settings: Settings,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Settings")]
impl Backend {
    fn read_all(&self, namespaces: Vec<String>) -> Settings {
        chosen(&self.settings, &namespaces)
    }

    fn read(&self, namespace: &str, key: &str) -> Result<OwnedValue, Error> {
        self.settings
            .get(namespace)
            .and_then(|keys| keys.get(key))
            .and_then(|value| value.try_clone().ok())
            .ok_or_else(|| Error::NotFound(format!("{namespace} {key} is not a setting")))
    }

    /// The interface's version, lowercase as the spec names it.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }

    /// A setting changed (M5.5c); xdg-desktop-portal passes it on to apps.
    #[zbus(signal)]
    async fn setting_changed(
        emitter: &SignalEmitter<'_>,
        namespace: &str,
        key: &str,
        value: Value<'_>,
    ) -> zbus::Result<()>;
}

/// The Lockdown backend (M6.14c): the portal's own switches, which its
/// location portal checks at each request (CreateSession and Start). Only
/// `disable-location` is set, from the person's `location.enabled` (ADR-008,
/// the same setting as the Location tile): while it is off every app gets
/// nothing from the location portal, whatever its own choice, and turning it
/// off refuses new requests but does not end one already running. Every
/// other switch is off. Without shell-ui the portal reads it as false, so
/// Location is not off.
struct Lockdown {
    location_off: bool,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Lockdown")]
impl Lockdown {
    #[zbus(property, name = "disable-printing")]
    fn disable_printing(&self) -> bool {
        false
    }

    #[zbus(property, name = "disable-save-to-disk")]
    fn disable_save_to_disk(&self) -> bool {
        false
    }

    #[zbus(property, name = "disable-application-handlers")]
    fn disable_application_handlers(&self) -> bool {
        false
    }

    #[zbus(property, name = "disable-camera")]
    fn disable_camera(&self) -> bool {
        false
    }

    #[zbus(property, name = "disable-microphone")]
    fn disable_microphone(&self) -> bool {
        false
    }

    #[zbus(property, name = "disable-sound-output")]
    fn disable_sound_output(&self) -> bool {
        false
    }

    #[zbus(property, name = "disable-location")]
    fn disable_location(&self) -> bool {
        self.location_off
    }
}

/// Tells the portal the Location switch changed (M6.14c): `off` says whether
/// the person's `location.enabled` is false now. The property's changed
/// signal goes out with it, and the value is set only when it differs.
pub fn follow_location(connection: &zbus::blocking::Connection, off: bool) {
    let lockdown = match connection.object_server().interface::<_, Lockdown>(PATH) {
        Ok(lockdown) => lockdown,
        Err(e) => {
            eprintln!("edel-shell-ui: {}", crate::messages::location_not_told(e));
            return;
        }
    };
    {
        let mut held = lockdown.get_mut();
        if held.location_off == off {
            return;
        }
        held.location_off = off;
    }
    let said = zbus::block_on(
        lockdown
            .get()
            .disable_location_changed(lockdown.signal_emitter()),
    );
    match said {
        Ok(()) => eprintln!(
            "edel-shell-ui: location {} told to the portal",
            if off { "off" } else { "on" }
        ),
        Err(e) => eprintln!("edel-shell-ui: {}", crate::messages::location_not_told(e)),
    }
}

/// Says again each setting apps read that changed (M5.5c, M6.16, M6.17):
/// the colour scheme, the accent, high contrast and the fonts from `tokens`,
/// and the header bars' button layout; each changed value is stored and
/// `setting_changed` goes out for it, so apps already open follow. Values
/// that did not change say nothing.
pub fn follow(connection: &zbus::blocking::Connection, tokens: &Tokens, layout: &str, high: bool) {
    let backend = match connection.object_server().interface::<_, Backend>(PATH) {
        Ok(backend) => backend,
        Err(e) => {
            eprintln!("edel-shell-ui: {}", crate::messages::portal_not_said(e));
            return;
        }
    };
    let mut changed = Vec::new();
    {
        let mut backend = backend.get_mut();
        for (namespace, keys) in settings(tokens, layout, high) {
            let stored = backend.settings.entry(namespace.clone()).or_default();
            for (key, value) in keys {
                if stored.get(&key) == Some(&value) {
                    continue;
                }
                if let Ok(copy) = value.try_clone() {
                    changed.push((namespace.clone(), key.clone(), copy));
                }
                stored.insert(key, value);
            }
        }
    }
    for (namespace, key, value) in changed {
        if key == BUTTON_LAYOUT {
            if let Ok(layout) = <&str>::try_from(&value) {
                eprintln!("edel-shell-ui: button layout {layout} told to apps");
            }
        } else {
            eprintln!("edel-shell-ui: {key} told to apps");
        }
        let said = zbus::block_on(Backend::setting_changed(
            backend.signal_emitter(),
            &namespace,
            &key,
            value.into(),
        ));
        if let Err(e) = said {
            let message = if key == BUTTON_LAYOUT {
                crate::messages::button_layout_not_said(e)
            } else {
                crate::messages::portal_not_said(e)
            };
            eprintln!("edel-shell-ui: {message}");
        }
    }
}

/// Serves the portal on the session's bus for as long as the connection
/// it returns lives; none, with a line saying why, without a session bus.
/// It then says each setting changed, as a shell-ui started again may say
/// other values than the one before it (M5.5c), so apps already open follow
/// at once; an app told a value it has does nothing. Later changes go out
/// through [`follow`] (M6.17).
///
/// The panel editor is served on the same connection (M5.31d, `shell_bus.rs`):
/// its object at `SHELL_PATH` is built in, and its name is requested once
/// the portal is up, so a name another program holds leaves the portal served.
pub fn serve(
    tokens: &Tokens,
    layout: &str,
    high: bool,
    edits: channel::Sender<Edit>,
    location_off: bool,
) -> Option<zbus::blocking::Connection> {
    let backend = Backend {
        settings: settings(tokens, layout, high),
    };
    let asking = Asking {
        edits: edits.clone(),
    };
    let shooting = Shooting {
        edits: edits.clone(),
        taken: AtomicU64::new(0),
    };
    let built = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at(PATH, backend))
        .and_then(|b| b.serve_at(PATH, asking))
        .and_then(|b| b.serve_at(PATH, shooting))
        .and_then(|b| b.serve_at(PATH, Lockdown { location_off }))
        .and_then(|b| b.serve_at(SHELL_PATH, ShellBus::new(edits)))
        .and_then(|b| b.build());
    let connection = match built {
        Ok(connection) => connection,
        Err(e) => {
            eprintln!("edel-shell-ui: {}", crate::messages::portal_not_served(e));
            return None;
        }
    };
    if let Err(e) = connection.request_name(SHELL_BUS) {
        eprintln!("edel-shell-ui: {}", crate::messages::edit_bus_not_served(e));
    }
    eprintln!("edel-shell-ui: serving the settings portal as {NAME}");
    tell_display(&connection);
    let said = connection
        .object_server()
        .interface::<_, Backend>(PATH)
        .and_then(|backend| {
            let emitter = backend.signal_emitter();
            zbus::block_on(async {
                for (namespace, keys) in settings(tokens, layout, high) {
                    for (key, value) in keys {
                        Backend::setting_changed(emitter, &namespace, &key, value.into()).await?;
                    }
                }
                Ok(())
            })
        });
    if let Err(e) = said {
        eprintln!("edel-shell-ui: {}", crate::messages::portal_not_said(e));
    }
    Some(connection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access;

    #[test]
    fn a_string_option_is_read_when_given_and_not_empty() {
        let mut options = HashMap::new();
        options.insert(
            "deny_label".to_string(),
            OwnedValue::try_from(Value::from("Skip")).unwrap(),
        );
        options.insert(
            "grant_label".to_string(),
            OwnedValue::try_from(Value::from("")).unwrap(),
        );
        assert_eq!(text_option(&options, "deny_label").as_deref(), Some("Skip"));
        assert_eq!(text_option(&options, "grant_label"), None, "empty is none");
        assert_eq!(text_option(&options, "missing"), None);
    }

    /// The tests that serve the portal's name on a session bus, one at a
    /// time: two at once would each own it in turn, and a call would reach
    /// the other's loop.
    static ON_THE_BUS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A question asked over the bus reaches the event loop as `Edit::Access`
    /// and its answer goes back to the caller (M6.3). Needs a session bus:
    /// without one, `serve` gives nothing and the test passes by saying so.
    #[test]
    fn an_access_question_is_asked_and_answered_over_the_bus() {
        let _alone = ON_THE_BUS.lock();
        use smithay_client_toolkit::reexports::calloop::EventLoop;
        use std::time::Duration;
        let (edits, asks) = channel::channel::<Edit>();
        let Some(_served) = serve(&Tokens::built_in(), BUTTONS_RIGHT, false, edits, false) else {
            return;
        };
        let client = zbus::blocking::Connection::session().expect("the session bus");
        let proxy =
            zbus::blocking::Proxy::new(&client, NAME, PATH, "org.freedesktop.impl.portal.Access")
                .expect("the Access proxy");
        let caller = std::thread::spawn(move || {
            let handle = OwnedObjectPath::try_from("/org/freedesktop/portal/desktop/request/t/x")
                .expect("a handle");
            proxy.call::<_, _, (u32, HashMap<String, OwnedValue>)>(
                "AccessDialog",
                &(
                    handle,
                    "org.example.Shot".to_string(),
                    String::new(),
                    String::new(),
                    String::new(),
                    "Take a picture?".to_string(),
                    HashMap::<String, OwnedValue>::new(),
                ),
            )
        });
        let mut event_loop: EventLoop<Option<Ask>> = EventLoop::try_new().expect("a loop");
        event_loop
            .handle()
            .insert_source(asks, |event, _, got: &mut Option<Ask>| {
                if let channel::Event::Msg(Edit::Access(ask)) = event {
                    *got = Some(ask);
                }
            })
            .expect("watching the asks");
        let mut got = None;
        for _ in 0..50 {
            if got.is_some() {
                break;
            }
            event_loop
                .dispatch(Some(Duration::from_millis(100)), &mut got)
                .expect("dispatching");
        }
        let ask = got.expect("the question reached the loop");
        assert_eq!(ask.app, "org.example.Shot");
        assert_eq!(ask.body, "Take a picture?");
        assert_eq!(
            ask.shown_title(),
            "Allow org.example.Shot to take screenshots?"
        );
        ask.answer.send(access::ALLOW);
        let (code, results) = caller
            .join()
            .expect("the caller returns")
            .expect("the answer");
        assert_eq!(code, access::ALLOW);
        assert!(results.is_empty());
    }

    /// The boolean options the screenshot portal passes are read as bools
    /// only; anything else, or no value, is none (M8.21 part 21f).
    #[test]
    fn a_bool_option_is_read_when_given_as_a_bool() {
        let mut options = HashMap::new();
        options.insert(
            "permission_store_checked".to_string(),
            OwnedValue::try_from(Value::from(true)).unwrap(),
        );
        options.insert(
            "off".to_string(),
            OwnedValue::try_from(Value::from(false)).unwrap(),
        );
        options.insert(
            "wrong".to_string(),
            OwnedValue::try_from(Value::from("yes")).unwrap(),
        );
        assert_eq!(
            bool_option(&options, "permission_store_checked"),
            Some(true)
        );
        assert_eq!(bool_option(&options, "off"), Some(false));
        assert_eq!(bool_option(&options, "wrong"), None, "a string is none");
        assert_eq!(bool_option(&options, "missing"), None);
    }

    /// An interactive request is asked of the person even when the store
    /// has answered; a store-checked one is not; one the store has not
    /// answered is asked (M8.21 part 21f).
    #[test]
    fn a_screenshot_asks_when_interactive_or_not_checked() {
        let flag = |key: &str, value: bool| {
            (
                key.to_string(),
                OwnedValue::try_from(Value::from(value)).unwrap(),
            )
        };
        let options =
            |pairs: Vec<(String, OwnedValue)>| pairs.into_iter().collect::<HashMap<_, _>>();
        assert!(must_ask(&options(vec![
            flag("interactive", true),
            flag("permission_store_checked", true),
        ])));
        assert!(!must_ask(&options(vec![flag(
            "permission_store_checked",
            true
        )])));
        assert!(must_ask(&options(vec![])), "no answer from the store asks");
        assert!(must_ask(&options(vec![flag("interactive", false)])));
    }

    #[test]
    fn the_screenshot_portal_is_named_as_the_portal_names_it() {
        use zbus::object_server::Interface;

        assert_eq!(
            Shooting::name().as_str(),
            "org.freedesktop.impl.portal.Screenshot"
        );
    }

    #[test]
    fn the_access_portal_is_served_on_the_settings_portal_path() {
        use zbus::object_server::Interface;

        // The dialog's interface name is the portal's own.
        assert_eq!(
            Asking::name().as_str(),
            "org.freedesktop.impl.portal.Access"
        );
    }

    /// The Lockdown backend says `disable-location` only while the person
    /// has Location off, and no other switch (M6.14c).
    #[test]
    fn the_lockdown_says_location_is_off_only_when_the_person_turned_it_off() {
        use zbus::object_server::Interface;

        assert_eq!(
            Lockdown::name().as_str(),
            "org.freedesktop.impl.portal.Lockdown"
        );
        assert!(Lockdown { location_off: true }.disable_location());
        assert!(
            !Lockdown {
                location_off: false
            }
            .disable_location()
        );
        assert!(!Lockdown { location_off: true }.disable_camera());
    }

    /// Served with a session bus, the Lockdown properties read as set and
    /// `follow_location` changes the one that is told (M6.14c). Without a
    /// session bus the test passes by saying nothing.
    #[test]
    fn the_lockdown_is_served_and_follows_the_location_switch() {
        let _alone = ON_THE_BUS.lock();
        let (edits, _asks) = channel::channel::<Edit>();
        let Some(served) = serve(&Tokens::built_in(), BUTTONS_RIGHT, false, edits, true) else {
            return;
        };
        let client = zbus::blocking::Connection::session().expect("the session bus");
        let proxy =
            zbus::blocking::Proxy::new(&client, NAME, PATH, "org.freedesktop.DBus.Properties")
                .expect("the properties proxy");
        let read = |name: &str| -> bool {
            let value: OwnedValue = proxy
                .call("Get", &("org.freedesktop.impl.portal.Lockdown", name))
                .expect("the property");
            bool::try_from(value).expect("a bool")
        };
        assert!(read("disable-location"), "the switch is off at start");
        assert!(!read("disable-camera"));
        follow_location(&served, false);
        assert!(!read("disable-location"), "the switch is on again");
        follow_location(&served, false);
        assert!(!read("disable-location"), "the same value says nothing");
    }

    #[test]
    fn the_tokens_say_dark_and_give_the_accent() {
        let tokens = Tokens::built_in();
        let all = settings(&tokens, BUTTONS_RIGHT, false);
        let appearance = &all[APPEARANCE];
        assert_eq!(u32::try_from(&appearance["color-scheme"]).unwrap(), 1);
        let (r, g, b) =
            <(f64, f64, f64)>::try_from(appearance["accent-color"].try_clone().unwrap()).unwrap();
        let accent = tokens.accent;
        assert!((r - f64::from(accent.r)).abs() < 1e-6);
        assert!((g - f64::from(accent.g)).abs() < 1e-6);
        assert!((b - f64::from(accent.b)).abs() < 1e-6);
        // The light scheme's tokens say light (M5.5c).
        let light = Tokens::built_in_scheme(edel::tokens::Scheme::Light);
        let all = settings(&light, BUTTONS_RIGHT, false);
        assert_eq!(u32::try_from(&all[APPEARANCE]["color-scheme"]).unwrap(), 2);
    }

    #[test]
    fn high_contrast_says_one() {
        let tokens = Tokens::built_in();
        let high = settings(&tokens, BUTTONS_RIGHT, true);
        assert_eq!(u32::try_from(&high[APPEARANCE]["contrast"]).unwrap(), 1);
        let normal = settings(&tokens, BUTTONS_RIGHT, false);
        assert_eq!(u32::try_from(&normal[APPEARANCE]["contrast"]).unwrap(), 0);
    }

    #[test]
    fn read_all_gives_the_namespaces_asked_for() {
        let all = settings(&Tokens::built_in(), BUTTONS_RIGHT, false);
        let names = |wanted: &[&str]| {
            let wanted: Vec<String> = wanted.iter().map(|w| w.to_string()).collect();
            let mut got: Vec<String> = chosen(&all, &wanted).into_keys().collect();
            got.sort();
            got
        };
        assert_eq!(names(&[]), [APPEARANCE, INTERFACE, WM]);
        assert_eq!(names(&["org.freedesktop.appearance"]), [APPEARANCE]);
        assert_eq!(names(&["org.freedesktop.*"]), [APPEARANCE]);
        assert_eq!(names(&["org.gnome.desktop.interface"]), [INTERFACE]);
        assert!(names(&["org.gnome.nothing"]).is_empty());
    }

    /// GNOME's layout for Classic's buttons, the release's default (M6.16).
    const BUTTONS_RIGHT: &str = ":minimize,maximize,close";

    /// Apps' own header bars read the button layout and the icon theme from
    /// the portal (M6.16): the layout under `org.gnome.desktop.wm.preferences`,
    /// and `Edel` as the interface's icon theme.
    #[test]
    fn the_portal_says_the_button_layout_and_the_icon_theme() {
        let all = settings(&Tokens::built_in(), BUTTONS_RIGHT, false);
        let layout = String::try_from(all[WM][BUTTON_LAYOUT].try_clone().unwrap()).unwrap();
        assert_eq!(layout, ":minimize,maximize,close");
        let theme = String::try_from(all[INTERFACE]["icon-theme"].try_clone().unwrap()).unwrap();
        assert_eq!(theme, "Edel");
        let left = settings(&Tokens::built_in(), "close,minimize,maximize:", false);
        let layout = String::try_from(left[WM][BUTTON_LAYOUT].try_clone().unwrap()).unwrap();
        assert_eq!(layout, "close,minimize,maximize:");
    }

    #[test]
    fn the_text_gives_the_fonts_in_points_and_the_scale() {
        let tokens = Tokens::built_in();
        let all = settings(&tokens, BUTTONS_RIGHT, false);
        let interface = &all[INTERFACE];
        let text = |key: &str| String::try_from(interface[key].try_clone().unwrap()).unwrap();
        // The release's 13 px is 9.75 points, which GTK shows as 10.
        assert_eq!(text("font-name"), "Inter Variable 10");
        assert_eq!(text("monospace-font-name"), "Noto Sans Mono 10");
        let scale = f64::try_from(interface["text-scaling-factor"].try_clone().unwrap()).unwrap();
        assert!((scale - 1.0).abs() < 1e-9);
        let mut big = tokens.clone();
        big.text_size = 20;
        big.text_percent = 150;
        let all = settings(&big, BUTTONS_RIGHT, false);
        let scale = f64::try_from(all[INTERFACE]["text-scaling-factor"].try_clone().unwrap());
        assert!((scale.unwrap() - 1.5).abs() < 1e-9);
        assert_eq!(
            String::try_from(all[INTERFACE]["font-name"].try_clone().unwrap()).unwrap(),
            "Inter Variable 15"
        );
    }
}
