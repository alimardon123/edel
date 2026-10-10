//! The shell's side of the panel's menu (M5.31b): `panel_menu.rs` lays it
//! out and draws it; this opens it at a right click on a panel where no
//! widget took the click, holds its surface and keyboard while it is open,
//! and runs the row a person chooses. Like the tiling styles' menu it is a
//! `Popup` hung on the panel at the pointer's place, with the keyboard
//! exclusive, and it is let go when it closes.

use edel::presets::Edge;
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_surface};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};

use crate::panel_menu::{self, Row, View};
use crate::popup::Popup;
use crate::{MARGIN, PANEL_MENU, Shell, fillets, paint};

/// The open panel menu: its surface, what it shows and the keyboard.
pub struct PanelMenu {
    pub popup: Popup<View>,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub view: View,
}

impl Shell {
    /// A right click on panel `i` at `x` logical pixels along it, where no
    /// widget gives an action for it: the panel menu opens beside the
    /// pointer, or closes if it is open (M5.31b).
    pub fn open_panel_menu(&mut self, i: usize, x: f32) {
        if self.panel_menu.is_some() {
            return self.close_panel_menu();
        }
        self.close_popups();
        let Some(panel) = self.panels.get(i) else {
            return;
        };
        let (edge, scale, panel_width) = (panel.edge, panel.scale, panel.width);
        let size = panel_menu::size(&self.tokens);
        let room = paint::shadow_room(&self.tokens, !fillets());
        let Some(popup) = Popup::new(self, PANEL_MENU, size, size, scale, room) else {
            return;
        };
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        // Centred on the pointer, kept inside the screen, as the styles'
        // menu is; the panel's exclusive zone keeps it above the panel.
        let most = (panel_width as i32 - size.0 as i32 - MARGIN).max(MARGIN);
        let left = (x as i32 - size.0 as i32 / 2).clamp(MARGIN, most);
        let surface = &popup.surface;
        surface.set_anchor(side | Anchor::LEFT);
        let m = MARGIN - room as i32;
        surface.set_margin(m, m, m, left - room as i32);
        surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        surface.commit();
        let keyboard = self.seat.seats().next().and_then(|seat| {
            self.seat
                .get_keyboard_with_repeat(
                    &self.qh,
                    &seat,
                    None,
                    self.handle.clone(),
                    Box::new(|shell: &mut Shell, _, event| shell.panel_menu_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for the panel menu: {e}"))
                .ok()
        });
        self.panel_menu = Some(PanelMenu {
            popup,
            keyboard,
            view: View { lit: 0 },
        });
    }

    /// Closes the panel menu and lets go of its keyboard and buffers.
    pub fn close_panel_menu(&mut self) {
        let Some(menu) = self.panel_menu.take() else {
            return;
        };
        if let Some(keyboard) = &menu.keyboard {
            keyboard.release();
        }
        eprintln!("edel-shell-ui: panel menu hidden");
    }

    /// Draws the panel menu if what it shows changed. Its first drawing
    /// logs that it is shown and where its row lies, which CI reads.
    pub fn draw_panel_menu(&mut self) {
        let Some(menu) = &mut self.panel_menu else {
            return;
        };
        let view = menu.view.clone();
        let Some(mut pixmap) = menu.popup.canvas(&view) else {
            return;
        };
        let scale = menu.popup.scale();
        panel_menu::paint(
            &mut pixmap,
            &view,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        if menu
            .popup
            .show(view, &pixmap, &self.tokens, "panel menu", &self.qh)
        {
            eprintln!("edel-shell-ui: panel menu shown");
            eprintln!(
                "edel-shell-ui: panel menu places {}",
                panel_menu::places(&self.tokens)
            );
        }
    }

    /// Does what row `row` of the menu does: Edit panels closes the menu
    /// and opens the editor.
    fn choose_panel_menu(&mut self, row: usize) {
        if let Some(Row::Edit) = panel_menu::ROWS.get(row) {
            self.close_panel_menu();
            self.open_editor();
        }
    }

    /// A key while the panel menu is open: Escape closes, Up and Down
    /// move, Tab moves down, Return and space choose.
    pub fn panel_menu_key(&mut self, event: KeyEvent) {
        let Some(menu) = &mut self.panel_menu else {
            return;
        };
        let last = panel_menu::ROWS.len().saturating_sub(1);
        match event.keysym {
            Keysym::Escape => return self.close_panel_menu(),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                let row = menu.view.lit;
                return self.choose_panel_menu(row);
            }
            Keysym::Up => menu.view.lit = menu.view.lit.saturating_sub(1),
            Keysym::Down | Keysym::Tab => menu.view.lit = (menu.view.lit + 1).min(last),
            _ => {}
        }
        self.draw_panel_menu();
    }

    /// The pointer on the panel menu: it lights the row under it, and a
    /// left click on a row chooses it.
    pub fn panel_menu_pointer(&mut self, event: &PointerEvent) {
        let Some(menu) = &self.panel_menu else {
            return;
        };
        // From the card's corner, inside the shadow's room.
        let room = menu.popup.room() as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let row = panel_menu::row_at(y, &self.tokens).filter(|_| x >= 0.0);
        match &event.kind {
            PointerEventKind::Motion { .. } => {
                if let (Some(row), Some(menu)) = (row, &mut self.panel_menu) {
                    if menu.view.lit != row {
                        menu.view.lit = row;
                        self.draw_panel_menu();
                    }
                }
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                if let Some(row) = row {
                    self.choose_panel_menu(row);
                }
            }
            _ => {}
        }
    }

    /// Whether `surface` is the open panel menu's.
    pub fn is_panel_menu(&self, surface: &wl_surface::WlSurface) -> bool {
        self.panel_menu
            .as_ref()
            .is_some_and(|m| m.popup.is(surface))
    }
}
