//! The lock screen's surfaces (M5.10): the shell's side of
//! ext-session-lock-v1. The compositor's `lock` event (Super+L, or a
//! session left locked by a shell-ui that ended) closes every popup and
//! asks to lock; once the compositor says the session is locked, every
//! screen gets a lock surface drawn by `lock.rs` and the keyboard goes to
//! it. Return checks the password on a thread (`lock::check`), whose
//! answer comes back over a channel: right unlocks and lets go of
//! everything, wrong says so after `lock::WRONG_WAIT`. A screen that comes
//! while locked gets its own surface. Caps Lock shows in the field, the
//! notifications that came unread are counted with their apps' icons, a
//! wrong password shakes the field on a timer (`lock::shake_at`, none with
//! `appearance.animations` other than full) and a click on the go button
//! checks the password as Return does (M5.10c). Logs `lock asked for`,
//! `screen locked, N screens`, `lock places ...` (`lock::places`, once per
//! screen size), `lock: wrong password` and `screen unlocked`.

use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::calloop::channel;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::client::protocol::{
    wl_keyboard, wl_output, wl_shm, wl_surface,
};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::session_lock::{
    SessionLock, SessionLockHandler, SessionLockSurface, SessionLockSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use tiny_skia::Pixmap;

use crate::Shell;
use crate::lock::{self, Checked, Field, Password, View};
use crate::messages;
use crate::paint;

/// The lock while it holds.
pub struct LockCard {
    session: SessionLock,
    screens: Vec<Screen>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    password: Password,
    field: Field,
    /// How many letters went to the helper, while it checks: the field
    /// keeps their dots.
    checking: Option<usize>,
    user: String,
    /// Whether Caps Lock is on, as the keyboard last said.
    caps: bool,
    /// When the field's shake started, while it shakes.
    shaking: Option<Instant>,
    /// Whether the pointer is over a go button, and the places line last
    /// logged.
    go_hover: bool,
    logged: String,
}

/// One screen's lock surface.
struct Screen {
    surface: SessionLockSurface,
    output: wl_output::WlOutput,
    /// Its size in logical pixels once configured, its scale and buffers.
    size: Option<(u32, u32)>,
    scale: u32,
    pool: Option<SlotPool>,
    drawn: Option<View>,
}

impl Shell {
    /// The `lock` event: every popup closes and the compositor is asked to
    /// lock. Nothing happens while a lock is already asked for or held.
    pub fn lock_screen(&mut self) {
        if self.lock.is_some() {
            return;
        }
        self.close_popups();
        match self.session_lock.lock(&self.qh) {
            Ok(session) => {
                eprintln!("edel-shell-ui: lock asked for");
                self.lock = Some(LockCard {
                    session,
                    screens: Vec::new(),
                    keyboard: None,
                    password: Password::default(),
                    field: if lock::checker().is_some() {
                        Field::Ready
                    } else {
                        Field::NoChecker
                    },
                    checking: None,
                    user: lock::user(),
                    caps: false,
                    shaking: None,
                    go_hover: false,
                    logged: String::new(),
                });
            }
            Err(e) => eprintln!("edel-shell-ui: {}", messages::lock_not_offered(e)),
        }
    }

    /// A lock surface on `output`, configured later.
    fn add_lock_screen(&mut self, output: wl_output::WlOutput) {
        let Some(card) = &mut self.lock else {
            return;
        };
        if card.screens.iter().any(|s| s.output == output) {
            return;
        }
        let surface = self.compositor.create_surface(&self.qh);
        let surface = card.session.create_lock_surface(surface, &output, &self.qh);
        let scale = self
            .outputs
            .info(&output)
            .map_or(1, |info| info.scale_factor.max(1) as u32);
        card.screens.push(Screen {
            surface,
            output,
            size: None,
            scale,
            pool: None,
            drawn: None,
        });
    }

    /// A screen went: its lock surface goes with it.
    pub fn lock_output_gone(&mut self, output: &wl_output::WlOutput) {
        if let Some(card) = &mut self.lock {
            card.screens.retain(|s| &s.output != output);
        }
    }

    /// A screen came while locked: it gets its own lock surface.
    pub fn lock_output_new(&mut self, output: wl_output::WlOutput) {
        if self.lock.as_ref().is_some_and(|c| c.session.is_locked()) {
            self.add_lock_screen(output);
        }
    }

    /// What every screen shows now.
    fn lock_view(&self) -> Option<View> {
        let card = self.lock.as_ref()?;
        let (time, date) = lock::time_and_date(&jiff::Zoned::now());
        let (field, dots) = match card.checking {
            Some(dots) => (Field::Checking, dots),
            None => (card.field, card.password.letters()),
        };
        let shake = card
            .shaking
            .map_or(0, |at| lock::shake_at(at.elapsed().as_millis() as u32));
        Some(View {
            dots,
            field,
            caps: card.caps,
            notices: self.live.unread,
            apps: lock::unread_apps(self.notifications.items(), self.live.unread),
            shake,
            go_hover: card.go_hover,
            ..View::new(time, date, card.user.clone())
        })
    }

    /// Draws every configured lock surface whose view changed: on a key,
    /// an answer and the minute.
    pub fn draw_lock(&mut self) {
        let Some(view) = self.lock_view() else {
            return;
        };
        let Some(card) = &mut self.lock else {
            return;
        };
        for screen in &mut card.screens {
            let Some((w, h)) = screen.size else {
                continue;
            };
            if screen.drawn.as_ref() == Some(&view) {
                continue;
            }
            let s = screen.scale;
            let (pw, ph) = (w * s, h * s);
            let Some(mut pixmap) = Pixmap::new(pw, ph) else {
                continue;
            };
            lock::paint(
                &mut pixmap,
                &view,
                &self.tokens,
                &mut self.text,
                Some(&mut self.icons),
                s as f32,
            );
            let places = lock::places(&lock::layout(
                w as f32,
                h as f32,
                &self.tokens,
                view.notices > 0,
            ));
            if places != card.logged {
                eprintln!("edel-shell-ui: lock places {places}");
                card.logged = places;
            }
            let bytes = (pw * ph * 4) as usize;
            if screen.pool.is_none() {
                // Two buffers' room, as each drawing replaces the last.
                screen.pool = SlotPool::new(2 * bytes, &self.shm)
                    .inspect_err(|e| eprintln!("edel-shell-ui: no memory for the lock screen: {e}"))
                    .ok();
            }
            let Some(pool) = &mut screen.pool else {
                continue;
            };
            let (buffer, canvas) = match pool.create_buffer(
                pw as i32,
                ph as i32,
                pw as i32 * 4,
                wl_shm::Format::Argb8888,
            ) {
                Ok(made) => made,
                Err(e) => {
                    eprintln!("edel-shell-ui: drawing the lock screen failed: {e}");
                    continue;
                }
            };
            paint::to_argb(&pixmap, canvas);
            let surface = screen.surface.wl_surface();
            surface.set_buffer_scale(s as i32);
            surface.damage_buffer(0, 0, pw as i32, ph as i32);
            if let Err(e) = buffer.attach_to(surface) {
                eprintln!("edel-shell-ui: drawing the lock screen failed: {e}");
                continue;
            }
            surface.commit();
            screen.drawn = Some(view.clone());
        }
    }

    /// A key while locked: Return checks the password, Backspace takes a
    /// letter back, Escape empties the field, and text is typed.
    pub fn lock_key(&mut self, event: KeyEvent) {
        let Some(card) = &mut self.lock else {
            return;
        };
        if card.checking.is_some() {
            return;
        }
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter => return self.check_password(),
            Keysym::BackSpace => card.password.pop(),
            Keysym::Escape => card.password.clear(),
            _ => {
                if let Some(text) = event
                    .utf8
                    .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                {
                    card.password.push(&text);
                }
            }
        }
        if card.field == Field::Wrong {
            card.field = Field::Ready;
        }
        self.draw_lock();
    }

    /// Return: the password goes to the helper on a thread of its own.
    fn check_password(&mut self) {
        let Some(card) = &mut self.lock else {
            return;
        };
        if card.password.letters() == 0 {
            return;
        }
        let Some(helper) = lock::checker() else {
            card.field = Field::NoChecker;
            eprintln!("edel-shell-ui: {}", messages::NO_PASSWORD_CHECKER);
            return self.draw_lock();
        };
        card.checking = Some(card.password.letters());
        eprintln!(
            "edel-shell-ui: lock: checking a password of {} letters",
            card.password.letters()
        );
        let password = card.password.take();
        let user = card.user.clone();
        let tx = self.lock_tx.clone();
        let spawned = std::thread::Builder::new()
            .name("lock".into())
            .spawn(move || {
                let _ = tx.send(lock::check(&helper, &user, password));
            });
        if let Err(e) = spawned {
            card.checking = None;
            eprintln!("edel-shell-ui: {}", messages::password_not_checked(e));
        }
        self.draw_lock();
    }

    /// The helper's answer.
    pub fn password_checked(&mut self, checked: Checked) {
        let Some(card) = &mut self.lock else {
            return;
        };
        card.checking = None;
        match checked {
            Checked::Right => return self.unlock_screen(),
            Checked::Wrong => {
                card.field = Field::Wrong;
                eprintln!("edel-shell-ui: lock: wrong password");
                return self.shake_field();
            }
            Checked::NoChecker(why) => {
                card.field = Field::NoChecker;
                eprintln!("edel-shell-ui: {}", messages::password_not_checked(why));
            }
        }
        self.draw_lock();
    }

    /// The right password: the compositor is told, and the surfaces, the
    /// keyboard and the buffers are let go.
    fn unlock_screen(&mut self) {
        let Some(card) = self.lock.take() else {
            return;
        };
        if let Some(keyboard) = &card.keyboard {
            keyboard.release();
        }
        card.session.unlock();
        drop(card);
        eprintln!("edel-shell-ui: screen unlocked");
        self.draw_all();
    }
}

impl Shell {
    /// Caps Lock went on or off: the field says so.
    pub fn lock_caps(&mut self, on: bool) {
        if let Some(card) = &mut self.lock
            && card.caps != on
        {
            card.caps = on;
            self.draw_lock();
        }
    }

    /// Whether `surface` is one of the lock's.
    pub fn is_lock(&self, surface: &wl_surface::WlSurface) -> bool {
        self.lock
            .as_ref()
            .is_some_and(|c| c.screens.iter().any(|s| s.surface.wl_surface() == surface))
    }

    /// The pointer on a lock surface: over the go button it lights it, and
    /// a click there checks the password, as Return does.
    pub fn lock_pointer(&mut self, event: &PointerEvent) {
        let Some(card) = &mut self.lock else {
            return;
        };
        let Some((w, h)) = card
            .screens
            .iter()
            .find(|s| s.surface.wl_surface() == &event.surface)
            .and_then(|s| s.size)
        else {
            return;
        };
        let at = lock::layout(w as f32, h as f32, &self.tokens, self.live.unread > 0);
        let (x, y) = (event.position.0 as f32, event.position.1 as f32);
        let over = lock::on_go(&at, x, y);
        match event.kind {
            PointerEventKind::Press { button, .. } if button == BTN_LEFT && over => {
                if card.checking.is_none() {
                    self.check_password();
                }
            }
            PointerEventKind::Leave { .. } if card.go_hover => {
                card.go_hover = false;
                self.draw_lock();
            }
            PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. }
                if card.go_hover != over =>
            {
                card.go_hover = over;
                self.draw_lock();
            }
            _ => {}
        }
    }

    /// A wrong password shakes the field, drawn on a timer until the shake
    /// is done, unless `appearance.animations` asks for less motion.
    fn shake_field(&mut self) {
        let (machine, person) = crate::settings_texts();
        let motion = edel::settings::chosen(
            edel::settings::ANIMATIONS,
            machine.as_deref(),
            person.as_deref(),
        );
        let still = motion.is_some_and(|m| m != "full");
        let Some(card) = &mut self.lock else {
            return;
        };
        if still {
            card.shaking = None;
            return self.draw_lock();
        }
        card.shaking = Some(Instant::now());
        self.draw_lock();
        let added = self.handle.insert_source(
            Timer::from_duration(Duration::from_millis(16)),
            |_, _, shell: &mut Shell| {
                let Some(card) = &mut shell.lock else {
                    return TimeoutAction::Drop;
                };
                let done = card
                    .shaking
                    .is_none_or(|at| at.elapsed().as_millis() as u32 >= lock::SHAKE_MS);
                if done {
                    card.shaking = None;
                }
                shell.draw_lock();
                if done {
                    TimeoutAction::Drop
                } else {
                    TimeoutAction::ToDuration(Duration::from_millis(16))
                }
            },
        );
        if let Err(e) = added {
            eprintln!("edel-shell-ui: no timer for the lock screen's shake: {e}");
            if let Some(card) = &mut self.lock {
                card.shaking = None;
            }
        }
    }
}

impl SessionLockHandler for Shell {
    /// Locked: a surface on every screen, and the keyboard.
    fn locked(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: SessionLock) {
        let outputs: Vec<wl_output::WlOutput> = self.outputs.outputs().collect();
        for output in outputs {
            self.add_lock_screen(output);
        }
        let keyboard = self.seat.seats().next().and_then(|seat| {
            self.seat
                .get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    self.handle.clone(),
                    Box::new(|shell: &mut Shell, _, event| shell.lock_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for the lock screen: {e}"))
                .ok()
        });
        if let Some(card) = &mut self.lock {
            card.keyboard = keyboard;
            eprintln!(
                "edel-shell-ui: screen locked, {} screens",
                card.screens.len()
            );
        }
    }

    /// The compositor refused the lock, or ended it.
    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, session: SessionLock) {
        if let Some(card) = self.lock.take() {
            if let Some(keyboard) = &card.keyboard {
                keyboard.release();
            }
            if !session.is_locked() {
                eprintln!("edel-shell-ui: {}", messages::LOCK_REFUSED);
            }
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        let Some(card) = &mut self.lock else {
            return;
        };
        if let Some(screen) = card
            .screens
            .iter_mut()
            .find(|s| s.surface.wl_surface() == surface.wl_surface())
        {
            let size = configure.new_size;
            if screen.size != Some(size) {
                screen.size = Some(size);
                screen.drawn = None;
                screen.pool = None;
            }
        }
        self.draw_lock();
    }
}

/// The channel the password's answer comes back on, inserted on `handle`.
pub fn answers(
    handle: &smithay_client_toolkit::reexports::calloop::LoopHandle<'static, Shell>,
) -> channel::Sender<Checked> {
    let (tx, rx) = channel::channel::<Checked>();
    let inserted = handle.insert_source(rx, |event, _, shell: &mut Shell| {
        if let channel::Event::Msg(checked) = event {
            shell.password_checked(checked);
        }
    });
    if let Err(e) = inserted {
        eprintln!("edel-shell-ui: {}", messages::password_not_checked(e));
    }
    tx
}
