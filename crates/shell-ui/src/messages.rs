//! What shell-ui says when something fails, in the words
//! `docs/MESSAGES.md` asks for (M5.28b): what failed, why and what to do
//! or what still works. Each function gives the text after the
//! `edel-shell-ui: ` prefix, lower case and with no final full stop; the
//! tests below hold the text a person reads. Routine status lines that
//! CI looks for ("launcher shown", "panel places") stay where they are
//! written.

use std::fmt::Display;

/// The context when the compositor cannot be reached.
pub const NO_COMPOSITOR: &str = "could not connect to the compositor; shell-ui runs inside an Edel OS session, started by edel-compositor";

/// The context when the compositor's list of features cannot be read.
pub const NO_GLOBALS: &str = "could not read what the compositor offers";

/// No font at all: neither fontconfig's nor the fallback files (M6.17).
pub const NO_FONT: &str = "no font was found, neither through fontconfig nor in the fallback files, so the panel and the lock screen draw no words; the lock still takes the password, and installing a font (or the fonts feature) brings the words back";

/// The context when the compositor lacks a Wayland feature shell-ui needs.
pub fn missing(global: &str) -> String {
    format!("the compositor does not offer {global}, so no panel can be drawn")
}

/// The compositor offers no lock (M5.10): the screen does not lock.
pub fn lock_not_offered(why: impl Display) -> String {
    format!(
        "could not lock the screen: the compositor does not offer ext-session-lock-v1 ({why}); the session stays as it is"
    )
}

/// The compositor refused the lock, as when another program holds one.
pub const LOCK_REFUSED: &str =
    "the screen was not locked: the compositor refused, as another lock screen may be up";

/// No password helper: the lock screen stays up and says so.
pub const NO_PASSWORD_CHECKER: &str = "the screen stays locked: no password helper (unix_chkpwd, part of Linux-PAM) is installed, so no password can be checked; switch to another terminal with Ctrl+Alt+F2 and log in there to end the session";

/// The password could not be checked this time.
pub fn password_not_checked(why: impl Display) -> String {
    format!("could not check the password: {why}; the screen stays locked, try again")
}

/// The greeter could not log a person in for a reason other than a wrong
/// user name or password (M5.10b).
pub fn login_failed(why: impl Display) -> String {
    format!("could not log in: {why}; the greeter asks again, and Ctrl+Alt+F2 gives a text login")
}

/// The login screen could not write down the desktop a person chose (M5.10c3b).
pub fn session_not_kept(why: impl Display) -> String {
    format!(
        "could not remember the desktop for this login: {why}; the login went on, and the next login starts the default unless the choice is made again"
    )
}

/// shell-ui stops with an error.
pub fn stopped(why: impl Display) -> String {
    format!("could not run: {why}; the compositor starts it again unless it keeps failing")
}

/// An app the launcher starts cannot start.
pub fn app_not_started(name: &str, why: impl Display) -> String {
    format!("could not start {name}: {why}; check that it is installed")
}

/// A website typed in the launcher cannot be made a web app (M6.4); the
/// launcher stays open so the address can be changed.
pub fn web_app_not_added(url: &str, why: impl Display) -> String {
    format!(
        "could not make {url} an app: {why}; the launcher is still open, so change the address and try again"
    )
}

/// The settings portal cannot be served.
pub fn portal_not_served(why: impl Display) -> String {
    format!(
        "could not serve the settings portal: {why}; apps do not follow the light or dark setting or the accent until the next login"
    )
}

/// The panel editor cannot be asked for over the session bus: Settings'
/// Edit panels does not open it, the panel's right-click menu still does.
pub fn edit_bus_not_served(why: impl Display) -> String {
    format!(
        "could not serve the panel editor on the session bus: {why}; Settings' Edit panels does not open it, but a panel's right-click menu does"
    )
}

/// The portal cannot tell apps its settings.
pub fn portal_not_said(why: impl Display) -> String {
    format!(
        "the settings portal could not tell apps its settings: {why}; apps already open keep their colours until the setting changes"
    )
}

/// D-Bus could not be told the session's Wayland display, so a service it
/// starts for an app (a portal's backend) cannot reach the screen.
pub fn display_not_told(why: impl Display) -> String {
    format!(
        "could not tell D-Bus the session's Wayland display: {why}; screenshots and screen sharing fail until the next login"
    )
}

/// The header bars' button layout cannot be told to apps (M6.16): apps
/// already open keep their buttons where they were until the next change.
pub fn button_layout_not_said(why: impl Display) -> String {
    format!(
        "could not tell apps where their title bar buttons go: {why}; apps already open keep theirs until the setting changes"
    )
}

/// An app's permission cannot be set in the portal's store (M6.14a): the
/// portal keeps its own answer, asking as it did, until the setting changes
/// or the login ends.
pub fn app_permission_not_set(app: &str, what: &str, why: impl Display) -> String {
    format!(
        "could not set what {app} may use ({what}): {why}; the portal keeps its own answer until the next change or login"
    )
}

/// An app's Flatpak override cannot be written (M6.14a): the app keeps its
/// own reach into the person's files until the setting changes or the login
/// ends.
pub fn app_files_not_written(app: &str, why: impl Display) -> String {
    format!(
        "could not write {app}'s Flatpak override: {why}; it keeps its own reach into your files until the next change or login"
    )
}

/// The terminal's theme file cannot be written (M6.13): new terminals start
/// in foot's own scheme until it can.
pub fn terminal_theme_not_written(why: impl Display) -> String {
    format!(
        "could not write the terminal's theme file: {why}; new terminals start in foot's own scheme until it can be written"
    )
}

/// The apps' stylesheet cannot be written (M6.18).
pub fn apps_colours_not_written(why: impl Display) -> String {
    format!(
        "could not write the apps' colours to gtk.css: {why}; apps keep the colours they had until it can be written"
    )
}

/// The tray cannot be served.
pub fn tray_not_served(why: impl Display) -> String {
    format!(
        "could not serve the tray: {why}; apps' tray icons are not shown, and another tray on the session's bus may be using the name"
    )
}

/// The tray cannot tell apps about itself or an item leaving.
pub fn tray_not_said(why: impl Display) -> String {
    format!(
        "the tray could not tell apps what changed: {why}; icons still show, but an app may not notice"
    )
}

/// An app registered a tray item with something that is no address.
pub fn tray_bad_item(service: &str) -> String {
    format!(
        "{service:?} is not a bus name or an object path; register with the app's bus name, as in org.kde.StatusNotifierItem-1-1"
    )
}

/// A click could not reach a tray item's app.
pub fn tray_call_failed(id: &str, method: &str, why: impl Display) -> String {
    format!(
        "could not ask the tray item {id} to {method}: {why}; the app may have closed or may not offer it"
    )
}

/// A play, pause or track command could not reach a media player (M5.9d).
pub fn player_call_failed(bus: &str, method: &str, why: impl Display) -> String {
    format!(
        "could not ask the media player {bus} to {method}: {why}; the app may have closed or may not offer it"
    )
}

/// The notification server cannot be served.
pub fn notifications_not_served(why: impl Display) -> String {
    format!(
        "could not serve notifications: {why}; apps' notifications are not shown, and another notification daemon on the session's bus may be using the name"
    )
}

/// A signal to an app (a notification closed, a button pressed) could not
/// be sent.
pub fn notification_not_said(signal: &str, why: impl Display) -> String {
    format!(
        "could not send {signal} to the app: {why}; the notification is handled here all the same"
    )
}

/// The update check could not run or its answer was not read.
pub fn update_check_failed(why: impl Display) -> String {
    format!("could not check for a newer release: {why}; the check runs again in six hours")
}

/// A notice's stamp cannot be written down in the person's state folder.
pub fn stamp_not_kept(name: &str, why: impl Display) -> String {
    format!(
        "could not keep {name} in the person's state folder: {why}; the notice may show again at the next start"
    )
}

/// Do not disturb cannot be written down.
pub fn dnd_not_kept(why: impl Display) -> String {
    format!("could not keep do not disturb: {why}; it stays as it was")
}

/// There is no home folder to keep do not disturb in.
pub const DND_NO_HOME: &str = "no home folder to keep do not disturb in; run edel settings set notifications.do_not_disturb=true instead";

/// The tray's apps kept in the panel cannot be written down.
/// The portal could not be told the Location switch (M6.14c).
pub fn location_not_told(why: impl Display) -> String {
    format!(
        "could not tell the location portal the Location switch: {why}; apps keep what the portal had until the switch changes again"
    )
}

/// The Location tile could not be kept in the person's file (M6.14c).
pub fn location_not_kept(why: impl Display) -> String {
    format!("could not keep location: {why}; it stays as it was")
}

/// There is no home folder to keep the Location switch in (M6.14c).
pub const LOCATION_NO_HOME: &str = "no home folder to keep location in; run edel settings set location.enabled=false to turn it off instead, or location.enabled=true to turn it on";

/// shell-ui cannot follow what apps ask the location portal for (M6.14c).
pub fn location_not_watched(why: impl Display) -> String {
    format!(
        "could not follow what apps ask for their place: {why}; the sign does not show an app that gets it, and apps set to ask are not asked each time"
    )
}

/// GeoClue would not take shell-ui as its agent, which it needs before it
/// answers the portal on Edel OS (M6.14c).
pub fn location_agent_not_added(why: impl Display) -> String {
    format!(
        "GeoClue did not take the shell as its agent: {why}; apps get their place only if another agent runs"
    )
}

/// GeoClue does not say the place is in use, so the sign ends the use
/// after a few seconds (M6.14c).
pub fn location_not_in_use(why: impl Display) -> String {
    format!("GeoClue does not say the place is in use: {why}; the sign shows the app for 10 s only")
}

pub fn tray_not_kept(why: impl Display) -> String {
    format!("could not keep the tray's apps in the panel: {why}; they stay as they were")
}

/// There is no home folder to keep the tray's apps in the panel in.
pub const TRAY_NO_HOME: &str = "no home folder to keep the tray's apps in the panel in; run edel settings set panels.tray='[\"APP\"]' instead, naming the apps by their tray Id";

/// The apps pinned to the apps widget cannot be written down (M5.31d).
pub fn pins_not_kept(why: impl Display) -> String {
    format!(
        "could not keep the pinned apps: {why}; the next change to the settings puts the file's list back"
    )
}

/// There is no home folder to keep the pinned apps in (M5.31d).
pub const PINS_NO_HOME: &str = "no home folder to keep the pinned apps in; run edel settings set panels.pinned='[\"APP\"]' instead, naming the apps by their id or role";

/// The panels the editor made cannot be written down.
pub fn panels_not_kept(why: impl Display) -> String {
    format!("could not keep the panels: {why}; they stay as they were")
}

/// A change to the panels the editor was asked for is refused: the panels
/// stay as they were.
pub fn panel_not_changed(why: impl Display) -> String {
    format!("could not change the panels: {why}; they stay as they were")
}

/// The lock screen's edit mode cannot write its parts down: the edit stays
/// open, so Done can be tried again (M5.10c6).
pub fn lock_screen_not_kept(why: impl Display) -> String {
    format!(
        "could not keep the lock screen: {why}; the edit stays open, so Done can be tried again"
    )
}

/// There is no home folder to keep the lock screen's parts in (M5.10c6).
pub const LOCK_SCREEN_NO_HOME: &str = "no home folder to keep the lock screen in; run edel settings set 'lock_screen.parts=[...]' with the parts as TOML instead";

/// There is no home folder to keep the panels in.
pub const PANELS_NO_HOME: &str = "no home folder to keep the panels in; run edel settings set panels.list='[...]' with the panels as TOML instead";

/// A chosen tiling style cannot be written down.
pub fn style_not_kept(style: &str, why: impl Display) -> String {
    format!("could not keep the {style} tiling style: {why}; it stays as it was")
}

/// There is no home folder to write the style in.
pub fn style_no_home(style: &str) -> String {
    format!(
        "no home folder to keep the {style} tiling style in; run edel settings set layout.tiling_style={style} instead"
    )
}

/// The kernel's news of network and power changes cannot be listened to.
pub fn status_no_news(why: impl Display) -> String {
    format!(
        "could not listen for network and power changes: {why}; the status icons are read when quick settings open and at each minute instead of at each change"
    )
}

/// A click on quick settings could not do what it asked.
pub fn quick_not_done(what: &str, why: impl Display) -> String {
    format!("could not {what}: {why}; quick settings show what the machine reports")
}

/// Dark style cannot be written down.
pub fn dark_style_not_kept(why: impl Display) -> String {
    format!("could not keep the colour scheme: {why}; it stays as it was")
}

/// There is no home folder to keep the colour scheme in.
pub const DARK_STYLE_NO_HOME: &str = "no home folder to keep the colour scheme in; run edel settings set appearance.mode=dark instead";

/// The clock's timer cannot start.
pub fn clock_not_started(why: impl Display) -> String {
    format!("the clock's timer did not start: {why}; the clock may stop updating")
}

/// The panel cannot be drawn.
pub fn panel_not_drawn(why: impl Display) -> String {
    format!("could not draw the panel: {why}; it is drawn again at the next change")
}

/// The pages of a panel buffer no longer shown cannot be given back.
pub fn pages_not_given_back(why: impl Display) -> String {
    format!(
        "could not give back the memory of an old panel picture: {why}; it stays in use until the panel closes"
    )
}

/// Huge pages could not be turned off for shell-ui.
pub fn huge_pages_on(why: impl Display) -> String {
    format!(
        "could not turn off huge pages: {why}; shell-ui may hold up to 2 MiB more memory than it needs"
    )
}

/// The compositor ended the connection.
pub const COMPOSITOR_GONE: &str = "the compositor went away; the panel closes with it";

/// A picture of the lock screen cannot be read, so the screen leaves it out
/// (M5.10c5): its path and why, and what to do.
pub fn picture_not_read(path: &str, why: impl Display) -> String {
    format!(
        "could not read the picture {path:?}, so the lock screen leaves it out: {why}; check the path and that the file is a PNG or JPEG picture"
    )
}

/// `--look-picture` names a look this machine and the person do not have
/// (M5.10c7c): the name and the looks there are.
pub fn look_not_known(name: &str, known: &[String]) -> String {
    format!(
        "there is no lock look named {name:?}; the looks here are {}; name one of those, or a look of your own in lock_screen.looks",
        known.join(", ")
    )
}

/// A clock's time zone has no zone file on this machine, so the clock shows
/// this machine's time (M5.10c7d): the zone's name and what happens.
pub fn clock_zone_missing(zone: &str) -> String {
    format!(
        "lock: the clock's time zone {zone} is not on this machine; it shows this machine's time"
    )
}

/// A power choice on the lock screen failed (M5.10c).
pub fn power_failed(what: &str, why: impl Display) -> String {
    format!(
        "could not {what}: {why}; the power rules come with the login feature (doas), so check that it is installed"
    )
}

/// A power choice that logind refused or could not be asked, where `edel`
/// is not installed (M6.17).
pub fn power_failed_logind(what: &str, why: impl Display) -> String {
    format!(
        "could not {what} through logind: {why}; the power menu asks systemd-logind or elogind where Edel OS's edel is not installed, so check that one of them runs and lets this seat {what}"
    )
}

/// The power profile could not be asked for (M7.8): the computer keeps the
/// profile it has, and the next status read asks again.
pub fn profile_not_set(why: impl Display) -> String {
    format!("could not set the power profile: {why}; the computer keeps the profile it has")
}

/// A screenshot an app asked for could not be made (M8.21 part 21f): the app
/// gets no picture and may ask again.
pub fn screenshot_not_saved(why: impl Display) -> String {
    format!(
        "could not take the screenshot an app asked for: {why}; the app gets no picture and can ask again"
    )
}

/// The Access dialog cannot be drawn (M6.3): the question is answered no,
/// so the app that asked takes no picture.
pub const ACCESS_NOT_SHOWN: &str = "could not draw the dialog asking whether an app may take a screenshot; the request was cancelled, so no picture is taken";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_access_dialog_that_cannot_be_drawn_says_what_follows() {
        assert!(ACCESS_NOT_SHOWN.contains("the request was cancelled"));
        assert!(ACCESS_NOT_SHOWN.ends_with("no picture is taken"));
    }

    #[test]
    fn a_choice_that_is_not_kept_says_the_login_went_on() {
        let text = session_not_kept("Permission denied (os error 13)");
        assert!(
            text.starts_with("could not remember the desktop for this login: Permission denied")
        );
        assert!(text.contains("the login went on"));
    }

    #[test]
    fn a_clock_zone_this_machine_lacks_says_what_it_shows() {
        assert_eq!(
            clock_zone_missing("Asia/Tokyo"),
            "lock: the clock's time zone Asia/Tokyo is not on this machine; it shows this machine's time"
        );
    }

    #[test]
    fn a_button_layout_not_told_says_what_still_works() {
        assert_eq!(
            button_layout_not_said("no answer"),
            "could not tell apps where their title bar buttons go: no answer; apps already open keep theirs until the setting changes"
        );
    }

    #[test]
    fn an_app_permission_not_set_says_what_still_works() {
        assert_eq!(
            app_permission_not_set("org.gnome.Snapshot", "camera", "no answer"),
            "could not set what org.gnome.Snapshot may use (camera): no answer; the portal keeps its own answer until the next change or login"
        );
        assert_eq!(
            app_files_not_written("org.gnome.Snapshot", "no answer"),
            "could not write org.gnome.Snapshot's Flatpak override: no answer; it keeps its own reach into your files until the next change or login"
        );
    }

    #[test]
    fn a_terminal_theme_not_written_says_what_still_works() {
        assert_eq!(
            terminal_theme_not_written("Permission denied (os error 13)"),
            "could not write the terminal's theme file: Permission denied (os error 13); new terminals start in foot's own scheme until it can be written"
        );
    }

    #[test]
    fn an_apps_colours_not_written_says_what_still_works() {
        assert_eq!(
            apps_colours_not_written("Permission denied (os error 13)"),
            "could not write the apps' colours to gtk.css: Permission denied (os error 13); apps keep the colours they had until it can be written"
        );
    }

    #[test]
    fn a_power_choice_that_fails_says_what_to_check() {
        assert_eq!(
            power_failed("restart", "exit status: 1"),
            "could not restart: exit status: 1; the power rules come with the login feature (doas), so check that it is installed"
        );
        assert_eq!(
            power_failed_logind("suspend", "Access denied"),
            "could not suspend through logind: Access denied; the power menu asks systemd-logind or elogind where Edel OS's edel is not installed, so check that one of them runs and lets this seat suspend"
        );
    }

    #[test]
    fn a_failure_to_start_says_what_happens_next() {
        assert_eq!(
            stopped("the compositor does not offer wl_shm, so no panel can be drawn"),
            "could not run: the compositor does not offer wl_shm, so no panel can be drawn; the compositor starts it again unless it keeps failing"
        );
        assert_eq!(
            missing("zwlr_layer_shell_v1"),
            "the compositor does not offer zwlr_layer_shell_v1, so no panel can be drawn"
        );
        assert!(NO_COMPOSITOR.starts_with("could not connect to the compositor;"));
    }

    #[test]
    fn a_refused_panel_change_says_the_panels_stay() {
        assert_eq!(
            panel_not_changed("there is no widget at that place on the panels"),
            "could not change the panels: there is no widget at that place on the panels; they stay as they were"
        );
    }

    #[test]
    fn an_app_that_will_not_start_says_what_to_check() {
        assert_eq!(
            app_not_started("Foot", "No such file or directory (os error 2)"),
            "could not start Foot: No such file or directory (os error 2); check that it is installed"
        );
    }

    #[test]
    fn a_web_app_that_is_not_made_says_why_and_what_to_do() {
        assert_eq!(
            web_app_not_added(
                "https://example.org",
                "a web app named example.org is already there (edel-web-example-org); remove it first with edel web-app remove example.org"
            ),
            "could not make https://example.org an app: a web app named example.org is already there (edel-web-example-org); remove it first with edel web-app remove example.org; the launcher is still open, so change the address and try again"
        );
    }

    #[test]
    fn the_portal_says_what_apps_lose() {
        assert_eq!(
            portal_not_served("no session bus"),
            "could not serve the settings portal: no session bus; apps do not follow the light or dark setting or the accent until the next login"
        );
        assert!(
            portal_not_said("x")
                .starts_with("the settings portal could not tell apps its settings: x;")
        );
    }

    #[test]
    fn the_panel_editor_says_what_settings_loses() {
        assert!(
            edit_bus_not_served("x")
                .starts_with("could not serve the panel editor on the session bus: x;")
        );
    }

    #[test]
    fn the_tray_says_what_apps_lose() {
        assert_eq!(
            tray_not_served("name already taken"),
            "could not serve the tray: name already taken; apps' tray icons are not shown, and another tray on the session's bus may be using the name"
        );
        assert_eq!(
            tray_bad_item("x y"),
            "\"x y\" is not a bus name or an object path; register with the app's bus name, as in org.kde.StatusNotifierItem-1-1"
        );
        assert_eq!(
            tray_call_failed(":1.4/StatusNotifierItem", "Activate", "no such method"),
            "could not ask the tray item :1.4/StatusNotifierItem to Activate: no such method; the app may have closed or may not offer it"
        );
        assert!(tray_not_said("x").starts_with("the tray could not tell apps what changed: x;"));
    }

    #[test]
    fn a_media_player_that_does_not_answer_says_what_to_check() {
        assert_eq!(
            player_call_failed("org.mpris.MediaPlayer2.vlc", "PlayPause", "no such method"),
            "could not ask the media player org.mpris.MediaPlayer2.vlc to PlayPause: no such method; the app may have closed or may not offer it"
        );
    }

    #[test]
    fn notifications_say_what_apps_lose_and_what_still_works() {
        assert_eq!(
            notifications_not_served("name already taken"),
            "could not serve notifications: name already taken; apps' notifications are not shown, and another notification daemon on the session's bus may be using the name"
        );
        assert_eq!(
            notification_not_said("NotificationClosed", "connection closed"),
            "could not send NotificationClosed to the app: connection closed; the notification is handled here all the same"
        );
        assert_eq!(
            dnd_not_kept("Permission denied (os error 13)"),
            "could not keep do not disturb: Permission denied (os error 13); it stays as it was"
        );
        assert!(DND_NO_HOME.contains("edel settings set notifications.do_not_disturb=true"));
    }

    #[test]
    fn the_location_messages_say_what_stays_and_what_to_do() {
        assert_eq!(
            location_not_kept("Permission denied (os error 13)"),
            "could not keep location: Permission denied (os error 13); it stays as it was"
        );
        assert!(LOCATION_NO_HOME.contains("edel settings set location.enabled=false"));
        assert!(
            location_agent_not_added("Access denied")
                .starts_with("GeoClue did not take the shell as its agent: Access denied; apps")
        );
        assert_eq!(
            location_not_in_use("no system bus"),
            "GeoClue does not say the place is in use: no system bus; the sign shows the app for 10 s only"
        );
    }

    #[test]
    fn a_tray_layout_that_is_not_kept_says_what_stays() {
        assert_eq!(
            tray_not_kept("Permission denied (os error 13)"),
            "could not keep the tray's apps in the panel: Permission denied (os error 13); they stay as they were"
        );
        assert!(TRAY_NO_HOME.contains("edel settings set panels.tray="));
    }

    #[test]
    fn pinned_apps_that_are_not_kept_say_what_happens_next() {
        assert_eq!(
            pins_not_kept("Permission denied (os error 13)"),
            "could not keep the pinned apps: Permission denied (os error 13); the next change to the settings puts the file's list back"
        );
        assert!(PINS_NO_HOME.contains("edel settings set panels.pinned="));
    }

    #[test]
    fn a_style_that_is_not_kept_says_the_command_that_works() {
        assert_eq!(
            style_no_home("split"),
            "no home folder to keep the split tiling style in; run edel settings set layout.tiling_style=split instead"
        );
        assert_eq!(
            style_not_kept("scroll", "Permission denied (os error 13)"),
            "could not keep the scroll tiling style: Permission denied (os error 13); it stays as it was"
        );
    }

    #[test]
    fn quick_settings_say_what_they_could_not_do_and_what_still_works() {
        assert_eq!(
            quick_not_done("turn Wi-Fi off", "nmcli failed: not authorized"),
            "could not turn Wi-Fi off: nmcli failed: not authorized; quick settings show what the machine reports"
        );
        assert_eq!(
            dark_style_not_kept("Permission denied (os error 13)"),
            "could not keep the colour scheme: Permission denied (os error 13); it stays as it was"
        );
        assert!(DARK_STYLE_NO_HOME.contains("edel settings set appearance.mode=dark"));
        assert!(
            status_no_news("denied").starts_with("could not listen for network and power changes")
        );
    }

    #[test]
    fn no_message_has_a_dash_or_a_final_full_stop() {
        let all = [
            NO_COMPOSITOR.to_string(),
            NO_GLOBALS.to_string(),
            COMPOSITOR_GONE.to_string(),
            clock_not_started("x"),
            look_not_known("x", &["classic".to_string()]),
            panel_not_drawn("x"),
            portal_not_said("x"),
            display_not_told("x"),
            edit_bus_not_served("x"),
            tray_not_served("x"),
            tray_not_said("x"),
            tray_bad_item("x"),
            tray_call_failed("a", "b", "x"),
            player_call_failed("a", "b", "x"),
            notifications_not_served("x"),
            notification_not_said("a", "x"),
            dnd_not_kept("x"),
            tray_not_kept("x"),
            TRAY_NO_HOME.to_string(),
            update_check_failed("x"),
            stamp_not_kept("a", "x"),
            DND_NO_HOME.to_string(),
            status_no_news("x"),
            quick_not_done("a", "x"),
            dark_style_not_kept("x"),
            DARK_STYLE_NO_HOME.to_string(),
            pins_not_kept("x"),
            PINS_NO_HOME.to_string(),
            lock_screen_not_kept("x"),
            LOCK_SCREEN_NO_HOME.to_string(),
            picture_not_read("/a.jpg", "x"),
            location_not_told("x"),
            location_not_kept("x"),
            LOCATION_NO_HOME.to_string(),
            location_not_watched("x"),
            location_not_in_use("x"),
            screenshot_not_saved("x"),
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
