//! The panel editor on the session bus (M5.31d): Settings' Panels group
//! asks for the editor as a panel's right-click menu does, with the call
//! `org.edel.Shell1.EditPanels` on `org.edel.Shell`. It is served on the
//! connection of the settings portal (`portal.rs`), on zbus's thread, with
//! no thread of its own; the call reaches the event loop over a channel,
//! which opens the editor there.

use smithay_client_toolkit::reexports::calloop::channel;

/// The object served at `SHELL_PATH`, which asks the event loop to open
/// the panel editor.
pub struct ShellBus {
    edits: channel::Sender<()>,
}

impl ShellBus {
    /// The object, sending each call to the event loop through `edits`.
    pub fn new(edits: channel::Sender<()>) -> ShellBus {
        ShellBus { edits }
    }
}

#[zbus::interface(name = "org.edel.Shell1")]
impl ShellBus {
    /// Opens the panel editor, as a panel's right-click menu does; it
    /// returns at once and the editor opens on the event loop.
    fn edit_panels(&self) {
        let _ = self.edits.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::panel_edit::{EDIT_PANELS, SHELL_INTERFACE};
    use zbus::object_server::Interface;

    #[test]
    fn the_interface_and_method_are_the_names_edel_owns() {
        let (edits, _) = channel::channel::<()>();
        let bus = ShellBus::new(edits);
        assert_eq!(ShellBus::name().as_str(), SHELL_INTERFACE);
        let mut xml = String::new();
        bus.introspect_to_writer(&mut xml, 0);
        assert!(
            xml.contains(&format!("<method name=\"{EDIT_PANELS}\"")),
            "{xml}"
        );
    }
}
