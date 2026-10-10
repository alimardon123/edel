//! The shell's side of the panel's menu (M5.31b, M5.31c): `panel_menu.rs`
//! lays it out and draws it; this opens it at a right click on a panel
//! where no widget took the click, holds its surface and keyboard while it
//! is open, and makes the change a choice says. A change is written at
//! once as the person's `panels.list` (the drawer's writer, `save_panels`)
//! and the Undo bar offers to take it back. Like the tiling styles' menu it
//! is a `Popup` hung on the panel at the pointer's place, with the keyboard
//! exclusive, and it is let go when it closes.

use edel::i18n::tr;
use edel::presets::Edge;
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_surface};
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};

use crate::a11y;
use crate::editor_card;
use crate::messages;
use crate::panel_menu::{self, Facts, Hit, Row, Step, View};
use crate::popup::Popup;
use crate::{MARGIN, PANEL_MENU, Shell, fillets, paint};

/// The open panel menu: its surface, what it shows, the keyboard, the edge
/// of the panel it was opened on (its spec is found by that edge) and what a
/// screen reader reads.
pub struct PanelMenu {
    pub popup: Popup<View>,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub view: View,
    pub edge: Edge,
    pub reader: a11y::Reader,
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
        let Some(facts) = self.panel_facts(edge) else {
            return;
        };
        let view = View {
            rows: panel_menu::rows(&facts),
            facts,
            lit: 0,
        };
        let size = panel_menu::size(&view, &self.tokens);
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
            view,
            edge,
            reader: a11y::Reader::new(tr("Panel menu")),
        });
    }

    /// What the panel on `edge` is, from its spec, and whether no panel lies
    /// along the other edge.
    fn panel_facts(&self, edge: Edge) -> Option<Facts> {
        let spec = self.panel_specs.iter().find(|p| p.edge == edge)?;
        let other = panel_menu::opposite(edge);
        Some(Facts {
            style: spec.style,
            size: spec.size,
            floating: spec.floating,
            hide: spec.hide,
            screens: spec.screens,
            edge,
            other_edge_free: !self.panel_specs.iter().any(|p| p.edge == other),
            panels: self.panel_specs.len(),
        })
    }

    /// Closes the panel menu and lets go of its keyboard, buffers and reader.
    pub fn close_panel_menu(&mut self) {
        let Some(menu) = self.panel_menu.take() else {
            return;
        };
        if let Some(keyboard) = &menu.keyboard {
            keyboard.release();
        }
        eprintln!("edel-shell-ui: panel menu hidden");
    }

    /// Draws the panel menu if what it shows changed. Its first drawing logs
    /// that it is shown and where its parts lie, which CI reads.
    pub fn draw_panel_menu(&mut self) {
        let Some(menu) = &mut self.panel_menu else {
            return;
        };
        let view = menu.view.clone();
        let layout = panel_menu::layout(&view, &self.tokens);
        let Some(mut pixmap) = menu.popup.canvas(&view) else {
            return;
        };
        let scale = menu.popup.scale();
        panel_menu::paint(
            &mut pixmap,
            &view,
            &layout,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        let places = panel_menu::places(&view, &self.tokens);
        if menu
            .popup
            .show(view.clone(), &pixmap, &self.tokens, "panel menu", &self.qh)
        {
            eprintln!("edel-shell-ui: panel menu shown");
            eprintln!("edel-shell-ui: panel menu places {places}");
        }
        let size = (f64::from(layout.size.0), f64::from(layout.size.1));
        menu.reader.update(size, panel_menu::nodes(&view, &layout));
    }

    /// Does what `hit` chooses: Edit panels closes the menu and opens the
    /// editor; any other row or segment changes the panels.
    fn choose_panel_menu(&mut self, hit: Hit) {
        let Some((edge, scale, row)) = self.panel_menu.as_ref().and_then(|menu| {
            let i = match hit {
                Hit::Row(i) | Hit::Segment(i, _) => i,
            };
            let row = menu.view.rows.get(i).copied()?;
            Some((menu.edge, menu.popup.scale() as u32, row))
        }) else {
            return;
        };
        match row {
            Row::Edit => {
                self.close_panel_menu();
                self.open_editor();
            }
            _ => self.change_panels(row, hit, edge, scale),
        }
    }

    /// Makes the change `row` and `hit` say to the panel on `edge`. The new
    /// panels are written as the person's `panels.list`, then shown, and
    /// the Undo bar opens over the panel for the old ones, unless the writer
    /// had nothing to write. A refused change is logged and the menu stays.
    fn change_panels(&mut self, row: Row, hit: Hit, edge: Edge, scale: u32) {
        let original = self.panel_specs.clone();
        let Some(index) = original.iter().position(|p| p.edge == edge) else {
            return;
        };
        let (new, what) = match panel_menu::change(&original, index, row, hit) {
            Ok(Some(done)) => done,
            Ok(None) => return,
            Err(e) => {
                eprintln!(
                    "edel-shell-ui: {}",
                    messages::panel_not_changed(format!("{e:#}"))
                );
                return;
            }
        };
        let Some(saved) = self.save_panels(&new) else {
            return;
        };
        self.close_panel_menu();
        self.panels_changed(new.clone());
        eprintln!("edel-shell-ui: panel menu changed {what}, {saved}");
        if saved == editor_card::NOTHING {
            return;
        }
        // The Undo bar sits above the panel the change leaves on its edge:
        // where a move put it, else the one on `edge`, or the first left.
        let at = match row {
            Row::Move(to) => to,
            _ => edge,
        };
        let Some(bar) = new.iter().find(|p| p.edge == at).or(new.first()) else {
            return;
        };
        let room = paint::shadow_room(&self.tokens, !fillets());
        let above = self.above_panel(bar.style, bar.size, bar.floating, room);
        self.open_undo_bar(bar.edge, above, scale, original);
    }

    /// A key while the panel menu is open, as `panel_menu::key` says it acts.
    pub fn panel_menu_key(&mut self, event: KeyEvent) {
        let Some(key) = editor_card::key_of(event.keysym) else {
            return;
        };
        let Some(menu) = &self.panel_menu else {
            return;
        };
        match panel_menu::key(&menu.view, key) {
            Step::Move(lit) => {
                if let Some(menu) = &mut self.panel_menu {
                    menu.view.lit = lit;
                }
                self.draw_panel_menu();
            }
            Step::Choose(hit) => self.choose_panel_menu(hit),
            Step::Close => self.close_panel_menu(),
            Step::Nothing => {}
        }
    }

    /// The pointer on the panel menu: it lights the row under it, and a left
    /// click on a row or a segment chooses it.
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
        let layout = panel_menu::layout(&menu.view, &self.tokens);
        match &event.kind {
            PointerEventKind::Motion { .. } => {
                let row = panel_menu::row_at(&layout, x, y);
                if let (Some(row), Some(menu)) = (row, &mut self.panel_menu) {
                    if menu.view.lit != row {
                        menu.view.lit = row;
                        self.draw_panel_menu();
                    }
                }
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                if let Some(hit) = panel_menu::hit(&layout, x, y) {
                    self.choose_panel_menu(hit);
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
