//! Clipboard history's shell side (M7.13): ext-data-control-v1, through
//! which shell-ui sees each new selection, and the list's surface.
//!
//! Each selection whose offer lists a text or a PNG (`clipboard::wanted`,
//! never a password manager's secret) is read on a short thread and kept
//! in `clipboard::History`, in memory only, newest first, while
//! `clipboard.history` is on. Super+V (`edel-shell-v1`'s `clipboard`
//! event) opens the list, `edel-clipboard`, on the overlay layer with the
//! keyboard; the compositor puts it on the pointer's screen. Enter or a
//! click sets the chosen item as the selection again, from a source of our
//! own, closes the list and asks the compositor to paste it (`paste`);
//! Delete or a row's button removes an item and Clear all empties the
//! list. The lock (`lock_card.rs`) empties it, and so does turning the key
//! off. Without the protocol (another compositor) Super+V shows the empty
//! list and nothing is kept.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::time::Duration;

use edel::i18n::tr;
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::client::backend::ObjectData;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_seat};
use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;
use std::sync::Arc;

use crate::clipboard::{self, History, Item};
use crate::clipboard_list::{self, Layout, Part, View};
use crate::popup::{self, Popup};
use crate::{CLIPBOARD, Shell, a11y, fillets, paint, settings_texts};

/// How long a copying app may take to hand over what it copied.
const READ_WAIT: Duration = Duration::from_secs(2);

/// What shell-ui keeps of the clipboard: the protocol's objects, the
/// history and, while open, the list.
pub struct Clipboard {
    manager: Option<ExtDataControlManagerV1>,
    device: Option<ExtDataControlDeviceV1>,
    /// Our own source while it is the selection: its selection is not read.
    own: Option<ExtDataControlSourceV1>,
    /// Where the reading threads send what they read.
    read: Sender<Item>,
    pub history: History,
    pub list: Option<List>,
}

/// The open list: its surface, keyboard, screen reader and what it shows.
pub struct List {
    popup: Popup<View>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    reader: a11y::Reader,
    view: View,
    layout: Layout,
}

impl Clipboard {
    /// Binds the manager, if the compositor offers it, with the history on
    /// as `clipboard.history` says; what the threads read comes back to
    /// the loop `handle` runs.
    pub fn bind(
        globals: &GlobalList,
        qh: &QueueHandle<Shell>,
        handle: &LoopHandle<'static, Shell>,
    ) -> Clipboard {
        let manager = globals.bind(qh, 1..=1, Manager).ok();
        if manager.is_none() {
            eprintln!(
                "edel-shell-ui: the compositor offers no ext_data_control_manager_v1, so no clipboard history"
            );
        }
        let (read, items) = channel::channel();
        let inserted = handle.insert_source(items, |event, _, shell: &mut Shell| {
            if let channel::Event::Msg(item) = event {
                shell.clipboard_copied(item);
            }
        });
        if let Err(e) = inserted {
            eprintln!("edel-shell-ui: no clipboard history: {e}");
        }
        Clipboard {
            manager,
            device: None,
            own: None,
            read,
            history: History::new(history_on()),
            list: None,
        }
    }

    /// Listens to `seat`'s selections, once.
    pub fn listen(&mut self, seat: &wl_seat::WlSeat, qh: &QueueHandle<Shell>) {
        if let (Some(manager), None) = (&self.manager, &self.device) {
            self.device = Some(manager.get_data_device(seat, qh, Device));
        }
    }
}

/// Whether `clipboard.history` is on, the person's file over the
/// machine's; absent, it is.
fn history_on() -> bool {
    let (machine, person) = settings_texts();
    edel::settings::flag(
        edel::settings::CLIPBOARD_HISTORY,
        machine.as_deref(),
        person.as_deref(),
    )
    .unwrap_or(true)
}

impl Shell {
    /// A text or picture was read from a new selection.
    fn clipboard_copied(&mut self, item: Item) {
        // A copy still being read when the screen locked arrives after the
        // history was cleared: nothing copied stays past the lock (M7.13,
        // the whole-system review of 2026-10-25).
        if self.lock.is_some() {
            return;
        }
        let what = match &item {
            Item::Text(text) => format!("a text of {} bytes", text.len()),
            Item::Picture { png, .. } => format!("a picture of {} bytes", png.len()),
        };
        if self.clipboard.history.add(item) {
            eprintln!(
                "edel-shell-ui: clipboard kept {what}, {} kept",
                self.clipboard.history.len()
            );
            self.clipboard_list_changed();
        }
    }

    /// Super+V: the list opens, or closes if it is open.
    pub fn clipboard_key_pressed(&mut self) {
        if self.clipboard.list.is_some() {
            return self.close_clipboard();
        }
        if self.lock.is_some() {
            return;
        }
        self.close_popups();
        let (view, layout) = self.clipboard_build(0);
        let size = layout.card;
        let room = paint::shadow_room(&self.tokens, !fillets());
        let scale = self.panels.first().map_or(1, |p| p.scale);
        let Some(popup) = Popup::new(self, CLIPBOARD, size, size, scale, room) else {
            return;
        };
        // No anchor: the compositor centres it on the pointer's screen.
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
                    Box::new(|shell: &mut Shell, _, event| shell.clipboard_key(event)),
                )
                .inspect_err(|e| {
                    eprintln!("edel-shell-ui: no keyboard for the clipboard history: {e}")
                })
                .ok()
        });
        self.clipboard.list = Some(List {
            popup,
            keyboard,
            reader: a11y::Reader::new(tr("Clipboard history")),
            view,
            layout,
        });
    }

    /// The view and layout of the history now, `lit` kept inside it.
    fn clipboard_build(&mut self, lit: usize) -> (View, Layout) {
        let lit = lit.min(self.clipboard.history.len().saturating_sub(1));
        let width = self.screen_size().0 as u32;
        let view = clipboard_list::view(&self.clipboard.history, lit, width);
        let layout = clipboard_list::layout(&view, &self.tokens, &mut |text, size| {
            self.text.line(text, size).width
        });
        (view, layout)
    }

    /// The history changed: an open list is laid out again and drawn.
    fn clipboard_list_changed(&mut self) {
        let Some(list) = &self.clipboard.list else {
            return;
        };
        let (lit, hover) = (list.view.lit, list.view.hover);
        let (mut view, layout) = self.clipboard_build(lit);
        view.hover = hover;
        if let Some(list) = &mut self.clipboard.list {
            if layout.card != list.layout.card {
                list.popup.resize(layout.card, &self.compositor);
            }
            list.view = view;
            list.layout = layout;
        }
        self.draw_clipboard();
    }

    /// Closes the list and lets go of its surface, keyboard and buffers.
    pub fn close_clipboard(&mut self) {
        let Some(list) = self.clipboard.list.take() else {
            return;
        };
        if let Some(keyboard) = &list.keyboard {
            self.let_go(keyboard);
        }
        eprintln!("edel-shell-ui: clipboard history hidden");
    }

    /// Whether `surface` is the open list's.
    pub fn is_clipboard(
        &self,
        surface: &smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface,
    ) -> bool {
        self.clipboard
            .list
            .as_ref()
            .is_some_and(|l| l.popup.is(surface))
    }

    /// The list's surface was configured, scaled or framed.
    pub fn clipboard_popup(&mut self) -> Option<&mut Popup<View>> {
        self.clipboard.list.as_mut().map(|l| &mut l.popup)
    }

    /// Draws the list if what it shows changed; its first drawing logs its
    /// places, which CI reads.
    pub fn draw_clipboard(&mut self) {
        let Some(list) = &mut self.clipboard.list else {
            return;
        };
        let (items, focus) = clipboard_list::nodes(&list.view, &list.layout);
        let (w, h) = list.layout.card;
        list.reader
            .update_focused((f64::from(w), f64::from(h)), items, focus);
        let Some(mut pixmap) = list.popup.canvas(&list.view) else {
            return;
        };
        let scale = list.popup.scale();
        clipboard_list::paint(
            &mut pixmap,
            &list.view,
            &list.layout,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        let view = list.view.clone();
        if list.popup.show(
            view.clone(),
            &pixmap,
            &self.tokens,
            edel::settings::CLIPBOARD_HISTORY,
            &self.qh,
        ) {
            eprintln!(
                "edel-shell-ui: clipboard history shown, {} items",
                view.lines.len()
            );
            eprintln!(
                "edel-shell-ui: clipboard places {}",
                clipboard_list::places(&list.layout)
            );
        }
    }

    /// A key while the list is open: Escape closes, Return pastes the lit
    /// item, Delete removes it, Up, Down, Tab and Shift+Tab move.
    pub fn clipboard_key(&mut self, event: KeyEvent) {
        let Some(list) = &self.clipboard.list else {
            return;
        };
        let (lit, count) = (list.view.lit, self.clipboard.history.len());
        match event.keysym {
            Keysym::Escape => self.close_clipboard(),
            Keysym::Return | Keysym::KP_Enter => self.clipboard_paste(lit),
            Keysym::Delete | Keysym::KP_Delete => self.clipboard_remove(lit),
            Keysym::Up => self.clipboard_light(lit.saturating_sub(1)),
            Keysym::Down => self.clipboard_light((lit + 1).min(count.saturating_sub(1))),
            Keysym::Tab => self.clipboard_light(step(lit, count, popup::tab_step(self.shift))),
            Keysym::ISO_Left_Tab => self.clipboard_light(step(lit, count, -1)),
            _ => {}
        }
    }

    /// Lights row `lit`.
    fn clipboard_light(&mut self, lit: usize) {
        if let Some(list) = &mut self.clipboard.list {
            if list.view.lit != lit {
                list.view.lit = lit;
                self.draw_clipboard();
            }
        }
    }

    /// The pointer on the list, from the card's corner.
    pub fn clipboard_pointer(&mut self, event: &PointerEvent) {
        let Some(list) = &mut self.clipboard.list else {
            return;
        };
        let room = list.popup.room() as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let part = clipboard_list::hit(&list.layout, x, y);
        match &event.kind {
            PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                let lit = match part {
                    Some(Part::Row(i) | Part::Remove(i)) => i,
                    _ => list.view.lit,
                };
                if list.view.hover != part || list.view.lit != lit {
                    list.view.hover = part;
                    list.view.lit = lit;
                    self.draw_clipboard();
                }
            }
            PointerEventKind::Leave { .. } => {
                if list.view.hover.take().is_some() {
                    self.draw_clipboard();
                }
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => match part {
                Some(Part::Row(i)) => self.clipboard_paste(i),
                Some(Part::Remove(i)) => self.clipboard_remove(i),
                Some(Part::Clear) => {
                    self.clipboard.history.clear();
                    eprintln!("edel-shell-ui: clipboard history cleared");
                    self.clipboard_list_changed();
                }
                None => {}
            },
            _ => {}
        }
    }

    /// Removes item `index`.
    fn clipboard_remove(&mut self, index: usize) {
        if self.clipboard.history.remove(index).is_some() {
            eprintln!("edel-shell-ui: clipboard item {index} removed");
            self.clipboard_list_changed();
        }
    }

    /// Item `index` becomes the selection, from our own source, the list
    /// closes and the compositor pastes it into the window under it.
    fn clipboard_paste(&mut self, index: usize) {
        let Some(item) = self.clipboard.history.get(index).cloned() else {
            return;
        };
        self.close_clipboard();
        let (Some(manager), Some(device)) = (&self.clipboard.manager, &self.clipboard.device)
        else {
            return;
        };
        let source = manager.create_data_source(&self.qh, Source(item.clone()));
        source.offer(item.mime().to_string());
        if let Item::Text(_) = item {
            for mime in &clipboard::TEXT_TYPES[1..] {
                source.offer(mime.to_string());
            }
        }
        device.set_selection(Some(&source));
        if let Some(old) = self.clipboard.own.replace(source) {
            old.destroy();
        }
        // The chosen item is the newest again.
        self.clipboard.history.add(item);
        self.link.paste();
        eprintln!("edel-shell-ui: clipboard item {index} pasted");
    }

    /// The screen locks or the key turns off: the list closes and the
    /// history empties.
    pub fn clipboard_forget(&mut self, why: &str) {
        self.close_clipboard();
        if !self.clipboard.history.is_empty() {
            self.clipboard.history.clear();
            eprintln!("edel-shell-ui: clipboard history emptied, {why}");
        }
    }

    /// The settings files changed: `clipboard.history` is followed.
    pub fn clipboard_settings_changed(&mut self) {
        let on = history_on();
        if on != self.clipboard.history.on() {
            if !on {
                self.clipboard_forget("clipboard.history is off");
            }
            self.clipboard.history.set_on(on);
            eprintln!(
                "edel-shell-ui: clipboard history {}",
                if on { "on" } else { "off" }
            );
        }
    }
}

/// The row after (or before, `by` -1) `lit` of `count`, wrapping as Tab does.
fn step(lit: usize, count: usize, by: i32) -> usize {
    if count == 0 {
        return 0;
    }
    (lit as i64 + i64::from(by)).rem_euclid(count as i64) as usize
}

/// The manager's user data.
struct Manager;

impl Dispatch2<ExtDataControlManagerV1, Shell> for Manager {
    fn event(
        &self,
        _: &mut Shell,
        _: &ExtDataControlManagerV1,
        _: <ExtDataControlManagerV1 as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
    }
}

/// The device's user data.
struct Device;

impl Dispatch2<ExtDataControlDeviceV1, Shell> for Device {
    fn event(
        &self,
        shell: &mut Shell,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        use ext_data_control_device_v1::Event;
        match event {
            Event::Selection { id: Some(offer) } => {
                let types = offer
                    .data::<Offer>()
                    .map(|o| o.0.lock().unwrap().clone())
                    .unwrap_or_default();
                let ours = shell.clipboard.own.is_some();
                let wanted = clipboard::wanted(&types);
                if !ours && shell.clipboard.history.on() && shell.lock.is_none() {
                    match wanted {
                        Some(mime) => receive(&offer, mime, shell.clipboard.read.clone()),
                        None if types.iter().any(|t| t == clipboard::SECRET) => {
                            eprintln!("edel-shell-ui: clipboard skipped a secret");
                        }
                        None => {}
                    }
                }
                offer.destroy();
            }
            Event::Finished => {
                eprintln!("edel-shell-ui: clipboard device finished");
                shell.clipboard.device = None;
            }
            _ => {}
        }
    }

    /// The device's `data_offer` event makes an offer.
    fn event_created_child(_: u16, qh: &QueueHandle<Shell>) -> Arc<dyn ObjectData> {
        qh.make_data::<ExtDataControlOfferV1, Offer>(Offer::default())
    }
}

/// An offer's user data: the MIME types it lists.
#[derive(Default)]
struct Offer(Mutex<Vec<String>>);

impl Dispatch2<ExtDataControlOfferV1, Shell> for Offer {
    fn event(
        &self,
        _: &mut Shell,
        _: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            self.0.lock().unwrap().push(mime_type);
        }
    }
}

/// Reads `offer` as `mime` on a short thread, up to the size kept, and
/// sends what it makes an item of to `to`.
fn receive(offer: &ExtDataControlOfferV1, mime: &'static str, to: Sender<Item>) {
    let Ok((mut ours, theirs)) = UnixStream::pair() else {
        return;
    };
    offer.receive(mime.to_string(), std::os::fd::AsFd::as_fd(&theirs));
    drop(theirs);
    let most = if mime == clipboard::PICTURE_TYPE {
        clipboard::MOST_PICTURE
    } else {
        clipboard::MOST_TEXT
    };
    let spawned = std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || {
            let _ = ours.set_read_timeout(Some(READ_WAIT));
            let mut bytes = Vec::new();
            if std::io::Read::by_ref(&mut ours)
                .take(most as u64 + 1)
                .read_to_end(&mut bytes)
                .is_err()
            {
                return;
            }
            let item = if mime == clipboard::PICTURE_TYPE {
                Item::picture(bytes)
            } else {
                String::from_utf8(bytes).ok().and_then(Item::text)
            };
            if let Some(item) = item {
                let _ = to.send(item);
            }
        });
    if let Err(e) = spawned {
        eprintln!("edel-shell-ui: clipboard not read: {e}");
    }
}

/// Our source's user data: the item it hands over.
struct Source(Item);

impl Dispatch2<ExtDataControlSourceV1, Shell> for Source {
    fn event(
        &self,
        shell: &mut Shell,
        source: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { fd, .. } => {
                // Written on a thread, so a slow reader never holds the shell.
                let bytes = self.0.bytes().to_vec();
                let _ = std::thread::Builder::new()
                    .name("clipboard".into())
                    .spawn(move || {
                        let mut out = std::fs::File::from(fd);
                        let _ = out.write_all(&bytes);
                    });
            }
            ext_data_control_source_v1::Event::Cancelled => {
                if shell.clipboard.own.as_ref() == Some(source) {
                    shell.clipboard.own = None;
                }
                source.destroy();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_steps_round_the_rows() {
        assert_eq!(step(0, 3, 1), 1);
        assert_eq!(step(2, 3, 1), 0);
        assert_eq!(step(0, 3, -1), 2);
        assert_eq!(step(0, 0, 1), 0);
    }
}
