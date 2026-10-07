//! `edel-testclient --globals` and `--security-context` (roadmap M5.22):
//! print the globals the compositor offers, on one line, as a plain client
//! sees them or as a sandboxed one does. With `--security-context` the
//! client asks for a listening socket of its own through
//! `wp_security_context_v1`, as Flatpak does for its apps, connects through
//! it and lists what that connection sees:
//!
//!     globals wl_compositor wl_shm ... zwlr_layer_shell_v1 ...
//!     sandboxed globals wl_compositor wl_shm ...

use std::os::fd::AsFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use anyhow::{Context, Result};
use smithay_client_toolkit::reexports::client::globals::{GlobalListContents, registry_queue_init};
use smithay_client_toolkit::reexports::client::protocol::wl_registry;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::security_context::v1::client::{
    wp_security_context_manager_v1::WpSecurityContextManagerV1,
    wp_security_context_v1::WpSecurityContextV1,
};

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpSecurityContextManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpSecurityContextManagerV1,
        _: <WpSecurityContextManagerV1 as smithay_client_toolkit::reexports::client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpSecurityContextV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpSecurityContextV1,
        _: <WpSecurityContextV1 as smithay_client_toolkit::reexports::client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// The interfaces `connection` is offered, in the order offered.
fn globals(connection: &Connection) -> Result<Vec<String>> {
    let (globals, _queue) =
        registry_queue_init::<State>(connection).context("reading the globals")?;
    Ok(globals
        .contents()
        .clone_list()
        .into_iter()
        .map(|g| g.interface)
        .collect())
}

pub fn run(sandboxed: bool) -> Result<()> {
    let connection = Connection::connect_to_env().context("connecting to the compositor")?;
    if !sandboxed {
        println!("globals {}", globals(&connection)?.join(" "));
        return Ok(());
    }
    let (list, mut queue) =
        registry_queue_init::<State>(&connection).context("reading the globals")?;
    let manager: WpSecurityContextManagerV1 = list
        .bind(&queue.handle(), 1..=1, ())
        .context("the compositor offers no wp_security_context_manager_v1")?;
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?;
    let path = dir.join(format!("edel-sandbox-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).context("making the sandbox's socket")?;
    // The compositor listens while the other end of this pair is open.
    let (close, _keep) = UnixStream::pair().context("making the close pair")?;
    let context = manager.create_listener(listener.as_fd(), close.as_fd(), &queue.handle(), ());
    context.set_sandbox_engine("org.edel.testclient".into());
    context.set_app_id("edel-testclient".into());
    context.commit();
    queue
        .roundtrip(&mut State)
        .context("creating the security context")?;
    let stream = UnixStream::connect(&path).context("connecting through the sandbox's socket")?;
    let inside = Connection::from_socket(stream).context("speaking Wayland in the sandbox")?;
    let seen = globals(&inside);
    let _ = std::fs::remove_file(&path);
    println!("sandboxed globals {}", seen?.join(" "));
    Ok(())
}
