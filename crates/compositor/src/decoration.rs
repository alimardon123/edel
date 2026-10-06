//! Title bars on screen (M4.4): xdg-decoration with the server side
//! preferred, and KDE's server decoration protocol for GTK, which knows
//! only that one (M5.6a), each window's frame (its bar's texture, its border and where
//! it was before it was maximized), maximizing, and closing. The bar's
//! geometry and pixels are `edel_compositor::frame`'s; the title's font
//! loads on a thread of its own, so the first window never waits for it,
//! and bars drawn before it arrives are drawn again with their titles.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::backend::renderer::element::solid::SolidColorBuffer;
use smithay::desktop::Window;
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::calloop::channel::{self, Event};
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State;
use smithay::reexports::wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration::{
    Mode as KdeMode, OrgKdeKwinServerDecoration,
};
use smithay::reexports::wayland_server::WEnum;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Rectangle, Transform};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::kde::decoration::{KdeDecorationHandler, KdeDecorationState};
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;
use smithay::wayland::shell::xdg::{ToplevelSurface, XdgToplevelSurfaceData};

use edel_compositor::frame::{Insets, Look, Text, paint};
use edel_compositor::tokens::Tokens;

use crate::state::Edel;

/// What the compositor keeps for each window, in its user data.
#[derive(Default)]
pub struct FrameData {
    /// The bar's pixels and what they show.
    bar: Option<(MemoryRenderBuffer, Look)>,
    /// Left, right and bottom.
    pub borders: [SolidColorBuffer; 3],
    /// The frame before the window was maximized; set while it is.
    pub restore: Option<Rectangle<i32, Logical>>,
    /// It asked to be maximized before it was shown.
    pub maximize_when_placed: bool,
    /// The frame before the window went fullscreen; set while it is
    /// (M5.20).
    pub before_fullscreen: Option<Rectangle<i32, Logical>>,
    /// It asked to be fullscreen before it was shown.
    pub fullscreen_when_placed: bool,
    /// Its size and whether it has our bar, when last told to the policy.
    pub shape: Option<(smithay::utils::Size<i32, Logical>, bool)>,
    /// Its surfaces as last drawn, for its close animation (M5.11b).
    pub picture: Vec<crate::animate::Part>,
}

impl FrameData {
    /// The bar showing `look`, drawn again only if it shows something else.
    pub fn bar(
        &mut self,
        look: Look,
        tokens: &Tokens,
        text: Option<&mut Text>,
    ) -> &MemoryRenderBuffer {
        let stale = self.bar.as_ref().is_none_or(|(_, drawn)| *drawn != look);
        if stale {
            let size = (look.width.max(1), look.height.max(1));
            let full = Rectangle::from_size(size.into());
            let mut buffer = match self.bar.take() {
                Some((buffer, _)) => buffer,
                None => MemoryRenderBuffer::new(Fourcc::Argb8888, size, 1, Transform::Normal, None),
            };
            {
                let mut context = buffer.render();
                context.resize(size);
                let drawn: Result<(), std::convert::Infallible> = context.draw(|pixels| {
                    paint(pixels, &look, tokens, text);
                    Ok(vec![full])
                });
                let _ = drawn;
                context.update_opaque_regions(Some(vec![full]));
            }
            self.bar = Some((buffer, look));
        }
        &self.bar.as_ref().expect("drawn above").0
    }

    /// The bar's last pixels and their size, for a closing window's
    /// picture (M5.11b).
    pub fn last_bar(&self) -> Option<(MemoryRenderBuffer, (i32, i32))> {
        self.bar
            .as_ref()
            .map(|(buffer, look)| (buffer.clone(), (look.width, look.height)))
    }
}

/// The frame data of `window`, made when first asked for.
pub fn data(window: &Window) -> &RefCell<FrameData> {
    window
        .user_data()
        .insert_if_missing(|| RefCell::new(FrameData::default()));
    window
        .user_data()
        .get::<RefCell<FrameData>>()
        .expect("inserted above")
}

/// Whether the compositor draws `window`'s bar: it asked for that, or let
/// the compositor choose, through xdg-decoration or, as GTK does, through
/// KDE's protocol.
pub fn server_side(window: &Window) -> bool {
    let Some(toplevel) = window.toplevel() else {
        return false;
    };
    // Read in place: the pointer asks this of every window as it moves.
    with_states(toplevel.wl_surface(), |states| {
        let data = states
            .data_map
            .get::<XdgToplevelSurfaceData>()?
            .lock()
            .ok()?;
        Some(match data.current.decoration_mode {
            Some(mode) => mode == Mode::ServerSide,
            None => states
                .data_map
                .get::<KdeServerSide>()
                .is_some_and(|kde| kde.0.load(Ordering::Relaxed)),
        })
    })
    .unwrap_or(false)
}

/// A surface's choice through KDE's protocol: our bar or its own.
struct KdeServerSide(AtomicBool);

/// The window's title, or nothing.
pub fn title(window: &Window) -> String {
    let Some(toplevel) = window.toplevel() else {
        return String::new();
    };
    with_states(toplevel.wl_surface(), |states| {
        let data = states
            .data_map
            .get::<XdgToplevelSurfaceData>()?
            .lock()
            .ok()?;
        data.title.clone()
    })
    .unwrap_or_default()
}

/// Loads the title font, the tokens' interface `family`, on a thread; the
/// bars get their titles when it arrives, or stay without if no font
/// loads.
pub fn load_text(handle: &LoopHandle<'static, Edel>, family: &str, px: u32) {
    let family = family.to_string();
    let (sender, receiver) = channel::channel();
    let inserted = handle.insert_source(receiver, |event, _, state: &mut Edel| {
        if let Event::Msg(result) = event {
            match result {
                Ok(text) => {
                    state.text = Some(text);
                    state.dirty = true;
                }
                Err(e) => eprintln!("edel-compositor: {e:#}; title bars show no titles"),
            }
        }
    });
    if let Err(e) = inserted {
        eprintln!("edel-compositor: title bars show no titles: {e}");
        return;
    }
    let started = thread::Builder::new().name("font".into()).spawn(move || {
        let _ = sender.send(Text::load(&family, px as f32));
    });
    if let Err(e) = started {
        eprintln!("edel-compositor: title bars show no titles: {e}");
    }
}

impl Edel {
    /// What the frame adds round `window`: nothing for a window that draws
    /// its own, nor in tiling when `layout.title_bars = "floating-only"`,
    /// nor while it is fullscreen.
    pub fn insets(&self, window: &Window) -> Insets {
        if server_side(window) && self.bars_shown() && !self.is_fullscreen(window) {
            Insets::server_side(&self.tokens)
        } else {
            Insets::default()
        }
    }

    /// `window`'s frame on screen, or the window itself if it has none.
    pub fn frame_of(&self, window: &Window) -> Option<Rectangle<i32, Logical>> {
        Some(
            self.insets(window)
                .frame(self.space.element_geometry(window)?),
        )
    }

    pub fn is_maximized(&self, window: &Window) -> bool {
        data(window).borrow().restore.is_some()
    }

    pub fn toggle_maximized(&mut self, window: &Window) {
        if self.is_maximized(window) {
            self.unmaximize(window);
        } else {
            self.maximize(window);
        }
    }

    /// The window fills its screen, title bar and all.
    pub fn maximize(&mut self, window: &Window) {
        if self.is_fullscreen(window) {
            // It is maximized again when it leaves fullscreen.
            let before = data(window).borrow().before_fullscreen;
            let mut frame = data(window).borrow_mut();
            if frame.restore.is_none() {
                frame.restore = before;
            }
            return;
        }
        let (Some((_, area)), Some(frame)) = (self.home(window), self.frame_of(window)) else {
            return;
        };
        let mut data = data(window).borrow_mut();
        if data.restore.is_none() {
            data.restore = Some(frame);
        }
        drop(data);
        self.reframe(window, area, true);
    }

    /// The window goes back to where it was before it was maximized.
    pub fn unmaximize(&mut self, window: &Window) {
        let Some(restore) = data(window).borrow_mut().restore.take() else {
            return;
        };
        self.reframe(window, restore, false);
    }

    /// A maximized window dragged by its bar: it goes back to its size,
    /// under the pointer at the same place along its bar; returns the
    /// window's new place.
    pub fn unmaximize_under(
        &mut self,
        window: &Window,
        pointer: smithay::utils::Point<f64, Logical>,
    ) -> Option<Rectangle<i32, Logical>> {
        let now = self.frame_of(window)?;
        let restore = data(window).borrow_mut().restore.take()?;
        let along =
            ((pointer.x - f64::from(now.loc.x)) / f64::from(now.size.w.max(1))).clamp(0.0, 1.0);
        let x = pointer.x - along * f64::from(restore.size.w);
        let frame = Rectangle::new((x.round() as i32, now.loc.y).into(), restore.size);
        self.reframe(window, frame, false);
        // It follows the pointer at once, not a slide.
        self.animations.stop_slide(window);
        self.space.element_geometry(window)
    }

    /// Puts `window`'s frame at `frame` and tells the window its new size
    /// and whether it is maximized. A maximized window covers the screen
    /// over the policy's places; one back from maximized goes where the
    /// policy says, its tile in tiling. A window on a hidden workspace
    /// hears its state and size but stays hidden; its workspace's policy
    /// places it when the workspace is shown.
    fn reframe(&mut self, window: &Window, frame: Rectangle<i32, Logical>, maximized: bool) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let place = self.insets(window).window(frame);
        toplevel.with_pending_state(|state| {
            if maximized {
                state.states.set(State::Maximized);
            } else {
                state.states.unset(State::Maximized);
            }
            state.size = Some(place.size);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        if self.desks.hidden_on(window).is_some() {
            return;
        }
        if maximized {
            self.slide(window, place.loc);
            self.space.map_element(window.clone(), place.loc, true);
            self.dirty = true;
            self.state_changed();
        } else {
            self.placed(window, place);
        }
    }

    /// Asks `window` to close, as its close button and Super+Q do.
    pub fn close(&self, window: &Window) {
        if let Some(toplevel) = window.toplevel() {
            toplevel.send_close();
        }
    }

    /// The window with the keyboard.
    pub fn focused_window(&self) -> Option<Window> {
        let surface = self.seat.get_keyboard()?.current_focus()?;
        self.window_of(&surface)
    }
}

impl XdgDecorationHandler for Edel {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        set_mode(&toplevel, Mode::ServerSide);
    }

    /// A window that asks to draw its own bar does; one that asks for ours
    /// gets it.
    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: Mode) {
        set_mode(&toplevel, mode);
    }

    /// A window that leaves the choice to the compositor gets our bar.
    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        set_mode(&toplevel, Mode::ServerSide);
    }
}

/// GTK 4 and 3 ask through KDE's protocol, not xdg-decoration: a window
/// without a header bar of its own asks for ours, and one with a header
/// bar (libadwaita's apps, Firefox) for its own, as with xdg-decoration.
impl KdeDecorationHandler for Edel {
    fn kde_decoration_state(&self) -> &KdeDecorationState {
        &self.kde_decorations
    }

    fn request_mode(
        &mut self,
        surface: &WlSurface,
        decoration: &OrgKdeKwinServerDecoration,
        mode: WEnum<KdeMode>,
    ) {
        let WEnum::Value(mode) = mode else {
            return;
        };
        let server = mode == KdeMode::Server;
        decoration.mode(mode);
        with_states(surface, |states| {
            states
                .data_map
                .insert_if_missing_threadsafe(|| KdeServerSide(AtomicBool::new(false)));
            if let Some(kde) = states.data_map.get::<KdeServerSide>() {
                kde.0.store(server, Ordering::Relaxed);
            }
        });
        // A window already placed gets or loses its bar now.
        if self.window_of(surface).is_some() {
            self.relayout();
        }
    }
}

fn set_mode(toplevel: &ToplevelSurface, mode: Mode) {
    toplevel.with_pending_state(|state| state.decoration_mode = Some(mode));
    if toplevel.is_initial_configure_sent() {
        toplevel.send_pending_configure();
    }
}
