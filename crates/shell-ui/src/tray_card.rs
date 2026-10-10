//! The shell's side of the tray's grid (M5.9g): the arrow opens it, and
//! `trayview.rs` lays it out and draws it. Spec B makes the surface.

use crate::Shell;

impl Shell {
    /// Opens the tray's grid of the apps behind its arrow, or closes it
    /// (M5.9g).
    pub fn toggle_tray_grid(&mut self) {
        eprintln!("edel-shell-ui: tray grid asked");
    }
}
