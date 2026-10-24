//! The looks side by side on the lock (M5.10c7h): the surface, `edel-lock-looks`,
//! on the overlay layer over the whole screen with the keyboard exclusive,
//! made as the lock editor's is (`open_lock_looks`). Input and the writes
//! live here; the drawing, layout and places are `lock_looks.rs`. What a
//! choice writes is the library's (`edel::lock_screen::chose_text` and
//! `saved_as_text`), the same text Settings' Lock screen page writes.

use std::collections::HashMap;

use edel::i18n::tr;
use edel::lock_screen;
use edel::places;
use edel::settings;
use edel::wallpapers::Wallpaper;
use smithay_client_toolkit::compositor::FrameCallbackData;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_shm};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerSurface,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use tiny_skia::Pixmap;

use crate::Shell;
use crate::a11y;
use crate::lock;
use crate::lock_looks::{self, Kit, Part, View};
use crate::messages;
use crate::paint;
use crate::quick;
use crate::widgets;

/// The surface's name, as the compositor's layer rules see it.
const LOCK_LOOKS: &str = "edel-lock-looks";

/// The looks switcher on the lock: its surface, the looks and the one shown.
pub struct LockLooks {
    surface: LayerSurface,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pool: Option<SlotPool>,
    scale: u32,
    screen: (f32, f32),
    compact: bool,
    ready: bool,
    waiting: bool,
    dirty: bool,
    shown_logged: bool,
    logged: String,
    /// The looks in order: the built-in ones, then the person's own by name.
    names: Vec<String>,
    /// The look in use, as the lock shows it now.
    in_use: usize,
    /// The look in the middle, and the parts the keyboard and pointer hold.
    shown: usize,
    focus: Option<Part>,
    hover: Option<Part>,
    /// Each look's picture as the lock draws it, made when first shown.
    thumbs: HashMap<String, Pixmap>,
    pictures: lock::Pictures,
    backdrops: lock::Backdrops,
    wallpaper: Wallpaper,
    battery: bool,
    battery_view: Option<(u8, bool)>,
    scrolled: widgets::Scrolled,
    reader: a11y::Reader,
}

impl LockLooks {
    /// The switcher's view: the screen, the looks, the one shown, the focus
    /// and the pointer's part.
    fn view(&self) -> View {
        View {
            screen: self.screen,
            compact: self.compact,
            names: self.names.clone(),
            shown: self.shown,
            in_use: self.in_use,
            focus: self.focus,
            hover: self.hover,
        }
    }

    /// The lock's view of the time and the person for the pictures: no
    /// notices, the battery the status area reads.
    fn look_view(&self) -> lock::View {
        let now = edel::timezones::now();
        let (time, date) = lock::time_and_date(&now);
        let mut view = lock::View::new(time, date, lock::user());
        view.when = Some(lock::When::of(&now));
        view.battery = self.battery_view;
        view
    }
}

/// The two settings files' texts as the switcher writes them: the machine's
/// and the person's (empty with none).
fn texts() -> (Option<String>, String) {
    let read = |path: std::path::PathBuf| std::fs::read_to_string(places::found(&path)).ok();
    (
        read(places::machine_settings()),
        places::person_settings().and_then(read).unwrap_or_default(),
    )
}

/// Writes the person's settings text (what a choice of the look changed),
/// through the library's writer; a refusal says why as the lock screen's
/// message does.
fn write_person(text: &str) -> Result<(), String> {
    let path =
        places::person_settings().ok_or_else(|| messages::LOCK_SCREEN_NO_HOME.to_string())?;
    settings::write_text(&path, text).map_err(|e| messages::lock_screen_not_kept(format!("{e:#}")))
}

/// What a choice of `name` wrote, for the log: `Ok(None)` when the file
/// already said so, else what changed.
fn choose_on_disk(name: &str) -> Result<Option<String>, String> {
    let (machine, person) = texts();
    let text = lock_screen::chose_text(&person, machine.as_deref(), name)?;
    if text == person {
        return Ok(None);
    }
    write_person(&text)?;
    let had = settings::read(&person)
        .map(|read| read.file.lock_screen)
        .unwrap_or_default();
    let taken = had.parts.is_some() || had.background.is_some();
    Ok(Some(if taken {
        "lock_screen.look written; widgets and wallpaper taken out".to_string()
    } else {
        "lock_screen.look written".to_string()
    }))
}

impl Shell {
    /// Opens the looks side by side (M5.10c7h): nothing while the lock, the
    /// panel editor, the lock editor or this is up. It starts on the look in
    /// use, in the middle.
    pub fn open_lock_looks(&mut self) {
        if self.lock.is_some()
            || self.editor.is_some()
            || self.lock_editor.is_some()
            || self.lock_looks.is_some()
        {
            return;
        }
        self.close_popups();
        let (machine, person) = texts();
        let (machine_dark, person_dark) = crate::settings_texts();
        let dark = quick::is_dark(machine_dark.as_deref(), person_dark.as_deref());
        let lock_settings = lock::lock_settings(machine.as_deref(), Some(&person));
        let mut names: Vec<String> = lock_screen::LOOKS.iter().map(|n| n.to_string()).collect();
        if let Some(own) = &lock_settings.looks {
            names.extend(own.keys().cloned());
        }
        let in_use_name = lock_settings
            .look
            .clone()
            .unwrap_or_else(|| lock_screen::CLASSIC.to_string());
        let in_use = names.iter().position(|n| *n == in_use_name).unwrap_or(0);
        let battery = self.live.status.battery.is_some();
        let battery_view = self.battery_view();
        let surface = self.compositor.create_surface(&self.qh);
        let surface = self.layers.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some(LOCK_LOOKS),
            None,
        );
        // Over every panel, on the whole screen: the exclusive zone -1 keeps
        // the panels where they are, and the keyboard is the switcher's alone.
        surface.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        surface.set_size(0, 0);
        surface.set_exclusive_zone(-1);
        surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        surface.commit();
        let keyboard = self.seat.seats().next().and_then(|seat| {
            self.seat
                .get_keyboard_with_repeat(
                    &self.qh,
                    &seat,
                    None,
                    self.handle.clone(),
                    Box::new(|shell: &mut Shell, _, event| shell.lock_looks_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for the looks: {e}"))
                .ok()
        });
        self.lock_looks = Some(LockLooks {
            surface,
            keyboard,
            pool: None,
            scale: 1,
            screen: (0.0, 0.0),
            compact: false,
            ready: false,
            waiting: false,
            dirty: true,
            shown_logged: false,
            logged: String::new(),
            names,
            in_use,
            shown: in_use,
            focus: None,
            hover: None,
            thumbs: HashMap::new(),
            pictures: lock::Pictures::default(),
            backdrops: lock::Backdrops::default(),
            wallpaper: lock::wallpaper_of(machine.as_deref(), Some(&person), dark),
            battery,
            battery_view,
            scrolled: widgets::Scrolled::default(),
            reader: a11y::Reader::new(tr("Lock screen looks")),
        });
    }

    /// The screen's first configure: the surface's size is the screen's, and
    /// the switcher may draw.
    pub fn lock_looks_configured(&mut self, size: (u32, u32)) {
        let Some(ll) = self.lock_looks.as_mut() else {
            return;
        };
        ll.screen = (size.0 as f32, size.1 as f32);
        ll.compact = ll.screen.0 < lock::COMPACT_BELOW;
        ll.ready = true;
        ll.dirty = true;
        self.draw_lock_looks();
    }

    /// The compositor showed the last drawing: the next may go.
    pub fn lock_looks_framed(&mut self) {
        if let Some(ll) = self.lock_looks.as_mut() {
            ll.waiting = false;
        }
        self.draw_lock_looks();
    }

    /// The screen's scale changed.
    pub fn lock_looks_scaled(&mut self, factor: i32) {
        if let Some(ll) = self.lock_looks.as_mut() {
            ll.scale = factor.clamp(1, 4) as u32;
            ll.dirty = true;
        }
        self.draw_lock_looks();
    }

    /// Whether `surface` is the open switcher's.
    pub fn is_lock_looks(&self, surface: &WlSurface) -> bool {
        self.lock_looks
            .as_ref()
            .is_some_and(|ll| ll.surface.wl_surface() == surface)
    }

    /// Closes the switcher, writing nothing (Escape, the keyboard leaving, a
    /// close from the compositor).
    pub fn close_lock_looks(&mut self) {
        let Some(ll) = self.lock_looks.take() else {
            return;
        };
        if let Some(keyboard) = &ll.keyboard {
            keyboard.release();
        }
        drop(ll);
        eprintln!("edel-shell-ui: lock looks hidden");
    }

    /// Makes the shown look the one in use, writing it (Return, Use this
    /// look). The switcher closes once written; a refused write stays open.
    fn lock_looks_use(&mut self) {
        let Some(ll) = self.lock_looks.as_ref() else {
            return;
        };
        if ll.shown == ll.in_use {
            return;
        }
        let name = ll.names[ll.shown].clone();
        match choose_on_disk(&name) {
            Ok(what) => {
                let what = what.unwrap_or_else(|| "nothing to write".to_string());
                eprintln!("edel-shell-ui: lock look {name} chosen, {what}");
                self.close_lock_looks();
            }
            Err(message) => eprintln!("edel-shell-ui: {message}"),
        }
    }

    /// Customize: the shown look is chosen first when it is not in use, and
    /// the lock editor opens on it.
    fn lock_looks_customize(&mut self) {
        let Some(ll) = self.lock_looks.as_ref() else {
            return;
        };
        let name = ll.names[ll.shown].clone();
        let in_use = ll.shown == ll.in_use;
        if !in_use {
            if let Err(message) = choose_on_disk(&name) {
                return eprintln!("edel-shell-ui: {message}");
            }
        }
        eprintln!("edel-shell-ui: lock looks: customize {name}");
        self.close_lock_looks();
        self.open_lock_editor();
    }

    /// The plus: a copy of the shown look is saved and chosen, and the lock
    /// editor opens on it.
    fn lock_looks_plus(&mut self) {
        let Some(ll) = self.lock_looks.as_ref() else {
            return;
        };
        let base = ll.names[ll.shown].clone();
        if ll.shown != ll.in_use {
            if let Err(message) = choose_on_disk(&base) {
                return eprintln!("edel-shell-ui: {message}");
            }
        }
        let (machine, person) = texts();
        let saved = lock_screen::saved_as_text(&person, machine.as_deref(), &base)
            .and_then(|(text, name)| write_person(&text).map(|()| name));
        match saved {
            Ok(name) => {
                eprintln!("edel-shell-ui: lock looks: new look {name}");
                self.close_lock_looks();
                self.open_lock_editor();
            }
            Err(message) => eprintln!("edel-shell-ui: {message}"),
        }
    }

    /// Shows the look at `to` in the middle and logs it.
    fn lock_looks_show(&mut self, to: usize) {
        let Some(ll) = self.lock_looks.as_mut() else {
            return;
        };
        if to >= ll.names.len() || to == ll.shown {
            return;
        }
        ll.shown = to;
        ll.dirty = true;
        let name = ll.names[to].clone();
        eprintln!("edel-shell-ui: lock looks shows {name}");
        self.draw_lock_looks();
    }

    /// A key while the switcher is open: Escape leaves, Left and Right step,
    /// Tab walks the buttons, Return acts on the focus (or uses the look
    /// shown), and C customizes.
    pub fn lock_looks_key(&mut self, event: KeyEvent) {
        let Some(ll) = self.lock_looks.as_mut() else {
            return;
        };
        let (shown, count, focus) = (ll.shown, ll.names.len(), ll.focus);
        let shift = self.shift;
        match event.keysym {
            Keysym::Escape => return self.close_lock_looks(),
            Keysym::Left => {
                let to = lock_looks::step(shown, count, false);
                return self.lock_looks_show(to);
            }
            Keysym::Right => {
                let to = lock_looks::step(shown, count, true);
                return self.lock_looks_show(to);
            }
            Keysym::Tab | Keysym::ISO_Left_Tab => {
                ll.focus =
                    lock_looks::next_focus(focus, shift || event.keysym == Keysym::ISO_Left_Tab);
                ll.dirty = true;
            }
            Keysym::Return | Keysym::KP_Enter => match focus {
                Some(Part::Customize) => return self.lock_looks_customize(),
                Some(Part::Plus) => return self.lock_looks_plus(),
                None if shown == ll.in_use => return self.close_lock_looks(),
                Some(Part::Use) if shown == ll.in_use => {}
                _ => return self.lock_looks_use(),
            },
            Keysym::c | Keysym::C => return self.lock_looks_customize(),
            _ => {}
        }
        self.draw_lock_looks();
    }

    /// The pointer over the switcher: motion lights a part, a left click acts
    /// on the part under it, and a horizontal scroll or swipe steps.
    pub fn lock_looks_pointer(&mut self, event: &PointerEvent) {
        let Some(ll) = self.lock_looks.as_mut() else {
            return;
        };
        let (x, y) = (event.position.0 as f32, event.position.1 as f32);
        let layout = lock_looks::layout(&ll.view());
        let part = lock_looks::hit(&layout, x, y);
        match event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                if ll.hover != part {
                    ll.hover = part;
                    ll.dirty = true;
                    self.draw_lock_looks();
                }
            }
            PointerEventKind::Leave { .. } => {
                if ll.hover.take().is_some() {
                    ll.dirty = true;
                    self.draw_lock_looks();
                }
            }
            PointerEventKind::Press { button, .. } if button == BTN_LEFT => match part {
                Some(Part::Customize) => self.lock_looks_customize(),
                Some(Part::Use) => self.lock_looks_use(),
                Some(Part::Plus) => self.lock_looks_plus(),
                Some(Part::Dot(i)) => self.lock_looks_show(i),
                Some(Part::Left) => {
                    let to = lock_looks::step(ll.shown, ll.names.len(), false);
                    self.lock_looks_show(to);
                }
                Some(Part::Right) => {
                    let to = lock_looks::step(ll.shown, ll.names.len(), true);
                    self.lock_looks_show(to);
                }
                None => {}
            },
            PointerEventKind::Axis { horizontal, .. } => {
                // Only a horizontal scroll steps: right is the next look.
                let n = ll.scrolled.steps(
                    horizontal.value120,
                    horizontal.discrete,
                    horizontal.absolute,
                );
                if n != 0 {
                    let to = (ll.shown as i64 + i64::from(n))
                        .clamp(0, ll.names.len().max(1) as i64 - 1)
                        as usize;
                    self.lock_looks_show(to);
                }
            }
            _ => {}
        }
    }

    /// Makes the look's pictures the switcher draws (the middle and both
    /// neighbours), each once.
    fn lock_looks_thumbs(&mut self, ll: &mut LockLooks) {
        let view = ll.look_view();
        let shown = ll.shown as isize;
        for at in [shown - 1, shown, shown + 1] {
            let Some(name) = usize::try_from(at).ok().and_then(|i| ll.names.get(i)) else {
                continue;
            };
            if ll.thumbs.contains_key(name) {
                continue;
            }
            let kit = Kit {
                tokens: &self.tokens,
                text: &mut self.text,
                icons: &mut self.icons,
                pictures: &mut ll.pictures,
                backdrops: &mut ll.backdrops,
                wallpaper: &ll.wallpaper,
            };
            if let Some(picture) =
                lock_looks::look_picture(kit, name, ll.screen, ll.compact, &view, ll.battery)
            {
                ll.thumbs.insert(name.clone(), picture);
            }
        }
    }

    /// Draws the switcher if it may draw and what it shows changed.
    fn draw_lock_looks(&mut self) {
        let Some(mut ll) = self.lock_looks.take() else {
            return;
        };
        self.paint_lock_looks(&mut ll);
        self.lock_looks = Some(ll);
    }

    fn paint_lock_looks(&mut self, ll: &mut LockLooks) {
        if !ll.ready || ll.waiting || !ll.dirty || ll.names.is_empty() {
            return;
        }
        self.lock_looks_thumbs(ll);
        let s = ll.scale;
        let view = ll.view();
        let layout = lock_looks::layout(&view);
        let (pw, ph) = (ll.screen.0 as u32 * s, ll.screen.1 as u32 * s);
        let Some(mut pixmap) = Pixmap::new(pw, ph) else {
            return;
        };
        let at = |i: isize| -> Option<&Pixmap> {
            usize::try_from(i)
                .ok()
                .and_then(|i| ll.names.get(i))
                .and_then(|name| ll.thumbs.get(name))
        };
        let shown = ll.shown as isize;
        let pictures = [at(shown - 1), at(shown), at(shown + 1)];
        lock_looks::paint(
            &mut pixmap,
            &view,
            &layout,
            pictures,
            &self.tokens,
            &mut self.text,
            s as f32,
        );
        let places = lock_looks::places(&view, &layout);
        let focus = lock_looks::focus_index(&view, &layout);
        ll.reader.update_focused(
            (f64::from(ll.screen.0), f64::from(ll.screen.1)),
            lock_looks::nodes(&view, &layout),
            focus,
        );
        if places != ll.logged {
            eprintln!("edel-shell-ui: lock looks places {places}");
            ll.logged = places;
        }
        present(ll, pixmap, (pw, ph), s, &self.qh, &self.shm);
        if !ll.shown_logged {
            ll.shown_logged = true;
            eprintln!(
                "edel-shell-ui: lock looks shown, {} looks, {} in the middle",
                ll.names.len(),
                ll.names[ll.shown]
            );
        }
        ll.dirty = false;
    }
}

/// Hands a finished drawing to the compositor, through two buffers' pool,
/// and asks for the frame callback that lets the next drawing go.
fn present(
    ll: &mut LockLooks,
    pixmap: Pixmap,
    (pw, ph): (u32, u32),
    s: u32,
    qh: &smithay_client_toolkit::reexports::client::QueueHandle<Shell>,
    shm: &smithay_client_toolkit::shm::Shm,
) {
    let bytes = (pw * ph * 4) as usize;
    if ll.pool.is_none() {
        ll.pool = SlotPool::new(2 * bytes, shm)
            .inspect_err(|e| eprintln!("edel-shell-ui: no memory for the looks: {e}"))
            .ok();
    }
    let Some(pool) = ll.pool.as_mut() else {
        return;
    };
    let (buffer, canvas) = match pool.create_buffer(
        pw as i32,
        ph as i32,
        pw as i32 * 4,
        wl_shm::Format::Argb8888,
    ) {
        Ok(made) => made,
        Err(e) => {
            eprintln!("edel-shell-ui: drawing the looks failed: {e}");
            return;
        }
    };
    paint::to_argb(&pixmap, canvas);
    let surface = ll.surface.wl_surface().clone();
    surface.set_buffer_scale(s as i32);
    surface.damage_buffer(0, 0, pw as i32, ph as i32);
    if let Err(e) = buffer.attach_to(&surface) {
        eprintln!("edel-shell-ui: drawing the looks failed: {e}");
        return;
    }
    surface.frame(qh, FrameCallbackData(surface.clone()));
    ll.surface.commit();
    ll.waiting = true;
}
