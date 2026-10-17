//! `--clipboard` (roadmap M7.13): the window is also a clipboard user. Key
//! 1 copies `first copied text` and key 2 `second copied text` to the
//! selection, printing `copied TEXT`; Ctrl+V reads the selection and prints
//! `pasted TEXT`, or `pasted nothing` when there is none. The client has no
//! xkb, so the keys are the raw evdev codes wl_keyboard sends. A copy it
//! made itself is pasted from its own text, never through a pipe.

use anyhow::{Context, Result, bail};
use smithay_client_toolkit::reexports::client::event_created_child;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_keyboard, wl_seat,
};
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
};
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::Client;

const TEXT: &str = "text/plain;charset=utf-8";
const PLAIN: &str = "text/plain";
/// The evdev codes of the keys the test uses.
const KEY_1: u32 = 2;
const KEY_2: u32 = 3;
const KEY_V: u32 = 47;
const KEY_LEFTCTRL: u32 = 29;
const KEY_RIGHTCTRL: u32 = 97;
/// Control in wl_keyboard's depressed modifiers mask.
const CONTROL: u32 = 1 << 2;
/// How long a paste waits for the other client to write the text.
const READ_LIMIT: Duration = Duration::from_secs(5);

/// The clipboard state, kept beside the window while `--clipboard` runs.
pub struct State {
    manager: wl_data_device_manager::WlDataDeviceManager,
    device: wl_data_device::WlDataDevice,
    /// The offer of the current selection, from whichever client holds it.
    offer: Option<wl_data_offer::WlDataOffer>,
    /// The copy this client made and still holds, with its text.
    own: Option<(wl_data_source::WlDataSource, String)>,
    depressed_ctrl: bool,
    left_ctrl: bool,
    right_ctrl: bool,
    /// Set by Ctrl+V; the main loop pastes outside the dispatch.
    paste: bool,
}

/// The mime types an offer lists, kept as the offer's user data.
#[derive(Default)]
pub struct Offered(Mutex<Vec<String>>);

/// The text a source offers, kept as the source's user data.
pub struct Copied(String);

/// Binds the seat, its keyboard and a data device for `--clipboard`.
pub fn bind(globals: &GlobalList, qh: &QueueHandle<Client>) -> Result<State> {
    let seat: wl_seat::WlSeat = globals.bind(qh, 1..=7, ()).context("no wl_seat")?;
    let manager: wl_data_device_manager::WlDataDeviceManager = globals
        .bind(qh, 3..=3, ())
        .context("no wl_data_device_manager version 3")?;
    seat.get_keyboard(qh, ());
    let device = manager.get_data_device(&seat, qh, ());
    Ok(State {
        manager,
        device,
        offer: None,
        own: None,
        depressed_ctrl: false,
        left_ctrl: false,
        right_ctrl: false,
        paste: false,
    })
}

/// Pastes when Ctrl+V was pressed since the last call: from the client's own
/// copy when it holds the selection, else from the other client's offer.
pub fn paste(
    connection: &Connection,
    queue: &mut EventQueue<Client>,
    client: &mut Client,
) -> Result<()> {
    let Some(clip) = client.clipboard.as_mut() else {
        return Ok(());
    };
    if !std::mem::take(&mut clip.paste) {
        return Ok(());
    }
    if let Some((_, text)) = &clip.own {
        let text = text.clone();
        say(&format!("pasted {text}"));
        return Ok(());
    }
    let Some(offer) = clip.offer.clone() else {
        say("pasted nothing");
        return Ok(());
    };
    let Some(mime) = offer
        .data::<Offered>()
        .and_then(|offered| offered.0.lock().ok().and_then(|types| wanted(&types)))
    else {
        say("pasted nothing");
        return Ok(());
    };
    let (read, write) = pipe()?;
    offer.receive(mime.to_string(), write.as_fd());
    connection.flush().context("sending the paste request")?;
    // The write end is closed once the request is sent, so the read ends
    // when the other client has written and closed its end.
    drop(write);
    queue
        .roundtrip(client)
        .context("waiting for the compositor")?;
    let bytes = read_to_end(&read)?;
    say(&format!("pasted {}", String::from_utf8_lossy(&bytes)));
    Ok(())
}

/// The first text type an offer lists, UTF-8 first.
fn wanted(types: &[String]) -> Option<&'static str> {
    [TEXT, PLAIN]
        .into_iter()
        .find(|text| types.iter().any(|offered| offered == text))
}

impl State {
    fn ctrl(&self) -> bool {
        self.depressed_ctrl || self.left_ctrl || self.right_ctrl
    }

    /// Makes `text` the selection, as the key event with `serial` allows.
    fn copy(&mut self, text: &str, serial: u32, qh: &QueueHandle<Client>) {
        let source = self
            .manager
            .create_data_source(qh, Copied(text.to_string()));
        source.offer(TEXT.to_string());
        source.offer(PLAIN.to_string());
        self.device.set_selection(Some(&source), serial);
        self.own = Some((source, text.to_string()));
        say(&format!("copied {text}"));
    }
}

/// Prints one line for the test to read, and flushes it.
fn say(line: &str) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn pipe() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: pipe2 writes two descriptors into the two slots it is given.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error()).context("making a pipe");
    }
    // SAFETY: both descriptors are new, and nothing else owns them.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Reads the pipe to its end. It polls with a limit, so a client that never
/// writes ends the paste with an error instead of a wait for ever.
fn read_to_end(read: &OwnedFd) -> Result<Vec<u8>> {
    let deadline = Instant::now() + READ_LIMIT;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let mut poll = libc::pollfd {
            fd: read.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd, waiting at most 100 ms.
        let ready = unsafe { libc::poll(&mut poll, 1, 100) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error).context("waiting for the selection");
        }
        if ready == 0 {
            if Instant::now() > deadline {
                bail!("the selection's owner wrote nothing in {READ_LIMIT:?}");
            }
            continue;
        }
        // SAFETY: reads at most the chunk's length into the chunk.
        let n = unsafe { libc::read(read.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len()) };
        if n < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error).context("reading the selection");
        }
        if n == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..n as usize]);
    }
}

impl Dispatch<wl_data_device_manager::WlDataDeviceManager, ()> for Client {
    fn event(
        _: &mut Self,
        _: &wl_data_device_manager::WlDataDeviceManager,
        _: <wl_data_device_manager::WlDataDeviceManager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for Client {
    fn event(
        state: &mut Self,
        _: &wl_data_device::WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(clip) = state.clipboard.as_mut() else {
            return;
        };
        if let wl_data_device::Event::Selection { id } = event {
            if let Some(old) = clip.offer.take() {
                old.destroy();
            }
            clip.offer = id;
        }
    }

    event_created_child!(Client, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, Offered::default()),
    ]);
}

impl Dispatch<wl_data_offer::WlDataOffer, Offered> for Client {
    fn event(
        _: &mut Self,
        _: &wl_data_offer::WlDataOffer,
        event: wl_data_offer::Event,
        offered: &Offered,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = event {
            if let Ok(mut types) = offered.0.lock() {
                types.push(mime_type);
            }
        }
    }
}

impl Dispatch<wl_data_source::WlDataSource, Copied> for Client {
    fn event(
        state: &mut Self,
        source: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        copied: &Copied,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                if mime_type == TEXT || mime_type == PLAIN {
                    let mut file = std::fs::File::from(fd);
                    let _ = file.write_all(copied.0.as_bytes());
                }
            }
            wl_data_source::Event::Cancelled => {
                if let Some(clip) = state.clipboard.as_mut() {
                    if clip.own.as_ref().is_some_and(|(held, _)| held == source) {
                        clip.own = None;
                    }
                }
                source.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Client {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let Some(clip) = state.clipboard.as_mut() else {
            return;
        };
        match event {
            wl_keyboard::Event::Modifiers { mods_depressed, .. } => {
                clip.depressed_ctrl = mods_depressed & CONTROL != 0;
            }
            wl_keyboard::Event::Key {
                serial,
                key,
                state: key_state,
                ..
            } => {
                let pressed = key_state == WEnum::Value(wl_keyboard::KeyState::Pressed);
                match key {
                    KEY_LEFTCTRL => clip.left_ctrl = pressed,
                    KEY_RIGHTCTRL => clip.right_ctrl = pressed,
                    KEY_1 if pressed => clip.copy("first copied text", serial, qh),
                    KEY_2 if pressed => clip.copy("second copied text", serial, qh),
                    KEY_V if pressed && clip.ctrl() => clip.paste = true,
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
