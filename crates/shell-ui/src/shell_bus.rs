//! The panel editor on the session bus (M5.31d): Settings' Panels group
//! asks for the editor as a panel's right-click menu does, with the call
//! `org.edel.Shell1.EditPanels` on `org.edel.Shell`. It is served on the
//! connection of the settings portal (`portal.rs`), on zbus's thread, with
//! no thread of its own; the call reaches the event loop over a channel,
//! which opens the editor there.

use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Poll, Waker};

use smithay_client_toolkit::reexports::calloop::channel;

use crate::access::{self, Ask};

/// Which editor a call over the bus asks for (M5.31d, M5.10c6): the panels'
/// or the lock screen's, or a question the Access portal asks the person
/// (M6.3, the screenshot's dialog).
pub enum Edit {
    Panels,
    LockScreen,
    /// The looks side by side over the lock (M5.10c7h).
    LockLooks,
    /// The Access portal's dialog, with the answer's way back to the call.
    Access(Ask),
}

/// The sending half of one answer, kept by the event loop: `send` gives the
/// answer, and dropping it unanswered gives `access::CANCEL`, so a call
/// waiting for it is never left waiting.
pub struct Answer(Arc<Mutex<Slot>>);

/// The receiving half, which the call awaits on zbus's thread.
pub struct Reply(Arc<Mutex<Slot>>);

/// What the two halves share: the answer once given, whether the answer was
/// dropped unanswered, and the waker of the call waiting for it.
#[derive(Default)]
struct Slot {
    answer: Option<u32>,
    dropped: bool,
    waker: Option<Waker>,
}

/// One question's two halves (M6.3): `Answer` goes to the event loop, `Reply`
/// to the call that asked. No channel crate: a slot and a waker.
pub fn answer() -> (Answer, Reply) {
    let slot = Arc::new(Mutex::new(Slot::default()));
    (Answer(Arc::clone(&slot)), Reply(slot))
}

fn lock(slot: &Mutex<Slot>) -> MutexGuard<'_, Slot> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Answer {
    /// Gives `answer` to the call waiting for it.
    pub fn send(self, answer: u32) {
        let mut slot = lock(&self.0);
        slot.answer = Some(answer);
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

impl Drop for Answer {
    fn drop(&mut self) {
        let mut slot = lock(&self.0);
        slot.dropped = true;
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

impl Reply {
    /// The answer, once given; `access::CANCEL` when the question was dropped
    /// unanswered.
    pub fn wait(self) -> impl Future<Output = u32> {
        futures_lite::future::poll_fn(move |cx| {
            let mut slot = lock(&self.0);
            if let Some(answer) = slot.answer {
                return Poll::Ready(answer);
            }
            if slot.dropped {
                return Poll::Ready(access::CANCEL);
            }
            slot.waker = Some(cx.waker().clone());
            Poll::Pending
        })
    }
}

/// The object served at `SHELL_PATH`, which asks the event loop to open an
/// editor.
pub struct ShellBus {
    edits: channel::Sender<Edit>,
}

impl ShellBus {
    /// The object, sending each call to the event loop through `edits`.
    pub fn new(edits: channel::Sender<Edit>) -> ShellBus {
        ShellBus { edits }
    }
}

#[zbus::interface(name = "org.edel.Shell1")]
impl ShellBus {
    /// Opens the panel editor, as a panel's right-click menu does; it
    /// returns at once and the editor opens on the event loop.
    fn edit_panels(&self) {
        let _ = self.edits.send(Edit::Panels);
    }

    /// Opens the lock screen's edit mode over the desktop, as Settings'
    /// Edit lock screen does (M5.10c6); it returns at once.
    fn edit_lock_screen(&self) {
        let _ = self.edits.send(Edit::LockScreen);
    }

    /// Opens the looks side by side over the lock (M5.10c7h); it returns at
    /// once and the switcher opens on the event loop.
    fn edit_lock_looks(&self) {
        let _ = self.edits.send(Edit::LockLooks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::panel_edit::{EDIT_LOCK_LOOKS, EDIT_LOCK_SCREEN, EDIT_PANELS, SHELL_INTERFACE};
    use zbus::object_server::Interface;

    #[test]
    fn an_answer_reaches_the_call_waiting_for_it_or_cancels_it() {
        let (sent, reply) = answer();
        sent.send(access::DENY);
        assert_eq!(zbus::block_on(reply.wait()), access::DENY);
        let (unsent, reply) = answer();
        drop(unsent);
        assert_eq!(zbus::block_on(reply.wait()), access::CANCEL);
    }

    #[test]
    fn the_interface_and_methods_are_the_names_edel_owns() {
        let (edits, _) = channel::channel::<Edit>();
        let bus = ShellBus::new(edits);
        assert_eq!(ShellBus::name().as_str(), SHELL_INTERFACE);
        let mut xml = String::new();
        bus.introspect_to_writer(&mut xml, 0);
        assert!(
            xml.contains(&format!("<method name=\"{EDIT_PANELS}\"")),
            "{xml}"
        );
        assert!(
            xml.contains(&format!("<method name=\"{EDIT_LOCK_SCREEN}\"")),
            "{xml}"
        );
        assert!(
            xml.contains(&format!("<method name=\"{EDIT_LOCK_LOOKS}\"")),
            "{xml}"
        );
    }
}
