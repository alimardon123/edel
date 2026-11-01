//! The pop-up cards shell-ui shows over the desktop, named once (the
//! architecture review of 2026-10-31, finding 26): [`Card`] lists them,
//! [`Shell::card_of`] finds the one a surface is, and every event the
//! surfaces get (configure, scale, frame, pointer, the keyboard leaving,
//! closed) is one exhaustive `match` here, so a new card that misses an
//! event does not compile. Each card's own module still draws it and
//! answers its input. The panels, the desktop's surfaces and the lock
//! screen are not cards: they keep their own paths in `main.rs`.

use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind};

use crate::popup::Shown;
use crate::{Shell, app_windows, launcher, styles};

/// Every pop-up card, in the order [`Shell::card_of`] asks for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    Access,
    PanelMenu,
    Editor,
    LockEditor,
    LockLooks,
    UndoBar,
    Launcher,
    Clipboard,
    Styles,
    InUse,
    AppWindows,
    Quick,
    Banner,
    Osd,
    Centre,
    TrayGrid,
    Tooltip,
    Switcher,
}

impl Card {
    pub const ALL: [Card; 18] = [
        Card::Access,
        Card::PanelMenu,
        Card::Editor,
        Card::LockEditor,
        Card::LockLooks,
        Card::UndoBar,
        Card::Launcher,
        Card::Clipboard,
        Card::Styles,
        Card::InUse,
        Card::AppWindows,
        Card::Quick,
        Card::Banner,
        Card::Osd,
        Card::Centre,
        Card::TrayGrid,
        Card::Tooltip,
        Card::Switcher,
    ];

    /// Whether the card closes when the keyboard leaves it: the ones that
    /// take the keyboard, not the notices that only show something.
    fn closes_on_leave(self) -> bool {
        !matches!(
            self,
            Card::Access
                | Card::UndoBar
                | Card::Banner
                | Card::Osd
                | Card::Tooltip
                | Card::Switcher
        )
    }
}

impl Shell {
    /// The card whose surface `surface` is, if any.
    pub fn card_of(&self, surface: &WlSurface) -> Option<Card> {
        Card::ALL
            .into_iter()
            .find(|card| self.is_card(*card, surface))
    }

    fn is_card(&self, card: Card, surface: &WlSurface) -> bool {
        match card {
            Card::Access => self.is_access(surface),
            Card::PanelMenu => self.is_panel_menu(surface),
            Card::Editor => self.is_editor(surface),
            Card::LockEditor => self.is_lock_editor(surface),
            Card::LockLooks => self.is_lock_looks(surface),
            Card::UndoBar => self.is_undo_bar(surface),
            Card::Launcher => self.is_launcher(surface),
            Card::Clipboard => self.is_clipboard(surface),
            Card::Styles => self.is_styles(surface),
            Card::InUse => self.is_in_use(surface),
            Card::AppWindows => self.is_app_windows(surface),
            Card::Quick => self.is_quick(surface),
            Card::Banner => self.is_banner(surface),
            Card::Osd => self.is_osd(surface),
            Card::Centre => self.is_centre(surface),
            Card::TrayGrid => self.is_tray_grid(surface),
            Card::Tooltip => self.is_tooltip(surface),
            Card::Switcher => self.is_switcher(surface),
        }
    }

    /// The card's pop-up surface, for the events every card meets alike;
    /// None for the lock screen's editors, which are whole screens.
    fn card_popup(&mut self, card: Card) -> Option<&mut dyn Shown> {
        fn shown<V: Clone + PartialEq>(popup: &mut crate::popup::Popup<V>) -> &mut dyn Shown {
            popup
        }
        match card {
            Card::Access => self.access.as_mut().map(|c| shown(&mut c.popup)),
            Card::PanelMenu => self.panel_menu.as_mut().map(|c| shown(&mut c.popup)),
            Card::Editor => self.editor.as_mut().map(|c| shown(&mut c.popup)),
            Card::LockEditor | Card::LockLooks => None,
            Card::UndoBar => self.undo_bar.as_mut().map(|c| shown(&mut c.popup)),
            Card::Launcher => self.menu.as_mut().map(|c| shown(&mut c.popup)),
            Card::Clipboard => self.clipboard_popup().map(shown),
            Card::Styles => self.styles.as_mut().map(|c| shown(&mut c.popup)),
            Card::InUse => self.in_use_menu.as_mut().map(|c| shown(&mut c.popup)),
            Card::AppWindows => self.app_windows.as_mut().map(|c| shown(&mut c.popup)),
            Card::Quick => self.quick.as_mut().map(|c| shown(c.popup_mut())),
            Card::Banner => self.banner.as_mut().map(|c| shown(c.popup_mut())),
            Card::Osd => self.osd.as_mut().map(|c| shown(&mut c.popup)),
            Card::Centre => self.centre.as_mut().map(|c| shown(c.popup_mut())),
            Card::TrayGrid => self.tray_grid.as_mut().map(|c| shown(&mut c.popup)),
            Card::Tooltip => self.tooltip.as_mut().map(|c| shown(&mut c.popup)),
            Card::Switcher => self.flip.as_mut().map(shown),
        }
    }

    /// Draws the card if what it shows changed.
    fn draw_card(&mut self, card: Card) {
        match card {
            Card::Access => self.draw_access(),
            Card::PanelMenu => self.draw_panel_menu(),
            Card::Editor => self.draw_editor(),
            Card::LockEditor => self.draw_lock_editor(),
            Card::LockLooks => self.draw_lock_looks(),
            Card::UndoBar => self.draw_undo_bar(),
            Card::Launcher => self.draw_launcher(),
            Card::Clipboard => self.draw_clipboard(),
            Card::Styles => self.draw_styles(),
            Card::InUse => self.draw_in_use(),
            Card::AppWindows => self.draw_app_windows(),
            Card::Quick => self.draw_quick(),
            Card::Banner => self.draw_banner(),
            Card::Osd => self.draw_osd(),
            Card::Centre => self.draw_centre(),
            Card::TrayGrid => self.draw_tray_grid(),
            Card::Tooltip => self.draw_tooltip(),
            Card::Switcher => self.draw_switcher(),
        }
    }

    /// Closes or hides the card, as its own module does.
    pub fn close_card(&mut self, card: Card) {
        match card {
            Card::Access => self.close_access(),
            Card::PanelMenu => self.close_panel_menu(),
            Card::Editor => self.close_editor(false),
            Card::LockEditor => self.close_lock_editor(false),
            Card::LockLooks => self.close_lock_looks(),
            Card::UndoBar => self.hide_undo_bar(),
            Card::Launcher => self.close_launcher(),
            Card::Clipboard => self.close_clipboard(),
            Card::Styles => self.close_styles(),
            Card::InUse => self.close_in_use(),
            Card::AppWindows => self.close_app_windows(),
            Card::Quick => self.close_quick(),
            Card::Banner => self.hide_banner(),
            Card::Osd => self.hide_osd(),
            Card::Centre => self.close_centre(),
            Card::TrayGrid => self.close_tray_grid(),
            Card::Tooltip => self.hide_tooltip(),
            Card::Switcher => self.hide_switcher(),
        }
    }

    /// The compositor sized the card's surface.
    pub fn card_configured(&mut self, card: Card, size: (u32, u32)) {
        match card {
            Card::LockEditor => return self.lock_editor_configured(size),
            Card::LockLooks => return self.lock_looks_configured(size),
            Card::Centre => return self.centre_configured(size),
            _ => {}
        }
        if let Some(popup) = self.card_popup(card) {
            popup.configured();
        }
        self.draw_card(card);
    }

    /// The card's screen has a new scale.
    pub fn card_scaled(&mut self, card: Card, factor: i32) {
        match card {
            Card::LockEditor => return self.lock_editor_scaled(factor),
            Card::LockLooks => return self.lock_looks_scaled(factor),
            _ => {}
        }
        if let Some(popup) = self.card_popup(card) {
            popup.set_scale(factor);
        }
        self.draw_card(card);
    }

    /// The compositor showed the card's last drawing.
    pub fn card_framed(&mut self, card: Card) {
        match card {
            Card::LockEditor => return self.lock_editor_framed(),
            Card::LockLooks => return self.lock_looks_framed(),
            _ => {}
        }
        if let Some(popup) = self.card_popup(card) {
            popup.framed();
        }
        self.draw_card(card);
    }

    /// The keyboard left the card's surface.
    pub fn card_left(&mut self, card: Card) {
        if card.closes_on_leave() {
            self.close_card(card);
        }
    }

    /// A pointer event on the card's surface.
    pub fn card_pointer(&mut self, card: Card, event: &PointerEvent) {
        match card {
            Card::Access => self.access_pointer(event),
            Card::PanelMenu => self.panel_menu_pointer(event),
            Card::Editor => self.editor_pointer(event),
            Card::LockEditor => self.lock_editor_pointer(event),
            Card::LockLooks => self.lock_looks_pointer(event),
            Card::UndoBar => self.undo_pointer(event),
            Card::Launcher => self.launcher_pointer(event),
            Card::Clipboard => self.clipboard_pointer(event),
            Card::Styles => self.styles_pointer(event),
            Card::InUse => self.in_use_pointer(event),
            Card::AppWindows => self.app_windows_pointer(event),
            Card::Quick => self.quick_pointer(event),
            Card::Banner => self.banner_pointer(event),
            Card::Osd => self.osd_pointer(event),
            Card::Centre => self.centre_pointer(event),
            Card::TrayGrid => self.tray_pointer(event),
            // A tooltip and the switcher take no pointer.
            Card::Tooltip | Card::Switcher => {}
        }
    }

    fn launcher_pointer(&mut self, event: &PointerEvent) {
        // From the card's corner, inside the shadow's room.
        let room = self.menu.as_ref().map_or(0, |m| m.popup.room()) as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let row = launcher::row_at(y, &self.tokens).filter(|_| x >= 0.0);
        match &event.kind {
            PointerEventKind::Motion { .. } => {
                if let Some(row) = row.filter(|r| *r < self.launcher.view().names.len()) {
                    self.launcher.selected = row;
                    self.draw_launcher();
                }
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                if let Some(url) = self.launcher.install_url(row) {
                    self.install_web_app(&url);
                } else if row.is_some() && self.launcher.start(row).is_some() {
                    self.close_launcher();
                }
            }
            _ => {}
        }
    }

    fn in_use_pointer(&mut self, event: &PointerEvent) {
        let room = self.in_use_menu.as_ref().map_or(0, |m| m.popup.room()) as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        match &event.kind {
            PointerEventKind::Motion { .. } => self.in_use_motion(x, y),
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                self.in_use_press(x, y)
            }
            _ => {}
        }
    }

    fn app_windows_pointer(&mut self, event: &PointerEvent) {
        let room = self.app_windows.as_ref().map_or(0, |m| m.popup.room()) as f32;
        let count = self.app_windows.as_ref().map_or(0, |m| m.view.titles.len());
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let row = app_windows::row_at(y, count, &self.tokens).filter(|_| x >= 0.0);
        match &event.kind {
            PointerEventKind::Motion { .. } => {
                if let (Some(row), Some(menu)) = (row, &mut self.app_windows) {
                    menu.view.lit = row;
                    self.draw_app_windows();
                }
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                if let Some(row) = row {
                    self.choose_app_window(row);
                }
            }
            _ => {}
        }
    }

    fn styles_pointer(&mut self, event: &PointerEvent) {
        let room = self.styles.as_ref().map_or(0, |m| m.popup.room()) as f32;
        let (x, y) = (
            event.position.0 as f32 - room,
            event.position.1 as f32 - room,
        );
        let row = styles::row_at(y, &self.tokens).filter(|_| x >= 0.0);
        match &event.kind {
            PointerEventKind::Motion { .. } => {
                if let (Some(row), Some(menu)) = (row, &mut self.styles) {
                    menu.view.lit = row;
                    self.draw_styles();
                }
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                if let Some(row) = row {
                    self.choose_style(row);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every card's place in `ALL`; a card added to the enum and not to
    /// this match does not compile, and one missing from `ALL` fails.
    fn place(card: Card) -> usize {
        match card {
            Card::Access => 0,
            Card::PanelMenu => 1,
            Card::Editor => 2,
            Card::LockEditor => 3,
            Card::LockLooks => 4,
            Card::UndoBar => 5,
            Card::Launcher => 6,
            Card::Clipboard => 7,
            Card::Styles => 8,
            Card::InUse => 9,
            Card::AppWindows => 10,
            Card::Quick => 11,
            Card::Banner => 12,
            Card::Osd => 13,
            Card::Centre => 14,
            Card::TrayGrid => 15,
            Card::Tooltip => 16,
            Card::Switcher => 17,
        }
    }

    #[test]
    fn every_card_is_listed_once() {
        for (i, card) in Card::ALL.into_iter().enumerate() {
            assert_eq!(place(card), i, "{card:?}");
        }
    }

    #[test]
    fn the_keyboard_leaving_closes_the_cards_that_take_it() {
        let stay: Vec<Card> = Card::ALL
            .into_iter()
            .filter(|c| !c.closes_on_leave())
            .collect();
        assert_eq!(
            stay,
            [
                Card::Access,
                Card::UndoBar,
                Card::Banner,
                Card::Osd,
                Card::Tooltip,
                Card::Switcher
            ]
        );
    }
}
