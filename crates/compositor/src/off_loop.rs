//! Work too slow for a frame (the whole-system review of 2026-10-25):
//! `Edel::off_loop` runs it on a thread of its own and its answer back on
//! the event loop, so reading, fitting and blurring a picture never holds
//! a frame. The wallpaper and the overview's backdrop use it.

use std::thread;

use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::calloop::channel::{self, Event};

use crate::state::Edel;

/// What a thread's work does on the event loop when it is done.
pub type Answer = Box<dyn FnOnce(&mut Edel) + Send>;

/// Puts the answers' end in the event loop.
pub fn start(handle: &LoopHandle<'static, Edel>, state: &mut Edel) {
    let (sender, answers) = channel::channel::<Answer>();
    let inserted = handle.insert_source(answers, |event, _, state: &mut Edel| {
        if let Event::Msg(answer) = event {
            answer(state);
        }
    });
    match inserted {
        Ok(_) => state.off_loop = Some(sender),
        Err(e) => eprintln!("edel-compositor: slow work runs in the frame: {e}"),
    }
}

impl Edel {
    /// Runs `work` on a thread named `name` and its answer on the event
    /// loop; with no loop to answer on (a test) both run here, now.
    pub fn off_loop<W>(&mut self, name: &str, work: W)
    where
        W: FnOnce() -> Answer + Send + 'static,
    {
        let Some(sender) = self.off_loop.clone() else {
            work()(self);
            return;
        };
        let started = thread::Builder::new().name(name.into()).spawn(move || {
            let _ = sender.send(work());
        });
        if let Err(e) = started {
            eprintln!("edel-compositor: could not start the {name} thread: {e}");
        }
    }
}
