//! `/run/edel/session/state.toml` (M4.3): what the compositor shows, the
//! outputs and the windows with their places, for CI's tests now and for
//! shell-ui and Settings later. A thread of its own writes it, so the
//! main loop never waits on a disk (ADR-002); when states arrive faster
//! than it writes, only the newest is written.

use std::path::Path;
use std::sync::mpsc::{Sender, channel};
use std::thread;

use edel_compositor::messages;

/// Where the session keeps the file (M4.1 made the directory).
pub const PATH: &str = edel::places::STATE_FILE;

/// The only state file format so far.
pub const FORMAT: i64 = 1;

pub struct StateFile {
    sender: Option<Sender<String>>,
}

impl StateFile {
    /// Starts the writer, or nothing outside an Edel OS session (no
    /// `/run/edel/session`, as when developing in a window).
    pub fn start() -> StateFile {
        let in_session = Path::new(PATH).parent().is_some_and(Path::is_dir);
        if !in_session {
            return StateFile { sender: None };
        }
        let (sender, receiver) = channel::<String>();
        let started = thread::Builder::new()
            .name("state-file".into())
            .spawn(move || {
                while let Ok(mut text) = receiver.recv() {
                    while let Ok(newer) = receiver.try_recv() {
                        text = newer;
                    }
                    if let Err(e) = edel::files::replace(Path::new(PATH), text.as_bytes()) {
                        eprintln!("edel-compositor: {}", messages::state_not_written(PATH, e));
                    }
                }
            });
        match started {
            Ok(_) => StateFile {
                sender: Some(sender),
            },
            Err(e) => {
                eprintln!("edel-compositor: the state file writer did not start: {e}");
                StateFile { sender: None }
            }
        }
    }

    pub fn send(&self, text: String) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(text);
        }
    }
}
