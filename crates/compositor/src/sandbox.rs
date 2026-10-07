//! Only the shell may use the shell's protocols (roadmap M5.22). The
//! compositor offers `wp_security_context_v1`, with which a sandbox such as
//! Flatpak opens a socket of its own for its apps; a client that connects
//! through such a socket carries its security context, and the layer
//! shell, the window list (wlr-foreign-toplevel-management), the
//! workspaces (ext-workspace-v1) and `edel-shell-v1` are hidden from it,
//! as is the security context manager itself, so a sandbox cannot nest
//! one. So a sandboxed app cannot draw over the lock screen, list other
//! windows or close them. Programs that are not sandboxed keep these
//! protocols, as other projects' panels and docks need them (ADR-010).

use std::sync::Arc;

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
    use super::*;

    #[test]
    fn only_a_client_no_sandbox_marked_may_see_the_shells_protocols() {
        assert!(shell_may_with::<()>(None));
        // Any context is a sandbox, named or not.
        assert!(!shell_may_with(Some(&("org.flatpak", "org.example.App"))));
        assert!(!shell_may_with(Some(&())));
    }
}
