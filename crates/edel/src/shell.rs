//! The desktop session in `edel status` (roadmap M5.11, M5.25a): what the
//! session is doing, read from the state file the compositor writes, so the
//! command line sees what Settings will show. It also waits while a game
//! plays (M7.7c): an update or an add-on fetch takes the machine's disk and
//! network from the game, so it waits until the compositor says the game
//! ended.

use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use edel::i18n::tr;
use inotify::{Inotify, WatchMask};

/// Where the compositor keeps what it shows (M4.3).
pub const STATE: &str = edel::places::STATE_FILE;

/// The session's effect tier, `lite`, `balanced` or `full`, while a
/// desktop session runs.
pub fn tier_now() -> Option<String> {
    tier_of(&fs::read_to_string(STATE).ok()?)
}

/// The `tier` key of a state file.
fn tier_of(text: &str) -> Option<String> {
    let table: toml::Table = text.parse().ok()?;
    table.get("tier")?.as_str().map(String::from)
}

/// Whether a state file says a game plays and its compositor runs (M7.7c):
/// `playing` is true and `pid` names a live process. A file without a pid,
/// or with one that is gone (a compositor that died), is not playing.
fn playing_of(text: &str, alive: impl Fn(u32) -> bool) -> bool {
    let Ok(table) = text.parse::<toml::Table>() else {
        return false;
    };
    let playing = table.get("playing").and_then(toml::Value::as_bool) == Some(true);
    let pid = table
        .get("pid")
        .and_then(toml::Value::as_integer)
        .and_then(|pid| u32::try_from(pid).ok());
    playing && pid.is_some_and(alive)
}

/// Whether a game plays now (M7.7c), as the state file says.
pub fn playing_now() -> bool {
    fs::read_to_string(STATE).is_ok_and(|text| playing_of(&text, crate::update::alive))
}

/// Waits while a game plays (M7.7c), then returns. `who` names the command
/// in the lines it prints. Wakes the moment the compositor writes the state
/// file: inotify watches its folder, and a thread reads the events, so the
/// 60 s timeout only catches a compositor that died without writing the
/// file. Without inotify, the file is read again every 30 s.
pub fn wait_for_game(who: &str) {
    if !playing_now() {
        return;
    }
    println!(
        "{who}: {}",
        tr("a game is playing, so this waits until it ends")
    );
    let events = watch_state();
    while playing_now() {
        match &events {
            Some(events) => {
                if let Err(RecvTimeoutError::Disconnected) =
                    events.recv_timeout(Duration::from_secs(60))
                {
                    thread::sleep(Duration::from_secs(30));
                }
            }
            None => thread::sleep(Duration::from_secs(30)),
        }
    }
    println!("{who}: {}", tr("the game ended; going on"));
}

/// A channel with one `()` for each batch of events in the state file's
/// folder, from a thread that reads them blocking. `None` when inotify
/// cannot watch the folder.
fn watch_state() -> Option<mpsc::Receiver<()>> {
    let dir = Path::new(STATE).parent()?;
    let mut inotify = Inotify::init().ok()?;
    inotify
        .watches()
        .add(
            dir,
            WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO | WatchMask::DELETE,
        )
        .ok()?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0; 4096];
        while inotify.read_events_blocking(&mut buffer).is_ok() {
            if sender.send(()).is_err() {
                break;
            }
        }
    });
    Some(receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_plays_only_while_its_compositor_lives() {
        let live = |pid: u32| pid == 42;
        let playing = "format = 1\nplaying = true\npid = 42\n";
        assert!(playing_of(playing, live));
        assert!(!playing_of("format = 1\nplaying = true\npid = 7\n", live));
        assert!(!playing_of("format = 1\nplaying = false\npid = 42\n", live));
        assert!(!playing_of("format = 1\nplaying = true\n", live));
        assert!(!playing_of("playing = [", live));
    }

    #[test]
    fn the_tier_comes_from_the_state_file() {
        let state =
            "format = 1\npolicy = \"floating\"\ntier = \"lite\"\n\n[[windows]]\ntitle = \"foot\"\n";
        assert_eq!(tier_of(state).as_deref(), Some("lite"));
        assert_eq!(tier_of("format = 1\n"), None);
        assert_eq!(tier_of("not toml ["), None);
    }
}
