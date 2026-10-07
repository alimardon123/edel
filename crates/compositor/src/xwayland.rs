//! X11 apps (roadmap M4.7): the compositor holds an X11 display's sockets
//! and starts xwayland-satellite, which runs XWayland and turns its windows
//! into ordinary Wayland windows, when the first X11 app connects. Until
//! then nothing runs and no memory goes to X11; an X11 failure cannot take
//! the desktop down; and X11 windows get the same title bars and policies
//! as the rest, with no X11 code here. If it stops, the next X11 app starts
//! it again. Without `xwayland-satellite` in the image (feature
//! `xwayland`) there are no X11 apps, and nothing else changes.

use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write as _};
use std::os::fd::{AsRawFd as _, OwnedFd};
use std::os::linux::net::SocketAddrExt as _;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::net::{SocketAddr, UnixListener};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;

use smithay::reexports::calloop::channel::{self, Event, Sender};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction, RegistrationToken};

use edel_compositor::messages;

use crate::state::Edel;

const PROGRAM: &str = "xwayland-satellite";
/// Where X11 apps look for a display's socket; the same name is used in
/// the abstract namespace, which they try first.
const SOCKETS: &str = "/tmp/.X11-unix";

/// The X11 display the compositor holds for X11 apps.
pub struct X11Display {
    pub number: u32,
    /// The display's sockets, in the abstract namespace and in `SOCKETS`.
    listeners: Vec<UnixListener>,
    /// Their event sources: on while nothing serves the display.
    tokens: Vec<RegistrationToken>,
    handle: LoopHandle<'static, Edel>,
    /// Our Wayland socket, for satellite.
    wayland: String,
    stopped: Sender<io::Result<ExitStatus>>,
    running: bool,
}

/// Takes the first free X11 display and waits on its sockets, if the
/// image can run X11 apps.
pub fn listen(handle: &LoopHandle<'static, Edel>, state: &mut Edel, wayland: &str) {
    if !installed() {
        eprintln!("edel-compositor: no {PROGRAM} in this image, so no X11 apps");
        return;
    }
    let (number, listeners) = match claim() {
        Ok(claimed) => claimed,
        Err(e) => {
            eprintln!("edel-compositor: no X11 display for X11 apps: {e}");
            return;
        }
    };
    let (stopped, exits) = channel::channel();
    let watched = handle.insert_source(exits, |event, _, state: &mut Edel| {
        if let Event::Msg(status) = event {
            satellite_stopped(state, status);
        }
    });
    if let Err(e) = watched {
        eprintln!("edel-compositor: watching {PROGRAM} failed: {e}");
        release(number);
        return;
    }
    let mut tokens = Vec::new();
    for (index, listener) in listeners.iter().enumerate() {
        let token = listener
            .try_clone()
            .map_err(|e| e.to_string())
            .and_then(|fd| {
                let source = Generic::new(fd, Interest::READ, Mode::Level);
                handle
                    .insert_source(source, move |_, _, state: &mut Edel| {
                        start_satellite(state, index);
                        Ok(PostAction::Disable)
                    })
                    .map_err(|e| e.to_string())
            });
        match token {
            Ok(token) => tokens.push(token),
            Err(e) => {
                eprintln!("edel-compositor: watching the X11 display failed: {e}");
                for token in tokens {
                    handle.remove(token);
                }
                release(number);
                return;
            }
        }
    }
    eprintln!("edel-compositor: X11 apps on :{number}, served once one connects");
    state.x11 = Some(X11Display {
        number,
        listeners,
        tokens,
        handle: handle.clone(),
        wayland: wayland.to_string(),
        stopped,
        running: false,
    });
}

/// An X11 app is connecting: satellite starts and serves the display, and
/// the sockets' sources rest until it stops. `firing` is the source that
/// woke, which disables itself.
fn start_satellite(state: &mut Edel, firing: usize) {
    let Some(x11) = state.x11.as_mut() else {
        return;
    };
    if x11.running {
        return;
    }
    for (index, token) in x11.tokens.iter().enumerate() {
        if index != firing {
            x11.handle.disable(token).ok();
        }
    }
    match x11.spawn() {
        Ok(()) => {
            x11.running = true;
            eprintln!("edel-compositor: {PROGRAM} started for :{}", x11.number);
        }
        Err(e) => {
            eprintln!("edel-compositor: {}", messages::x11_not_started(PROGRAM, e));
            x11.refuse_waiting();
            x11.wait_again();
        }
    }
}

fn satellite_stopped(state: &mut Edel, status: io::Result<ExitStatus>) {
    let Some(x11) = state.x11.as_mut() else {
        return;
    };
    match status {
        Ok(status) => eprintln!("edel-compositor: {PROGRAM} stopped: {status}"),
        Err(e) => eprintln!("edel-compositor: {PROGRAM} stopped: {e}"),
    }
    x11.running = false;
    x11.refuse_waiting();
    x11.wait_again();
}

impl X11Display {
    /// Starts satellite with copies of the sockets it inherits (ours close
    /// on exec), and a thread that says when it stops.
    fn spawn(&self) -> io::Result<()> {
        let inherited: Vec<OwnedFd> = self
            .listeners
            .iter()
            .map(|l| rustix::io::dup(l).map_err(io::Error::from))
            .collect::<io::Result<_>>()?;
        let mut command = Command::new(PROGRAM);
        command.arg(format!(":{}", self.number));
        for fd in &inherited {
            command.arg("-listenfd").arg(fd.as_raw_fd().to_string());
        }
        // XWayland's own messages (keymap warnings) would bury the log.
        let mut child = command
            .env("WAYLAND_DISPLAY", &self.wayland)
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        drop(inherited);
        let stopped = self.stopped.clone();
        thread::Builder::new()
            .name("x11-watch".into())
            .spawn(move || {
                let _ = stopped.send(child.wait());
            })?;
        Ok(())
    }

    /// Apps that connected while nothing could serve them are turned away,
    /// so they fail at once and the sockets wake only for new ones.
    fn refuse_waiting(&self) {
        for listener in &self.listeners {
            if listener.set_nonblocking(true).is_err() {
                continue;
            }
            while listener.accept().is_ok() {}
        }
    }

    fn wait_again(&self) {
        for token in &self.tokens {
            if let Err(e) = self.handle.enable(token) {
                eprintln!("edel-compositor: watching the X11 display again failed: {e}");
            }
        }
    }
}

impl Drop for X11Display {
    fn drop(&mut self) {
        release(self.number);
    }
}

/// Whether satellite is on the `PATH`.
fn installed() -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            fs::metadata(dir.join(PROGRAM))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

fn lock_path(number: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/.X{number}-lock"))
}

fn socket_path(number: u32) -> PathBuf {
    Path::new(SOCKETS).join(format!("X{number}"))
}

/// The first display from 0 whose lock file is ours to take, with its
/// sockets bound. A lock left by a process that is gone is taken over.
fn claim() -> io::Result<(u32, Vec<UnixListener>)> {
    if !Path::new(SOCKETS).is_dir() {
        fs::create_dir(SOCKETS)?;
        fs::set_permissions(SOCKETS, fs::Permissions::from_mode(0o1777))?;
    }
    for number in 0..32 {
        if !lock(number)? {
            continue;
        }
        match bind(number) {
            Ok(listeners) => return Ok((number, listeners)),
            Err(_) => release(number),
        }
    }
    Err(io::Error::new(
        ErrorKind::AddrInUse,
        "displays 0 to 31 are all taken",
    ))
}

/// Takes display `number`'s lock file, as X servers do: our process id in
/// ten columns. Returns false if a live process holds it.
fn lock(number: u32) -> io::Result<bool> {
    let path = lock_path(number);
    for _ in 0..2 {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o444)
            .open(&path)
        {
            Ok(mut file) => {
                writeln!(file, "{:>10}", std::process::id())?;
                return Ok(true);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let holder = fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| text.trim().parse::<u32>().ok());
                if holder.is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists()) {
                    return Ok(false);
                }
                // Left by a process that is gone; another user's stays.
                if fs::remove_file(&path).is_err() {
                    return Ok(false);
                }
                let _ = fs::remove_file(socket_path(number));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

fn bind(number: u32) -> io::Result<Vec<UnixListener>> {
    let name = socket_path(number);
    let in_abstract = UnixListener::bind_addr(&SocketAddr::from_abstract_name(
        name.as_os_str().as_encoded_bytes(),
    )?)?;
    // We hold the lock, so a file under the name is a stale one.
    let _ = fs::remove_file(&name);
    let in_path = UnixListener::bind(&name)?;
    Ok(vec![in_abstract, in_path])
}

fn release(number: u32) {
    let _ = fs::remove_file(socket_path(number));
    let _ = fs::remove_file(lock_path(number));
}
