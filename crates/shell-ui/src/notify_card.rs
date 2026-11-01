//! The shell's side of notifications (M5.9b): what happens when an app
//! calls `Notify` (the list keeps it, a banner shows it unless do not
//! disturb keeps it away) and the two surfaces, each made when it is
//! shown and let go when it is closed so that, idle, shell-ui holds
//! nothing more than the list's few texts:
//!
//! - the banner (`edel-notification`, `banner.rs`) at the panel's
//!   corner where the status area is, a sheet across the screen when
//!   Compact, gone after its few seconds (a timer on the event loop, not
//!   a thread), kept while the pointer is over it, and never taking the
//!   keyboard;
//! - the notification centre (`edel-centre`, `centre.rs`) over the clock's
//!   panel, opened by a click on the clock, with the keyboard, closed by
//!   Escape, a second click on the clock or the keyboard leaving.
//!
//! Everything a person does comes back to [`Shell::dismiss`] (the
//! notification leaves the list and its app is told why) and
//! [`Shell::invoke`] (its app is told which action, then it is
//! dismissed). Do not disturb is `notifications.do_not_disturb` in the
//! person's settings file, read at each notification and written with
//! `edel::settings::write`, as Settings and `edel settings set` do.

use edel::hardware;
use edel::i18n::tr;
use edel::presets::Edge;
use edel::{places, settings};
use smithay_client_toolkit::reexports::calloop::RegistrationToken;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_surface};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};
use std::time::Duration;

use crate::notice::Hit;
use crate::notify::{self, Msg, Notification, Reason};
use crate::paint::{self, Face};
use crate::popup::Popup;
use crate::{
    BANNER, CENTRE, MARGIN, SETTINGS, Shell, a11y, banner, calendar, centre, messages, notice,
    telling,
};
use crate::{quick, settings_texts, widgets};

/// How long a banner the pointer left stays, milliseconds, before it
/// goes.
const AFTER_POINTER_MS: u64 = 2500;

/// The open banner: its surface, what a screen reader reads, and the
/// timer that ends it.
pub struct BannerCard {
    popup: Popup<banner::View>,
    reader: a11y::Reader,
    view: banner::View,
    id: u32,
    /// How long it stays, milliseconds; none while it waits to be dealt
    /// with.
    ms: Option<u64>,
    timer: Option<RegistrationToken>,
}

impl BannerCard {
    pub fn popup_mut(&mut self) -> &mut Popup<banner::View> {
        &mut self.popup
    }
}

/// The open notification centre: its surface, the keyboard, what a screen
/// reader reads and what it keeps besides the list.
pub struct CentreCard {
    popup: Popup<centre::View>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    reader: a11y::Reader,
    state: centre::State,
    scrolled: widgets::Scrolled,
    /// Where the card's parts were last logged, so a line says it only
    /// when it changed.
    logged: String,
    /// Where it opened: `button` hung on its button, or `left` or `right`
    /// as a sheet along that side (M5.9l).
    place: &'static str,
    /// A sheet still waiting for the height the compositor gives it.
    stretched: bool,
}

impl CentreCard {
    pub fn popup_mut(&mut self) -> &mut Popup<centre::View> {
        &mut self.popup
    }
}

/// Where the notification centre opens for `notifications.list` `list`
/// (M5.9l): `left` or `right` as a sheet along that side, else hung on its
/// button; a Compact screen always has its full-width sheet, so `button`.
fn centre_place(list: &str, compact: bool) -> &'static str {
    match (list, compact) {
        (_, true) => "button",
        ("left", _) => "left",
        ("right", _) => "right",
        _ => "button",
    }
}

impl Shell {
    // ---- What apps say ----

    /// The server's news: a notification came, or an app took one back.
    pub fn notify_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Notify(n) => self.notified(*n),
            Msg::Close(id) => self.dismiss(id, Reason::Recalled),
        }
    }

    /// A notification came: the list keeps it, and a banner shows it
    /// unless do not disturb keeps banners away (a critical one shows).
    fn notified(&mut self, n: Notification) {
        let (machine, person) = settings_texts();
        let dnd = notify::do_not_disturb(machine.as_deref(), person.as_deref());
        eprintln!(
            "edel-shell-ui: notification {} from {}: {}",
            n.id, n.app, n.summary
        );
        for id in self.notifications.add(n.clone()) {
            self.say_closed(id, Reason::Undefined);
            if self.banner.as_ref().is_some_and(|b| b.id == id) {
                self.hide_banner();
            }
        }
        self.count_unread(true);
        let hide_same = |shell: &mut Shell| {
            // The banner this one replaced shows what is no longer so.
            if shell.banner.as_ref().is_some_and(|b| b.id == n.id) {
                shell.hide_banner();
            }
        };
        if !notify::banner_wanted(&n, dnd) {
            hide_same(self);
            eprintln!(
                "edel-shell-ui: notification {} is listed, with no banner while do not disturb is on",
                n.id
            );
        } else if self.centre.is_some() {
            // The open centre lists it as it comes; a banner would lie over it.
            hide_same(self);
            eprintln!(
                "edel-shell-ui: notification {} is listed in the open notification centre",
                n.id
            );
        } else {
            self.show_banner(&n);
        }
        self.draw_centre();
    }

    /// `id` leaves the list, its banner closes and its app is told why.
    pub fn dismiss(&mut self, id: u32, reason: Reason) {
        if !self.notifications.remove(id) {
            return;
        }
        if self.banner.as_ref().is_some_and(|b| b.id == id) {
            self.hide_banner();
        }
        self.say_closed(id, reason);
        self.count_unread(false);
        self.draw_centre();
    }

    /// A person pressed `key` of notification `id`: its app is told, then
    /// the notification is dismissed. shell-ui's own notices tell no app:
    /// Open Updates, and the body (`default`), open Settings' Updates page.
    pub fn invoke(&mut self, id: u32, key: &str) {
        if telling::is_ours(id) {
            // An add-on's notice opens Add-ons; the others open Updates.
            let addons = self
                .notifications
                .items()
                .iter()
                .find(|n| n.id == id)
                .is_some_and(|n| n.buttons().any(|(k, _)| k == "addons"));
            match key {
                "addons" => self.open_page("addons"),
                notify::DEFAULT_ACTION if addons => self.open_page("addons"),
                "updates" | notify::DEFAULT_ACTION => self.open_page("updates"),
                _ => {}
            }
        } else if let Some(connection) = &self._portal {
            notify::invoked(connection, id, key);
        }
        self.dismiss(id, Reason::Dismissed);
    }

    /// Settings opens on `page`, as quick settings starts it.
    fn open_page(&mut self, page: &str) {
        let argv = [SETTINGS.to_string(), "--page".to_string(), page.to_string()];
        self.launcher.spawn(&argv, "Settings");
    }

    // ---- shell-ui's own notices (M5.9e) ----

    /// At start: a fallback record `edel::places::LAST_FALLBACK` holds is
    /// shown once for each person, as the list's notice; the stamp keeps
    /// the record's date, so a later start that finds the same record says
    /// it was seen.
    pub fn tell_fallback(&mut self) {
        let Ok(record) = std::fs::read_to_string(places::LAST_FALLBACK) else {
            return;
        };
        let Some(n) = telling::fallback(&record) else {
            return;
        };
        let key = telling::seen_key(&record);
        if telling::read_stamp(telling::FALLBACK_SEEN).as_deref() == Some(key.as_str()) {
            eprintln!("edel-shell-ui: fallback notice already seen");
            return;
        }
        self.notified(n);
        eprintln!("edel-shell-ui: fallback notice shown");
        if let Err(e) = telling::write_stamp(telling::FALLBACK_SEEN, &key) {
            eprintln!(
                "edel-shell-ui: {}",
                messages::stamp_not_kept(telling::FALLBACK_SEEN, e)
            );
        }
    }

    /// At start: the hardware packs `edel::places::PACKS_FILE` names (M7.11)
    /// are told once, as the list's notice; the stamp keeps their names, so
    /// a later start that finds the same packs says nothing more.
    pub fn tell_hardware(&mut self) {
        let Ok(text) = std::fs::read_to_string(places::PACKS_FILE) else {
            return;
        };
        let Some((key, n)) = telling::hardware(&hardware::read_packs(&text)) else {
            return;
        };
        if telling::read_stamp(telling::HARDWARE_SEEN).as_deref() == Some(key.as_str()) {
            eprintln!("edel-shell-ui: hardware notice already seen");
            return;
        }
        self.notified(n);
        eprintln!("edel-shell-ui: hardware notice shown");
        if let Err(e) = telling::write_stamp(telling::HARDWARE_SEEN, &key) {
            eprintln!(
                "edel-shell-ui: {}",
                messages::stamp_not_kept(telling::HARDWARE_SEEN, e)
            );
        }
    }

    /// The update check's answer (`main.rs` runs `edel update --check` on
    /// a timer): a newer release is announced once for each version.
    pub fn update_checked(&mut self, answer: Result<String, String>) {
        let out = match answer {
            Ok(out) => out,
            Err(why) => {
                eprintln!("edel-shell-ui: {}", messages::update_check_failed(why));
                return;
            }
        };
        let Some((version, n)) = telling::update(&out) else {
            return;
        };
        if telling::read_stamp(telling::UPDATE_SEEN).as_deref() == Some(version.as_str()) {
            return;
        }
        self.notified(n);
        eprintln!("edel-shell-ui: update {version} announced");
        if let Err(e) = telling::write_stamp(telling::UPDATE_SEEN, &version) {
            eprintln!(
                "edel-shell-ui: {}",
                messages::stamp_not_kept(telling::UPDATE_SEEN, e)
            );
        }
    }

    /// Every notification leaves the list.
    fn clear_all(&mut self) {
        if self.notifications.is_empty() {
            return;
        }
        for id in self.notifications.clear() {
            self.say_closed(id, Reason::Dismissed);
        }
        self.hide_banner();
        if let Some(card) = &mut self.centre {
            card.state.first = 0;
        }
        self.count_unread(false);
        self.draw_centre();
    }

    fn say_closed(&self, id: u32, reason: Reason) {
        // No app is listening for shell-ui's own notices.
        if telling::is_ours(id) {
            return;
        }
        if let Some(connection) = &self._portal {
            notify::closed(connection, id, reason);
        }
    }

    // ---- Do not disturb ----

    /// Whether do not disturb is on, by the settings files.
    pub fn do_not_disturb(&self) -> bool {
        let (machine, person) = settings_texts();
        notify::do_not_disturb(machine.as_deref(), person.as_deref())
    }

    /// Turns do not disturb `on` in the person's settings file (the key
    /// is taken out when it is what applies without it), as Settings and
    /// `edel settings set` write it; what is open shows it at once.
    pub fn set_dnd(&mut self, on: bool) {
        let (machine, _) = settings_texts();
        let value = notify::to_write(machine.as_deref(), on);
        match places::person_settings().map(|p| places::found(&p)) {
            Some(path) => match settings::write(&path, settings::DO_NOT_DISTURB, value) {
                Ok(()) => {
                    eprintln!(
                        "edel-shell-ui: do not disturb {}",
                        if on { "on" } else { "off" }
                    );
                    if let Some(card) = &mut self.centre {
                        card.state.dnd = on;
                    }
                    if let Some(card) = &mut self.quick {
                        card.set_dnd(on);
                    }
                }
                Err(e) => eprintln!(
                    "edel-shell-ui: {}",
                    messages::dnd_not_kept(format!("{e:#}"))
                ),
            },
            None => eprintln!("edel-shell-ui: {}", messages::DND_NO_HOME),
        }
        self.draw_centre();
        self.draw_quick();
    }

    // ---- The banner ----

    /// Whether `surface` is the open banner's.
    pub fn is_banner(&self, surface: &wl_surface::WlSurface) -> bool {
        self.banner.as_ref().is_some_and(|b| b.popup.is(surface))
    }

    /// The edge and scale of the panel holding widget `name`, else the
    /// first panel, else a panel along the bottom.
    pub(crate) fn panel_for(&self, name: &str) -> (Edge, u32) {
        self.panels
            .iter()
            .find(|p| p.row.all().any(|w| w.name == name))
            .or_else(|| self.panels.first())
            .map_or((Edge::Bottom, 1), |p| (p.edge, p.scale))
    }

    /// Shows `n` as a banner at the panel's corner where the status area
    /// is, instead of the one showing, which stays in the list.
    fn show_banner(&mut self, n: &Notification) {
        self.hide_banner();
        let (status, scale) = self.panel_for("status");
        let (machine, person) = settings_texts();
        let position = edel::settings::chosen(
            edel::settings::NOTIFICATION_POSITION,
            machine.as_deref(),
            person.as_deref(),
        );
        let (edge, across) = banner::place(position.as_deref(), status);
        let screen = self.screen_width();
        let (_, body, _, _) = notice::sizes(&self.tokens);
        let text = &mut self.text;
        let view = banner::view(n, screen, |t, face| text.line_in(t, body, face).width);
        let layout = banner::layout(&view);
        let room = paint::shadow_room(&self.tokens, !crate::fillets());
        let Some(mut popup) = Popup::new(self, BANNER, layout.size, layout.size, scale, room)
        else {
            return;
        };
        popup.set_cards(vec![banner::card(&layout, &self.tokens)], &self.compositor);
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        let m = MARGIN - room as i32;
        let surface = &popup.surface;
        if view.compact {
            surface.set_anchor(side | Anchor::LEFT | Anchor::RIGHT);
            surface.set_margin(m, -(room as i32), m, -(room as i32));
        } else {
            // A banner keeps clear of every panel: the compositor places a
            // layer with no exclusive zone of its own inside the others'.
            surface.set_anchor(match across {
                banner::Across::Left => side | Anchor::LEFT,
                banner::Across::Middle => side,
                banner::Across::Right => side | Anchor::RIGHT,
            });
            surface.set_margin(m, m, m, m);
        }
        // A banner never takes the keyboard from what a person is typing in.
        surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        surface.commit();
        let ms = notify::banner_ms(n);
        let timer = ms.and_then(|ms| self.banner_timer(n.id, ms));
        self.banner = Some(BannerCard {
            popup,
            reader: a11y::Reader::new(tr("Notification")),
            view,
            id: n.id,
            ms,
            timer,
        });
    }

    /// A timer that ends the banner of notification `id` in `ms`.
    fn banner_timer(&mut self, id: u32, ms: u64) -> Option<RegistrationToken> {
        self.handle
            .insert_source(
                Timer::from_duration(Duration::from_millis(ms)),
                move |_, _, shell: &mut Shell| {
                    if let Some(card) = &mut shell.banner {
                        if card.id == id {
                            card.timer = None;
                            shell.hide_banner();
                        }
                    }
                    TimeoutAction::Drop
                },
            )
            .inspect_err(|e| eprintln!("edel-shell-ui: no timer for a banner: {e}"))
            .ok()
    }

    /// Closes the banner and lets go of its buffers and reader; its
    /// notification stays in the list.
    pub fn hide_banner(&mut self) {
        let Some(card) = self.banner.take() else {
            return;
        };
        if let Some(token) = card.timer {
            self.handle.remove(token);
        }
        drop(card);
        eprintln!("edel-shell-ui: banner hidden");
    }

    /// Draws the banner if what it shows changed.
    pub fn draw_banner(&mut self) {
        let glass = self.glass();
        let Some(card) = &mut self.banner else {
            return;
        };
        let Some(mut pixmap) = card.popup.canvas(&card.view) else {
            return;
        };
        let scale = card.popup.scale();
        banner::paint(
            &mut pixmap,
            &card.view,
            &self.tokens,
            Some(&mut self.text),
            Some(&mut self.icons),
            scale,
        );
        card.popup
            .set_blur(&self.compositor, glass, self.tokens.radius_menu as f32);
        let first = card
            .popup
            .show(card.view.clone(), &pixmap, &self.tokens, "banner", &self.qh);
        if first {
            let layout = banner::layout(&card.view);
            eprintln!("edel-shell-ui: banner shown, notification {}", card.id);
            eprintln!("edel-shell-ui: banner places {}", banner::places(&layout));
            let nodes = banner::nodes(&card.view, &layout);
            let size = (f64::from(layout.size.0), f64::from(layout.size.1));
            card.reader.update(size, nodes);
        }
    }

    /// The pointer on the banner: it lights what it is over and holds the
    /// banner while it is there; a press on a button invokes its action,
    /// on the cross dismisses, on the rest invokes the `default` action
    /// when there is one.
    pub fn banner_pointer(&mut self, event: &PointerEvent) {
        let Some(card) = &self.banner else {
            return;
        };
        let room = card.popup.room() as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let over = banner::hit(&banner::layout(&card.view), x, y);
        let (id, timed) = (card.id, card.ms.is_some());
        let key = |a: usize| card.view.item.buttons.get(a).map(|b| b.key.clone());
        let default = card.view.item.default;
        match &event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                // Held while the pointer is over it.
                let token = self.banner.as_mut().and_then(|c| c.timer.take());
                if let Some(token) = token {
                    self.handle.remove(token);
                }
                if let Some(card) = &mut self.banner {
                    card.view.hover = over;
                }
                self.draw_banner();
            }
            PointerEventKind::Leave { .. } => {
                let waiting = self.banner.as_ref().is_some_and(|c| c.timer.is_some());
                let token = (timed && !waiting)
                    .then(|| self.banner_timer(id, AFTER_POINTER_MS))
                    .flatten();
                if let Some(card) = &mut self.banner {
                    card.view.hover = None;
                    card.timer = token.or(card.timer.take());
                }
                self.draw_banner();
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => match over {
                Some(Hit::Close) => self.dismiss(id, Reason::Dismissed),
                Some(Hit::Button(a)) => {
                    if let Some(key) = key(a) {
                        self.invoke(id, &key);
                    }
                }
                Some(Hit::Body) if default => self.invoke(id, notify::DEFAULT_ACTION),
                // No default action: the banner goes, the notification stays.
                Some(Hit::Body) => self.hide_banner(),
                None => {}
            },
            _ => {}
        }
    }

    // ---- The notification centre ----

    /// A click on the clock on panel `i`, whose middle lies `at` logical
    /// pixels along it: the centre opens hung on the clock, or closes if
    /// open.
    pub fn toggle_centre(&mut self, i: usize, at: f32) {
        if self.centre.is_some() {
            self.close_centre();
        } else {
            self.open_centre(true);
            self.hang_centre(i, at);
        }
    }

    /// A click on the notifications button (M5.9k) on panel `i`, whose
    /// middle lies `at` logical pixels along it: the notifications open
    /// alone, without the calendar, hung on the button, or close if open.
    pub fn toggle_notifications(&mut self, i: usize, at: f32) {
        if self.centre.is_some() {
            self.close_centre();
        } else {
            self.open_centre(false);
            self.hang_centre(i, at);
        }
    }

    /// The open list hung on the point `at` along panel `i`: centred on it
    /// and kept inside the panel, as the tray's grid hangs on its arrow,
    /// so the clock or the bell at either end or in the middle opens it
    /// beside itself.
    /// A Compact sheet stays across the screen, and a sheet along a side
    /// (M5.9l) stays where the compositor put it.
    fn hang_centre(&mut self, i: usize, at: f32) {
        let Some(panel_width) = self.panels.get(i).map(|p| p.width) else {
            return;
        };
        let room = paint::shadow_room(&self.tokens, !crate::fillets()) as i32;
        let Some(card) = &self.centre else {
            return;
        };
        if card.state.compact || card.place != "button" {
            return;
        }
        let size = card.state.width as i32;
        let left =
            (at as i32 - size / 2).clamp(MARGIN, (panel_width as i32 - size - MARGIN).max(MARGIN));
        let m = MARGIN - room;
        let surface = &card.popup.surface;
        let side = match self.panels[i].edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        surface.set_anchor(side | Anchor::LEFT);
        surface.set_margin(m, m, m, left - room);
        surface.commit();
    }

    /// The notifications button's count follows the list (M5.9k): one more
    /// for a notification that came while the list was closed, never more
    /// than the list holds; the panels are drawn again when it changes,
    /// and the log says so.
    fn count_unread(&mut self, came: bool) {
        let before = self.live.unread;
        if came && self.centre.is_none() {
            self.live.unread += 1;
        }
        self.live.unread = self.live.unread.min(self.notifications.len());
        if self.live.unread != before {
            eprintln!("edel-shell-ui: unread notifications {}", self.live.unread);
            self.draw_all();
            // The lock screen counts them too (M5.10c).
            self.draw_lock();
        }
    }

    pub fn is_centre(&self, surface: &wl_surface::WlSurface) -> bool {
        self.centre.as_ref().is_some_and(|c| c.popup.is(surface))
    }

    /// What the centre shows for `state`, and where it lies.
    fn centre_view_for(&mut self, state: &centre::State) -> (centre::View, centre::Layout) {
        let (_, _, small) = centre::sizes(&self.tokens);
        let (_, body, _, _) = notice::sizes(&self.tokens);
        let text = &mut self.text;
        let clear = text.line_in(tr("Clear all"), small, Face::SEMIBOLD).width;
        let view = centre::view(state, self.notifications.items(), clear, |t, face| {
            text.line_in(t, body, face).width
        });
        let layout = centre::layout(&view);
        (view, layout)
    }

    /// Opens the centre over the clock's panel (below it on a panel along
    /// the top), a sheet across the screen when Compact, with the
    /// keyboard; without the calendar, over the notifications button's
    /// panel (M5.9k). Opening it reads every notification. With
    /// `notifications.list` at `left` or `right` (M5.9l) it is a sheet
    /// along that side instead, its height what the compositor gives.
    fn open_centre(&mut self, calendar: bool) {
        self.close_launcher();
        self.close_styles();
        self.close_app_windows();
        self.close_in_use();
        self.close_quick();
        self.close_tray_grid();
        self.hide_banner();
        let (edge, scale) = self.panel_for(if calendar { "clock" } else { "notifications" });
        let (width, compact) = centre::width_for(self.screen_width());
        let place = centre_place(&self.live.list, compact);
        let today = calendar::today();
        let state = centre::State {
            width,
            compact,
            calendar,
            height: None,
            dnd: self.do_not_disturb(),
            month: calendar::Month::of(today),
            today,
            first: 0,
            hover: None,
            focus: None,
        };
        let (_, layout) = self.centre_view_for(&state);
        let size = layout.size;
        // Room to grow when notifications come while it is open.
        let most = (size.0, size.1 + 300);
        let room = paint::shadow_room(&self.tokens, !crate::fillets());
        let Some(mut popup) = Popup::new(self, CENTRE, size, most, scale, room) else {
            return;
        };
        popup.set_cards(centre::cards(&layout, &self.tokens), &self.compositor);
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        let m = MARGIN - room as i32;
        let surface = &popup.surface;
        let sheet = match place {
            "left" => Some(Anchor::LEFT),
            "right" => Some(Anchor::RIGHT),
            _ => None,
        };
        if let Some(along) = sheet {
            // Between the top and bottom edges, as far as the panels leave.
            surface.set_anchor(Anchor::TOP | Anchor::BOTTOM | along);
            surface.set_margin(m, m, m, m);
        } else if compact {
            surface.set_anchor(side | Anchor::LEFT | Anchor::RIGHT);
            surface.set_margin(m, -(room as i32), m, -(room as i32));
        } else {
            surface.set_anchor(side | Anchor::RIGHT);
            surface.set_margin(m, m, m, m);
        }
        surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        surface.commit();
        if sheet.is_some() {
            popup.stretch(&self.compositor);
        }
        let keyboard = self.seat.seats().next().and_then(|seat| {
            self.seat
                .get_keyboard_with_repeat(
                    &self.qh,
                    &seat,
                    None,
                    self.handle.clone(),
                    Box::new(|shell: &mut Shell, _, event| shell.centre_key(event)),
                )
                .inspect_err(|e| {
                    eprintln!("edel-shell-ui: no keyboard for the notification centre: {e}");
                })
                .ok()
        });
        self.centre = Some(CentreCard {
            popup,
            keyboard,
            reader: a11y::Reader::new(tr("Notifications")),
            state,
            scrolled: widgets::Scrolled::default(),
            logged: String::new(),
            place,
            stretched: sheet.is_some(),
        });
        // The clock lights while the centre is open, the bell while the
        // notifications it opened are; either way they are read.
        self.live.centre = calendar;
        self.live.bell_open = !calendar;
        if self.live.unread > 0 {
            self.live.unread = 0;
            eprintln!("edel-shell-ui: unread notifications 0");
        }
        self.draw_all();
    }

    /// The compositor configured the centre's surface (M5.9l). A sheet
    /// waiting for its height takes the height it is given, less the room
    /// round its card, and asks for exactly that, then draws at once: the
    /// size it asks for is the one it was given, so no second configure
    /// comes. Otherwise the card is drawn as it is.
    pub fn centre_configured(&mut self, size: (u32, u32)) {
        let Some(card) = &mut self.centre else {
            return;
        };
        if card.stretched {
            card.stretched = false;
            let height = size.1.saturating_sub(2 * card.popup.room());
            card.state.height = Some(height);
            card.popup
                .fill((card.state.width, height), &self.compositor);
        }
        card.popup_mut().configured();
        self.draw_centre();
    }

    /// Closes the centre and lets go of its keyboard, buffers and reader.
    pub fn close_centre(&mut self) {
        let Some(card) = self.centre.take() else {
            return;
        };
        if let Some(keyboard) = &card.keyboard {
            self.let_go(keyboard);
        }
        drop(card);
        self.live.centre = false;
        self.live.bell_open = false;
        self.draw_all();
        eprintln!("edel-shell-ui: notification centre hidden");
    }

    /// Draws the centre if what it shows changed, after asking the
    /// compositor for a new size if it needs one.
    pub fn draw_centre(&mut self) {
        let glass = self.glass();
        let Some(state) = self.centre.as_ref().map(|c| c.state.clone()) else {
            return;
        };
        let (view, layout) = self.centre_view_for(&state);
        let Some(card) = &mut self.centre else {
            return;
        };
        card.popup
            .set_cards(centre::cards(&layout, &self.tokens), &self.compositor);
        // The list may have got shorter than where it starts.
        card.state.first = state.first.min(self.notifications.len().saturating_sub(1));
        if card.popup.size() != layout.size {
            card.popup.resize(layout.size, &self.compositor);
            return;
        }
        let Some(mut pixmap) = card.popup.canvas(&view) else {
            return;
        };
        let scale = card.popup.scale();
        centre::paint(
            &mut pixmap,
            &view,
            &self.tokens,
            Some(&mut self.text),
            Some(&mut self.icons),
            scale,
        );
        card.popup
            .set_blur(&self.compositor, glass, self.tokens.radius_menu as f32);
        let first = card.popup.show(
            view.clone(),
            &pixmap,
            &self.tokens,
            "notification centre",
            &self.qh,
        );
        if first {
            eprintln!(
                "edel-shell-ui: notification centre shown, {} notifications",
                self.notifications.len()
            );
            eprintln!("edel-shell-ui: notification centre placed {}", card.place);
        }
        let places = centre::places(&layout);
        if places != card.logged {
            eprintln!("edel-shell-ui: centre places {places}");
            card.logged = places;
        }
        let (items, focused) = centre::items(&view, &layout);
        let size = (f64::from(layout.size.0), f64::from(layout.size.1));
        card.reader.update_focused(size, items, focused);
    }

    /// A key while the centre is open.
    pub fn centre_key(&mut self, event: KeyEvent) {
        let key = match event.keysym {
            Keysym::Escape => quick::Key::Escape,
            Keysym::Tab => quick::Key::Tab(false),
            Keysym::ISO_Left_Tab => quick::Key::Tab(true),
            Keysym::Left => quick::Key::Left,
            Keysym::Right => quick::Key::Right,
            Keysym::Up => quick::Key::Up,
            Keysym::Down => quick::Key::Down,
            Keysym::Home => quick::Key::Home,
            Keysym::End => quick::Key::End,
            Keysym::Return | Keysym::KP_Enter | Keysym::space => quick::Key::Activate,
            Keysym::Page_Down => return self.centre_act(centre::Act::Scroll(1)),
            Keysym::Page_Up => return self.centre_act(centre::Act::Scroll(-1)),
            _ => return,
        };
        let Some(state) = self.centre.as_ref().map(|c| c.state.clone()) else {
            return;
        };
        let (view, _) = self.centre_view_for(&state);
        let (focus, act) = centre::key(&view, state.focus, key);
        if let Some(card) = &mut self.centre {
            card.state.focus = focus;
        }
        match act {
            Some(act) => self.centre_act(act),
            None => self.draw_centre(),
        }
    }

    /// The pointer on the centre: it lights what it is over, a press does
    /// what the part does and the wheel scrolls the list.
    pub fn centre_pointer(&mut self, event: &PointerEvent) {
        let Some(state) = self.centre.as_ref().map(|c| c.state.clone()) else {
            return;
        };
        let (view, layout) = self.centre_view_for(&state);
        let Some(card) = &mut self.centre else {
            return;
        };
        let room = card.popup.room() as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let over = centre::hit(&layout, x, y);
        match &event.kind {
            PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                if card.state.hover != over {
                    card.state.hover = over;
                    self.draw_centre();
                }
            }
            PointerEventKind::Leave { .. } => {
                card.state.hover = None;
                card.scrolled.reset();
                self.draw_centre();
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                card.state.focus = None;
                if let Some(act) = over.and_then(|part| centre::press(&view, part)) {
                    self.centre_act(act);
                }
            }
            PointerEventKind::Axis {
                horizontal,
                vertical,
                ..
            } => {
                let n = card.scrolled.steps(
                    vertical.value120 + horizontal.value120,
                    vertical.discrete + horizontal.discrete,
                    vertical.absolute + horizontal.absolute,
                );
                if n != 0 {
                    self.centre_act(centre::Act::Scroll(n));
                }
            }
            _ => {}
        }
    }

    /// Does what a click or a key on the centre asked.
    fn centre_act(&mut self, act: centre::Act) {
        use centre::Act;
        match act {
            Act::Hide => self.close_centre(),
            Act::Close(id) => self.dismiss(id, Reason::Dismissed),
            Act::Action(id, key) => self.invoke(id, &key),
            Act::Default(id) => self.invoke(id, notify::DEFAULT_ACTION),
            Act::Clear => self.clear_all(),
            Act::Dnd => {
                let on = !self.centre.as_ref().is_some_and(|c| c.state.dnd);
                self.set_dnd(on);
            }
            Act::Month(by) => {
                if let Some(card) = &mut self.centre {
                    card.state.month = card.state.month.step(by);
                }
                self.draw_centre();
            }
            Act::Scroll(by) => {
                let last = self.notifications.len().saturating_sub(1) as i64;
                if let Some(card) = &mut self.centre {
                    let to = (card.state.first as i64 + i64::from(by)).clamp(0, last);
                    card.state.first = to as usize;
                }
                self.draw_centre();
            }
        }
    }
}
