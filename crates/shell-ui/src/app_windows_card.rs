//! The shell's side of the app windows menu (M6.19): a click on an app
//! with two or more windows opens its menu over the app's cell, with the
//! keyboard, and `app_windows.rs` says what it shows. It is opened, closed
//! and drawn as the layout button's styles menu is, and kept (its surface,
//! buffers, keyboard and screen reader) only while it is open. A row, or
//! Return on the lit row, brings that window forward.

use edel::i18n::tr;
use edel::presets::Edge;
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};

use crate::app_windows::{self, MOST, View};
use crate::popup::{self, Popup};
use crate::widgets::apps::belongs;
use crate::{APP_WINDOWS, AppWindowsMenu, MARGIN, Shell, a11y, fillets, paint};

impl Shell {
    /// A click on app `id`, which has two or more windows, `centre` logical
    /// pixels along panel `i`: its menu opens over the app's cell, or
    /// closes if it is open for the same app. Another popup's menu closes
    /// first, as the styles' menu does.
    pub(crate) fn toggle_app_windows(&mut self, i: usize, centre: f32, id: &str) {
        let same = self.app_windows.as_ref().is_some_and(|m| m.app == id);
        self.close_app_windows();
        self.close_in_use();
        if same {
            return;
        }
        self.close_launcher();
        self.close_styles();
        self.close_quick();
        self.close_centre();
        self.close_tray_grid();
        let tasks = self.toplevels.tasks();
        // The app's windows in the order the window list shows them, as
        // indices into `tasks`, so a row can be taken back to its window.
        let mine: Vec<usize> = (0..tasks.len())
            .filter(|&k| belongs(&tasks[k].app_id, id))
            .take(MOST)
            .collect();
        let titles: Vec<String> = mine.iter().map(|&k| tasks[k].title.clone()).collect();
        let focused = mine
            .iter()
            .position(|&k| tasks[k].focused && !tasks[k].minimized);
        // The row after the focused window is the lit one: the menu opens
        // on the window the person would go to next.
        let lit = focused.map_or(0, |f| (f + 1) % titles.len().max(1));
        let view = View {
            titles,
            focused,
            lit,
        };
        let size = app_windows::size(view.titles.len(), &self.tokens);
        let room = paint::shadow_room(&self.tokens, !fillets());
        let Some(panel) = self.panels.get(i) else {
            return;
        };
        let (edge, scale, panel_width) = (panel.edge, panel.scale, panel.width);
        let Some(popup) = Popup::new(self, APP_WINDOWS, size, size, scale, room) else {
            return;
        };
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        // Centred over the app's cell, kept inside the panel's width.
        let most = (panel_width as i32 - size.0 as i32 - MARGIN).max(MARGIN);
        let left = (centre as i32 - size.0 as i32 / 2).clamp(MARGIN, most);
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
                    Box::new(|shell: &mut Shell, _, event| shell.app_windows_key(event)),
                )
                .inspect_err(|e| {
                    eprintln!("edel-shell-ui: no keyboard for the app windows menu: {e}")
                })
                .ok()
        });
        self.app_windows = Some(AppWindowsMenu {
            popup,
            keyboard,
            view,
            reader: a11y::Reader::new(tr("Open windows")),
            app: id.to_string(),
            tasks: mine,
        });
    }

    /// Closes the app windows menu and lets go of its keyboard and buffers.
    pub(crate) fn close_app_windows(&mut self) {
        let Some(menu) = self.app_windows.take() else {
            return;
        };
        if let Some(keyboard) = &menu.keyboard {
            keyboard.release();
        }
        eprintln!("edel-shell-ui: app windows hidden");
    }

    /// Draws the app windows menu if what it shows changed.
    pub(crate) fn draw_app_windows(&mut self) {
        let Some(menu) = &mut self.app_windows else {
            return;
        };
        let view = menu.view.clone();
        let (items, focus) = app_windows::nodes(&view, &self.tokens);
        let (w, h) = app_windows::size(view.titles.len(), &self.tokens);
        menu.reader
            .update_focused((f64::from(w), f64::from(h)), items, focus);
        let Some(mut pixmap) = menu.popup.canvas(&view) else {
            return;
        };
        let scale = menu.popup.scale();
        app_windows::paint(
            &mut pixmap,
            &view,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        if menu.popup.show(
            view.clone(),
            &pixmap,
            &self.tokens,
            "app windows menu",
            &self.qh,
        ) {
            eprintln!(
                "edel-shell-ui: app windows shown, {} windows",
                view.titles.len()
            );
            eprintln!(
                "edel-shell-ui: app windows places {}",
                app_windows::places(&view, &self.tokens)
            );
        }
    }

    /// The window in row `row` brought forward and the menu closed. The
    /// windows are read again: when their number changed since the menu
    /// opened, its rows may name other windows, so it closes without
    /// bringing one forward, and the person can choose again.
    pub(crate) fn choose_app_window(&mut self, row: usize) {
        let Some(menu) = &self.app_windows else {
            return;
        };
        let (app, shown) = (menu.app.clone(), menu.tasks.len());
        let tasks = self.toplevels.tasks();
        let mine: Vec<usize> = (0..tasks.len())
            .filter(|&k| belongs(&tasks[k].app_id, &app))
            .take(MOST)
            .collect();
        let Some(&index) = mine.get(row).filter(|_| mine.len() == shown) else {
            return self.close_app_windows();
        };
        eprintln!("edel-shell-ui: app window chosen, {}", tasks[index].title);
        let seat = self.seat.seats().next();
        self.toplevels.activate(index, seat.as_ref());
        self.close_app_windows();
    }

    /// A key while the app windows menu is open: Escape closes, Return,
    /// Enter and space choose the lit row, Up and Down move it, and Tab and
    /// Shift+Tab move it a row as the styles' menu does.
    pub(crate) fn app_windows_key(&mut self, event: KeyEvent) {
        let Some(menu) = &mut self.app_windows else {
            return;
        };
        let last = menu.view.titles.len().saturating_sub(1);
        let by = popup::tab_step(self.shift || event.keysym == Keysym::ISO_Left_Tab);
        match event.keysym {
            Keysym::Escape => return self.close_app_windows(),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                let row = menu.view.lit;
                return self.choose_app_window(row);
            }
            Keysym::Up => menu.view.lit = menu.view.lit.saturating_sub(1),
            Keysym::Down => menu.view.lit = (menu.view.lit + 1).min(last),
            Keysym::Tab | Keysym::ISO_Left_Tab => {
                menu.view.lit = (menu.view.lit as i32 + by).clamp(0, last as i32) as usize;
            }
            _ => {}
        }
        self.draw_app_windows();
    }

    pub(crate) fn is_app_windows(&self, surface: &wl_surface::WlSurface) -> bool {
        self.app_windows
            .as_ref()
            .is_some_and(|m| m.popup.is(surface))
    }
}
