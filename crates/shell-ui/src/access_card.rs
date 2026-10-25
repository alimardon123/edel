//! The shell's side of the Access dialog (M6.3): the surface, `edel-access`,
//! centred on the screen over everything with the keyboard exclusive, made
//! when a question comes in (`shell_bus.rs`'s `Edit::Access`) and let go when
//! it is answered. Questions queue: while one shows, the next waits its turn
//! and is shown when the one before is answered. What it looks like is
//! `access.rs`. Logs: `access asked for`, `access places`, `access allowed
//! for`, `access denied for` and `access cancelled for`, which CI reads.

use edel::i18n::tr;
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_surface::WlSurface};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;

use crate::access::{self, ALLOW, Ask, Button, CANCEL, DENY, View};
use crate::messages;
use crate::popup::Popup;
use crate::shell_bus::Answer;

/// How many questions wait behind the one on screen.
const MOST_WAITING: usize = 4;
use crate::{ACCESS, Shell, a11y, paint};

/// The dialog while it shows: its popup, what it shows, the question's
/// answer (taken when it is given), the keyboard and what a screen reader
/// reads.
pub struct AccessCard {
    pub popup: Popup<View>,
    pub view: View,
    app: String,
    answer: Option<Answer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    /// Whether its places were logged: once for each dialog shown.
    logged: bool,
    reader: a11y::Reader,
}

impl Shell {
    /// A question came in from the Access portal: shown now when no dialog
    /// is up, else queued behind the one showing.
    pub fn access_asked(&mut self, ask: Ask) {
        if self.asks.len() >= MOST_WAITING {
            eprintln!(
                "edel-shell-ui: {}",
                messages::access_too_many(&ask.app, MOST_WAITING)
            );
            ask.answer.send(CANCEL);
        } else if self.access.is_some() || self.lock.is_some() {
            self.asks.push_back(ask);
        } else {
            self.show_access(ask);
        }
    }

    /// The screen locks: a question on screen is cancelled, so it cannot
    /// take the keys typed on the lock screen.
    pub fn cancel_access(&mut self) {
        self.answer_access(CANCEL);
    }

    /// Shows `ask` as the dialog, with Deny focused. A dialog that cannot be
    /// drawn answers cancelled, so the app is not left waiting, and the next
    /// question shows instead.
    fn show_access(&mut self, ask: Ask) {
        let title = ask.shown_title();
        let view = View {
            title: title.clone(),
            subtitle: ask.subtitle.clone(),
            body: ask.body.clone(),
            deny: ask.deny.clone(),
            allow: ask.grant.clone(),
            focus: Button::Deny,
            hover: None,
            width: self.screen_size().0 as u32,
            room: paint::shadow_room(&self.tokens, !crate::fillets()),
        };
        let layout = access::layout(&view, &self.tokens);
        let (_, scale) = self.panel_for("status");
        let Some(popup) = Popup::new(self, ACCESS, layout.size, layout.size, scale, layout.room)
        else {
            eprintln!("edel-shell-ui: {}", messages::ACCESS_NOT_SHOWN);
            ask.answer.send(CANCEL);
            return self.show_next_access();
        };
        // The dialog takes the keyboard while it is up: the person answers it.
        popup
            .surface
            .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        popup.surface.commit();
        let keyboard = self.seat.seats().next().and_then(|seat| {
            self.seat
                .get_keyboard_with_repeat(
                    &self.qh,
                    &seat,
                    None,
                    self.handle.clone(),
                    Box::new(|shell: &mut Shell, _, event| shell.access_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for the access dialog: {e}"))
                .ok()
        });
        eprintln!("edel-shell-ui: access asked for {}: {title}", ask.app);
        self.access = Some(AccessCard {
            popup,
            view,
            app: ask.app,
            answer: Some(ask.answer),
            keyboard,
            logged: false,
            reader: a11y::Reader::new(tr("Allow this app?")),
        });
        self.draw_access();
    }

    /// Shows the next queued question, if any, unless one is up or the
    /// screen is locked.
    pub fn show_next_access(&mut self) {
        if self.access.is_some() || self.lock.is_some() {
            return;
        }
        if let Some(ask) = self.asks.pop_front() {
            self.show_access(ask);
        }
    }

    /// Whether `surface` is the open dialog's.
    pub fn is_access(&self, surface: &WlSurface) -> bool {
        self.access.as_ref().is_some_and(|c| c.popup.is(surface))
    }

    /// Ends the open dialog with `code` (`access::ALLOW`, `DENY` or `CANCEL`):
    /// the answer goes to the call that asked, the dialog is let go, and the
    /// next question shows.
    fn answer_access(&mut self, code: u32) {
        let Some(mut card) = self.access.take() else {
            return;
        };
        if let Some(keyboard) = &card.keyboard {
            keyboard.release();
        }
        let verb = match code {
            ALLOW => "allowed",
            DENY => "denied",
            _ => "cancelled",
        };
        eprintln!("edel-shell-ui: access {verb} for {}", card.app);
        if let Some(answer) = card.answer.take() {
            answer.send(code);
        }
        drop(card);
        self.show_next_access();
    }

    /// Draws the dialog if it may draw and what it shows changed. Its first
    /// drawing logs where its parts lie, which CI reads.
    pub fn draw_access(&mut self) {
        let glass = self.glass();
        let Some(card) = &mut self.access else {
            return;
        };
        let view = card.view.clone();
        let layout = access::layout(&view, &self.tokens);
        let Some(mut pixmap) = card.popup.canvas(&view) else {
            return;
        };
        let scale = card.popup.scale();
        access::paint(
            &mut pixmap,
            &view,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        card.popup
            .set_blur(&self.compositor, glass, self.tokens.radius_menu as f32);
        card.popup
            .show(view.clone(), &pixmap, &self.tokens, "dialog", &self.qh);
        let room = layout.room as f32;
        let surface = (
            layout.size.0 as f32 + 2.0 * room,
            layout.size.1 as f32 + 2.0 * room,
        );
        let items = access::nodes(&view, &layout);
        let focus = access::focus_item(&view, &layout);
        card.reader.update_focused(
            (f64::from(surface.0), f64::from(surface.1)),
            items,
            Some(focus),
        );
        if !card.logged {
            card.logged = true;
            // In the surface's own coordinates: the compositor centres it in
            // the area the panels leave, which only it knows.
            eprintln!("edel-shell-ui: access places {}", access::places(&layout));
        }
    }

    /// A key while the dialog is up: Escape denies, Return and space press
    /// the focused button, Tab and Shift+Tab move between the two, and Left
    /// and Right choose Deny and Allow.
    pub fn access_key(&mut self, event: KeyEvent) {
        let Some(card) = &mut self.access else {
            return;
        };
        match event.keysym {
            Keysym::Escape => return self.answer_access(DENY),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                let answer = match card.view.focus {
                    Button::Deny => DENY,
                    Button::Allow => ALLOW,
                };
                return self.answer_access(answer);
            }
            Keysym::Tab | Keysym::ISO_Left_Tab => {
                card.view.focus = access::other(card.view.focus);
            }
            Keysym::Left => card.view.focus = Button::Deny,
            Keysym::Right => card.view.focus = Button::Allow,
            _ => return,
        }
        self.draw_access();
    }

    /// The pointer on the dialog: it lights the button under it, and a left
    /// press on a button answers with it. Elsewhere on the card, nothing.
    pub fn access_pointer(&mut self, event: &PointerEvent) {
        let Some(card) = &mut self.access else {
            return;
        };
        let (x, y) = (event.position.0 as f32, event.position.1 as f32);
        let over = access::hit(&access::layout(&card.view, &self.tokens), x, y);
        match &event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                if card.view.hover != over {
                    card.view.hover = over;
                    self.draw_access();
                }
            }
            PointerEventKind::Leave { .. } => {
                if card.view.hover.take().is_some() {
                    self.draw_access();
                }
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => match over {
                Some(Button::Deny) => self.answer_access(DENY),
                Some(Button::Allow) => self.answer_access(ALLOW),
                None => {}
            },
            _ => {}
        }
    }

    /// The dialog's surface was closed by the compositor: the question ends
    /// cancelled.
    pub fn close_access(&mut self) {
        self.answer_access(CANCEL);
    }
}
