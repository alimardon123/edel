//! The desktop steps aside for a game (M7.7c): while one plays, shell-ui's
//! timers do nothing, and the minute's work is done once when it ends.
//! The compositor says when a game plays on every screen (`playing`);
//! shell-ui also tells the compositor the battery's state, which the
//! compositor used to read itself every five seconds.

use crate::Shell;

impl Shell {
    /// A game plays on every screen, or none does any more (M7.7c). While
    /// one plays the minute's work waits (`tick`, `check_updates`); when
    /// it ends, the work the minute missed is done once.
    pub fn playing_changed(&mut self, on: bool) {
        if self.playing == on {
            return;
        }
        self.playing = on;
        if on {
            eprintln!("edel-shell-ui: a game is playing; the timers stop");
        } else {
            eprintln!("edel-shell-ui: the game ended; the timers go on");
            if std::mem::take(&mut self.missed_minute) {
                self.minute();
            }
        }
    }

    /// Tells the compositor the battery's state when the status reading
    /// has one it has not been sent yet (M7.7c).
    pub fn tell_on_battery(&mut self) {
        let Some(on) = self.live.status.on_battery else {
            return;
        };
        if self.battery_told == Some(on) {
            return;
        }
        self.battery_told = Some(on);
        self.link.on_battery(on);
        // The idle ladder has a sleep only on battery (M7.16b).
        self.follow_idle();
    }
}
