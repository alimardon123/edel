//! The shell's side of the tray's grid (M5.9g): the arrow opens it, and
//! `trayview.rs` lays it out and draws it. The grid is a surface of its
//! own, hung on the arrow, with the keyboard. An icon dragged from it onto
//! the panel is kept there, and an icon dragged off the panel goes behind
//! the arrow again; both write `layout.tray_in_panel` with
//! `edel::settings::write`, as do-not-disturb does, and the list's rule is
//! `edel::settings::tray_list_with` and `tray_value`, which Settings' Tray
//! card follows too (M5.9h).

use edel::i18n::tr;
use edel::places;
use edel::presets::Edge;
use edel::settings::{self, TRAY_IN_PANEL};
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_surface};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};

use crate::popup::Popup;
use crate::trayview::{self, Act, Key, Part, Step};
use crate::widgets::tray::{WIDGET as TRAY, arrow_middle, split};
use crate::{
    MARGIN, SETTINGS, Shell, TRAY_GRID, a11y, fillets, messages, paint, quick, settings_texts,
};

/// The page of Settings the tray's gear opens.
const LAYOUT_PAGE: &str = "layout";
/// How far a press must travel before it is a drag, logical pixels.
const DRAG: f32 = 6.0;

/// The open grid: its surface and what it shows, the keyboard, what a
/// screen reader reads, the press that may become a drag, and where it
/// hangs: the arrow's place along the panel (the item's Activate asks for
/// it, as the tray's own clicks do), the distance from the screen's edge
/// it hangs from and the left edge from the screen's left, logical pixels.
pub struct TrayCard {
    pub popup: Popup<trayview::View>,
    pub view: trayview::View,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub reader: a11y::Reader,
    pub logged: String,
    pub press: Option<(usize, f32, f32)>,
    pub edge: Edge,
    pub at: f32,
    pub gap: i32,
    pub left: i32,
}

/// Whether a press from `from` to `to` moved more than [`DRAG`] px.
pub fn moved(from: (f32, f32), to: (f32, f32)) -> bool {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    dx * dx + dy * dy > DRAG * DRAG
}

impl Shell {
    // ---- The grid's surface ----

    /// Opens the grid of the apps behind the arrow at `at` logical pixels
    /// along panel `i`, or closes it if open. With nothing behind the arrow
    /// it does nothing. Opening closes the other cards and pop-ups as
    /// quick settings does.
    pub fn toggle_tray_grid(&mut self, i: usize, at: f32) {
        // A tooltip over the arrow goes as the grid comes (M5.9h).
        self.hide_tooltip();
        if self.tray_grid.is_some() {
            return self.close_tray_grid();
        }
        let icons: Vec<trayview::Icon> = split(&self.live)
            .1
            .into_iter()
            .map(trayview::Icon::from)
            .collect();
        if icons.is_empty() {
            return;
        }
        self.close_launcher();
        self.close_styles();
        self.close_quick();
        self.close_centre();
        self.hide_osd();
        let Some(panel) = self.panels.get(i) else {
            return;
        };
        let (edge, scale, style, bar, floating, panel_width) = (
            panel.edge,
            panel.scale,
            panel.style,
            panel.size,
            panel.floating,
            panel.width,
        );
        let screen = self.screen_width();
        let compact = screen > 0 && screen < quick::COMPACT_BELOW;
        let view = trayview::View {
            icons,
            compact,
            ..trayview::View::default()
        };
        let layout = trayview::layout(&view);
        let room = paint::shadow_room(&self.tokens, !fillets());
        let Some(mut popup) = Popup::new(self, TRAY_GRID, layout.size, layout.size, scale, room)
        else {
            return;
        };
        popup.set_cards(trayview::cards(&layout, &self.tokens), &self.compositor);
        // Centred on the arrow, kept inside the screen, as the styles' menu
        // is; the card keeps its place and its shadow reaches past it.
        let size = layout.size.0 as i32;
        let most = (panel_width as i32 - size - MARGIN).max(MARGIN);
        let left = (at as i32 - size / 2).clamp(MARGIN, most);
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        let m = MARGIN - room as i32;
        let surface = &popup.surface;
        surface.set_anchor(side | Anchor::LEFT);
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
                    Box::new(|shell: &mut Shell, _, event| shell.tray_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for the tray grid: {e}"))
                .ok()
        });
        // The panel's height and the margin it hangs with, and the room
        // round the card: where the card's edge lies from the screen's edge.
        // A floating bar hangs its gap from the edge too (M5.31c).
        let lift = if floating { self.tokens.gap as i32 } else { 0 };
        let gap = paint::height(style, bar, &self.tokens) as i32 + lift + m + room as i32;
        self.tray_grid = Some(TrayCard {
            popup,
            view,
            keyboard,
            reader: a11y::Reader::new(tr("Hidden tray icons")),
            logged: String::new(),
            press: None,
            edge,
            at,
            gap,
            left,
        });
        // The arrow lights while the grid is open.
        self.live.tray_open = true;
        self.draw_all();
    }

    /// The tray's key (Super+B unless `[shortcuts]` says otherwise, M5.9h):
    /// the grid of the apps behind the arrow opens with the keyboard on its
    /// first app, or closes if it is open. With nothing behind the arrow,
    /// or no tray on the panels, nothing opens.
    pub fn tray_key_pressed(&mut self) {
        if self.tray_grid.is_some() {
            return self.close_tray_grid();
        }
        // The first panel holding the tray, and the tray's left edge there.
        let tray = self.panels.iter().enumerate().find_map(|(i, panel)| {
            let j = (0..panel.places.len())
                .find(|&j| panel.row.widget(j).is_some_and(|w| w.name == TRAY.name))?;
            Some((i, panel.places[j].0))
        });
        let behind = !split(&self.live).1.is_empty();
        match tray {
            Some((i, left)) if behind => {
                self.toggle_tray_grid(i, left + arrow_middle());
            }
            _ => {
                eprintln!("edel-shell-ui: tray key, but no app waits behind the arrow");
                return;
            }
        }
        // The keyboard starts on the first app.
        let Some(card) = &mut self.tray_grid else {
            return;
        };
        card.view.focus = Some(Part::Icon(0));
        self.draw_tray_grid();
        eprintln!("edel-shell-ui: tray grid opened from the keyboard");
    }

    /// Closes the grid and lets go of its keyboard and buffers.
    pub fn close_tray_grid(&mut self) {
        let Some(card) = self.tray_grid.take() else {
            return;
        };
        if let Some(keyboard) = &card.keyboard {
            keyboard.release();
        }
        drop(card);
        self.live.tray_open = false;
        self.draw_all();
        eprintln!("edel-shell-ui: tray grid hidden");
    }

    /// Whether `surface` is the open grid's.
    pub fn is_tray_grid(&self, surface: &wl_surface::WlSurface) -> bool {
        self.tray_grid.as_ref().is_some_and(|c| c.popup.is(surface))
    }

    /// Draws the grid if what it shows changed. Where its parts lie is
    /// logged first, then the first showing, so CI reads both in order.
    pub fn draw_tray_grid(&mut self) {
        let Some(card) = &mut self.tray_grid else {
            return;
        };
        let layout = trayview::layout(&card.view);
        let Some(mut pixmap) = card.popup.canvas(&card.view) else {
            return;
        };
        let scale = card.popup.scale();
        trayview::paint(
            &mut pixmap,
            &card.view,
            &layout,
            &self.tokens,
            Some(&mut self.icons),
            Some(&mut self.text),
            scale,
        );
        let side = match card.edge {
            Edge::Top => "top",
            Edge::Bottom => "bottom",
        };
        let places = format!(
            "{}, {side} {}, left {}",
            trayview::places(&layout, &card.view),
            card.gap,
            card.left
        );
        if places != card.logged {
            eprintln!("edel-shell-ui: tray grid places {places}");
            card.logged = places;
        }
        let first = card.popup.show(
            card.view.clone(),
            &pixmap,
            &self.tokens,
            "tray grid",
            &self.qh,
        );
        if first {
            eprintln!(
                "edel-shell-ui: tray grid shown, {} icons",
                card.view.icons.len()
            );
        }
        let size = (f64::from(layout.size.0), f64::from(layout.size.1));
        card.reader
            .update(size, trayview::nodes(&card.view, &layout));
    }

    /// The pointer on the grid. It lights what it is over; a press on an
    /// icon waits for the release, a drag past [`DRAG`] px makes it a drag;
    /// the gear opens the tray's settings; a right press asks the icon for
    /// its menu. The compositor keeps the pointer on the surface where a
    /// button went down, so a release off the card still comes here.
    pub fn tray_pointer(&mut self, event: &PointerEvent) {
        let Some(card) = &self.tray_grid else {
            return;
        };
        // From the card's corner, inside the shadow's room.
        let room = card.popup.room() as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let layout = trayview::layout(&card.view);
        let over = trayview::hit(&layout, x, y);
        match &event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                if let Some(card) = &mut self.tray_grid {
                    card.view.hover = over;
                    if let (Some((i, px, py)), None) = (card.press, card.view.dragging) {
                        if moved((px, py), (x, y)) {
                            card.view.dragging = Some(i);
                        }
                    }
                }
                self.draw_tray_grid();
            }
            PointerEventKind::Leave { .. } => {
                if let Some(card) = &mut self.tray_grid {
                    card.view.hover = None;
                }
                self.draw_tray_grid();
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => match over {
                Some(Part::Icon(i)) => {
                    if let Some(card) = &mut self.tray_grid {
                        card.press = Some((i, x, y));
                    }
                }
                Some(Part::Gear) => {
                    self.close_tray_grid();
                    self.tray_settings();
                }
                None => {}
            },
            PointerEventKind::Press { button, .. } if *button == BTN_RIGHT => {
                let Some(Part::Icon(i)) = over else {
                    return;
                };
                let Some(card) = &self.tray_grid else {
                    return;
                };
                let (id, at) = (card.view.icons[i].id.clone(), card.at);
                self.tray_call(&id, true, at);
                self.close_tray_grid();
            }
            PointerEventKind::Release { button, .. } if *button == BTN_LEFT => {
                self.tray_grid_release(x, y);
            }
            _ => {}
        }
    }

    /// The left button let go on the grid: a drag that ends off the card on
    /// the panel's side keeps its icon in the panel; a press that did not
    /// move activates the icon and closes the grid; any other drag ends
    /// where it began.
    fn tray_grid_release(&mut self, x: f32, y: f32) {
        let Some(card) = &mut self.tray_grid else {
            return;
        };
        let press = card.press.take();
        let dragging = card.view.dragging.take();
        let layout = trayview::layout(&card.view);
        match (dragging, press) {
            (Some(i), _) => {
                let beyond = match card.edge {
                    Edge::Bottom => y > layout.card.h,
                    Edge::Top => y < 0.0,
                };
                let app = card.view.icons.get(i).map(|icon| icon.app.clone());
                match app {
                    Some(app) if beyond && !trayview::inside(&layout, x, y) => {
                        self.keep_in_panel(&app, true);
                    }
                    _ => self.draw_tray_grid(),
                }
            }
            (None, Some((i, _, _))) => {
                let Some((id, at)) = card
                    .view
                    .icons
                    .get(i)
                    .map(|icon| (icon.id.clone(), card.at))
                else {
                    return;
                };
                self.tray_call(&id, false, at);
                self.close_tray_grid();
            }
            (None, None) => {}
        }
    }

    /// A key while the grid is open: the arrows and Tab move the focus,
    /// Return and Space activate, Menu or Shift+F10 opens an icon's menu,
    /// Escape closes; the gear opens the tray's settings.
    pub fn tray_key(&mut self, event: KeyEvent) {
        let key = match event.keysym {
            Keysym::Left => Key::Left,
            Keysym::Right => Key::Right,
            Keysym::Up => Key::Up,
            Keysym::Down => Key::Down,
            Keysym::Tab => Key::Tab,
            Keysym::Return | Keysym::KP_Enter | Keysym::space => Key::Activate,
            Keysym::Menu => Key::Menu,
            Keysym::F10 if self.shift => Key::Menu,
            Keysym::Escape => Key::Escape,
            _ => return,
        };
        let step = {
            let Some(card) = &self.tray_grid else {
                return;
            };
            trayview::key(&card.view, card.view.focus, key)
        };
        match step {
            Step::Focus(part) => {
                if let Some(card) = &mut self.tray_grid {
                    card.view.focus = Some(part);
                }
                self.draw_tray_grid();
            }
            Step::Act(Act::Activate(i)) => self.tray_grid_item(i, false),
            Step::Act(Act::Menu(i)) => self.tray_grid_item(i, true),
            Step::Act(Act::Settings) => {
                self.close_tray_grid();
                self.tray_settings();
            }
            Step::Close => self.close_tray_grid(),
            Step::Nothing => {}
        }
    }

    /// Asks the grid's icon `i` to activate, or for its menu with `menu`,
    /// and closes the grid.
    fn tray_grid_item(&mut self, i: usize, menu: bool) {
        let Some(card) = &self.tray_grid else {
            return;
        };
        let Some((id, at)) = card
            .view
            .icons
            .get(i)
            .map(|icon| (icon.id.clone(), card.at))
        else {
            return;
        };
        self.tray_call(&id, menu, at);
        self.close_tray_grid();
    }

    /// Opens Settings' Layout page, as quick settings opens its pages.
    fn tray_settings(&mut self) {
        let argv = [
            SETTINGS.to_string(),
            "--page".to_string(),
            LAYOUT_PAGE.to_string(),
        ];
        self.launcher.spawn(&argv, "Settings");
    }

    // ---- The panel's side of the drag ----

    /// The left button let go on the panel after a press on a kept icon
    /// (`tray_press`): a click activates it; dragged well off the panel
    /// (more than the panel's height beyond its edge), the icon goes behind
    /// the arrow. `x`, `y` are logical pixels on the panel's surface.
    pub fn tray_panel_release(&mut self, i: usize, x: f32, y: f32) {
        let Some((_, id, px, py)) = self.tray_press.take() else {
            return;
        };
        if !moved((px, py), (x, y)) {
            self.tray_call(&id, false, px);
            return;
        }
        if !self.off_panel(i, y) {
            return;
        }
        let app = self
            .live
            .tray
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.app.clone());
        if let Some(app) = app {
            self.keep_in_panel(&app, false);
        }
    }

    /// Keeps `app` in the panel (`keep`) or takes it out, behind the arrow:
    /// the person's `layout.tray_in_panel` is written as Settings writes a
    /// key, and what applies without the person's file is left out, as
    /// writers never write a default (ADR-008).
    pub fn keep_in_panel(&mut self, app: &str, keep: bool) {
        let list = settings::tray_list_with(&self.live.tray_in_panel, app, keep);
        let (machine, _) = settings_texts();
        let value = settings::tray_value(&list, machine.as_deref());
        match places::person_settings().map(|p| places::found(&p)) {
            Some(path) => match settings::write(&path, TRAY_IN_PANEL, value.as_deref()) {
                Ok(()) => {
                    if keep {
                        eprintln!("edel-shell-ui: tray: {app} kept in the panel");
                    } else {
                        eprintln!("edel-shell-ui: tray: {app} behind the arrow");
                    }
                    self.live.tray_in_panel = list;
                    self.tray_split_changed();
                }
                Err(e) => eprintln!(
                    "edel-shell-ui: {}",
                    messages::tray_not_kept(format!("{e:#}"))
                ),
            },
            None => eprintln!("edel-shell-ui: {}", messages::TRAY_NO_HOME),
        }
        self.draw_all();
    }

    /// The settings files changed (M5.9h): what the panel reads from them
    /// is read again, so a change by Settings or `edel settings set` shows
    /// at once. shell-ui's own writes come back here too and change
    /// nothing more.
    pub fn settings_changed(&mut self) {
        let now = crate::tray_in_panel();
        if now != self.live.tray_in_panel {
            self.live.tray_in_panel = now;
            self.tray_split_changed();
            self.draw_all();
        }
        // layout.panels (M5.31b): the panels follow the files at once; a
        // change of preset restarts shell-ui instead, as the compositor
        // says.
        let (machine, person) = crate::settings_texts();
        let wanted = edel::panel_edit::applying(machine.as_deref(), person.as_deref());
        if wanted != self.panel_specs {
            self.panels_changed(wanted);
        }
        // apps.pinned (M5.31d): the apps widget follows the files at once.
        let pins = edel::panel_edit::pins_applying(machine.as_deref(), person.as_deref());
        self.set_pins(pins);
    }

    /// The tray's split changed: the number behind the arrow is logged when
    /// it changes. A grid showing the same icons is drawn again with their
    /// news; one showing other icons closes, since the arrow it hangs on
    /// moves with the panel's width.
    pub fn tray_split_changed(&mut self) {
        let behind: Vec<trayview::Icon> = split(&self.live)
            .1
            .into_iter()
            .map(trayview::Icon::from)
            .collect();
        let count = behind.len();
        if count != self.tray_behind {
            self.tray_behind = count;
            eprintln!("edel-shell-ui: tray: {count} behind the arrow");
        }
        let same = self.tray_grid.as_ref().map(|card| {
            card.view
                .icons
                .iter()
                .map(|i| &i.id)
                .eq(behind.iter().map(|i| &i.id))
        });
        match same {
            None => {}
            Some(true) => {
                if let Some(card) = &mut self.tray_grid {
                    card.view.icons = behind;
                }
                self.draw_tray_grid();
            }
            Some(false) => self.close_tray_grid(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_is_more_than_six_pixels() {
        assert!(!moved((10.0, 10.0), (14.0, 14.0)), "about 5.7 px");
        assert!(moved((10.0, 10.0), (17.0, 10.0)), "7 px");
    }
}
