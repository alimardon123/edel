//! The system bus's news for the status area (M5.9a): when NetworkManager
//! (`org.freedesktop.NetworkManager`, its `StateChanged` and
//! `PropertiesChanged`) or UPower (the display device's
//! `PropertiesChanged`) say something changed, the event loop is woken
//! with [`Msg::Changed`] and reads the status afresh, so the icons follow
//! the machine without a timer of their own. It listens, never asks: the
//! system bus's connection is driven by a task on the session bus's zbus
//! thread, the one the portal and the tray already use, with a task for
//! each daemon, so it adds no thread (a thread of its own put shell-ui
//! 0.1 MiB over its budget on #157's first run). A machine without a
//! system bus, or a daemon that is not running, gives no news, and the
//! status is read on the clock's minute and when the card opens instead.

use std::path::Path;

use futures_lite::StreamExt;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::messages;
use crate::status::Msg;

const NETWORK: &str = "org.freedesktop.NetworkManager";
const NETWORK_PATH: &str = "/org/freedesktop/NetworkManager";
const POWER: &str = "org.freedesktop.UPower";
const POWER_PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// Connects to the system bus and listens for the news of each daemon
/// whose feature `features` has; nothing, and nothing said, when there is
/// none of them, and a line saying why when the bus cannot be reached.
/// The connection is driven on `host`'s zbus thread, the session bus's,
/// and lives as long as that thread does; without a host it gets a thread
/// of its own, and lives as long as what this returns.
pub fn serve(
    features: &Path,
    wake: &Sender<Msg>,
    host: Option<&zbus::blocking::Connection>,
) -> Option<zbus::blocking::Connection> {
    let has = |name: &str| features.join(format!("{name}.toml")).is_file();
    let wanted = (has("network"), has("power"));
    if wanted == (false, false) {
        return None;
    }
    if let Some(host) = host {
        let task = drive(zbus::connection::Builder::system(), wanted, wake.clone());
        host.inner().executor().spawn(task, "system bus").detach();
        return None;
    }
    match zbus::blocking::Connection::system() {
        Ok(connection) => {
            follow(&connection, wanted, wake);
            Some(connection)
        }
        Err(e) => {
            eprintln!("edel-shell-ui: {}", messages::status_no_bus(e));
            None
        }
    }
}

/// Connects to the system bus without zbus's own thread, starts the
/// daemons' tasks on it and runs them for as long as the thread it is
/// spawned on runs.
async fn drive(
    builder: zbus::Result<zbus::connection::Builder<'static>>,
    wanted: (bool, bool),
    wake: Sender<Msg>,
) {
    let built = async { builder?.internal_executor(false).build().await };
    let connection = match built.await {
        Ok(connection) => connection,
        Err(e) => {
            eprintln!("edel-shell-ui: {}", messages::status_no_bus(e));
            return;
        }
    };
    spawn_signals(&connection, wanted, &wake);
    let executor = connection.executor().clone();
    loop {
        executor.tick().await;
    }
}

/// Starts a task on `connection` for each daemon asked for, `(network,
/// power)`.
pub fn follow(connection: &zbus::blocking::Connection, wanted: (bool, bool), wake: &Sender<Msg>) {
    spawn_signals(connection.inner(), wanted, wake);
}

/// Starts the tasks of [`follow`] on an async connection's executor.
fn spawn_signals(
    connection: &zbus::Connection,
    (network, power): (bool, bool),
    wake: &Sender<Msg>,
) {
    let executor = connection.executor().clone();
    if network {
        let task = signals(
            connection.clone(),
            NETWORK,
            NETWORK_PATH,
            NETWORK,
            None,
            wake.clone(),
        );
        executor.spawn(task, "network news").detach();
    }
    if power {
        let task = signals(
            connection.clone(),
            POWER,
            POWER_PATH,
            PROPERTIES,
            Some("PropertiesChanged"),
            wake.clone(),
        );
        executor.spawn(task, "power news").detach();
    }
}

/// A proxy to `interface` of `path` on `name`, reading no properties.
async fn proxy(
    connection: &zbus::Connection,
    name: &'static str,
    path: &'static str,
    interface: &'static str,
) -> zbus::Result<zbus::Proxy<'static>> {
    zbus::proxy::Builder::new(connection)
        .destination(name)?
        .path(path)?
        .interface(interface)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

/// Says `Changed` for each signal of `interface` (only `member` if one is
/// named) the object `path` of `name` sends, until the loop is gone.
async fn signals(
    connection: zbus::Connection,
    name: &'static str,
    path: &'static str,
    interface: &'static str,
    member: Option<&'static str>,
    wake: Sender<Msg>,
) {
    let Ok(proxy) = proxy(&connection, name, path, interface).await else {
        return;
    };
    let stream = match member {
        Some(member) => proxy.receive_signal(member).await,
        None => proxy.receive_all_signals().await,
    };
    let Ok(mut stream) = stream else {
        return;
    };
    while stream.next().await.is_some() {
        if wake.send(Msg::Changed).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay_client_toolkit::reexports::calloop;
    use zbus::object_server::SignalEmitter;

    /// A stand-in for NetworkManager: the interface and the signal.
    struct Manager;

    #[zbus::interface(name = "org.freedesktop.NetworkManager")]
    impl Manager {
        #[zbus(signal)]
        async fn state_changed(emitter: &SignalEmitter<'_>, state: u32) -> zbus::Result<()>;
    }

    /// Needs a bus (`dbus-run-session -- cargo test -p edel-shell-ui
    /// watch`), else it has nothing to check and passes: a signal from the
    /// object that stands for NetworkManager wakes the loop, and one from
    /// something else does not.
    /// As the test below, with the listening connection driven on another
    /// connection's thread, as shell-ui drives the system bus's on the
    /// session bus's: it connects, and the signal still wakes the loop.
    #[test]
    fn a_connection_driven_on_another_thread_hears_the_signal() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("no bus: the news is not tried");
            return;
        }
        let (sender, channel) = calloop::channel::channel();
        let daemon = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name(NETWORK)
            .unwrap()
            .serve_at(NETWORK_PATH, Manager)
            .unwrap()
            .build()
            .unwrap();
        let host = zbus::blocking::Connection::session().unwrap();
        let task = drive(zbus::connection::Builder::session(), (true, false), sender);
        host.inner().executor().spawn(task, "driven").detach();
        let mut events = calloop::EventLoop::<u32>::try_new().unwrap();
        events
            .handle()
            .insert_source(channel, |event, _, seen| {
                if let calloop::channel::Event::Msg(Msg::Changed) = event {
                    *seen += 1;
                }
            })
            .unwrap();
        let emitter = daemon
            .object_server()
            .interface::<_, Manager>(NETWORK_PATH)
            .unwrap();
        let mut seen = 0;
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while seen == 0 && std::time::Instant::now() < until {
            zbus::block_on(Manager::state_changed(emitter.signal_emitter(), 70)).unwrap();
            events
                .dispatch(std::time::Duration::from_millis(100), &mut seen)
                .unwrap();
        }
        assert!(seen > 0, "no wake after a StateChanged");
    }

    #[test]
    fn a_signal_from_networkmanager_wakes_the_loop() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("no bus: the news is not tried");
            return;
        }
        let (sender, channel) = calloop::channel::channel();
        let daemon = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name(NETWORK)
            .unwrap()
            .serve_at(NETWORK_PATH, Manager)
            .unwrap()
            .build()
            .unwrap();
        let listener = zbus::blocking::Connection::session().unwrap();
        follow(&listener, (true, false), &sender);
        let mut events = calloop::EventLoop::<u32>::try_new().unwrap();
        events
            .handle()
            .insert_source(channel, |event, _, seen| {
                if let calloop::channel::Event::Msg(Msg::Changed) = event {
                    *seen += 1;
                }
            })
            .unwrap();
        let emitter = daemon
            .object_server()
            .interface::<_, Manager>(NETWORK_PATH)
            .unwrap();
        let mut seen = 0;
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        // The listener's match rule is added by a task; say it until heard.
        while seen == 0 && std::time::Instant::now() < until {
            zbus::block_on(Manager::state_changed(emitter.signal_emitter(), 70)).unwrap();
            events
                .dispatch(std::time::Duration::from_millis(100), &mut seen)
                .unwrap();
        }
        assert!(seen > 0, "no wake after a StateChanged");
    }
}
