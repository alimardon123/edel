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

/// The context when the compositor lacks a Wayland feature shell-ui needs.
pub fn missing(global: &str) -> String {
    format!("the compositor does not offer {global}, so no panel can be drawn")
}

/// shell-ui stops with an error.
pub fn stopped(why: impl Display) -> String {
    format!("could not run: {why}; the compositor starts it again unless it keeps failing")
}

/// An app the launcher starts cannot start.
pub fn app_not_started(name: &str, why: impl Display) -> String {
    format!("could not start {name}: {why}; check that it is installed")
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
pub fn tray_not_kept(why: impl Display) -> String {
    format!("could not keep the tray's apps in the panel: {why}; they stay as they were")
}

/// There is no home folder to keep the tray's apps in the panel in.
pub const TRAY_NO_HOME: &str = "no home folder to keep the tray's apps in the panel in; run edel settings set layout.tray_in_panel='[\"APP\"]' instead, naming the apps by their tray Id";

/// The panels the editor made cannot be written down.
pub fn panels_not_kept(why: impl Display) -> String {
    format!("could not keep the panels: {why}; they stay as they were")
}

/// A change to the panels the editor was asked for is refused: the panels
/// stay as they were.
pub fn panel_not_changed(why: impl Display) -> String {
    format!("could not change the panels: {why}; they stay as they were")
}

/// There is no home folder to keep the panels in.
pub const PANELS_NO_HOME: &str = "no home folder to keep the panels in; run edel settings set layout.panels='[...]' with the panels as TOML instead";

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_tray_layout_that_is_not_kept_says_what_stays() {
        assert_eq!(
            tray_not_kept("Permission denied (os error 13)"),
            "could not keep the tray's apps in the panel: Permission denied (os error 13); they stay as they were"
        );
        assert!(TRAY_NO_HOME.contains("edel settings set layout.tray_in_panel="));
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
            panel_not_drawn("x"),
            portal_not_said("x"),
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
