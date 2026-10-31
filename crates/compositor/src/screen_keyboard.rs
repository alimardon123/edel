//! The keyboard on the screen (roadmap M9.5): wvkbd, shown when a window
//! a finger tapped asks for text (text-input-v3, which GTK, Qt, foot and
//! Firefox speak), and hidden when no field asks any more or a real key is
//! pressed. A mouse click or a key forgets the tap, so a laptop typed on
//! with its own keyboard never sees it; a tap on a panel or on the
//! keyboard itself is no tap into a window, and leaves the one before. Nothing
//! of it runs until the first tap into a field: it is started then and kept, hidden, until the
//! desktop ends (`kill -USR1` hides it, `-USR2` shows it, as wvkbd takes
//! them). It types through the virtual keyboard protocol (`ime.rs`) and
//! is a layer surface along the bottom whose zone the windows make room
//! for. Its colours and font are the tokens', read when it starts.
//!
//! Text input, not input-method-v2, decides when it shows: an input
//! method is one program at a time, and fcitx5 (M6.8) keeps that place
//! for people who type a language their keyboard lacks.

use rustix::process::{Pid, Signal, kill_process};
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::text_input::TextInputSeat;

use edel_compositor::messages;
use edel_compositor::tokens::Tokens;

use crate::state::Edel;

/// The program: wvkbd with its layout for phones and tablets, international
/// keys included, from Alpine's `wvkbd-mobintl`.
const PROGRAM: &str = "wvkbd-mobintl";

/// Its height in pixels, the screen laid wide and laid tall.
const WIDE: u32 = 260;
const TALL: u32 = 320;

/// What the compositor knows of the keyboard on the screen.
#[derive(Default)]
pub struct ScreenKeyboard {
    /// The running wvkbd, once a tap into a field started it.
    pid: Option<Pid>,
    /// Whether it is on screen.
    shown: bool,
    /// Whether a field asked for text when the clients last spoke.
    asked: bool,
    /// The window's surface a finger last tapped, until a mouse or a key
    /// is used.
    tapped: Option<WlSurface>,
}

impl ScreenKeyboard {
    /// A finger tapped a window's `surface`.
    pub fn tapped(&mut self, surface: WlSurface) {
        self.tapped = Some(surface);
    }

    /// A mouse or a key was used: the tap no longer counts.
    pub fn forget_tap(&mut self) {
        self.tapped = None;
    }
}

/// wvkbd's arguments: its heights, and the tokens' colours (as `rrggbb`
/// or `rrggbbaa`) and interface font.
pub fn args(tokens: &Tokens) -> Vec<String> {
    let colour = |c: edel_compositor::tokens::Colour| c.hex().trim_start_matches('#').to_string();
    let mut args = vec![
        "-L".to_string(),
        WIDE.to_string(),
        "-H".to_string(),
        TALL.to_string(),
    ];
    for (flag, value) in [
        ("--bg", colour(tokens.panel)),
        ("--fg", colour(tokens.window)),
        ("--fg-sp", colour(tokens.card)),
        ("--press", colour(tokens.accent)),
        ("--press-sp", colour(tokens.accent)),
        ("--text", colour(tokens.panel_text)),
        ("--text-sp", colour(tokens.panel_text)),
        ("--fn", format!("{} 16", tokens.font)),
    ] {
        args.push(flag.to_string());
        args.push(value);
    }
    args
}

impl Edel {
    /// After the clients spoke: shows the keyboard when the window a
    /// finger tapped asks for text, and hides it once no field asks.
    pub fn follow_screen_keyboard(&mut self, handle: &LoopHandle<'static, Edel>) {
        let mut asking = None;
        self.seat
            .text_input()
            .with_active_text_input(|_, surface| asking = Some(surface.id()));
        let Some(field) = asking else {
            if std::mem::take(&mut self.screen_keyboard.asked) {
                self.hide_screen_keyboard();
            }
            return;
        };
        self.screen_keyboard.asked = true;
        let tapped = self
            .screen_keyboard
            .tapped
            .as_ref()
            .is_some_and(|surface| surface.id().same_client_as(&field));
        if tapped && !self.screen_keyboard.shown {
            self.show_screen_keyboard(handle);
        }
    }

    /// Hides the keyboard on the screen, if it shows: no field asks for
    /// text, or a real key was pressed.
    pub fn hide_screen_keyboard(&mut self) {
        if !self.screen_keyboard.shown {
            return;
        }
        self.screen_keyboard.shown = false;
        if let Some(pid) = self.screen_keyboard.pid {
            let _ = kill_process(pid, Signal::USR1);
        }
        eprintln!("edel-compositor: screen keyboard hidden");
    }

    /// Shows it: wvkbd shown again, or started the first time.
    fn show_screen_keyboard(&mut self, handle: &LoopHandle<'static, Edel>) {
        if let Some(pid) = self.screen_keyboard.pid {
            let _ = kill_process(pid, Signal::USR2);
            self.screen_keyboard.shown = true;
            eprintln!("edel-compositor: screen keyboard shown");
            return;
        }
        let started = crate::session::start_child(
            handle,
            self,
            PROGRAM,
            &args(&self.tokens),
            |state, pid, status| {
                if state.screen_keyboard.pid != Some(pid) {
                    return;
                }
                state.screen_keyboard.pid = None;
                state.screen_keyboard.shown = false;
                let how = status.map_or_else(|| "was lost".to_string(), |s| format!("ended ({s})"));
                eprintln!(
                    "edel-compositor: {}",
                    messages::screen_keyboard_ended(PROGRAM, &how)
                );
            },
        );
        match started {
            Ok(pid) => {
                self.screen_keyboard.pid = Some(pid);
                self.screen_keyboard.shown = true;
                eprintln!("edel-compositor: screen keyboard shown");
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => eprintln!(
                "edel-compositor: {}",
                messages::screen_keyboard_missing(PROGRAM)
            ),
            Err(e) => eprintln!(
                "edel-compositor: {}",
                messages::session_program_not_started(PROGRAM, e)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wvkbd_is_drawn_in_the_tokens_colours_and_font() {
        let tokens = Tokens::built_in();
        let args = args(&tokens);
        let value = |flag: &str| {
            let at = args.iter().position(|a| a == flag).unwrap();
            args[at + 1].clone()
        };
        assert_eq!(value("-L"), "260");
        assert_eq!(value("--bg"), tokens.panel.hex()[1..]);
        assert_eq!(value("--press"), tokens.accent.hex()[1..]);
        assert_eq!(value("--fn"), format!("{} 16", tokens.font));
        assert!(
            args.iter()
                .all(|a| !a.starts_with("--") || !a.contains('#'))
        );
    }
}
