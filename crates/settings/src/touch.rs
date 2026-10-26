//! Touch mode (M9.6): Settings reads `edel::touch`'s key from the settings
//! portal once, follows it as it flips, and while it is true marks the
//! window `edel-touch`, which the stylesheet grows to finger-sized rows
//! (`style::css`). With no portal, or no answer, it stays a pointer's size.

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const CLASS: &str = "edel-touch";

/// Follows touch mode on `window` for as long as the window lives.
pub fn follow(window: &gtk::ApplicationWindow) {
    let weak = window.downgrade();
    gio::DBusProxy::for_bus(
        gio::BusType::Session,
        gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
        None,
        PORTAL,
        PATH,
        SETTINGS,
        gio::Cancellable::NONE,
        move |proxy| {
            let Ok(proxy) = proxy else { return };
            let read = weak.clone();
            proxy.call(
                "ReadOne",
                Some(&(edel::touch::NAMESPACE, edel::touch::KEY).to_variant()),
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                move |answer| {
                    if let (Ok(answer), Some(window)) = (answer, read.upgrade()) {
                        show(&window, answer.child_value(0).as_variant().as_ref());
                    }
                },
            );
            let changed = weak.clone();
            proxy.connect_local("g-signal", false, move |values| {
                let name = values.get(2).and_then(|v| v.get::<String>().ok());
                let params = values.get(3).and_then(|v| v.get::<glib::Variant>().ok());
                if let (Some("SettingChanged"), Some(params), Some(window)) =
                    (name.as_deref(), params, changed.upgrade())
                {
                    let (namespace, key, value) = (
                        params.child_value(0),
                        params.child_value(1),
                        params.child_value(2),
                    );
                    if namespace.str() == Some(edel::touch::NAMESPACE)
                        && key.str() == Some(edel::touch::KEY)
                    {
                        show(&window, value.as_variant().as_ref());
                    }
                }
                None
            });
            // The proxy lives as long as the window, so its signal does.
            if let Some(window) = weak.upgrade() {
                window.connect_destroy(move |_| {
                    let _ = &proxy;
                });
            }
        },
    );
}

/// Marks `window` for touch when `value` is a true boolean, else unmarks it,
/// saying so on standard error when that changes the rows.
fn show(window: &gtk::ApplicationWindow, value: Option<&glib::Variant>) {
    let on = value.and_then(glib::Variant::get::<bool>).unwrap_or(false);
    if on == window.has_css_class(CLASS) {
        return;
    }
    if on {
        window.add_css_class(CLASS);
    } else {
        window.remove_css_class(CLASS);
    }
    let size = if on { "for touch" } else { "for a pointer" };
    eprintln!("edel-settings: rows sized {size}");
}
