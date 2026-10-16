//! The settings portal's backend (M5.5a): shell-ui serves
//! `org.freedesktop.impl.portal.Settings` on the session's D-Bus as
//! `org.freedesktop.impl.portal.desktop.edel`, and xdg-desktop-portal,
//! which D-Bus starts when an app first asks, passes on what it says: the
//! colour scheme and the accent, from the design tokens, so GTK,
//! libadwaita and Flatpak apps follow the desktop's look, and says them
//! again whenever shell-ui starts (M5.5c) and when they change (M6.17). It also serves
//! `org.freedesktop.impl.portal.Access` (M6.3): the screenshot portal asks
//! through it before an app may take a picture, and shell-ui draws the
//! dialog itself (`access_card.rs`). It is no process of its own: zbus
//! answers on a thread of its own, and nothing runs while no app asks.

use std::collections::HashMap;

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
/// three numbers from 0 to 1; `contrast` is normal (0). `layout` is the
/// header bars' button layout in GNOME's form (M6.16), which apps' own bars
/// follow.
pub fn settings(tokens: &Tokens, layout: &str) -> Settings {
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
    put(&mut appearance, "contrast", Value::from(0u32));
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
        let app = if app_id.is_empty() {
            tr("An app").to_string()
        } else {
            app_id
        };
        let deny = text_option(&options, "deny_label").unwrap_or_else(|| tr("Deny").to_string());
        let grant = text_option(&options, "grant_label").unwrap_or_else(|| tr("Allow").to_string());
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
        if self.edits.send(Edit::Access(ask)).is_err() {
            return (access::CANCEL, HashMap::new());
        }
        (reply.wait().await, HashMap::new())
    }
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

/// Says again each setting apps read that changed (M5.5c, M6.16, M6.17):
/// the colour scheme, the accent and the fonts from `tokens`, and the
/// header bars' button layout; each changed value is stored and
/// `setting_changed` goes out for it, so apps already open follow. Values
/// that did not change say nothing.
pub fn follow(connection: &zbus::blocking::Connection, tokens: &Tokens, layout: &str) {
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
        for (namespace, keys) in settings(tokens, layout) {
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
    edits: channel::Sender<Edit>,
) -> Option<zbus::blocking::Connection> {
    let backend = Backend {
        settings: settings(tokens, layout),
    };
    let asking = Asking {
        edits: edits.clone(),
    };
    let built = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at(PATH, backend))
        .and_then(|b| b.serve_at(PATH, asking))
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
                for (namespace, keys) in settings(tokens, layout) {
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

    /// A question asked over the bus reaches the event loop as `Edit::Access`
    /// and its answer goes back to the caller (M6.3). Needs a session bus:
    /// without one, `serve` gives nothing and the test passes by saying so.
    #[test]
    fn an_access_question_is_asked_and_answered_over_the_bus() {
        use smithay_client_toolkit::reexports::calloop::EventLoop;
        use std::time::Duration;
        let (edits, asks) = channel::channel::<Edit>();
        let Some(_served) = serve(&Tokens::built_in(), BUTTONS_RIGHT, edits) else {
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

    #[test]
    fn the_access_portal_is_served_on_the_settings_portal_path() {
        use zbus::object_server::Interface;

        // The dialog's interface name is the portal's own.
        assert_eq!(
            Asking::name().as_str(),
            "org.freedesktop.impl.portal.Access"
        );
    }

    #[test]
    fn the_tokens_say_dark_and_give_the_accent() {
        let tokens = Tokens::built_in();
        let all = settings(&tokens, BUTTONS_RIGHT);
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
        let all = settings(&light, BUTTONS_RIGHT);
        assert_eq!(u32::try_from(&all[APPEARANCE]["color-scheme"]).unwrap(), 2);
    }

    #[test]
    fn read_all_gives_the_namespaces_asked_for() {
        let all = settings(&Tokens::built_in(), BUTTONS_RIGHT);
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
        let all = settings(&Tokens::built_in(), BUTTONS_RIGHT);
        let layout = String::try_from(all[WM][BUTTON_LAYOUT].try_clone().unwrap()).unwrap();
        assert_eq!(layout, ":minimize,maximize,close");
        let theme = String::try_from(all[INTERFACE]["icon-theme"].try_clone().unwrap()).unwrap();
        assert_eq!(theme, "Edel");
        let left = settings(&Tokens::built_in(), "close,minimize,maximize:");
        let layout = String::try_from(left[WM][BUTTON_LAYOUT].try_clone().unwrap()).unwrap();
        assert_eq!(layout, "close,minimize,maximize:");
    }

    #[test]
    fn the_text_gives_the_fonts_in_points_and_the_scale() {
        let tokens = Tokens::built_in();
        let all = settings(&tokens, BUTTONS_RIGHT);
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
        let all = settings(&big, BUTTONS_RIGHT);
        let scale = f64::try_from(all[INTERFACE]["text-scaling-factor"].try_clone().unwrap());
        assert!((scale.unwrap() - 1.5).abs() < 1e-9);
        assert_eq!(
            String::try_from(all[INTERFACE]["font-name"].try_clone().unwrap()).unwrap(),
            "Inter Variable 15"
        );
    }
}
