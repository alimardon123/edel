//! Following the settings file (roadmap M4.5): the compositor reads the
//! machine's and the person's when it starts, and again whenever either
//! is written, so `edel settings set layout.tiling=true`
//! (or Settings, from M5) applies at once. inotify watches the two
//! directories, because writers replace the file through a rename; its
//! descriptor is one more source in the event loop, so following costs no
//! thread and nothing while nothing changes.

use std::fs;
use std::io;
use std::path::PathBuf;

use inotify::{EventMask, Inotify, WatchMask};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};

use edel::places;

use edel_compositor::messages;

use crate::state::Edel;

/// Reads the settings now and follows both files from here on.
pub fn start(handle: &LoopHandle<'static, Edel>, state: &mut Edel) {
    state.reload_settings();
    crate::sun::start(handle, state);
    let inotify = match Inotify::init() {
        Ok(inotify) => inotify,
        Err(e) => {
            eprintln!("edel-compositor: {}", messages::settings_not_followed(e));
            return;
        }
    };
    let mask = WatchMask::CLOSE_WRITE
        | WatchMask::MOVED_TO
        | WatchMask::MOVED_FROM
        | WatchMask::CREATE
        | WatchMask::DELETE;
    let machine = Some(PathBuf::from(places::DATA_DIR));
    // The person's directory is made if it is missing, so a file written
    // there later is seen too.
    let person = places::person_dir();
    if let Some(dir) = &person {
        let _ = fs::create_dir_all(dir);
    }
    let mut watched = 0;
    let mut person_watch = None;
    for (dir, is_person) in [(machine, false), (person.clone(), true)] {
        let Some(dir) = dir else { continue };
        match inotify.watches().add(&dir, mask) {
            Ok(watch) => {
                watched += 1;
                if is_person {
                    person_watch = Some(watch);
                }
            }
            Err(e) => eprintln!(
                "edel-compositor: {}",
                messages::folder_not_followed(&dir, e)
            ),
        }
    }
    if watched == 0 {
        return;
    }
    let inserted = handle.insert_source(
        Generic::new(inotify, Interest::READ, Mode::Level),
        move |_, inotify, state: &mut Edel| {
            let mut buffer = [0u8; 4096];
            let mut changed = false;
            let mut lost = false;
            loop {
                // SAFETY: the descriptor is only read here, never closed.
                match unsafe { inotify.get_mut() }.read_events(&mut buffer) {
                    Ok(events) => {
                        let mut any = false;
                        for event in events {
                            any = true;
                            // A watched folder removed (a reset, or by
                            // hand) ends its watch.
                            lost |= event.mask.contains(EventMask::IGNORED)
                                && person_watch.as_ref() == Some(&event.wd);
                            changed |= event
                                .name
                                .is_some_and(|n| places::is_settings_name(&n.to_string_lossy()));
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
            // The person's folder is made and watched again, so changes
            // keep applying at once (the whole-system review of 2026-10-25).
            if let Some(dir) = person.as_ref().filter(|_| lost) {
                let _ = fs::create_dir_all(dir);
                // SAFETY: as above, the descriptor is not closed.
                match unsafe { inotify.get_mut() }.watches().add(dir, mask) {
                    Ok(watch) => {
                        person_watch = Some(watch);
                        eprintln!(
                            "edel-compositor: {} was removed; following it again",
                            dir.display()
                        );
                    }
                    Err(e) => {
                        eprintln!("edel-compositor: {}", messages::folder_not_followed(dir, e))
                    }
                }
                changed = true;
            }
            if changed {
                state.reload_settings();
            }
            Ok(PostAction::Continue)
        },
    );
    if let Err(e) = inserted {
        eprintln!("edel-compositor: {}", messages::settings_not_followed(e));
    }
}
