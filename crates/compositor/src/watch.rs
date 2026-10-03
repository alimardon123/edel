//! Following the system file (roadmap M4.5): the compositor reads the
//! machine's and the person's `system.toml` when it starts, and again
//! whenever either is written, so `edel system set shell.tiling=true`
//! (or Settings, from M5) applies at once. inotify watches the two
//! directories, because writers replace the file through a rename; its
//! descriptor is one more source in the event loop, so following costs no
//! thread and nothing while nothing changes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use inotify::{Inotify, WatchMask};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};

use edel::system::MACHINE_FILE;
use edel_compositor::settings;

use crate::state::Edel;

/// The name both files have.
const NAME: &str = "system.toml";

/// Reads the settings now and follows both files from here on.
pub fn start(handle: &LoopHandle<'static, Edel>, state: &mut Edel) {
    state.reload_settings();
    let inotify = match Inotify::init() {
        Ok(inotify) => inotify,
        Err(e) => {
            eprintln!("edel-compositor: changes to {NAME} are not followed: {e}");
            return;
        }
    };
    let mask = WatchMask::CLOSE_WRITE
        | WatchMask::MOVED_TO
        | WatchMask::MOVED_FROM
        | WatchMask::CREATE
        | WatchMask::DELETE;
    let machine = Path::new(MACHINE_FILE).parent().map(Path::to_path_buf);
    // The person's directory is made if it is missing, so a file written
    // there later is seen too.
    let person = settings::person_file().and_then(|f| f.parent().map(PathBuf::from));
    if let Some(dir) = &person {
        let _ = fs::create_dir_all(dir);
    }
    let mut watched = 0;
    for dir in [machine, person].into_iter().flatten() {
        match inotify.watches().add(&dir, mask) {
            Ok(_) => watched += 1,
            Err(e) => eprintln!("edel-compositor: not following {}: {e}", dir.display()),
        }
    }
    if watched == 0 {
        return;
    }
    let inserted = handle.insert_source(
        Generic::new(inotify, Interest::READ, Mode::Level),
        |_, inotify, state: &mut Edel| {
            let mut buffer = [0u8; 4096];
            let mut changed = false;
            loop {
                // SAFETY: the descriptor is only read here, never closed.
                match unsafe { inotify.get_mut() }.read_events(&mut buffer) {
                    Ok(events) => {
                        let mut any = false;
                        for event in events {
                            any = true;
                            changed |= event.name.is_some_and(|n| n == NAME);
                        }
                        if !any {
                            break;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        eprintln!("edel-compositor: reading file changes failed: {e}");
                        break;
                    }
                }
            }
            if changed {
                state.reload_settings();
            }
            Ok(PostAction::Continue)
        },
    );
    if let Err(e) = inserted {
        eprintln!("edel-compositor: changes to {NAME} are not followed: {e}");
    }
}
