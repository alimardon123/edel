//! The in-use card's surface (M6.14b): a click on the sign opens the card
//! over it, with the keyboard, and `in_use_menu.rs` says what it shows. It
//! is opened, closed and drawn as the app windows menu is, and kept (its
//! surface, buffers, keyboard and screen reader) only while it is open.
//! Stop cuts the app off with `pw-cli destroy`; Settings opens the app's
//! page in `edel-settings --app`.

use std::process::{Command, Stdio};

use edel::apps;
use edel::i18n::tr;
use edel::presets::Edge;
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};

use crate::in_use::InUse;
use crate::in_use_menu::{self, Button, Layout, View};
use crate::popup::{self, Popup};
use crate::widgets::{Action, Input};
use crate::{IN_USE, InUseMenu, MARGIN, Shell, a11y, fillets, paint};

impl Shell {
    /// The view and layout for the uses now, on the current text and
    /// tokens, with `focus` kept where it still is.
    fn in_use_build(&mut self, pages: &[String], focus: Option<usize>) -> (View, Layout) {
        let mut view = in_use_menu::view(&self.in_use, pages, focus);
        let layout = in_use_menu::layout(&view, &self.tokens, &mut |text, size| {
            self.text.line(text, size).width
        });
        view.focus = view.focus.filter(|f| *f < layout.buttons.len());
        (view, layout)
    }

    /// A click on the sign, `centre` logical pixels along panel `i`: the
    /// card opens over the sign, or closes if it is open. It opens with
    /// nothing in use as with nothing to show. Other popups close first, as
    /// the app windows menu does.
    pub(crate) fn toggle_in_use(&mut self, i: usize, centre: f32) {
        let same = self.in_use_menu.is_some();
        self.close_in_use();
        if same || self.in_use.is_empty() {
            return;
        }
        self.close_launcher();
        self.close_styles();
        self.close_app_windows();
        self.close_quick();
        self.close_centre();
        self.close_tray_grid();
        let Some(panel) = self.panels.get(i) else {
            return;
        };
        let (edge, scale, panel_width) = (panel.edge, panel.scale, panel.width);
        // The apps with a page in Settings, read once as the card opens.
        let pages: Vec<String> = apps::read_all(&apps::dirs())
            .into_iter()
            .map(|app| app.id)
            .collect();
        let (view, layout) = self.in_use_build(&pages, None);
        let size = layout.size;
        let room = paint::shadow_room(&self.tokens, !fillets());
        let Some(popup) = Popup::new(self, IN_USE, size, size, scale, room) else {
            return;
        };
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        // Centred over the sign, kept inside the panel's width.
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
                    Box::new(|shell: &mut Shell, _, event| shell.in_use_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for the in-use card: {e}"))
                .ok()
        });
        self.in_use_menu = Some(InUseMenu {
            popup,
            keyboard,
            view,
            layout,
            reader: a11y::Reader::new(tr("In use now")),
            pages,
        });
    }

    /// Closes the in-use card and lets go of its keyboard and buffers.
    pub(crate) fn close_in_use(&mut self) {
        let Some(menu) = self.in_use_menu.take() else {
            return;
        };
        if let Some(keyboard) = &menu.keyboard {
            self.let_go(keyboard);
        }
        eprintln!("edel-shell-ui: in use card hidden");
    }

    /// Draws the card if what it shows changed. Its first drawing logs the
    /// places of its buttons, `in use card places`.
    pub(crate) fn draw_in_use(&mut self) {
        let glass = self.glass();
        let Some(menu) = &mut self.in_use_menu else {
            return;
        };
        let (items, focus) = in_use_menu::nodes(&menu.view, &menu.layout);
        let (w, h) = menu.layout.size;
        menu.reader
            .update_focused((f64::from(w), f64::from(h)), items, focus);
        let Some(mut pixmap) = menu.popup.canvas(&menu.view) else {
            return;
        };
        let scale = menu.popup.scale();
        in_use_menu::paint(
            &mut pixmap,
            &menu.view,
            &menu.layout,
            &self.tokens,
            Some(&mut self.text),
            Some(&mut self.icons),
            scale,
        );
        let view = menu.view.clone();
        menu.popup
            .set_blur(&self.compositor, glass, self.tokens.radius_menu as f32);
        if menu
            .popup
            .show(view.clone(), &pixmap, &self.tokens, "in use card", &self.qh)
        {
            eprintln!("edel-shell-ui: in use card shown, {} uses", view.rows.len());
            eprintln!(
                "edel-shell-ui: in use card places {}",
                in_use_menu::places(&view, &menu.layout)
            );
        }
    }

    /// The uses changed while the card is open: it closes when none is left,
    /// and otherwise is laid out again for them, its surface resized if it
    /// must be, and drawn.
    pub(crate) fn in_use_card_changed(&mut self) {
        let Some(menu) = &self.in_use_menu else {
            return;
        };
        if self.in_use.is_empty() {
            return self.close_in_use();
        }
        let pages = menu.pages.clone();
        let focus = menu.view.focus;
        let (view, layout) = self.in_use_build(&pages, focus);
        if let Some(menu) = &mut self.in_use_menu {
            if layout.size != menu.layout.size {
                menu.popup.resize(layout.size, &self.compositor);
            }
            menu.view = view;
            menu.layout = layout;
        }
        self.draw_in_use();
    }

    /// The pointer moved over the card: the button under it is held.
    pub(crate) fn in_use_motion(&mut self, x: f32, y: f32) {
        let Some(menu) = &mut self.in_use_menu else {
            return;
        };
        let held = in_use_menu::button_at(&menu.layout, x, y).map(|(i, _)| i);
        if held.is_some() && held != menu.view.focus {
            menu.view.focus = held;
            self.draw_in_use();
        }
    }

    /// A left press on the card at (`x`, `y`), card-local logical pixels:
    /// the button under it is pressed.
    pub(crate) fn in_use_press(&mut self, x: f32, y: f32) {
        let Some(menu) = &self.in_use_menu else {
            return;
        };
        let Some((_, button)) = in_use_menu::button_at(&menu.layout, x, y) else {
            return;
        };
        self.press_in_use(button);
    }

    /// A button was pressed: Stop or Settings for its row's app.
    fn press_in_use(&mut self, button: Button) {
        let Some(menu) = &self.in_use_menu else {
            return;
        };
        let Some(used) = menu.view.rows.get(button.row()).map(|row| row.used.clone()) else {
            return;
        };
        match button {
            Button::Stop(_) => self.stop_in_use(used),
            Button::Settings(_) => self.settings_in_use(used),
        }
    }

    /// Closes the card and asks PipeWire to destroy the client, on a thread
    /// of its own. It logs `in use stop APP client N` before and `in use
    /// stopped APP, exit N` after; the sign follows PipeWire's next batch.
    /// A row with no PipeWire client (the place, M6.14c) has no Stop, so
    /// nothing runs for it.
    fn stop_in_use(&mut self, used: InUse) {
        let Some(client) = used.client else {
            return;
        };
        self.close_in_use();
        let app = used.app;
        eprintln!("edel-shell-ui: in use stop {app} client {client}");
        let named = app.clone();
        let spawned = std::thread::Builder::new()
            .name("in-use-stop".into())
            .spawn(move || {
                let ran = Command::new("pw-cli")
                    .args(["destroy".to_string(), client.to_string()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                match ran {
                    Ok(status) => eprintln!(
                        "edel-shell-ui: in use stopped {named}, exit {}",
                        status.code().unwrap_or(-1)
                    ),
                    Err(e) => eprintln!("edel-shell-ui: could not stop {named}: {e}"),
                }
            });
        if let Err(e) = spawned {
            eprintln!("edel-shell-ui: could not stop {app}: {e}");
        }
    }

    /// Closes the card and opens the app's page in Settings, detached: a
    /// thread waits for Settings to end, so none is left a zombie.
    fn settings_in_use(&mut self, used: InUse) {
        self.close_in_use();
        let app = used.app;
        eprintln!("edel-shell-ui: in use settings for {app}");
        let ran = Command::new(crate::SETTINGS)
            .args(["--app".to_string(), app.clone()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match ran {
            Ok(mut child) => {
                let _ = std::thread::Builder::new()
                    .name("in-use-settings".into())
                    .spawn(move || {
                        let _ = child.wait();
                    });
            }
            Err(e) => eprintln!("edel-shell-ui: could not open Settings for {app}: {e}"),
        }
    }

    /// A key while the card is open: Escape closes, Return, Enter and space
    /// press the held button, Tab and Shift+Tab, Left and Up, Right and Down
    /// move the held button along.
    pub(crate) fn in_use_key(&mut self, event: KeyEvent) {
        let Some(menu) = &mut self.in_use_menu else {
            return;
        };
        let by = popup::tab_step(self.shift || event.keysym == Keysym::ISO_Left_Tab);
        let count = menu.layout.buttons.len();
        match event.keysym {
            Keysym::Escape => return self.close_in_use(),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                let held = menu
                    .view
                    .focus
                    .and_then(|f| menu.layout.buttons.get(f))
                    .map(|(button, _)| *button);
                if let Some(button) = held {
                    self.press_in_use(button);
                }
                return;
            }
            Keysym::Tab | Keysym::ISO_Left_Tab => {
                menu.view.focus = in_use_menu::moved(menu.view.focus, count, by);
            }
            Keysym::Right | Keysym::Down => {
                menu.view.focus = in_use_menu::moved(menu.view.focus, count, 1);
            }
            Keysym::Left | Keysym::Up => {
                menu.view.focus = in_use_menu::moved(menu.view.focus, count, -1);
            }
            _ => return,
        }
        self.draw_in_use();
    }

    /// Whether the point `x` logical pixels along panel `i` is on the sign.
    pub(crate) fn in_use_sign_at(&mut self, i: usize, x: f32) -> bool {
        matches!(
            self.action_at(i, x, Input::Click),
            Some((Action::InUse(_), ..))
        )
    }

    pub(crate) fn is_in_use(&self, surface: &wl_surface::WlSurface) -> bool {
        self.in_use_menu
            .as_ref()
            .is_some_and(|m| m.popup.is(surface))
    }
}
