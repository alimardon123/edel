//! What the compositor says when something fails, in the words
//! `docs/MESSAGES.md` asks for (M5.28b): what failed, why and what to do
//! or what still works. Each function gives the text after the
//! `edel-compositor: ` prefix, lower case and with no final full stop, so
//! the tests below hold the text a person reads. Routine status lines
//! that CI looks for ("windows now tiling", "output ... ready") stay
//! where they are written.

use std::fmt::Display;
use std::path::Path;

/// The context of the fatal error when no graphics card is found.
pub const NO_GPU: &str = "no graphics card found, so there is no screen to draw on; the desktop needs a GPU with a driver (a /dev/dri/card file), and a virtual machine needs a virtual GPU";

/// The context when the card list cannot be read.
pub const GPU_LIST: &str = "could not list the graphics cards; is udev running?";

/// The context when the card's file cannot be opened.
pub fn gpu_open(path: &Path) -> String {
    format!(
        "could not open the graphics card {}; this user needs to be in group video or seat",
        path.display()
    )
}

/// The context when the card udev names never shows its file.
/// `seen` says what this user found: why the card's file could not be
/// read, what /dev/dri held, what the kernel lists in /sys/class/drm and
/// what is mounted on /dev, so a log tells a missing file from one this
/// user may not see, and a file taken out from a card never made.
pub fn gpu_missing(path: &Path, seen: &str) -> String {
    format!(
        "the graphics card {} that udev names did not appear under /dev/dri within 10 s, and no other card did ({seen}); the desktop starts again in a moment and tries once more",
        path.display()
    )
}

/// The context when EGL, the way to draw with the card, does not start.
pub const NO_EGL: &str =
    "could not start EGL on the graphics card; its Mesa driver is missing or too old";

/// The compositor stops with an error.
pub fn stopped(why: impl Display) -> String {
    format!("the desktop stopped: {why}; the session ends")
}

/// The program started with `--` cannot start; the session is its.
pub fn program_not_started(name: &str, why: impl Display) -> String {
    format!("could not start {name}: {why}; the session ends")
}

/// An app a shortcut starts cannot start.
pub fn app_not_started(name: &str, why: impl Display) -> String {
    format!("could not start {name}: {why}; check that it is installed")
}

/// The overview's plus could not add a workspace (M5.2j-b).
pub fn workspace_not_added(why: impl Display) -> String {
    format!(
        "could not add a workspace: {why}; set the count on Settings' Workspaces page or with edel settings set workspaces.count"
    )
}

/// The workspace names cannot be written down in their new order (M5.2p).
pub fn names_not_kept(why: impl Display) -> String {
    format!(
        "could not keep the workspace names in their new order: {why}; the names in the settings file stay as they were"
    )
}

/// There is no home folder to keep the workspace names in (M5.2p).
pub const NAMES_NO_HOME: &str = "no home folder to keep the workspace names in; run edel settings set workspaces.names='[\"Mail\", \"\", \"Code\"]' instead, one name per workspace in order, empty where none";

/// shell-ui cannot start.
pub fn shell_ui_not_started(name: &str, why: impl Display) -> String {
    format!("could not start {name}: {why}")
}

/// shell-ui keeps failing and is left alone.
pub fn shell_ui_given_up(name: &str, how: &str, tries: u32, secs: u64) -> String {
    format!(
        "{name} {how}, and failed {tries} times in a row within {secs} s of starting; it is not started again, so there is no panel until the next login, and the lines above say why"
    )
}

/// shell-ui cannot be ended to start again with new settings.
pub fn shell_ui_not_ended(name: &str, why: impl Display) -> String {
    format!(
        "could not end {name} to start it again: {why}; it keeps its old settings until the next login"
    )
}

/// A program of the session's list (`[session] start`) is not in this image.
pub fn session_program_missing(name: &str) -> String {
    format!(
        "{name} is not in this image, so the session does not start it; the programs of the sound feature are, and a preset's [session] start names the others"
    )
}

/// A program of the session's list cannot start.
pub fn session_program_not_started(name: &str, why: impl Display) -> String {
    format!("could not start {name}: {why}; the rest of the desktop works without it")
}

/// A program of the session's list ran a while and stopped.
pub fn session_program_stopped(name: &str, how: &str) -> String {
    format!(
        "{name} {how} after running a while, and is not started again; what it served (sound, for PipeWire) stops until the next login"
    )
}

/// A program of the session's list keeps failing and is left alone.
pub fn session_program_given_up(name: &str, how: &str, tries: u32, secs: u64) -> String {
    format!(
        "{name} {how}, and failed {tries} times in a row within {secs} s of starting; it is not started again, so what it served (sound, for PipeWire) is missing until the next login, and the lines above say why"
    )
}

/// A screen cannot be lit.
pub fn screen_dark(name: &str, why: impl Display) -> String {
    format!("output {name} stays dark: {why}; the other screens work")
}

/// The list of screens cannot be read.
pub const SCREEN_LIST: &str =
    "could not read the screens from the graphics card; the screens in use stay as they are";

/// Input devices do not come back after a terminal switch.
pub const INPUT_LOST: &str =
    "the input devices did not come back; unplug and plug in the keyboard, or log in again";

/// Screens do not come back after a terminal switch.
pub fn screens_lost(why: impl Display) -> String {
    format!(
        "the screens did not come back: {why}; switch to another terminal and back, or log in again"
    )
}

/// Switching to another terminal fails.
pub fn terminal_switch(vt: impl Display, why: impl Display) -> String {
    format!("could not switch to terminal {vt}: {why}")
}

/// A frame cannot be drawn.
pub fn frame_not_drawn(why: impl Display) -> String {
    format!("could not draw a frame: {why}; the screen keeps the one before")
}

/// A frame cannot be sent to the screen.
pub fn frame_not_shown(why: impl Display) -> String {
    format!("could not show a frame: {why}; the screen keeps the one before")
}

/// The health file cannot be written.
pub fn ready_not_written(path: &str, why: impl Display) -> String {
    format!(
        "could not write {path}: {why}; the machine cannot tell that the desktop started, and an update could be rolled back"
    )
}

/// The state file cannot be written.
pub fn state_not_written(path: &str, why: impl Display) -> String {
    format!("could not write {path}: {why}; tools that read the desktop's state see an old one")
}

/// Changes to the settings files cannot be watched at all.
pub fn settings_not_followed(why: impl Display) -> String {
    format!("changes to the settings are not followed: {why}; log in again to apply a change")
}

/// One folder of settings cannot be watched.
pub fn folder_not_followed(dir: &Path, why: impl Display) -> String {
    format!(
        "changes in {} are not followed: {why}; log in again to apply a change",
        dir.display()
    )
}

/// `region.keyboard` names layouts xkb cannot load.
pub fn keyboard_not_loaded(written: &str, why: impl Display) -> String {
    format!("region.keyboard \"{written}\" could not be loaded: {why}; keeping the layout in use")
}

/// A shortcut note: two modifiers cannot be tapped alone (no prefix, as
/// the notes of `shortcuts::bind` are printed with one).
pub fn modifier_alone(action: &str) -> String {
    format!(
        "shortcuts.{action}: only one modifier can be tapped alone; add a key, as in Super+Space, or use one modifier"
    )
}

/// A shortcut note: no key has this name.
pub fn no_such_key(action: &str, name: &str) -> String {
    format!("shortcuts.{action}: no key is called \"{name}\"; the shortcut is left out")
}

/// The X11 server cannot start.
pub fn x11_not_started(program: &str, why: impl Display) -> String {
    format!("could not start {program}: {why}; X11 apps cannot open until the next login")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_to_start_says_what_to_do() {
        assert_eq!(
            stopped("could not open the seat: no socket"),
            "the desktop stopped: could not open the seat: no socket; the session ends"
        );
        assert_eq!(
            program_not_started("agreety", "No such file or directory (os error 2)"),
            "could not start agreety: No such file or directory (os error 2); the session ends"
        );
        assert_eq!(
            app_not_started("foot", "No such file or directory (os error 2)"),
            "could not start foot: No such file or directory (os error 2); check that it is installed"
        );
    }

    #[test]
    fn the_panel_giving_up_says_what_is_missing() {
        assert_eq!(
            shell_ui_given_up("edel-shell-ui", "ended (exit status: 1)", 5, 10),
            "edel-shell-ui ended (exit status: 1), and failed 5 times in a row within 10 s of starting; it is not started again, so there is no panel until the next login, and the lines above say why"
        );
        assert_eq!(
            shell_ui_not_ended("edel-shell-ui", "no such process"),
            "could not end edel-shell-ui to start it again: no such process; it keeps its old settings until the next login"
        );
    }

    #[test]
    fn a_session_program_that_fails_says_what_stops() {
        assert_eq!(
            session_program_given_up("wireplumber", "ended (exit status: 1)", 4, 5),
            "wireplumber ended (exit status: 1), and failed 4 times in a row within 5 s of starting; it is not started again, so what it served (sound, for PipeWire) is missing until the next login, and the lines above say why"
        );
        assert!(session_program_missing("pipewire").starts_with("pipewire is not in this image"));
    }

    #[test]
    fn a_missing_graphics_card_says_what_the_desktop_needs() {
        assert!(NO_GPU.starts_with("no graphics card found, so there is no screen to draw on;"));
        assert!(NO_GPU.contains("a virtual machine needs a virtual GPU"));
        assert_eq!(
            gpu_open(Path::new("/dev/dri/card0")),
            "could not open the graphics card /dev/dri/card0; this user needs to be in group video or seat"
        );
        assert!(
            gpu_missing(Path::new("/dev/dri/card0"), "/dev/dri/card0: not found").contains(
                "no other card did (/dev/dri/card0: not found); the desktop starts again"
            )
        );
        assert_eq!(
            screen_dark("HDMI-A-1", "no free CRTC drives it"),
            "output HDMI-A-1 stays dark: no free CRTC drives it; the other screens work"
        );
    }

    #[test]
    fn a_file_that_cannot_be_written_says_what_follows() {
        assert_eq!(
            ready_not_written("/run/edel/session/ready", "Permission denied (os error 13)"),
            "could not write /run/edel/session/ready: Permission denied (os error 13); the machine cannot tell that the desktop started, and an update could be rolled back"
        );
        assert_eq!(
            settings_not_followed("Too many open files (os error 24)"),
            "changes to the settings are not followed: Too many open files (os error 24); log in again to apply a change"
        );
    }

    #[test]
    fn a_refused_setting_names_the_key_and_what_happens() {
        assert_eq!(
            keyboard_not_loaded("xx", "BadLayout"),
            "region.keyboard \"xx\" could not be loaded: BadLayout; keeping the layout in use"
        );
        assert_eq!(
            modifier_alone("open_launcher"),
            "shortcuts.open_launcher: only one modifier can be tapped alone; add a key, as in Super+Space, or use one modifier"
        );
        assert_eq!(
            no_such_key("close_window", "Foo"),
            "shortcuts.close_window: no key is called \"Foo\"; the shortcut is left out"
        );
    }

    #[test]
    fn no_message_has_a_dash_or_a_final_full_stop() {
        let all = [
            NO_GPU.to_string(),
            GPU_LIST.to_string(),
            NO_EGL.to_string(),
            SCREEN_LIST.to_string(),
            INPUT_LOST.to_string(),
            screens_lost("x"),
            terminal_switch(2, "x"),
            frame_not_drawn("x"),
            frame_not_shown("x"),
            state_not_written("/run/x", "x"),
            folder_not_followed(Path::new("/data"), "x"),
            x11_not_started("xwayland-satellite", "x"),
            shell_ui_not_started("edel-shell-ui", "x"),
            names_not_kept("x"),
            NAMES_NO_HOME.to_string(),
            session_program_missing("pipewire"),
            session_program_not_started("pipewire", "x"),
            session_program_stopped("pipewire", "ended (exit status: 1)"),
            session_program_given_up("pipewire", "ended (exit status: 1)", 4, 5),
        ];
        for text in all {
            assert!(!text.ends_with('.'), "{text}");
            assert!(
                !text.contains('\u{2013}') && !text.contains('\u{2014}'),
                "{text}"
            );
        }
    }
}
