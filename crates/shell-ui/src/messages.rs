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

/// The portal cannot tell apps its settings.
pub fn portal_not_said(why: impl Display) -> String {
    format!(
        "the settings portal could not tell apps its settings: {why}; apps already open keep their colours until the setting changes"
    )
}

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

/// The clock's timer cannot start.
pub fn clock_not_started(why: impl Display) -> String {
    format!("the clock's timer did not start: {why}; the clock may stop updating")
}

/// The panel cannot be drawn.
pub fn panel_not_drawn(why: impl Display) -> String {
    format!("could not draw the panel: {why}; it is drawn again at the next change")
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
    fn no_message_has_a_dash_or_a_final_full_stop() {
        let all = [
            NO_COMPOSITOR.to_string(),
            NO_GLOBALS.to_string(),
            COMPOSITOR_GONE.to_string(),
            clock_not_started("x"),
            panel_not_drawn("x"),
            portal_not_said("x"),
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
