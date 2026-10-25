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
pub const NO_GPU: &str = "the desktop could not start: no graphics card answered; on a virtual machine, give it a virtual graphics card; on a computer, run edel report and send the file it writes";

/// The context when the card list cannot be read.
pub const GPU_LIST: &str = "the desktop could not start: the list of graphics cards could not be read; restart the computer, and if this comes back run edel report and send the file it writes";

/// The context when the card's file cannot be opened.
pub fn gpu_open(path: &Path) -> String {
    format!(
        "could not open the graphics card {}: this account may not use the screen; log out and in again, and if it repeats run doas edel settings apply, which gives the people at the computer the screen",
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
pub const NO_EGL: &str = "the desktop could not draw on this graphics card: its driver is missing or too old; run edel report and send the file it writes, as this card needs support in Edel OS";

/// Started inside another desktop, but built without the development
/// window (M6.17): it draws on the graphics card instead.
pub const NO_DEVELOPMENT_WINDOW: &str = "WAYLAND_DISPLAY or DISPLAY is set, but this build has no development window, so it draws on the graphics card; to run inside another desktop, build with cargo build -p edel-compositor --features winit";

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

/// A shortcut for a default app (M6.10) found no app of its kind: it does
/// nothing, and the line says which key to set.
pub fn no_default_app(role: &str) -> String {
    format!("no {role} app installed; set default_apps.{role}")
}

/// An app `startup.apps` names (M6.10) is not installed: it is skipped, and
/// the line says which list to fix.
pub fn startup_app_missing(id: &str) -> String {
    format!("startup app {id} is not installed; take it out of startup.apps")
}

/// A screen copy a client asked for could not be filled (M6.3): the
/// client is told `failed` and may ask again.
pub fn copy_failed(output: &str, why: impl Display) -> String {
    format!("a screen copy of {output} failed: {why}; the client is told so and may ask again")
}

/// The overview's plus could not add a workspace (M5.2j-b).
pub fn workspace_not_added(why: impl Display) -> String {
    format!(
        "could not add a workspace: {why}; set the count on Settings' Workspaces page or with edel settings set workspaces.count"
    )
}

/// A list kept by workspace, such as `workspaces.names`, cannot be
/// written down in the workspaces' new order (M5.2p, M5.12b5).
pub fn order_not_kept(key: &str, why: impl Display) -> String {
    format!(
        "could not keep {key} in the workspaces' new order: {why}; the settings file keeps it as it was"
    )
}

/// There is no home folder to keep a list kept by workspace in (M5.2p).
pub fn order_no_home(key: &str) -> String {
    format!(
        "no home folder to keep {key} in the workspaces' new order; run edel settings set {key}='[\"first\", \"\", \"third\"]' instead, one entry per workspace in order, empty where none"
    )
}

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

/// shell-ui keeps failing while the screen is locked: it is started again
/// all the same, as nothing else unlocks the screen (M6.17).
pub fn shell_ui_kept_for_lock(name: &str, how: &str, secs: f32) -> String {
    format!(
        "{name} {how} and keeps failing, but the screen is locked and {name} is what unlocks it, so it starts again in {secs:.0} s; the lines above say why it fails"
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

/// The input method the settings name is not in this image (M6.8).
pub fn input_method_missing(name: &str) -> String {
    format!(
        "{name} is not in this image, so typing through an input method is not possible; the input-method feature brings it, and typing stays in the keyboard layout until then"
    )
}

/// The input method ended while the desktop still wants it (M6.8).
pub fn input_method_ended(name: &str, how: &str) -> String {
    format!(
        "{name} {how}; typing through the input method stops until region.input_method is set again or the next login"
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

/// A screen cannot be turned off: it stays on until the next input.
pub fn screens_not_off(screen: &str, why: impl Display) -> String {
    format!("could not turn screen {screen} off: {why}; it stays on and turns on at the next input")
}

/// The settings turn every connected screen off: one stays on.
pub fn every_screen_off(screen: &str) -> String {
    format!(
        "the settings turn every screen off, so {screen} stays on; turn another screen on in Settings, Displays, before turning this one off"
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

/// A game's smoothness record could not be kept (M7.7d): `edel report`
/// shows the one before.
pub fn game_record_not_kept(why: impl Display) -> String {
    format!(
        "could not keep the game's smoothness record: {why}; edel report shows the one before, and the log line above has this one"
    )
}

/// A frame cannot be sent to the screen.
pub fn frame_not_shown(why: impl Display) -> String {
    format!("could not show a frame: {why}; the screen keeps the one before")
}

/// The rounded corners' shaders cannot be built: windows keep square
/// corners, and the rest of the desktop works.
pub fn corners_square(why: impl Display) -> String {
    format!(
        "rounded corners are off: the GPU could not build their shader ({why}); windows keep square corners"
    )
}

/// The live blur's shaders cannot be made or drawn: popups keep their own
/// colour, and the rest of the desktop is unchanged (M8.21 part 21d).
pub fn blur_off(why: &dyn Display) -> String {
    format!(
        "live blur is off: its shaders could not be made or drawn ({why}); popups keep their own colour and the rest of the desktop is unchanged"
    )
}

/// A screen refuses variable refresh (M7.7).
pub fn vrr_refused(screen: &str, why: impl Display) -> String {
    format!(
        "screen {screen} refused variable refresh: {why}; it keeps its fixed refresh, and games still run"
    )
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

/// The timer that turns auto dark at sunset and light at sunrise cannot be
/// made or set (M7.14).
pub fn sun_not_followed(why: impl Display) -> String {
    format!(
        "Automatic style does not follow the sun: {why}; the colour scheme changes when the settings do, or at the next login"
    )
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

/// The wallpaper picture cannot be shown, so the background colour does
/// (M5.10c4b); `why` is what `wallpaper::load` said.
pub fn wallpaper_not_read(path: &Path, why: &str) -> String {
    format!(
        "the wallpaper {} could not be shown, so the background colour shows instead: {why}; choose another in Settings, Appearance, or run edel settings reset appearance.wallpaper",
        path.display()
    )
}

/// `sound.low_latency` is on but this system has no low-latency file (M7.6b).
pub fn low_latency_missing(path: &Path) -> String {
    format!(
        "sound.low_latency is on, but there is no {}, so PipeWire keeps its own delay; the sound feature brings the file",
        path.display()
    )
}

/// The person's low-latency link could not be made or taken out (M7.6b).
pub fn low_latency_not_linked(link: &Path, why: impl Display) -> String {
    format!(
        "could not change the low-latency sound link {}: {why}; PipeWire keeps the delay it has, and the link is tried again when sound.low_latency changes or at the next login",
        link.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_app_shortcut_with_no_app_names_the_key_to_set() {
        assert_eq!(
            no_default_app("files"),
            "no files app installed; set default_apps.files"
        );
    }

    #[test]
    fn a_startup_app_not_installed_names_the_list_to_fix() {
        assert_eq!(
            startup_app_missing("org.example.Gone"),
            "startup app org.example.Gone is not installed; take it out of startup.apps"
        );
    }

    #[test]
    fn a_live_blur_that_is_off_says_what_stays() {
        assert_eq!(
            blur_off(&"no shader"),
            "live blur is off: its shaders could not be made or drawn (no shader); popups keep their own colour and the rest of the desktop is unchanged"
        );
    }

    #[test]
    fn a_failed_screen_copy_says_what_failed_and_what_the_client_does() {
        assert_eq!(
            copy_failed("Virtual-1", "no texture for the copy"),
            "a screen copy of Virtual-1 failed: no texture for the copy; the client is told so and may ask again"
        );
    }

    #[test]
    fn a_wallpaper_that_cannot_be_shown_says_what_shows_instead() {
        assert_eq!(
            wallpaper_not_read(
                Path::new("/home/ali/sea.jpg"),
                "the file could not be read: No such file or directory (os error 2)"
            ),
            "the wallpaper /home/ali/sea.jpg could not be shown, so the background colour shows instead: the file could not be read: No such file or directory (os error 2); choose another in Settings, Appearance, or run edel settings reset appearance.wallpaper"
        );
    }

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
        assert!(NO_GPU.starts_with("the desktop could not start: no graphics card answered;"));
        assert!(NO_GPU.contains("on a virtual machine, give it a virtual graphics card"));
        assert_eq!(
            gpu_open(Path::new("/dev/dri/card0")),
            "could not open the graphics card /dev/dri/card0: this account may not use the screen; log out and in again, and if it repeats run doas edel settings apply, which gives the people at the computer the screen"
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
    fn a_build_without_the_development_window_says_how_to_get_it() {
        assert!(NO_DEVELOPMENT_WINDOW.contains("--features winit"));
        assert!(NO_DEVELOPMENT_WINDOW.contains("draws on the graphics card"));
    }

    #[test]
    fn no_message_has_a_dash_or_a_final_full_stop() {
        let all = [
            NO_GPU.to_string(),
            GPU_LIST.to_string(),
            NO_EGL.to_string(),
            NO_DEVELOPMENT_WINDOW.to_string(),
            SCREEN_LIST.to_string(),
            INPUT_LOST.to_string(),
            screens_lost("x"),
            screens_not_off("x", "x"),
            every_screen_off("x"),
            terminal_switch(2, "x"),
            frame_not_drawn("x"),
            frame_not_shown("x"),
            state_not_written("/run/x", "x"),
            folder_not_followed(Path::new("/data"), "x"),
            x11_not_started("xwayland-satellite", "x"),
            shell_ui_not_started("edel-shell-ui", "x"),
            order_not_kept("workspaces.names", "x"),
            order_no_home("workspaces.names"),
            session_program_missing("pipewire"),
            session_program_not_started("pipewire", "x"),
            session_program_stopped("pipewire", "ended (exit status: 1)"),
            session_program_given_up("pipewire", "ended (exit status: 1)", 4, 5),
            input_method_missing("fcitx5"),
            input_method_ended("fcitx5", "ended (exit status: 1)"),
            low_latency_missing(Path::new("/usr/share/edel/sound/low-latency.conf")),
            low_latency_not_linked(Path::new("/home/ci/.config/pipewire/x.conf"), "x"),
        ];
        for text in all {
            assert!(!text.ends_with('.'), "{text}");
            assert!(
                !text.contains('\u{2013}') && !text.contains('\u{2014}'),
                "{text}"
            );
        }
    }

    #[test]
    fn low_latency_says_what_sound_keeps() {
        assert_eq!(
            low_latency_not_linked(
                Path::new("/home/ci/.config/pipewire/pipewire.conf.d/90-edel-low-latency.conf"),
                "Permission denied (os error 13)"
            ),
            "could not change the low-latency sound link /home/ci/.config/pipewire/pipewire.conf.d/90-edel-low-latency.conf: Permission denied (os error 13); PipeWire keeps the delay it has, and the link is tried again when sound.low_latency changes or at the next login"
        );
    }
}
