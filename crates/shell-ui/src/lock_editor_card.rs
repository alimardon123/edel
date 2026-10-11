//! The shell's side of the lock screen's edit mode (M5.10c6b): Settings'
//! Edit lock screen, or the session bus's `EditLockScreen`, puts the lock
//! screen up live over the whole main screen, on the overlay layer with the
//! keyboard exclusive. `lock_editor.rs` says what is drawn and where a drop
//! lands; this holds the surface, the pointer and the keyboard, and saves
//! what Done is pressed with into a look (M5.10c7a: a built-in look never
//! changes, so a change makes a new look of the person's; never a default, as
//! the panel editor's writes). Escape leaves without writing.
//! Done opens the Undo bar (`editor_card.rs`), which puts the person's line
//! back for ten seconds. Logs `lock editor shown, N parts, M in the drawer`,
//! `lock editor places ...`, `lock editor drag NAME`, `lock editor WORDS`,
//! `lock editor undo`, `lock editor done, WHAT`, `lock editor undone` and
//! `lock editor hidden`. The keyboard moves a focus round the parts and the
//! drawer (Tab), moves or takes away the focused part (the arrows, Shift for
//! ten steps, and Delete), and adds, undoes or finishes as its keys say
//! (M5.10c6c), logging `lock editor focus NAME`, `lock editor nudged NAME`
//! and `lock editor took away NAME`.

use std::collections::HashMap;
use std::rc::Rc;

use edel::i18n::tr;
use edel::lock_screen::{self, BACKGROUND, Background, LOOK, Part, Placed};
use edel::places;
use edel::presets::Edge;
use edel::settings::{self, LockScreen};
use edel::tokens::Tokens;
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

use crate::a11y;
use crate::corner;
use crate::edit_kit::{self, Style};
use crate::editor::{self, DRAG_START, Kind, Picture};
use crate::editor_card::{self, LockBefore, Undo};
use crate::lock::{self, Backdrops, Pictures};
use crate::lock_editor::{self, Drag, Focus, From, Step};
use crate::messages;
use crate::paint;
use crate::part_card;
use crate::player;
use crate::quick;
use crate::trayview::Key;
use crate::widgets::keyboard;
use crate::{LOCK_EDITOR, Shell, fillets, settings_texts};

/// What Done wrote, for its log line and for Undo (M5.10c6b, M5.10c6c3): the
/// words the log says, and the three lines' values before the write.
#[derive(Debug)]
struct Saved {
    what: String,
    before: LockBefore,
}

/// The log words of what Done did when nothing changed.
const NOTHING: &str = "nothing to write";
/// The settings key of the person's own looks (M5.10c5c), which Done writes
/// first, as the look must exist before the look line names it.
const LOOKS: &str = "lock_screen.looks";
/// The Settings page the part card's button opens (M5.10c6c2).
const LOCK_PAGE: &str = "lock_screen";

/// The lock screen's edit mode while it is open: its surface and keyboard,
/// the parts it started from (`original`), what it shows now (`view`, which
/// holds the parts being edited), whether the machine has a battery and the
/// screen is Compact, the background and wallpaper the parts stand on, and
/// what was drawn last. `ready` is set at the first configure and `waiting`
/// while the compositor has not shown the last drawing.
pub struct LockEditor {
    surface: LayerSurface,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pool: Option<SlotPool>,
    scale: u32,
    ready: bool,
    waiting: bool,
    shown: bool,
    original: Vec<Placed>,
    original_editing: String,
    original_background: Background,
    /// The person's lock settings over the machine's, as they were at open:
    /// what a look chosen in the Looks tab brings (`lock_editor::choose_look`).
    lock: LockScreen,
    view: lock_editor::View,
    battery: bool,
    compact: bool,
    /// Each look's name and parts: the built-in ones, then the person's own
    /// (M5.10c6c3). `thumbs` keeps each look's and background's picture, so
    /// each is drawn once.
    looks: Vec<(String, Vec<Placed>)>,
    thumbs: HashMap<String, Picture>,
    wallpaper: Wallpaper,
    pictures: Pictures,
    backdrops: Backdrops,
    drawn: Option<(lock_editor::View, u32)>,
    logged: String,
    /// The part card open beside a part (M5.10c6c2): the message's words as
    /// they were when it opened, and the card's places as last logged.
    card_before: Option<Placed>,
    card_logged: String,
    /// What a screen reader reads of the edit mode, updated at each drawing
    /// (M5.10c6c).
    reader: a11y::Reader,
}

/// Makes the drawer's tiles for the tab shown (M5.10c6c3): the parts not
/// shown, the looks or the backgrounds. The drawer keeps the height of its
/// tallest tab, so the tabs never make it jump.
fn refresh_tiles(le: &mut LockEditor) {
    let parts = lock_editor::drawer_tiles(&le.view.parts, le.battery);
    le.view.drawer.tallest = parts
        .len()
        .max(le.looks.len())
        .max(lock_editor::background_choices().len());
    le.view.drawer.tiles = match le.view.drawer.tab {
        1 => lock_editor::look_tiles(&le.looks, Some(le.view.editing.as_str())),
        2 => lock_editor::background_tiles(&le.view.background),
        _ => parts,
    };
    let (heading, subtitle) = lock_editor::drawer_words(&le.view);
    le.view.drawer.heading = Some(heading);
    le.view.drawer.subtitle = subtitle;
}

/// The parts were moved, added to or taken away from (or changed by a card):
/// they are the person's own now, so no look is in use (M5.10c6c3).
fn parts_moved(le: &mut LockEditor) {
    lock_editor::parts_moved(&mut le.view);
    refresh_tiles(le);
}

/// Shows the drawer's tab `tab` (M5.10c6c3), logging it. Nothing for a tab
/// there is not or the one shown already.
fn set_tab(le: &mut LockEditor, tab: usize) {
    if tab >= le.view.drawer.tabs.len() || tab == le.view.drawer.tab {
        return;
    }
    le.view.drawer.tab = tab;
    lock_editor::set_focus(
        &mut le.view,
        Some(lock_editor::Focus::Drawer(editor::Part::Tab(tab))),
    );
    refresh_tiles(le);
    let name = le.view.drawer.tabs[tab].to_lowercase();
    eprintln!("edel-shell-ui: lock editor tab {name}");
}

/// Chooses the look at `i` of the Looks tab (M5.10c6c3): the edit's parts
/// become its parts and its background its background, and the look is in
/// use until a part moves. Logs `lock editor look NAME`.
fn choose_look(le: &mut LockEditor, i: usize) {
    let Some(name) = lock_editor::choose_look(&mut le.view, &le.lock, &le.looks, le.compact, i)
    else {
        return;
    };
    refresh_tiles(le);
    eprintln!("edel-shell-ui: lock editor look {name}");
}

/// The lock screen's layout of `parts` on a screen `screen` logical pixels
/// big, as the lock draws it: the player's card at its one-page size, the
/// corner's buttons kept clear (`corner_view`, with no menu), the count shown
/// (two notifications, so the count is a part to move), the battery when the
/// machine has one, and `background` behind. Returns the layout and the
/// corner's layout.
fn layout_of(
    text: &mut paint::Text,
    tokens: &Tokens,
    screen: (f32, f32),
    parts: &[Placed],
    battery: bool,
    background: &Background,
    corner_view: &corner::View,
) -> (lock::Layout, corner::Layout) {
    let corner_at = corner::layout(screen.0, screen.1, corner_view);
    let look = lock::Look {
        parts,
        player: Some(player::wide_size(1)),
        people: false,
        keep_out: lock::keep_out(&corner_at),
        battery,
        background: background.clone(),
    };
    let at = lock::layout(screen.0, screen.1, tokens, true, &look, text);
    (at, corner_at)
}

/// Whether a drag has gone far enough to be a drag (`DRAG_START`).
fn far(drag: &Drag) -> bool {
    (drag.now.0 - drag.start.0).hypot(drag.now.1 - drag.start.1) >= DRAG_START
}

/// The name a drag's start shows in its log line: the part's, or the
/// drawer tile's.
fn name_of(view: &lock_editor::View, from: From) -> &'static str {
    match from {
        From::Part(i) => view.parts.get(i).map_or("part", |p| p.part.name()),
        From::Drawer(part) => part.name(),
    }
}

/// Moves the keyboard's focus to `focus` (M5.10c6c), logging the part it lands
/// on (`lock editor focus NAME`) when that changes it.
fn focus_to(le: &mut LockEditor, focus: Option<Focus>) {
    if le.view.focus == focus {
        return;
    }
    lock_editor::set_focus(&mut le.view, focus);
    if let Some(focus) = focus {
        eprintln!(
            "edel-shell-ui: lock editor focus {}",
            lock_editor::focus_name(&le.view, focus)
        );
    }
}

/// Moves the part at `i` by `dx`, `dy` steps by the arrow keys, its anchor
/// kept (`lock_screen::nudged`), logging `lock editor nudged NAME`.
fn nudge(le: &mut LockEditor, i: usize, dx: i32, dy: i32) {
    let Some((moved, name)) = le
        .view
        .parts
        .get(i)
        .map(|p| (lock_screen::nudged(p, dx, dy), p.part.name()))
    else {
        return;
    };
    le.view.parts[i] = moved;
    parts_moved(le);
    eprintln!("edel-shell-ui: lock editor nudged {name}");
}

/// Takes the part at `i` away (`lock_screen::without`, never the person),
/// logging `lock editor took away NAME`. The focus lets go, as the part is
/// gone.
fn take_away(le: &mut LockEditor, i: usize) {
    let Some(name) = le.view.parts.get(i).map(|p| p.part.name()) else {
        return;
    };
    le.view.parts = lock_screen::without(&le.view.parts, i);
    parts_moved(le);
    lock_editor::set_focus(&mut le.view, None);
    eprintln!("edel-shell-ui: lock editor took away {name}");
}

/// Adds the drawer's `part` at the screen's middle (`lock_editor::added_at_middle`,
/// as a drop there would), logging `lock editor added NAME`. The tile it
/// was is gone, so the focus lets go.
fn add(le: &mut LockEditor, part: Part) {
    let Some((parts, words)) = lock_editor::added_at_middle(&le.view, part) else {
        return;
    };
    le.view.parts = parts;
    parts_moved(le);
    lock_editor::set_focus(&mut le.view, None);
    eprintln!("edel-shell-ui: lock editor {words}");
}

/// Opens the card of the part at `index` beside it (M5.10c6c2), logging
/// `lock editor card NAME shown`. Nothing for a part with no card, or while a
/// card is open.
fn open_card(le: &mut LockEditor, index: usize) {
    let Some(placed) = le.view.parts.get(index).cloned() else {
        return;
    };
    if le.view.card.is_some() || !part_card::card_for(placed.part) {
        return;
    }
    eprintln!(
        "edel-shell-ui: lock editor card {} shown",
        placed.part.name()
    );
    le.card_before = Some(placed.clone());
    le.view.card = Some(part_card::open(index, placed));
}

/// Closes the open card (M5.10c6c2), logging `lock editor card hidden`. A
/// message whose words were all taken away keeps the words it had when the
/// card opened (`part_card::settled`). Nothing when no card is open.
fn end_card(le: &mut LockEditor) {
    let Some(card) = le.view.card.take() else {
        return;
    };
    if let (Some(before), Some(part)) = (le.card_before.take(), le.view.parts.get_mut(card.index)) {
        *part = part_card::settled(part.clone(), &before);
    }
    le.card_logged.clear();
    eprintln!("edel-shell-ui: lock editor card hidden");
}

/// Applies a card's choice to the part it changes (`part_card::apply`),
/// logging what the part is now (`lock editor WORDS`) and showing it at once.
/// A choice that changes nothing is no change and logs nothing.
fn choose(le: &mut LockEditor, choice: part_card::Choice) {
    let Some(card) = le.view.card.as_mut() else {
        return;
    };
    let Some(part) = le.view.parts.get_mut(card.index) else {
        return;
    };
    let next = part_card::apply(part, &choice);
    if next == *part {
        return;
    }
    eprintln!(
        "edel-shell-ui: lock editor {}",
        part_card::said(&choice, &next)
    );
    *part = next.clone();
    card.placed = next;
    parts_moved(le);
}

/// Moves the keyboard's focus on the open card to `line`.
fn card_focus(le: &mut LockEditor, line: Option<part_card::Line>) {
    if let Some(card) = le.view.card.as_mut() {
        part_card::set_focus(card, line);
    }
}

/// Lights the card's line under the pointer, `None` for none.
fn card_hover(le: &mut LockEditor, line: Option<part_card::Line>) {
    if let Some(card) = le.view.card.as_mut() {
        card.hover = line;
    }
}

impl Shell {
    /// Opens the lock screen's edit mode (M5.10c6b): nothing while the lock
    /// is up, the panel editor is open or this is open. It starts from the
    /// parts the lock shows now (`lock_screen::applied` of the person's
    /// settings over the machine's), with the wallpaper those settings name.
    pub fn open_lock_editor(&mut self) {
        if self.lock.is_some() || self.editor.is_some() || self.lock_editor.is_some() {
            return;
        }
        self.close_popups();
        let (machine, person) = settings_texts();
        let screen = self.screen_size();
        let compact = screen.0 > 0.0 && screen.0 < lock::COMPACT_BELOW;
        let dark = quick::is_dark(machine.as_deref(), person.as_deref());
        let lock_settings = lock::lock_settings(machine.as_deref(), person.as_deref());
        let applied = lock_screen::applied(&lock_settings, compact);
        let original = applied.parts;
        let battery = self.live.status.battery.is_some();
        // The looks the Looks tab offers: the built-in ones, then the
        // person's own (M5.10c6c3). The look in use is the file's, while it
        // is a look this file has and the parts are not set by hand.
        let mut looks: Vec<(String, Vec<Placed>)> = lock_screen::LOOKS
            .iter()
            .filter_map(|name| {
                lock_screen::look_parts(name, compact).map(|parts| (name.to_string(), parts))
            })
            .collect();
        if let Some(own) = &lock_settings.looks {
            looks.extend(
                own.iter()
                    .map(|(name, look)| (name.clone(), look.parts.clone())),
            );
        }
        // The edit starts inside the look that applies: the file's, when it
        // has one, else classic (M5.10c7a).
        let editing = lock_settings
            .look
            .clone()
            .filter(|name| looks.iter().any(|(n, _)| n == name))
            .unwrap_or_else(|| lock_screen::CLASSIC.to_string());
        let view = lock_editor::View {
            card: None,
            parts: original.clone(),
            editing: editing.clone(),
            changed: false,
            background: applied.background.clone(),
            screen,
            drag: None,
            guides: Vec::new(),
            hover: None,
            focus: None,
            drawer: editor::View {
                kind: Kind::LockScreen,
                tiles: Vec::new(),
                tabs: vec![tr("Parts"), tr("Looks"), tr("Background")],
                tab: 0,
                tallest: 0,
                compact,
                width: screen.0 as u32,
                hover: None,
                focus: None,
                lifted: None,
                taking: false,
                heading: None,
                subtitle: None,
            },
        };
        let surface = self.compositor.create_surface(&self.qh);
        let surface = self.layers.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some(LOCK_EDITOR),
            None,
        );
        // Over every panel, on the whole screen: the exclusive zone -1 keeps
        // the panels where they are, and the keyboard is the editor's alone.
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
                    Box::new(|shell: &mut Shell, _, event| shell.lock_editor_key(event)),
                )
                .inspect_err(|e| eprintln!("edel-shell-ui: no keyboard for the lock editor: {e}"))
                .ok()
        });
        let mut le = LockEditor {
            surface,
            keyboard,
            pool: None,
            scale: 1,
            ready: false,
            waiting: false,
            shown: false,
            original_editing: editing,
            original_background: view.background.clone(),
            original,
            lock: lock_settings,
            view,
            battery,
            compact,
            looks,
            thumbs: HashMap::new(),
            wallpaper: lock::wallpaper_of(machine.as_deref(), person.as_deref(), dark),
            pictures: Pictures::default(),
            backdrops: Backdrops::default(),
            drawn: None,
            logged: String::new(),
            card_before: None,
            card_logged: String::new(),
            reader: a11y::Reader::new(tr("Edit lock screen")),
        };
        refresh_tiles(&mut le);
        self.lock_editor = Some(le);
    }

    /// The screen's first configure: the surface's size is the screen's, and
    /// the edit mode may draw.
    pub fn lock_editor_configured(&mut self, size: (u32, u32)) {
        let Some(le) = self.lock_editor.as_mut() else {
            return;
        };
        le.view.screen = (size.0 as f32, size.1 as f32);
        le.view.drawer.width = size.0;
        le.view.drawer.compact = (size.0 as f32) < lock::COMPACT_BELOW;
        le.ready = true;
        self.draw_lock_editor();
    }

    /// The compositor showed the last drawing: the next may go.
    pub fn lock_editor_framed(&mut self) {
        if let Some(le) = self.lock_editor.as_mut() {
            le.waiting = false;
        }
        self.draw_lock_editor();
    }

    /// The screen's scale changed.
    pub fn lock_editor_scaled(&mut self, factor: i32) {
        if let Some(le) = self.lock_editor.as_mut() {
            le.scale = factor.clamp(1, 4) as u32;
        }
        self.draw_lock_editor();
    }

    /// Whether `surface` is the open edit mode's.
    pub fn is_lock_editor(&self, surface: &WlSurface) -> bool {
        self.lock_editor
            .as_ref()
            .is_some_and(|le| le.surface.wl_surface() == surface)
    }

    /// Draws the edit mode if what it shows changed and the compositor has
    /// shown the last drawing.
    pub fn draw_lock_editor(&mut self) {
        let Some(mut le) = self.lock_editor.take() else {
            return;
        };
        self.paint_lock_editor(&mut le);
        self.lock_editor = Some(le);
    }

    /// The corner's buttons as the lock shows them, with the keyboard
    /// layout's short name when there are two or more, and no menu.
    fn lock_editor_corner(&self) -> corner::View {
        let (layouts, active) = &self.live.keyboard;
        let layout = if layouts.len() >= 2 {
            layouts.get(*active).map(|name| keyboard::short(name))
        } else {
            None
        };
        corner::View {
            layout,
            menu: false,
            restart_waiting: false,
            hover: None,
            session: None,
            chip_w: 0.0,
            sessions: None,
        }
    }

    /// The lock's view as the edit mode shows it: the time and date now, the
    /// person, the field ready and empty, two unread notifications with no
    /// apps (so the count is a tile), and the battery the status area reads.
    fn lock_editor_look(&self) -> lock::View {
        let now = jiff::Zoned::now();
        let (time, date) = lock::time_and_date(&now);
        let mut view = lock::View::new(time, date, lock::user());
        // Two notices from two apps, so the count shows both its words and
        // its apps' tiles as it does with notifications waiting.
        view.notices = 2;
        view.apps = vec![
            ("Mail".into(), "mail".into()),
            ("Calendar".into(), "calendar".into()),
        ];
        view.when = Some(lock::When::of(&now));
        view.battery = self.battery_view();
        view
    }

    /// Paints `le` if it may draw and what it shows changed: the lock as it
    /// stands, the parts' outlines and drags, the corner, then the drawer.
    fn paint_lock_editor(&mut self, le: &mut LockEditor) {
        if !le.ready || le.waiting {
            return;
        }
        self.attach_thumbnails(le);
        let s = le.scale;
        let screen = le.view.screen;
        let shown = (le.view.clone(), s);
        if le.drawn.as_ref() == Some(&shown) {
            return;
        }
        let corner_view = self.lock_editor_corner();
        let (at, corner_at) = layout_of(
            &mut self.text,
            &self.tokens,
            screen,
            &le.view.parts,
            le.battery,
            &le.view.background,
            &corner_view,
        );
        let drawer = editor::layout(&le.view.drawer);
        let place = lock_editor::drawer_rect(screen, &drawer);
        let preview = lock_editor::preview(screen, &drawer);
        let (pw, ph) = (screen.0 as u32 * s, screen.1 as u32 * s);
        // The lock as the whole screen shows it, then laid smaller above
        // the drawer, so the drawer never covers a part.
        let Some(mut pixmap) = Pixmap::new(pw, ph) else {
            return;
        };
        let lock_view = self.lock_editor_look();
        lock::paint(
            &mut pixmap,
            &lock_view,
            &at,
            &self.tokens,
            &mut self.text,
            Some(&mut self.icons),
            &mut le.pictures,
            &mut le.backdrops,
            &le.wallpaper,
            s as f32,
        );
        corner::paint(
            &mut pixmap,
            &corner_view,
            &corner_at,
            &self.tokens,
            &mut self.text,
            s as f32,
        );
        lock_editor::draw_placeholders(
            &mut pixmap,
            &le.view,
            &at,
            &self.tokens,
            &mut self.text,
            s as f32,
        );
        // The lock laid smaller on the stage, then the edit's marks drawn in
        // the screen's own size over it, so their words and lines stay sharp.
        let lock = pixmap;
        let Some(mut pixmap) = Pixmap::new(pw, ph) else {
            return;
        };
        let picture = lock_editor::picture_rect(screen, preview);
        let whole = crate::popup::Rect::new(0.0, 0.0, screen.0, screen.1);
        edit_kit::stage(
            &mut pixmap,
            whole,
            picture,
            &self.tokens,
            Style::GLASS,
            s as f32,
        );
        edit_kit::stage_frame(
            &mut pixmap,
            &lock,
            picture,
            &self.tokens,
            Style::GLASS,
            s as f32,
        );
        lock_editor::paint(
            &mut pixmap,
            &le.view,
            &at,
            preview,
            &self.tokens,
            &mut self.text,
            s as f32,
        );
        lock_editor::paint_drawer(
            &mut pixmap,
            &le.view,
            &self.tokens,
            &mut self.text,
            s as f32,
        );
        let card = self.card_layout(le, &at, preview);
        if let (Some(view), Some(l)) = (le.view.card.as_ref(), card.as_ref()) {
            part_card::paint(&mut pixmap, view, l, &self.tokens, &mut self.text, s as f32);
        }
        let places = lock_editor::places(&le.view, &at, preview, place, &drawer);
        if places != le.logged {
            eprintln!("edel-shell-ui: lock editor places {places}");
            le.logged = places;
        }
        if let Some(l) = card.as_ref() {
            let log = part_card::places(l);
            if log != le.card_logged {
                eprintln!("edel-shell-ui: lock editor card places {log}");
                le.card_logged = log;
            }
        }
        let mut items = lock_editor::nodes(&le.view, &at, preview, place, &drawer);
        let mut focus = lock_editor::focus_node(&le.view, &at);
        if let (Some(view), Some(l)) = (le.view.card.as_ref(), card.as_ref()) {
            if view.focus.is_some() {
                focus = Some(items.len());
            }
            items.extend(part_card::nodes(view, l));
        }
        le.reader
            .update_focused((f64::from(screen.0), f64::from(screen.1)), items, focus);
        let bytes = (pw * ph * 4) as usize;
        if le.pool.is_none() {
            // Two buffers' room, as each drawing replaces the last.
            le.pool = SlotPool::new(2 * bytes, &self.shm)
                .inspect_err(|e| eprintln!("edel-shell-ui: no memory for the lock editor: {e}"))
                .ok();
        }
        let Some(pool) = le.pool.as_mut() else {
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
                eprintln!("edel-shell-ui: drawing the lock editor failed: {e}");
                return;
            }
        };
        paint::to_argb(&pixmap, canvas);
        let surface = le.surface.wl_surface().clone();
        surface.set_buffer_scale(s as i32);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        if let Err(e) = buffer.attach_to(&surface) {
            eprintln!("edel-shell-ui: drawing the lock editor failed: {e}");
            return;
        }
        surface.frame(&self.qh, FrameCallbackData(surface.clone()));
        le.surface.commit();
        le.waiting = true;
        le.drawn = Some(shown);
        if !le.shown {
            le.shown = true;
            eprintln!(
                "edel-shell-ui: lock editor shown, {} parts, {} in the drawer",
                le.view.parts.len(),
                le.view.drawer.tiles.len()
            );
        }
    }

    /// Gives each look's and each background's tile its picture (M5.10c6c3),
    /// drawn the first time and kept in `thumbs`, so each is drawn once. The
    /// Parts tab has none.
    fn attach_thumbnails(&mut self, le: &mut LockEditor) {
        let tab = le.view.drawer.tab;
        if tab == 0 {
            return;
        }
        let lock_view = self.lock_editor_look();
        for i in 0..le.view.drawer.tiles.len() {
            let name = le.view.drawer.tiles[i].name.clone();
            let (key, parts, background) = if tab == 1 {
                let Some((_, parts)) = le.looks.iter().find(|(n, _)| *n == name) else {
                    continue;
                };
                (
                    format!("look:{name}"),
                    parts.clone(),
                    lock_screen::look_background(&name),
                )
            } else {
                let choice = lock_editor::background_choices()
                    .into_iter()
                    .find(|(n, _, _)| *n == name)
                    .and_then(|(_, _, background)| background);
                let Some(background) = choice else {
                    continue;
                };
                (
                    format!("background:{name}"),
                    lock_screen::classic(le.compact),
                    background,
                )
            };
            let picture = match le.thumbs.get(&key) {
                Some(picture) => picture.clone(),
                None => {
                    let Some(pixmap) = lock_editor::thumbnail(
                        &lock_view,
                        &parts,
                        &background,
                        le.battery,
                        &self.tokens,
                        &mut self.text,
                        Some(&mut self.icons),
                        &mut le.pictures,
                        &mut le.backdrops,
                        &le.wallpaper,
                    ) else {
                        continue;
                    };
                    let picture = Picture(Rc::new(pixmap));
                    le.thumbs.insert(key, picture.clone());
                    picture
                }
            };
            le.view.drawer.tiles[i].picture = Some(picture);
        }
    }

    /// Chooses the background at `i` of the Background tab (M5.10c6c3): the
    /// edit's background becomes it and the stage is drawn with it. The
    /// picture opens Settings' lock screen page and changes nothing here.
    fn choose_background(&mut self, le: &mut LockEditor, i: usize) {
        let Some((_, _, background)) = lock_editor::background_choices().into_iter().nth(i) else {
            return;
        };
        match background {
            Some(background) => {
                eprintln!(
                    "edel-shell-ui: lock editor background {}",
                    lock_screen::background_value(&background)
                );
                lock_editor::background_chosen(&mut le.view, background);
                refresh_tiles(le);
            }
            None => {
                let argv = [
                    crate::SETTINGS.to_string(),
                    "--page".to_string(),
                    LOCK_PAGE.to_string(),
                ];
                self.launcher.spawn(&argv, "Settings");
                eprintln!("edel-shell-ui: lock editor picture in settings");
            }
        }
    }

    /// Acts on a tile of a drawer tab that is not the Parts one (M5.10c6c3):
    /// a look is chosen, or a background.
    fn drawer_choose(&mut self, le: &mut LockEditor, i: usize) {
        match le.view.drawer.tab {
            1 => choose_look(le, i),
            2 => self.choose_background(le, i),
            _ => {}
        }
    }

    /// The open card's layout beside its part on this screen (M5.10c6c2), from
    /// the lock's layout and the drawing's preview. None without an open card,
    /// or when its part is not drawn.
    fn card_layout(
        &self,
        le: &LockEditor,
        at: &lock::Layout,
        shown: lock_editor::Preview,
    ) -> Option<part_card::Layout> {
        let card = le.view.card.as_ref()?;
        let near = (*at.placed.get(card.index)?)?;
        Some(part_card::layout(
            card,
            shown.to_screen(near),
            le.view.screen,
            &self.tokens,
        ))
    }

    /// A key while a part's card is open (M5.10c6c2): `part_card::key` says
    /// what it does, and the card's step is done by `card_step`.
    fn lock_card_key(&mut self, key: Option<Key>, shift: bool, typed: Option<char>) {
        let Some(mut le) = self.lock_editor.take() else {
            return;
        };
        let step = match le.view.card.as_ref() {
            Some(card) => part_card::key(card, key, shift, typed),
            None => part_card::Step::Nothing,
        };
        self.card_step(&mut le, step);
        self.paint_lock_editor(&mut le);
        self.lock_editor = Some(le);
    }

    /// What a card's step does (M5.10c6c2): moves the focus, changes the part,
    /// closes the card, or opens Settings' Lock screen page through the
    /// launcher, as quick settings' pages open.
    fn card_step(&mut self, le: &mut LockEditor, step: part_card::Step) {
        match step {
            part_card::Step::Focus(line) => card_focus(le, line),
            part_card::Step::Choose(choice) => choose(le, choice),
            part_card::Step::Settings => {
                let argv = [
                    crate::SETTINGS.to_string(),
                    "--page".to_string(),
                    LOCK_PAGE.to_string(),
                ];
                self.launcher.spawn(&argv, "Settings");
            }
            part_card::Step::Close => end_card(le),
            part_card::Step::Nothing => {}
        }
    }

    /// The pointer while a part's card is open (M5.10c6c2): a press on the
    /// card is its control's; a press away from it closes the card and does
    /// nothing else, and no drag starts. A motion lights the line it is over.
    fn lock_card_pointer(
        &mut self,
        le: &mut LockEditor,
        event: &PointerEvent,
        at: &lock::Layout,
        shown: lock_editor::Preview,
        (sx, sy): (f32, f32),
    ) {
        let Some(l) = self.card_layout(le, at, shown) else {
            end_card(le);
            self.paint_lock_editor(le);
            return;
        };
        match &event.kind {
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                if l.card.contains(sx, sy) {
                    if let Some(hit) = part_card::hit(&l, sx, sy) {
                        card_focus(le, Some(hit.line()));
                    }
                    let step = part_card::press(&l, sx, sy);
                    self.card_step(le, step);
                } else {
                    end_card(le);
                }
                self.paint_lock_editor(le);
            }
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                let line = part_card::hit(&l, sx, sy).map(part_card::Hit::line);
                if le.view.card.as_ref().is_some_and(|c| c.hover != line) {
                    card_hover(le, line);
                    self.paint_lock_editor(le);
                }
            }
            PointerEventKind::Leave { .. } => {
                card_hover(le, None);
                self.paint_lock_editor(le);
            }
            _ => {}
        }
    }

    /// The layout the edit mode's pointer hits, with the drawer's place.
    fn lock_editor_geometry(
        &mut self,
        le: &LockEditor,
    ) -> (
        lock::Layout,
        editor::Layout,
        crate::popup::Rect,
        lock_editor::Preview,
    ) {
        let corner_view = self.lock_editor_corner();
        let (at, _) = layout_of(
            &mut self.text,
            &self.tokens,
            le.view.screen,
            &le.view.parts,
            le.battery,
            &le.view.background,
            &corner_view,
        );
        let drawer = editor::layout(&le.view.drawer);
        let place = lock_editor::drawer_rect(le.view.screen, &drawer);
        let shown = lock_editor::preview(le.view.screen, &drawer);
        (at, drawer, place, shown)
    }

    /// A key while the edit mode is open (M5.10c6c): `lock_editor::key` says
    /// what it does with the focus. Escape leaves without writing and Done
    /// (Return with nothing focused, or Done's button) writes. Tab's keyboard
    /// sends Shift+Tab as ISO_Left_Tab, which counts as Shift.
    pub fn lock_editor_key(&mut self, event: KeyEvent) {
        let Some(le) = self.lock_editor.as_ref() else {
            return;
        };
        let sym = event.keysym;
        let shift_tab = sym == Keysym::ISO_Left_Tab;
        let key = if shift_tab {
            Some(Key::Tab)
        } else {
            editor_card::key_of(sym)
        };
        if le.view.card.is_some() {
            // The open card takes the keys first (M5.10c6c2), letters too.
            let typed = event.utf8.as_deref().and_then(|text| text.chars().next());
            self.lock_card_key(key, self.shift || shift_tab, typed);
            return;
        }
        let Some(key) = key else {
            return;
        };
        // Space is Activate too, but with nothing focused it must not finish
        // the edit.
        if sym == Keysym::space && le.view.focus.is_none() {
            return;
        }
        let step = lock_editor::key(&le.view, key, self.shift || shift_tab);
        match step {
            Step::Done => self.close_lock_editor(true),
            Step::Close => self.close_lock_editor(false),
            step => {
                let Some(mut le) = self.lock_editor.take() else {
                    return;
                };
                match step {
                    Step::Focus(focus) => focus_to(&mut le, focus),
                    Step::Nudge(i, dx, dy) => nudge(&mut le, i, dx, dy),
                    Step::Remove(i) => take_away(&mut le, i),
                    Step::Add(part) => add(&mut le, part),
                    Step::Tab(i) => {
                        set_tab(&mut le, i);
                        focus_to(&mut le, Some(Focus::Drawer(editor::Part::Tab(i))));
                    }
                    Step::Look(i) => choose_look(&mut le, i),
                    Step::Background(i) => self.choose_background(&mut le, i),
                    Step::Undo => self.undo_lock_editor(&mut le),
                    Step::Open(i) => open_card(&mut le, i),
                    Step::Done | Step::Close | Step::Nothing => {}
                }
                self.paint_lock_editor(&mut le);
                self.lock_editor = Some(le);
            }
        }
    }

    /// The pointer on the edit mode: a press on a part or a drawer tile
    /// starts a drag, a drag's motion says what it is over, its release
    /// drops it, and Undo and Done in the drawer act on a press.
    pub fn lock_editor_pointer(&mut self, event: &PointerEvent) {
        let Some(mut le) = self.lock_editor.take() else {
            return;
        };
        let done = self.lock_editor_on_pointer(&mut le, event);
        self.lock_editor = Some(le);
        if done {
            self.close_lock_editor(true);
        }
    }

    /// The pointer's event on `le`; true when Done was pressed.
    fn lock_editor_on_pointer(&mut self, le: &mut LockEditor, event: &PointerEvent) -> bool {
        // The pointer on the screen (the drawer's) and on the lock screen
        // being edited, drawn smaller above it (the parts', the drags').
        let (sx, sy) = (event.position.0 as f32, event.position.1 as f32);
        let (at, drawer, place, shown) = self.lock_editor_geometry(le);
        let (x, y) = shown.to_lock((sx, sy));
        if le.view.card.is_some() {
            self.lock_card_pointer(le, event, &at, shown, (sx, sy));
            return false;
        }
        let inside = place.contains(sx, sy);
        let on_drawer = if inside {
            editor::hit(&drawer, sx - place.x, sy - place.y)
        } else {
            None
        };
        let on_part = at
            .placed
            .iter()
            .position(|r| r.is_some_and(|r| r.contains(x, y)));
        // A press is the pointer's own: the keyboard's focus lets go (M5.10c6c).
        if matches!(event.kind, PointerEventKind::Press { .. }) {
            lock_editor::set_focus(&mut le.view, None);
        }
        match &event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                let mut started = None;
                if let Some(drag) = le.view.drag.as_mut() {
                    drag.now = (x, y);
                    drag.over_drawer = inside;
                    if !drag.moving && far(drag) {
                        drag.moving = true;
                        started = Some(drag.from);
                    }
                }
                if let Some(from) = started {
                    eprintln!(
                        "edel-shell-ui: lock editor drag {}",
                        name_of(&le.view, from)
                    );
                }
                le.view.hover = on_part;
                le.view.drawer.hover = on_drawer;
                self.lock_editor_show_drag(le, &at);
            }
            PointerEventKind::Leave { .. } => {
                le.view.drag = None;
                le.view.hover = None;
                le.view.drawer.hover = None;
                le.view.guides.clear();
                le.view.drawer.lifted = None;
                le.view.drawer.taking = false;
                self.paint_lock_editor(le);
            }
            PointerEventKind::Press { button, .. } if *button == BTN_LEFT => {
                if inside {
                    match on_drawer {
                        Some(editor::Part::Undo) => self.undo_lock_editor(le),
                        Some(editor::Part::Done) => return true,
                        Some(editor::Part::Tab(i)) => set_tab(le, i),
                        Some(editor::Part::Tile(i)) if le.view.drawer.tab != 0 => {
                            self.drawer_choose(le, i)
                        }
                        Some(editor::Part::Tile(i)) => {
                            let part = le
                                .view
                                .drawer
                                .tiles
                                .get(i)
                                .and_then(|tile| lock_editor::part_named(&tile.name));
                            if let Some(part) = part {
                                le.view.drag = Some(Drag {
                                    from: From::Drawer(part),
                                    start: (x, y),
                                    now: (x, y),
                                    moving: false,
                                    over_drawer: false,
                                });
                            }
                        }
                        None => {}
                    }
                } else if let Some(i) = on_part {
                    le.view.drag = Some(Drag {
                        from: From::Part(i),
                        start: (x, y),
                        now: (x, y),
                        moving: false,
                        over_drawer: false,
                    });
                }
            }
            PointerEventKind::Release { button, .. } if *button == BTN_LEFT => {
                self.lock_editor_release(le, &at);
            }
            _ => {}
        }
        false
    }

    /// Shows a drag as it is: the guides it snaps to, the drawer's tile
    /// lifted or its outline when a part is over the drawer, then paints.
    fn lock_editor_show_drag(&mut self, le: &mut LockEditor, at: &lock::Layout) {
        let moving = le.view.drag.filter(|d| d.moving);
        match moving {
            Some(drag) => {
                lock_editor::dragged_rect(&mut le.view, at);
                le.view.drawer.lifted = match drag.from {
                    From::Drawer(part) => le
                        .view
                        .drawer
                        .tiles
                        .iter()
                        .position(|tile| tile.name == part.name()),
                    From::Part(_) => None,
                };
                le.view.drawer.taking = matches!(drag.from, From::Part(_)) && drag.over_drawer;
            }
            None => {
                le.view.guides.clear();
                le.view.drawer.lifted = None;
                le.view.drawer.taking = false;
            }
        }
        self.paint_lock_editor(le);
    }

    /// The pointer let go: a drag that moved is dropped where it is (its
    /// words logged, the parts changed, the drawer's tiles made again), a
    /// person dragged onto the drawer is refused and the log says so, and a
    /// press that did not move was a click.
    fn lock_editor_release(&mut self, le: &mut LockEditor, at: &lock::Layout) {
        let Some(drag) = le.view.drag else {
            return;
        };
        if drag.moving {
            match lock_editor::drop(&le.view, at) {
                Some((parts, words)) => {
                    le.view.parts = parts;
                    parts_moved(le);
                    eprintln!("edel-shell-ui: lock editor {words}");
                }
                None => {
                    if let From::Part(i) = drag.from {
                        if drag.over_drawer && !lock_screen::removable(&le.view.parts, i) {
                            eprintln!("edel-shell-ui: lock editor: the person stays");
                        }
                    }
                }
            }
        }
        if let (false, From::Part(i)) = (drag.moving, drag.from) {
            // A press and release that did not move open the part's card.
            open_card(le, i);
        }
        le.view.drag = None;
        le.view.guides.clear();
        le.view.drawer.lifted = None;
        le.view.drawer.taking = false;
        self.paint_lock_editor(le);
    }

    /// Undo in the drawer: the parts the edit mode started from, again.
    fn undo_lock_editor(&mut self, le: &mut LockEditor) {
        end_card(le);
        le.view.parts = le.original.clone();
        le.view.editing = le.original_editing.clone();
        le.view.changed = false;
        le.view.background = le.original_background.clone();
        refresh_tiles(le);
        le.view.drag = None;
        le.view.guides.clear();
        eprintln!("edel-shell-ui: lock editor undo");
        self.paint_lock_editor(le);
    }

    /// Writes the parts Done asks for, as the person's `lock_screen.parts`:
    /// the list as `lock_screen::parts_value` writes it, or the person's line
    /// taken out when what applies without it is what the edit shows (a
    /// default is never written), or nothing when nothing changed. None when
    /// the write fails, which is logged: the edit mode stays open.
    fn write_lock_editor(&self, le: &LockEditor) -> Option<Saved> {
        let (machine, person) = settings_texts();
        let person_lock: LockScreen = person
            .as_deref()
            .and_then(|text| settings::read(text).ok())
            .map(|read| read.file.lock_screen)
            .unwrap_or_default();
        let before = LockBefore {
            looks: person_lock
                .looks
                .as_ref()
                .and_then(|own| toml::Value::try_from(own).ok())
                .map(|value| value.to_string()),
            look: person_lock.look.clone(),
            parts: person_lock
                .parts
                .as_deref()
                .and_then(|parts| lock_screen::parts_value(parts).ok()),
            background: person_lock.background.clone(),
        };
        let lock = lock::lock_settings(machine.as_deref(), person.as_deref());
        let lines = match lock_editor::written(&le.view, &lock) {
            Ok(lines) => lines,
            Err(e) => {
                eprintln!("edel-shell-ui: {}", messages::lock_screen_not_kept(e));
                return None;
            }
        };
        // A line `written` leaves alone keeps its value; the looks go first,
        // as the look line names one of them.
        let edits = [
            (
                LOOKS,
                lines.looks.unwrap_or_else(|| before.looks.clone()),
                before.looks.clone(),
                LOOKS,
            ),
            (
                LOOK,
                lines.look.unwrap_or_else(|| before.look.clone()),
                before.look.clone(),
                LOOK,
            ),
            (
                lock_screen::PARTS,
                lines.parts.unwrap_or_else(|| before.parts.clone()),
                before.parts.clone(),
                "lock_screen.parts",
            ),
            (
                BACKGROUND,
                lines
                    .background
                    .unwrap_or_else(|| before.background.clone()),
                before.background.clone(),
                BACKGROUND,
            ),
        ];
        let changed: Vec<_> = edits
            .into_iter()
            .filter(|(_, new, old, _)| new != old)
            .collect();
        if changed.is_empty() {
            return Some(Saved {
                what: NOTHING.into(),
                before,
            });
        }
        let Some(path) = places::person_settings().map(|p| places::found(&p)) else {
            eprintln!("edel-shell-ui: {}", messages::LOCK_SCREEN_NO_HOME);
            return None;
        };
        let mut words = Vec::new();
        for (key, new, _, word) in changed {
            if let Err(e) = settings::write(&path, key, new.as_deref()) {
                eprintln!(
                    "edel-shell-ui: {}",
                    messages::lock_screen_not_kept(format!("{e:#}"))
                );
                return None;
            }
            words.push(format!(
                "{word} {}",
                if new.is_some() {
                    "written"
                } else {
                    "taken out"
                }
            ));
        }
        Some(Saved {
            what: words.join("; "),
            before,
        })
    }

    /// Closes the edit mode (M5.10c6b). With `write` (Done) the parts are
    /// written and, when that changed the file, the Undo bar opens where the
    /// drawer was, at the bottom of the screen; a failed write keeps the edit
    /// mode open. Without it (Escape, the keyboard leaving) nothing is
    /// written. Does nothing while the edit mode is not open.
    pub fn close_lock_editor(&mut self, write: bool) {
        if let Some(le) = self.lock_editor.as_mut() {
            end_card(le);
        }
        let Some(le) = self.lock_editor.as_ref() else {
            return;
        };
        if write {
            let Some(saved) = self.write_lock_editor(le) else {
                return;
            };
            eprintln!("edel-shell-ui: lock editor done, {}", saved.what);
            let scale = le.scale;
            let changed = saved.what != NOTHING;
            self.drop_lock_editor();
            if changed {
                let above = self.lock_bar_above();
                self.open_undo_bar(Edge::Bottom, above, scale, Undo::LockScreen(saved.before));
            }
        } else {
            if le.view.parts != le.original {
                eprintln!("edel-shell-ui: lock editor undone");
            }
            self.drop_lock_editor();
        }
    }

    /// Lets go of the edit mode's surface and keyboard, and logs it hidden.
    fn drop_lock_editor(&mut self) {
        let Some(le) = self.lock_editor.take() else {
            return;
        };
        if let Some(keyboard) = &le.keyboard {
            keyboard.release();
        }
        drop(le);
        eprintln!("edel-shell-ui: lock editor hidden");
    }

    /// How far above the bottom edge the Undo bar after Done starts: above the
    /// bottom panel as the panel editor's bar does, else at the edge.
    fn lock_bar_above(&self) -> i32 {
        let room = crate::paint::shadow_room(&self.tokens, !fillets());
        self.panels
            .iter()
            .find(|p| p.edge == Edge::Bottom)
            .map_or(0, |p| self.above_panel(p.style, p.size, p.floating, room))
    }

    /// Writes the person's `lock_screen.parts` as `value` (the list as
    /// `lock_screen::parts_value` writes it), or takes the line out for None,
    /// for Undo on the Undo bar (M5.10c6b). Returns what it did, or None after
    /// logging why it could not.
    /// Puts the three lines of the person's file back as Done found them
    /// (M5.10c6c3), each line written or taken out as it was. Returns the
    /// words the log says; None when a write fails, which is logged.
    pub(crate) fn save_lock_lines(&self, before: &LockBefore) -> Option<String> {
        let Some(path) = places::person_settings().map(|p| places::found(&p)) else {
            eprintln!("edel-shell-ui: {}", messages::LOCK_SCREEN_NO_HOME);
            return None;
        };
        let lines = [
            (LOOKS, before.looks.as_deref(), LOOKS),
            (LOOK, before.look.as_deref(), LOOK),
            (
                lock_screen::PARTS,
                before.parts.as_deref(),
                "lock_screen.parts",
            ),
            (BACKGROUND, before.background.as_deref(), BACKGROUND),
        ];
        let mut words = Vec::new();
        for (key, value, word) in lines {
            if let Err(e) = settings::write(&path, key, value) {
                eprintln!(
                    "edel-shell-ui: {}",
                    messages::lock_screen_not_kept(format!("{e:#}"))
                );
                return None;
            }
            words.push(format!(
                "{word} {}",
                if value.is_some() {
                    "written"
                } else {
                    "taken out"
                }
            ));
        }
        Some(words.join("; "))
    }
}
