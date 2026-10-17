//! Auto by the sun (M7.14): under `appearance.mode = "auto"` with a place,
//! the settings carry the sun's next change (`Settings::next_scheme`, from
//! `edel::sun`), and one timer waits for it. The timer runs on the wall
//! clock (`CLOCK_REALTIME`), at an absolute time, and is cancelled when the
//! clock is set, so a machine that slept through sunset or had its clock
//! changed reads the settings again at once; nothing polls. When it fires,
//! `reload_settings` works out the scheme anew, the tokens follow, and
//! shell-ui follows on the `settings` event it sends (M6.17).

use std::os::fd::{AsFd, OwnedFd};

use rustix::io::Errno;
use rustix::time::{
    Itimerspec, TimerfdClockId, TimerfdFlags, TimerfdTimerFlags, Timespec, timerfd_create,
    timerfd_settime,
};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};

use edel_compositor::messages;

use crate::state::Edel;

/// The timer, a copy of the descriptor the event loop reads, and the change
/// it was last set for.
#[derive(Default)]
pub struct Timer {
    fd: Option<OwnedFd>,
    armed: Option<edel::sun::Change>,
}

/// Makes the timer and sets it for the settings read so far.
pub fn start(handle: &LoopHandle<'static, Edel>, state: &mut Edel) {
    let made = timerfd_create(
        TimerfdClockId::Realtime,
        TimerfdFlags::CLOEXEC | TimerfdFlags::NONBLOCK,
    )
    .and_then(|fd| Ok((rustix::io::dup(&fd)?, fd)));
    let (kept, read) = match made {
        Ok(fds) => fds,
        Err(e) => {
            eprintln!("edel-compositor: {}", messages::sun_not_followed(e));
            return;
        }
    };
    let inserted = handle.insert_source(
        Generic::new(read, Interest::READ, Mode::Level),
        |_, fd, state: &mut Edel| {
            let mut buffer = [0u8; 8];
            match rustix::io::read(fd.as_fd(), &mut buffer) {
                // The time came, or the clock was set: read the settings
                // again, which sets the timer for the next change.
                Ok(_) | Err(Errno::CANCELED) => {
                    state.sun.armed = None;
                    state.reload_settings();
                }
                Err(_) => {}
            }
            Ok(PostAction::Continue)
        },
    );
    match inserted {
        Ok(_) => {
            state.sun.fd = Some(kept);
            state.arm_sun();
        }
        Err(e) => eprintln!("edel-compositor: {}", messages::sun_not_followed(e.error)),
    }
}

impl Edel {
    /// Sets the timer for the settings' next change, or stops it when there
    /// is none; logs the change when it is a new one, for CI.
    pub fn arm_sun(&mut self) {
        let next = self.settings.next_scheme;
        let Some(fd) = &self.sun.fd else { return };
        if self.sun.armed == next {
            return;
        }
        let at = Timespec {
            tv_sec: next.map_or(0, |c| c.at),
            tv_nsec: 0,
        };
        let spec = Itimerspec {
            it_interval: Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: at,
        };
        let flags = TimerfdTimerFlags::ABSTIME | TimerfdTimerFlags::CANCEL_ON_SET;
        if let Err(e) = timerfd_settime(fd, flags, &spec) {
            eprintln!("edel-compositor: {}", messages::sun_not_followed(e));
            return;
        }
        self.sun.armed = next;
        if let Some(change) = next {
            eprintln!(
                "edel-compositor: colour scheme follows the sun, {} in {} s",
                change.to.name(),
                change.at - edel::sun::now()
            );
        }
    }
}
