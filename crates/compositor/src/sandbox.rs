//! Only the shell may use the shell's protocols (roadmap M5.22). The
//! compositor offers `wp_security_context_v1`, with which a sandbox such as
//! Flatpak opens a socket of its own for its apps; a client that connects
//! through such a socket carries its security context, and the layer
//! shell, the window list (wlr-foreign-toplevel-management), the
//! workspaces (ext-workspace-v1), `edel-shell-v1` and the screen copy
//! (wlr-screencopy, M6.3, so a sandboxed app takes a picture through the
//! portal, which asks) are hidden from it, as is the security context
//! manager itself, so a sandbox cannot nest one. So a sandboxed app
//! cannot draw over the lock screen, list other windows, close them or
//! read the screen. Programs that are not sandboxed keep these
//! protocols, as other projects' panels and docks need them (ADR-010).

use std::sync::Arc;
use std::time::Duration;

use smithay::delegate_security_context;
use smithay::reexports::wayland_server::Client;
use smithay::wayland::security_context::{
    SecurityContext, SecurityContextHandler, SecurityContextListenerSource,
};

use crate::state::{ClientState, Edel};

/// Whether a client with `context` may see the shell's protocols: only
/// one that no sandbox marked, whatever the sandbox called itself.
pub fn shell_may_with<C>(context: Option<&C>) -> bool {
    context.is_none()
}

/// Whether `client` may see the shell's protocols. A client inserted
/// without our state (XWayland's own) is the compositor's, and may.
pub fn shell_may(client: &Client) -> bool {
    shell_may_with(
        client
            .get_data::<ClientState>()
            .and_then(|state| state.security_context.as_ref()),
    )
}

/// How long an xdg-activation token may wait before it is used: a click
/// that opens a link is used at once, as anvil also holds.
pub const TOKEN_LIFE: Duration = Duration::from_secs(10);

/// Whether a token asked for by a client, `age` old, may bring a window
/// forward and give it the keyboard (the whole-system review of
/// 2026-10-25, finding 7). A sandboxed app's token must come from the
/// keyboard's own window and from an input newer than the keyboard went
/// there, so no app can take the keys on a timer while the person types
/// elsewhere; the person's own programs, which can do anything anyway, are
/// held only to the token's life, as before.
pub fn may_activate(
    sandboxed: bool,
    age: Duration,
    holds_keyboard: bool,
    input_since: bool,
) -> bool {
    age <= TOKEN_LIFE && (!sandboxed || (holds_keyboard && input_since))
}

impl SecurityContextHandler for Edel {
    /// A sandbox made a socket for its apps: every client that connects
    /// through it carries `context`, until the sandbox closes it.
    fn context_created(&mut self, source: SecurityContextListenerSource, context: SecurityContext) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let engine = context.sandbox_engine.clone().unwrap_or_default();
        let app = context.app_id.clone().unwrap_or_default();
        let made = handle.insert_source(source, move |stream, _, state: &mut Edel| {
            let data = Arc::new(ClientState {
                security_context: Some(context.clone()),
                ..ClientState::default()
            });
            if let Err(e) = state.display.insert_client(stream, data) {
                eprintln!("edel-compositor: a sandboxed client could not connect: {e}");
            }
        });
        match made {
            Ok(_) => eprintln!("edel-compositor: a sandbox socket for {engine} {app}"),
            Err(e) => eprintln!("edel-compositor: no sandbox socket for {engine} {app}: {e}"),
        }
    }
}

delegate_security_context!(Edel);

#[cfg(test)]
mod tests {
    #[test]
    fn a_sandboxed_app_comes_forward_only_from_the_keyboards_window_after_an_input() {
        use super::{TOKEN_LIFE, may_activate};
        use std::time::Duration;
        let now = Duration::from_millis(50);
        // A link clicked in the focused app opens in another.
        assert!(may_activate(true, now, true, true));
        // An app in the background asking on a timer.
        assert!(!may_activate(true, now, false, true));
        // The keyboard's own app, with no input since it got the keyboard.
        assert!(!may_activate(true, now, true, false));
        // The person's own programs, as before.
        assert!(may_activate(false, now, false, false));
        // Any token kept past its life.
        assert!(!may_activate(false, TOKEN_LIFE + now, true, true));
    }

    use super::*;

    #[test]
    fn only_a_client_no_sandbox_marked_may_see_the_shells_protocols() {
        assert!(shell_may_with::<()>(None));
        // Any context is a sandbox, named or not.
        assert!(!shell_may_with(Some(&("org.flatpak", "org.example.App"))));
        assert!(!shell_may_with(Some(&())));
    }
}
