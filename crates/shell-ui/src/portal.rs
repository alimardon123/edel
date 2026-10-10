//! The settings portal's backend (M5.5a): shell-ui serves
//! `org.freedesktop.impl.portal.Settings` on the session's D-Bus as
//! `org.freedesktop.impl.portal.desktop.edel`, and xdg-desktop-portal,
//! which D-Bus starts when an app first asks, passes on what it says: the
//! colour scheme and the accent, from the design tokens, so GTK,
//! libadwaita and Flatpak apps follow the desktop's look, and says them
//! again whenever shell-ui starts (M5.5c). It is no process
//! of its own: zbus answers on a thread of its own, and nothing runs
//! while no app asks.

use std::collections::HashMap;

use edel::panel_edit::{SHELL_BUS, SHELL_PATH};
use edel::tokens::{Colour, Tokens};
use smithay_client_toolkit::reexports::calloop::channel;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

use crate::shell_bus::ShellBus;

/// The name xdg-desktop-portal finds the backend by, from
/// `/usr/share/xdg-desktop-portal/portals/edel.portal`.
pub const NAME: &str = "org.freedesktop.impl.portal.desktop.edel";
const PATH: &str = "/org/freedesktop/portal/desktop";
const APPEARANCE: &str = "org.freedesktop.appearance";

/// What the portal says, by namespace and key.
type Settings = HashMap<String, HashMap<String, OwnedValue>>;

/// The settings from `tokens`: `color-scheme` prefers dark (1) when the
/// background is dark, else light (2); `accent-color` is the accent as
/// three numbers from 0 to 1; `contrast` is normal (0).
pub fn settings(tokens: &Tokens) -> Settings {
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
    let mut put = |key: &str, value: Value| {
        if let Ok(value) = OwnedValue::try_from(value) {
            appearance.insert(key.to_string(), value);
        }
    };
    put("color-scheme", Value::from(scheme));
    put("accent-color", Value::from(accent));
    put("contrast", Value::from(0u32));
    HashMap::from([(APPEARANCE.to_string(), appearance)])
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

/// Serves the portal on the session's bus for as long as the connection
/// it returns lives; none, with a line saying why, without a session bus.
/// It then says each setting changed, as shell-ui starts again when the
/// colour scheme does (M5.5c), so apps already open follow at once; an app
/// told a value it has does nothing.
///
/// The panel editor is served on the same connection (M5.31d, `shell_bus.rs`):
/// its object at `SHELL_PATH` is built in, and its name is requested once
/// the portal is up, so a name another program holds leaves the portal served.
pub fn serve(tokens: &Tokens, edits: channel::Sender<()>) -> Option<zbus::blocking::Connection> {
    let backend = Backend {
        settings: settings(tokens),
    };
    let built = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at(PATH, backend))
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
    let said = connection
        .object_server()
        .interface::<_, Backend>(PATH)
        .and_then(|backend| {
            let emitter = backend.signal_emitter();
            zbus::block_on(async {
                for (namespace, keys) in settings(tokens) {
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

    #[test]
    fn the_tokens_say_dark_and_give_the_accent() {
        let tokens = Tokens::built_in();
        let all = settings(&tokens);
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
        let all = settings(&light);
        assert_eq!(u32::try_from(&all[APPEARANCE]["color-scheme"]).unwrap(), 2);
    }

    #[test]
    fn read_all_gives_the_namespaces_asked_for() {
        let all = settings(&Tokens::built_in());
        let names = |wanted: &[&str]| {
            let wanted: Vec<String> = wanted.iter().map(|w| w.to_string()).collect();
            let mut got: Vec<String> = chosen(&all, &wanted).into_keys().collect();
            got.sort();
            got
        };
        assert_eq!(names(&[]), [APPEARANCE]);
        assert_eq!(names(&["org.freedesktop.appearance"]), [APPEARANCE]);
        assert_eq!(names(&["org.freedesktop.*"]), [APPEARANCE]);
        assert!(names(&["org.gnome.desktop.interface"]).is_empty());
    }
}
