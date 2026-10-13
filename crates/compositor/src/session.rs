//! The session's own programs (roadmap M5.7b): what the preset's
//! `[session] start` lists (`edel::presets::Session`), by default the
//! sound system, PipeWire, WirePlumber and the PulseAudio server apps
//! speak to it through. They run for this person only, as the person, so
//! nothing of them runs on a machine nobody is using, and they start once
//! the desktop is on screen, after shell-ui, in the order listed (each
//! needs the one before).
//!
//! Started once, not kept alive: a program that fails within a few
//! seconds of starting is tried again a few times, because WirePlumber
//! started before PipeWire's socket exists fails at once and is right
//! a moment later; one that fails after running a while is left, and the
//! log says so, rather than hiding that sound stopped. They end with the
//! compositor, the kernel sending each SIGTERM when it goes, a crash
//! included, so the next session never meets a second sound system. A
//! change of preset does not start or end them until the next login.
//!
//! The input method (M6.8) is the one exception that follows the settings
//! while the desktop runs: fcitx5 starts when `region.input_method` is
//! fcitx5 and is stopped when it changes away (`follow_input_method`), so
//! it runs only for a person who types in a language their keyboard lacks.

use std::io::ErrorKind;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process, set_parent_process_death_signal};
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::calloop::channel::{self, Event};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};

use edel_compositor::messages;

use crate::state::Edel;

/// A failure this soon after starting is tried again.
const QUICK: Duration = Duration::from_secs(5);

/// Runs of one program, the first included, before it is given up on.
const TRIES: u32 = 4;

/// How long after a quick failure it starts again.
const AGAIN: Duration = Duration::from_millis(500);

/// What the compositor knows of the session's programs.
#[derive(Default)]
pub struct Services {
    started: bool,
    /// The input method the compositor runs, while the settings name one
    /// (M6.8).
    input_method: Option<Input>,
}

/// The input method's program, as `region.input_method` names it (M6.8).
const INPUT_METHOD: &str = "fcitx5";

/// A running input method: its process and whether the compositor asked
/// it to stop, which its end is logged as.
struct Input {
    pid: Pid,
    asked: Arc<AtomicBool>,
}

/// Starts the session's programs the first time the desktop is on
/// screen, unless the session runs one program instead (the greeter).
pub fn start(handle: &LoopHandle<'static, Edel>, state: &mut Edel) {
    if state.services.started || state.program.is_some() {
        return;
    }
    state.services.started = true;
    let env = state.cursors.env();
    for command in state.settings.session_start() {
        spawn(handle, command, 1, env.clone());
    }
    follow_input_method(handle, state);
    start_apps(state);
}

/// Starts the input method when `region.input_method` is fcitx5 and
/// stops it when the setting moves away (M6.8). Only once the session's
/// programs have started, so never under the greeter, and never under the
/// development window, which starts none of them.
pub fn follow_input_method(handle: &LoopHandle<'static, Edel>, state: &mut Edel) {
    if !state.services.started || state.program.is_some() {
        return;
    }
    let wanted = state.settings.input_method.as_deref() == Some(INPUT_METHOD);
    match (wanted, state.services.input_method.is_some()) {
        (true, false) => start_input_method(handle, state),
        (false, true) => stop_input_method(state),
        _ => {}
    }
}

/// The input method is started as a session program is, kept by the
/// compositor as its child and ended with it, but never started again:
/// it runs as long as the setting asks for it.
fn start_input_method(handle: &LoopHandle<'static, Edel>, state: &mut Edel) {
    // fcitx5 types into Wayland apps, so it needs the desktop's socket,
    // which session programs do not get. It gets no X display: connecting
    // to one would start XWayland, which waits for an X11 app (M4.7).
    let mut process = Command::new(INPUT_METHOD);
    process
        .envs(state.cursors.env())
        .env("WAYLAND_DISPLAY", &state.socket)
        .env_remove("DISPLAY")
        .stdin(Stdio::null());
    ends_with_us(&mut process);
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            eprintln!(
                "edel-compositor: {}",
                messages::input_method_missing(INPUT_METHOD)
            );
            return;
        }
        Err(e) => {
            eprintln!(
                "edel-compositor: {}",
                messages::session_program_not_started(INPUT_METHOD, e)
            );
            return;
        }
    };
    let pid = Pid::from_child(&child);
    eprintln!(
        "edel-compositor: started {INPUT_METHOD}, pid {}",
        child.id()
    );
    let asked = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&asked);
    let (sender, receiver) = channel::channel::<Option<ExitStatus>>();
    let inserted = handle.insert_source(receiver, move |event, _, state: &mut Edel| {
        if let Event::Msg(status) = event {
            if state
                .services
                .input_method
                .as_ref()
                .is_some_and(|input| input.pid == pid)
            {
                state.services.input_method = None;
            }
            if stopping.load(Ordering::SeqCst) {
                eprintln!("edel-compositor: input method off, {INPUT_METHOD} ended");
            } else {
                let how = status.map_or_else(|| "was lost".to_string(), |s| format!("ended ({s})"));
                eprintln!(
                    "edel-compositor: {}",
                    messages::input_method_ended(INPUT_METHOD, &how)
                );
            }
        }
    });
    if let Err(e) = inserted {
        eprintln!("edel-compositor: watching {INPUT_METHOD} failed: {e}");
        return;
    }
    let waiter = thread::Builder::new()
        .name(format!("session-{INPUT_METHOD}"))
        .spawn(move || {
            let _ = sender.send(child.wait().ok());
        });
    if let Err(e) = waiter {
        eprintln!("edel-compositor: waiting for {INPUT_METHOD} failed: {e}");
        return;
    }
    state.services.input_method = Some(Input { pid, asked });
}

/// Asks the running input method to stop (SIGTERM); its end is logged when
/// it comes (`start_input_method`). A program that already ended is no
/// reason to say anything more.
fn stop_input_method(state: &mut Edel) {
    let Some(input) = state.services.input_method.take() else {
        return;
    };
    input.asked.store(true, Ordering::SeqCst);
    let _ = kill_process(input.pid, Signal::TERM);
}

/// Starts the apps `startup.apps` names (M6.10), after the session's own
/// programs, each through the same `program::run` the shortcuts use, so it
/// gets the session's socket and display. An id no installed app has is
/// logged and skipped. The list is read when the session starts, so a change
/// takes effect at the next login.
fn start_apps(state: &Edel) {
    let apps = edel::apps::read_all(&edel::apps::dirs());
    for id in &state.settings.startup {
        let Some(app) = startup_app(&apps, id) else {
            eprintln!("edel-compositor: {}", messages::startup_app_missing(id));
            continue;
        };
        if crate::program::run(state, &edel::apps::command(app), &app.id) {
            eprintln!("edel-compositor: started startup app {}", app.id);
        }
    }
}

/// The installed app a `startup.apps` id names (M6.10): the one whose
/// desktop file id is `id`, none when no installed app has it.
fn startup_app<'a>(apps: &'a [edel::apps::App], id: &str) -> Option<&'a edel::apps::App> {
    apps.iter().find(|app| app.id == id)
}

/// The cursor's variables the programs are given (M5.12b5 part A).
type Env = [(&'static str, String); 2];

/// Makes the kernel end `process`'s program with the thread that starts
/// it, the compositor's main thread, where every spawn happens.
fn ends_with_us(process: &mut Command) {
    // SAFETY: the closure only makes the prctl system call, which is safe
    // between fork and exec. The kernel then ends the program with the
    // thread that started it, the compositor's main thread, where every
    // spawn happens.
    unsafe {
        process.pre_exec(|| {
            set_parent_process_death_signal(Some(Signal::TERM))?;
            Ok(())
        });
    }
}

fn spawn(handle: &LoopHandle<'static, Edel>, command: String, run: u32, env: Env) {
    let mut words = command.split_whitespace();
    let Some(name) = words.next().map(str::to_string) else {
        return;
    };
    let mut process = Command::new(&name);
    process.args(words).envs(env.clone()).stdin(Stdio::null());
    ends_with_us(&mut process);
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            eprintln!(
                "edel-compositor: {}",
                messages::session_program_missing(&name)
            );
            return;
        }
        Err(e) => {
            eprintln!(
                "edel-compositor: {}",
                messages::session_program_not_started(&name, e)
            );
            return;
        }
    };
    eprintln!("edel-compositor: started {name}, pid {}", child.id());
    let began = Instant::now();
    let (sender, receiver) = channel::channel::<Option<ExitStatus>>();
    let again = handle.clone();
    let inserted = handle.insert_source(receiver, move |event, _, _: &mut Edel| {
        if let Event::Msg(status) = event {
            ended(&again, command.clone(), run, began, status, env.clone());
        }
    });
    if let Err(e) = inserted {
        eprintln!("edel-compositor: watching {name} failed: {e}");
        return;
    }
    let waiter = thread::Builder::new()
        .name(format!("session-{name}"))
        .spawn(move || {
            let _ = sender.send(child.wait().ok());
        });
    if let Err(e) = waiter {
        eprintln!("edel-compositor: waiting for {name} failed: {e}");
    }
}

/// A program of the session ended: tried again if it failed right after
/// starting, said so if it ran a while and stopped.
fn ended(
    handle: &LoopHandle<'static, Edel>,
    command: String,
    run: u32,
    began: Instant,
    status: Option<ExitStatus>,
    env: Env,
) {
    let name = command
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    let failed = status.is_none_or(|s| !s.success());
    let how = status.map_or_else(|| "was lost".to_string(), |s| format!("ended ({s})"));
    if !failed {
        eprintln!("edel-compositor: {name} {how}");
        return;
    }
    if began.elapsed() >= QUICK {
        eprintln!(
            "edel-compositor: {}",
            messages::session_program_stopped(&name, &how)
        );
        return;
    }
    if run >= TRIES {
        eprintln!(
            "edel-compositor: {}",
            messages::session_program_given_up(&name, &how, TRIES, QUICK.as_secs())
        );
        return;
    }
    eprintln!("edel-compositor: {name} {how}; starting it again");
    let later = handle.clone();
    let timer = handle.insert_source(Timer::from_duration(AGAIN), move |_, _, _: &mut Edel| {
        spawn(&later, command.clone(), run + 1, env.clone());
        TimeoutAction::Drop
    });
    if let Err(e) = timer {
        eprintln!("edel-compositor: starting {name} again failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_startup_id_names_the_installed_app_with_that_desktop_file_id() {
        // read_all gives each app its id from its file name, so the test
        // writes two desktop files in a folder of its own.
        let dir = std::env::temp_dir().join(format!("edel-startup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("org.example.Notes.desktop"),
            "[Desktop Entry]\nType=Application\nName=Notes\nExec=notes\n",
        )
        .unwrap();
        let apps = edel::apps::read_all(std::slice::from_ref(&dir));
        let found = startup_app(&apps, "org.example.Notes").map(|a| a.name.clone());
        assert_eq!(found.as_deref(), Some("Notes"));
        assert!(
            startup_app(&apps, "org.example.Notes.desktop").is_none(),
            "the id has no .desktop"
        );
        assert!(
            startup_app(&apps, "org.example.Gone").is_none(),
            "not installed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
