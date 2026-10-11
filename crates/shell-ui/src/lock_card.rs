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
//! checks the password as Return does (M5.10c). The bottom right corner
//! (`corner.rs`) holds Power, whose menu runs its choice through `power.rs`,
//! and the keyboard layout; it takes the pointer, and Escape closes the menu
//! first. The player card (M5.10c4, `player.rs`) shows in the bottom left
//! corner once something plays while locked, stays until unlock and redraws
//! each second while it plays; its close button hides it while nothing
//! plays. Logs `lock asked for`, `screen locked, N screens`, `lock places ...`
//! (`lock::places`, the player's card and play button when shown, and
//! `corner::places`, once per screen size), `lock: wrong password`,
//! `power menu shown, 3 rows`, `power menu hidden`, `power: NAME`,
//! `lock player shown, TITLE by ARTIST`, `lock player hidden` and
//! `screen unlocked`.

use std::time::{Duration, Instant};

use edel::tokens::Tokens;
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
use crate::corner::{self, Button};
use crate::lock::{self, Checked, Field, Password, View};
use crate::messages;
use crate::mpris;
use crate::paint::{self, Text, fill};
use crate::player::{self, PlayerView};
use crate::popup::{Rect, icon_in};
use crate::power;
use crate::quick_card;
use crate::widgets::keyboard;

/// The room between the player card and what lies over it on a Compact
/// screen, the clock or the count of notifications (M5.10c4).
const CARD_GAP: f32 = 16.0;
/// The close button (M5.10c4): a disc this side, 8 px in from the card's
/// top right corner, its glyph this side.
const CLOSE: f32 = 20.0;
const CLOSE_IN: f32 = 8.0;
const CLOSE_GLYPH: f32 = 10.0;
/// How often the card redraws while what it shows plays, so its bar moves.
const TICK: Duration = Duration::from_secs(1);

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
    /// Power's menu is open, and whether an update waits for a restart
    /// (read when the menu opens).
    menu: bool,
    restart_waiting: bool,
    /// The corner's button or row under the pointer.
    corner_hover: Option<corner::Hit>,
    /// The player card (M5.10c4). `_follow` follows the players while the
    /// lock holds (held only for its drop). `page` is the player shown, kept
    /// to the `pages` there are; `player` is the one the card holds, frozen
    /// with no previous or next once `gone` (every player has left). `seen`
    /// is set by the first play and keeps the card until unlock; `hidden` is
    /// the close button's, undone by the next play; `ticking` is the 1 s
    /// redraw's timer running. `cover` is the held player's picture, read
    /// once per file. `player_at` is what the pointer is on.
    _follow: Option<mpris::Follow>,
    page: usize,
    pages: usize,
    player: Option<mpris::Player>,
    gone: bool,
    seen: bool,
    hidden: bool,
    ticking: bool,
    cover: Option<(String, Option<Pixmap>)>,
    player_at: PlayerAt,
}

/// What the pointer is on, of the player card (M5.10c4): nothing of it, its
/// plain area or one of its parts, or the close button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerAt {
    Away,
    Card(Option<player::Part>),
    Close,
}

/// One screen's lock surface.
struct Screen {
    surface: SessionLockSurface,
    output: wl_output::WlOutput,
    /// Its size in logical pixels once configured, its scale and buffers.
    size: Option<(u32, u32)>,
    scale: u32,
    pool: Option<SlotPool>,
    /// What was drawn last, to skip a drawing that would look the same.
    drawn: Option<Shown>,
}

/// What a screen's drawing shows: the lock's view, the corner's and the
/// player card's, when it shows.
#[derive(Clone, PartialEq)]
struct Shown {
    lock: View,
    corner: corner::View,
    player: Option<PlayerDraw>,
}

/// The player card as the lock draws it now (M5.10c4).
#[derive(Clone, PartialEq)]
struct PlayerDraw {
    view: PlayerView,
    pages: usize,
    page: usize,
    /// The part under the pointer, and whether the pointer is over the card
    /// at all (the close button shows over it while nothing plays).
    hover: Option<player::Part>,
    over: bool,
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
                let follow = self
                    ._portal
                    .as_ref()
                    .zip(self.player_tx.clone())
                    .map(|(connection, events)| mpris::follow(connection, events));
                self.lock = Some(LockCard {
                    _follow: follow,
                    page: 0,
                    pages: 0,
                    player: None,
                    gone: false,
                    seen: false,
                    hidden: false,
                    ticking: false,
                    cover: None,
                    player_at: PlayerAt::Away,
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
                    menu: false,
                    restart_waiting: false,
                    corner_hover: None,
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

    /// What the lock's corner shows now: the short name of the layout in
    /// use when there are two or more, and Power's menu. `None` without a
    /// lock.
    fn corner_view(&self) -> Option<corner::View> {
        let card = self.lock.as_ref()?;
        let (layouts, active) = &self.live.keyboard;
        let layout = if layouts.len() >= 2 {
            layouts.get(*active).map(|name| keyboard::short(name))
        } else {
            None
        };
        Some(corner::View {
            layout,
            menu: card.menu,
            restart_waiting: card.restart_waiting,
            hover: card.corner_hover,
        })
    }

    /// Draws every configured lock surface whose view changed: on a key,
    /// an answer, the minute, a player's news and the card's second.
    pub fn draw_lock(&mut self) {
        let Some(view) = self.lock_view() else {
            return;
        };
        let Some(corner_now) = self.corner_view() else {
            return;
        };
        let mut player = self.lock.as_mut().and_then(|card| card.player_draw());
        // The app's icon, at the largest screen's scale; the card draws it
        // smaller on the others.
        if let (Some(draw), Some(card)) = (&mut player, &self.lock) {
            let scale = card.screens.iter().map(|sc| sc.scale).max().unwrap_or(1);
            if let Some(held) = card.player.clone() {
                let px = (player::APP_ICON * scale as f32).round() as u32;
                draw.view.app_icon =
                    quick_card::player_icon(&held, &self.live.installed, &mut self.icons, px);
            }
        }
        let shown = Shown {
            lock: view,
            corner: corner_now,
            player,
        };
        let Some(card) = &mut self.lock else {
            return;
        };
        for screen in &mut card.screens {
            let Some((w, h)) = screen.size else {
                continue;
            };
            if screen.drawn.as_ref() == Some(&shown) {
                continue;
            }
            let s = screen.scale;
            let (pw, ph) = (w * s, h * s);
            let Some(mut pixmap) = Pixmap::new(pw, ph) else {
                continue;
            };
            lock::paint(
                &mut pixmap,
                &shown.lock,
                &self.tokens,
                &mut self.text,
                Some(&mut self.icons),
                s as f32,
            );
            let (fw, fh) = (w as f32, h as f32);
            // The player card lies over the lock and under the corner.
            let mut parts = None;
            if let Some(draw) = &shown.player {
                parts = Some(paint_player(
                    &mut pixmap,
                    draw,
                    player_card(fw, fh, draw.pages, &self.tokens, self.live.unread > 0),
                    fw < lock::COMPACT_BELOW,
                    s as f32,
                    &self.tokens,
                    &mut self.text,
                ));
            }
            let corner_at = corner::layout(fw, fh, &shown.corner);
            corner::paint(
                &mut pixmap,
                &shown.corner,
                &corner_at,
                &self.tokens,
                &mut self.text,
                s as f32,
            );
            let lock_places =
                lock::places(&lock::layout(fw, fh, &self.tokens, shown.lock.notices > 0));
            let player_places = parts.map_or(String::new(), |p: player::Parts| {
                format!(", player {}, play {}", place(p.card), place(p.play))
            });
            let places = format!(
                "{lock_places}{player_places}, {}",
                corner::places(&corner_at)
            );
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
            screen.drawn = Some(shown.clone());
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
        if card.menu && matches!(event.keysym, Keysym::Escape) {
            return self.set_power_menu(false);
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
        self.live.players.clear();
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

    /// Opens or closes Power's menu, logging it, and draws again. Opening
    /// reads whether an update waits for a restart.
    fn set_power_menu(&mut self, open: bool) {
        let Some(card) = &mut self.lock else {
            return;
        };
        if open {
            card.restart_waiting = power::restart_waiting();
            card.menu = true;
            eprintln!(
                "edel-shell-ui: power menu shown, {} rows",
                power::CHOICES.len()
            );
        } else {
            card.menu = false;
            eprintln!("edel-shell-ui: power menu hidden");
        }
        self.draw_lock();
    }

    /// Closes Power's menu, then runs `choice` on a thread of its own, so
    /// the drawing never waits for it.
    fn run_power(&mut self, choice: power::Choice) {
        self.set_power_menu(false);
        eprintln!("edel-shell-ui: power: {}", choice.name());
        let spawned = std::thread::Builder::new()
            .name("power".into())
            .spawn(move || {
                if let Err(e) = power::run(choice) {
                    eprintln!("edel-shell-ui: {e}");
                }
            });
        if let Err(e) = spawned {
            eprintln!(
                "edel-shell-ui: {}",
                messages::power_failed(choice.name(), e)
            );
        }
    }

    /// The pointer on a lock surface. Over the corner, a button lights up
    /// and a click acts: Power opens or closes its menu, the layout button
    /// goes to the next layout, a row runs its choice. A click elsewhere
    /// closes an open menu. Over the go button, the pointer lights it and
    /// a click checks the password, as Return does.
    pub fn lock_pointer(&mut self, event: &PointerEvent) {
        let Some(corner_now) = self.corner_view() else {
            return;
        };
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
        let (x, y) = (event.position.0 as f32, event.position.1 as f32);
        let at = lock::layout(w as f32, h as f32, &self.tokens, self.live.unread > 0);
        let over = lock::on_go(&at, x, y);
        let corner_at = corner::layout(w as f32, h as f32, &corner_now);
        let hit = corner::hit(&corner_at, x, y);
        let player_now = card.player_under(
            (w as f32, h as f32),
            &self.tokens,
            self.live.unread > 0,
            (x, y),
        );
        match event.kind {
            PointerEventKind::Press { button, .. }
                if button == BTN_LEFT && player_now != PlayerAt::Away =>
            {
                self.player_press(player_now)
            }
            PointerEventKind::Press { button, .. } if button == BTN_LEFT => match hit {
                Some(corner::Hit::Button(Button::Power)) => {
                    let open = !card.menu;
                    self.set_power_menu(open);
                }
                Some(corner::Hit::Button(Button::Layout)) => {
                    if card.menu {
                        self.set_power_menu(false);
                    }
                    self.link.next_keyboard_layout();
                    self.draw_lock();
                }
                Some(corner::Hit::Row(i)) => {
                    if let Some(&choice) = power::CHOICES.get(i) {
                        self.run_power(choice);
                    }
                }
                None if card.menu => self.set_power_menu(false),
                None if over && card.checking.is_none() => self.check_password(),
                None => {}
            },
            PointerEventKind::Leave { .. } => {
                let changed = card.go_hover
                    || card.corner_hover.is_some()
                    || card.player_at != PlayerAt::Away;
                card.go_hover = false;
                card.corner_hover = None;
                card.player_at = PlayerAt::Away;
                if changed {
                    self.draw_lock();
                }
            }
            PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. }
                if card.go_hover != over
                    || card.corner_hover != hit
                    || card.player_at != player_now =>
            {
                card.go_hover = over;
                card.corner_hover = hit;
                card.player_at = player_now;
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

/// The player card's place on a screen `w` by `h` logical pixels (M5.10c4):
/// the wide pill, 24 px in from the left and the bottom, in the room
/// `lock::layout` keeps free there. On a Compact screen it
/// is as wide as the screen less 48 px and lies `CARD_GAP` under the clock,
/// or under the count of notifications while one shows, as
/// `docs/mockups/shell/lock-phone.jpg` draws it.
fn player_card(w: f32, h: f32, pages: usize, tokens: &Tokens, notices: bool) -> Rect {
    let (wide, high) = player::wide_size(pages);
    if w >= lock::COMPACT_BELOW {
        return Rect::new(corner::EDGE, h - corner::EDGE - high, wide, high);
    }
    let at = lock::layout(w, h, tokens, notices);
    let top = at
        .count
        .map_or(at.clock + at.clock_size * 1.2, |c| c + 36.0)
        + CARD_GAP;
    Rect::new(corner::EDGE, top, (w - 2.0 * corner::EDGE).max(0.0), high)
}

/// The close button's place on a card at `card`: its top right corner.
fn close_rect(card: Rect) -> Rect {
    Rect::new(
        card.right() - CLOSE_IN - CLOSE,
        card.y + CLOSE_IN,
        CLOSE,
        CLOSE,
    )
}

/// What the card shows of `held` at `now`, with its cover when it has one;
/// `draw_lock` adds the app's icon.
fn view_of(held: &mpris::Player, cover: Option<Pixmap>, now: Instant) -> PlayerView {
    PlayerView {
        title: held.title.clone(),
        artist: held.artist.clone(),
        identity: held.identity.clone(),
        playing: held.playing,
        can_previous: held.can_previous,
        can_next: held.can_next,
        cover,
        app_icon: None,
        position: mpris::position_now(held, now),
        length: held.length,
    }
}

/// `X+Y+WxH`, whole logical pixels, as the places lines write them.
fn place(r: Rect) -> String {
    format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h)
}

/// Draws the player card at `card` on a screen at scale `s`: its shadow and
/// glass, as the field's (`docs/mockups/shell/lock-media.jpg`), then its
/// parts, then the close button while nothing plays and the pointer is over
/// the card. Returns the parts, which the places line names.
fn paint_player(
    pixmap: &mut Pixmap,
    draw: &PlayerDraw,
    card: Rect,
    compact: bool,
    s: f32,
    tokens: &Tokens,
    text: &mut Text,
) -> player::Parts {
    let (x, y, w, h) = card.device(s);
    let r = h / 2.0;
    lock::lift(pixmap, card, card.h / 2.0, s, tokens);
    lock::glass(pixmap, (x, y, w, h), r, s, tokens);
    let parts = player::place_wide(card, compact, draw.pages, draw.page);
    let marks = player::Marks {
        hover: draw.hover,
        focus: None,
        page: draw.page,
    };
    player::paint(pixmap, &draw.view, &parts, marks, tokens, Some(text), s);
    if draw.over && !draw.view.playing {
        let close = close_rect(card);
        let (cx, cy, cw, ch) = close.device(s);
        fill(pixmap, cx, cy, cw, ch, cw / 2.0, tokens.lock_glass);
        icon_in(pixmap, "close", CLOSE_GLYPH, close, s, tokens.backdrop_text);
    }
    parts
}

impl LockCard {
    /// Whether the card shows: something has played, and its close button
    /// has not hidden it since.
    fn shows(&self) -> bool {
        self.seen && !self.hidden && self.player.is_some()
    }

    /// The card's parts on a screen `w` by `h`, while it shows.
    fn player_parts(
        &self,
        w: f32,
        h: f32,
        tokens: &Tokens,
        notices: bool,
    ) -> Option<player::Parts> {
        self.shows().then(|| {
            player::place_wide(
                player_card(w, h, self.pages, tokens, notices),
                w < lock::COMPACT_BELOW,
                self.pages,
                self.page,
            )
        })
    }

    /// What the pointer at (`x`, `y`) is on, on a screen `w` by `h`.
    fn player_under(
        &self,
        (w, h): (f32, f32),
        tokens: &Tokens,
        notices: bool,
        (x, y): (f32, f32),
    ) -> PlayerAt {
        let Some(parts) = self.player_parts(w, h, tokens, notices) else {
            return PlayerAt::Away;
        };
        if !parts.card.contains(x, y) {
            return PlayerAt::Away;
        }
        let idle = self.player.as_ref().is_some_and(|p| !p.playing);
        if idle && close_rect(parts.card).contains(x, y) {
            return PlayerAt::Close;
        }
        let Some(held) = &self.player else {
            return PlayerAt::Away;
        };
        let view = view_of(held, None, Instant::now());
        PlayerAt::Card(player::hit(&view, &parts, x, y))
    }

    /// The card as the lock draws it now, while it shows.
    fn player_draw(&mut self) -> Option<PlayerDraw> {
        if !self.shows() {
            return None;
        }
        let cover = self.cover();
        let held = self.player.as_ref()?;
        Some(PlayerDraw {
            view: view_of(held, cover, Instant::now()),
            pages: self.pages,
            page: self.page,
            hover: match self.player_at {
                PlayerAt::Card(part) => part,
                _ => None,
            },
            over: self.player_at != PlayerAt::Away,
        })
    }

    /// The held player's cover as a picture, read from its file once and
    /// kept (as quick settings keeps its own). None when it has none or the
    /// file does not load, and the card then shows the app's initial.
    fn cover(&mut self) -> Option<Pixmap> {
        let Some(path) = self.player.as_ref().and_then(|p| p.cover.clone()) else {
            self.cover = None;
            return None;
        };
        if self.cover.as_ref().is_none_or(|(held, _)| *held != path) {
            let picture = quick_card::load_cover(&path);
            self.cover = Some((path, picture));
        }
        self.cover.as_ref().and_then(|(_, picture)| picture.clone())
    }
}

impl Shell {
    /// What plays changed while the screen is locked (M5.10c4, from
    /// `player_changed`). The page keeps its index, clamped to the players.
    /// The card holds the player on its page, or, once all have gone, the
    /// last one, frozen with no previous or next. The first play shows the
    /// card and says what plays; a play after a close button's hiding shows
    /// it again.
    pub fn lock_players_changed(&mut self) {
        let players = self.live.players.clone();
        let Some(card) = &mut self.lock else {
            return;
        };
        card.pages = players.len();
        card.page = card.page.min(players.len().saturating_sub(1));
        card.gone = players.is_empty();
        match players.get(card.page) {
            Some(player) => card.player = Some(player.clone()),
            None => {
                if let Some(held) = &mut card.player {
                    held.playing = false;
                    held.can_previous = false;
                    held.can_next = false;
                }
            }
        }
        let mut shown = None;
        if players.iter().any(|p| p.playing) {
            card.hidden = false;
            if !card.seen {
                card.seen = true;
                shown = card.player.as_ref().map(|p| {
                    format!(
                        "edel-shell-ui: lock player shown, {} by {}",
                        p.title, p.artist
                    )
                });
            }
        }
        let start =
            card.shows() && card.player.as_ref().is_some_and(|p| p.playing) && !card.ticking;
        if let Some(line) = shown {
            eprintln!("{line}");
        }
        self.draw_lock();
        if start {
            self.start_player_timer();
        }
    }

    /// Starts the card's redraw each second, on the loop as the shake's
    /// timer is, for as long as what it shows plays.
    fn start_player_timer(&mut self) {
        let added =
            self.handle
                .insert_source(Timer::from_duration(TICK), |_, _, shell: &mut Shell| {
                    let Some(card) = &mut shell.lock else {
                        return TimeoutAction::Drop;
                    };
                    let plays = card.shows() && card.player.as_ref().is_some_and(|p| p.playing);
                    if !plays {
                        card.ticking = false;
                        return TimeoutAction::Drop;
                    }
                    shell.draw_lock();
                    TimeoutAction::ToDuration(TICK)
                });
        match added {
            Ok(_) => {
                if let Some(card) = &mut self.lock {
                    card.ticking = true;
                }
            }
            Err(e) => eprintln!("edel-shell-ui: no timer for the lock's player: {e}"),
        }
    }

    /// A left press on the player card (M5.10c4): the close button hides the
    /// card until something plays again; a dot turns to its page; Previous,
    /// Play or Next asks the shown player over MPRIS, as quick settings does,
    /// and none of them after every player has gone.
    fn player_press(&mut self, at: PlayerAt) {
        match at {
            PlayerAt::Away | PlayerAt::Card(None) => {}
            PlayerAt::Close => {
                if let Some(card) = &mut self.lock {
                    card.hidden = true;
                    card.player_at = PlayerAt::Away;
                }
                eprintln!("edel-shell-ui: lock player hidden");
                self.draw_lock();
            }
            PlayerAt::Card(Some(part)) => {
                let Some(card) = &mut self.lock else {
                    return;
                };
                let method = match part {
                    player::Part::Dot(page) => {
                        card.page = page.min(card.pages.saturating_sub(1));
                        return self.draw_lock();
                    }
                    _ if card.gone => return,
                    player::Part::Previous => "Previous",
                    player::Part::PlayPause => "PlayPause",
                    player::Part::Next => "Next",
                };
                let Some(bus) = card.player.as_ref().map(|p| p.bus.clone()) else {
                    return;
                };
                if let Some(connection) = &self._portal {
                    mpris::call(connection, &bus, method);
                }
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
            self.live.players.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The desktop, a smaller desktop and a phone, as the lock is checked on.
    const SCREENS: [(f32, f32); 3] = [(1280.0, 800.0), (1024.0, 768.0), (360.0, 740.0)];

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.y + b.h && b.y < a.y + a.h
    }

    /// The corner with Power only, as a lock with one layout shows it.
    fn corner_view(layouts: bool) -> corner::View {
        corner::View {
            layout: layouts.then(|| "EN".to_string()),
            menu: false,
            restart_waiting: false,
            hover: None,
        }
    }

    #[test]
    fn the_card_lies_inside_every_screen_checked() {
        for (w, h) in SCREENS {
            let card = player_card(w, h, 2, &Tokens::built_in(), true);
            assert!(
                card.x >= 0.0 && card.y >= 0.0 && card.right() <= w && card.y + card.h <= h,
                "{card:?} on {w}x{h}"
            );
        }
    }

    #[test]
    fn on_a_desktop_the_wide_card_sits_24_px_from_the_bottom_left() {
        assert_eq!(
            player_card(1280.0, 800.0, 1, &Tokens::built_in(), true),
            Rect::new(24.0, 676.0, 400.0, 100.0)
        );
        assert_eq!(
            player_card(1024.0, 768.0, 1, &Tokens::built_in(), true),
            Rect::new(24.0, 644.0, 400.0, 100.0)
        );
    }

    #[test]
    fn on_a_compact_screen_the_card_is_the_width_less_48_under_the_clock() {
        let tokens = Tokens::built_in();
        let at = lock::layout(360.0, 740.0, &tokens, true);
        let card = player_card(360.0, 740.0, 2, &tokens, true);
        assert_eq!((card.x, card.w), (24.0, 312.0));
        assert!(card.y > at.clock, "{card:?} is over the clock");
        assert!(
            card.y + card.h <= at.avatar.1 - at.avatar.2,
            "{card:?} meets the person"
        );
    }

    #[test]
    fn the_card_does_not_meet_the_corner_on_any_screen_checked() {
        for (w, h) in SCREENS {
            let card = player_card(w, h, 2, &Tokens::built_in(), true);
            for layouts in [false, true] {
                let corner_at = corner::layout(w, h, &corner_view(layouts));
                for (_, button) in &corner_at.buttons {
                    assert!(
                        !overlap(&card, button),
                        "{card:?} meets {button:?} on {w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn on_a_desktop_the_card_does_not_meet_the_field_or_the_person() {
        for (w, h) in &SCREENS[..2] {
            let at = lock::layout(*w, *h, &Tokens::built_in(), true);
            let (x, y, r) = at.avatar;
            let disc = Rect::new(x - r, y - r, 2.0 * r, 2.0 * r);
            assert!(!overlap(
                &player_card(*w, *h, 2, &Tokens::built_in(), true),
                &disc
            ));
            let field = at.field;
            assert!(
                !overlap(&player_card(*w, *h, 2, &Tokens::built_in(), true), &field),
                "{:?} meets the field {field:?} on {w}x{h}",
                player_card(*w, *h, 2, &Tokens::built_in(), true)
            );
        }
    }

    #[test]
    fn the_close_button_sits_8_px_in_from_the_cards_top_right_corner() {
        let card = Rect::new(24.0, 676.0, 400.0, 100.0);
        assert_eq!(close_rect(card), Rect::new(396.0, 684.0, 20.0, 20.0));
    }

    #[test]
    fn the_places_write_whole_logical_pixels() {
        assert_eq!(place(Rect::new(24.0, 676.4, 400.0, 99.6)), "24+676+400x100");
    }
}
