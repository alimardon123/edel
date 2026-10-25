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
use smithay::utils::{Buffer, Logical, Rectangle, Transform};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::kde::decoration::{KdeDecorationHandler, KdeDecorationState};
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;
use smithay::wayland::shell::xdg::{ToplevelSurface, XdgToplevelSurfaceData};

use edel_compositor::corners;
use edel_compositor::frame::{Border, Insets, Look, Shown, Text, paint};
use edel_compositor::tokens::Tokens;

use crate::state::Edel;

/// What the compositor keeps for each window, in its user data.
#[derive(Default)]
pub struct FrameData {
    /// The bar's pixels and what they show.
    bar: Option<(MemoryRenderBuffer, Look)>,
    /// Left, right and bottom: plain colours, the bottom one unused while
    /// the border is a gradient (M5.12b2b).
    pub borders: [SolidColorBuffer; 3],
    /// The bottom strip of a gradient border: its pixels, their size and
    /// the border they show (M5.12b2b).
    pub strip: Option<(MemoryRenderBuffer, (i32, i32), Border)>,
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
    /// The border below a rounded title bar, as last drawn (M5.14).
    pub ring: Option<crate::rounded::Ring>,
}

impl FrameData {
    /// The bar showing `look`, drawn again only if it shows something else.
    pub fn bar(
        &mut self,
        look: Look,
        tokens: &Tokens,
        text: Option<&mut Text>,
        icon: Option<&edel::app_icons::Picture>,
    ) -> &MemoryRenderBuffer {
        let stale = self.bar.as_ref().is_none_or(|(_, drawn)| *drawn != look);
        if stale {
            let size = (look.width.max(1), look.height.max(1));
            let full = Rectangle::from_size(size.into());
            let mut buffer = match self.bar.take() {
                Some((buffer, _)) => buffer,
                None => MemoryRenderBuffer::new(Fourcc::Argb8888, size, 1, Transform::Normal, None),
            };
            let opaque = bar_opaque(size, look.radius);
            {
                let mut context = buffer.render();
                context.resize(size);
                let drawn: Result<(), std::convert::Infallible> = context.draw(|pixels| {
                    paint(pixels, &look, tokens, text, icon);
                    Ok(vec![full])
                });
                let _ = drawn;
                context.update_opaque_regions(Some(opaque));
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

    /// The bottom strip of a gradient border, `px` pixels by the strip's
    /// size, drawn again only when its size or its colours change
    /// (M5.12b2b).
    pub fn gradient(&mut self, px: (i32, i32), border: Border) -> &MemoryRenderBuffer {
        let stale = self
            .strip
            .as_ref()
            .is_none_or(|(_, drawn, colours)| *drawn != px || *colours != border);
        if stale {
            let size = (px.0.max(1), px.1.max(1));
            let full = Rectangle::from_size(size.into());
            let mut buffer = match self.strip.take() {
                Some((buffer, _, _)) => buffer,
                None => MemoryRenderBuffer::new(Fourcc::Argb8888, size, 1, Transform::Normal, None),
            };
            {
                let mut context = buffer.render();
                context.resize(size);
                let bytes = border.strip(size.0 as usize, size.1 as usize);
                let drawn: Result<(), std::convert::Infallible> = context.draw(|pixels| {
                    let n = bytes.len().min(pixels.len());
                    pixels[..n].copy_from_slice(&bytes[..n]);
                    Ok(vec![full])
                });
                let _ = drawn;
                context.update_opaque_regions(Some(vec![full]));
            }
            self.strip = Some((buffer, px, border));
        }
        &self.strip.as_ref().expect("drawn above").0
    }

    /// The gradient strip's last pixels, its size and its border, for a
    /// closing window (M5.12b2b).
    pub fn last_strip(&self) -> Option<(MemoryRenderBuffer, (i32, i32), Border)> {
        self.strip
            .as_ref()
            .map(|(buffer, px, border)| (buffer.clone(), *px, *border))
    }
}

/// The part of a title bar `size` pixels big that is opaque: all of it,
/// but for the top corners of `radius` pixels, which are see-through, so
/// what lies under them is drawn (M5.14).
fn bar_opaque(size: (i32, i32), radius: u32) -> Vec<Rectangle<i32, Buffer>> {
    let full = Rectangle::from_size(size.into());
    if radius == 0 {
        return vec![full];
    }
    let r = radius as f32;
    let squares = corners::corner_squares(size.0, size.1, [r, r, 0.0, 0.0])
        .into_iter()
        .map(|(x, y, w, h)| Rectangle::new((x, y).into(), (w, h).into()));
    Rectangle::subtract_rects_many([full], squares)
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

/// The window's title as people see it, or nothing: a web app's without
/// the browser's name after it (`edel::web_apps::shown_title`, M6.4), so
/// its title bar, the switcher and the overview name the website.
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
        let title = data.title.as_deref()?;
        let app_id = data.app_id.as_deref().unwrap_or_default();
        Some(edel::web_apps::shown_title(app_id, title).to_string())
    })
    .unwrap_or_default()
}

/// The window's app id, which names the app it belongs to, or nothing.
pub fn app_id(window: &Window) -> String {
    let Some(toplevel) = window.toplevel() else {
        return String::new();
    };
    with_states(toplevel.wl_surface(), |states| {
        let data = states
            .data_map
            .get::<XdgToplevelSurfaceData>()?
            .lock()
            .ok()?;
        data.app_id.clone()
    })
    .unwrap_or_default()
}

/// The apps people have and their icons, for the title bars (M5.6a):
/// each window's app found once by its app id, each icon drawn once at a
/// size and kept.
pub struct AppIcons {
    apps: Option<Apps>,
    by_app_id: std::collections::HashMap<String, Option<String>>,
    drawn: edel::app_icons::Icons,
    /// Where a thread reading the apps sends them (`off_loop.rs`); with
    /// none (a test) they are read in the frame.
    answers: Option<smithay::reexports::calloop::channel::Sender<crate::off_loop::Answer>>,
    /// Whether a thread is reading the apps now.
    reading: bool,
}

/// The apps read, with the change times of the folders they were read from
/// and when those were last looked at, so an app installed during the
/// session is found (the whole-system review of 2026-10-25).
struct Apps {
    list: Vec<edel::apps::App>,
    stamps: Vec<Option<std::time::SystemTime>>,
    looked: std::time::Instant,
}

/// How often a window whose app was not found looks again at whether the
/// app folders changed: a few `stat`s, at most this often.
const LOOK_AGAIN: std::time::Duration = std::time::Duration::from_secs(2);

/// The change times of the folders apps are read from.
fn stamps() -> Vec<Option<std::time::SystemTime>> {
    edel::apps::dirs()
        .iter()
        .map(|dir| std::fs::metadata(dir).and_then(|m| m.modified()).ok())
        .collect()
}

impl AppIcons {
    /// The icons of the theme `theme`, `appearance.icon_theme` (M8.21 part g).
    pub fn new(theme: &str) -> AppIcons {
        AppIcons {
            apps: None,
            by_app_id: std::collections::HashMap::new(),
            drawn: edel::app_icons::Icons::new(edel::apps::data_dirs(), theme),
            answers: None,
            reading: false,
        }
    }

    /// Reads the apps on a thread of their own from now on, its answer sent
    /// through `answers`, so reading the app folders never holds a frame
    /// (the whole-system review of 2026-10-25).
    pub fn answer_on(
        &mut self,
        answers: smithay::reexports::calloop::channel::Sender<crate::off_loop::Answer>,
    ) {
        self.answers = Some(answers);
    }

    /// Takes the apps a thread read: windows whose app was not found look
    /// again.
    fn read(&mut self, list: Vec<edel::apps::App>, stamps: Vec<Option<std::time::SystemTime>>) {
        self.apps = Some(Apps {
            list,
            stamps,
            looked: std::time::Instant::now(),
        });
        self.reading = false;
        self.by_app_id.retain(|_, icon| icon.is_some());
    }

    /// The icon theme these icons come from.
    pub fn theme(&self) -> &str {
        self.drawn.theme()
    }

    /// Draws the icons from `theme` from now on (M8.21 part g). Only the
    /// icons and the theme's chain are made again: the apps read stay, so a
    /// change never reads the apps again on the compositor's thread.
    pub fn set_theme(&mut self, theme: &str) {
        self.drawn = edel::app_icons::Icons::new(edel::apps::data_dirs(), theme);
    }

    /// The apps, read the first time they are asked for, and read again
    /// when `missed` (an app was not found) and an app folder changed since;
    /// with a thread to read them, the apps read before (none the first
    /// time) serve until it answers.
    fn apps(&mut self, missed: bool) -> &[edel::apps::App] {
        let stale = match &mut self.apps {
            None => true,
            Some(apps) if missed && apps.looked.elapsed() >= LOOK_AGAIN => {
                apps.looked = std::time::Instant::now();
                stamps() != apps.stamps
            }
            Some(_) => false,
        };
        if stale && !self.reading {
            match self.answers.clone() {
                Some(answers) => {
                    self.reading = true;
                    let started =
                        std::thread::Builder::new()
                            .name("apps".into())
                            .spawn(move || {
                                let stamps = stamps();
                                let list = edel::apps::read_all(&edel::apps::dirs());
                                let _ = answers.send(Box::new(
                                    move |state: &mut crate::state::Edel| {
                                        state.app_icons.read(list, stamps);
                                        state.dirty = true;
                                    },
                                ));
                            });
                    if let Err(e) = started {
                        self.reading = false;
                        eprintln!("edel-compositor: could not start the apps thread: {e}");
                    }
                }
                None => self.read(edel::apps::read_all(&edel::apps::dirs()), stamps()),
            }
        }
        self.apps.as_ref().map_or(&[], |apps| &apps.list)
    }

    /// The icon's name for windows with `app_id`; the apps are read the
    /// first time one is asked for, and again when one is not found and an
    /// app folder changed.
    pub fn name(&mut self, app_id: &str) -> Option<String> {
        if app_id.is_empty() {
            return None;
        }
        let missed = matches!(self.by_app_id.get(app_id), Some(None));
        if missed {
            // Looks again only when the folders changed.
            self.apps(true);
        }
        if !self.by_app_id.contains_key(app_id) {
            let icon = edel::apps::of_window(self.apps(false), app_id).and_then(|a| a.icon.clone());
            self.by_app_id.insert(app_id.to_string(), icon);
        }
        self.by_app_id.get(app_id).cloned().flatten()
    }

    /// The app's own name for windows with `app_id`, as its launcher
    /// file gives it, if it has one.
    pub fn app_name(&mut self, app_id: &str) -> Option<String> {
        let found = edel::apps::of_window(self.apps(false), app_id).map(|a| a.name.clone());
        found.or_else(|| edel::apps::of_window(self.apps(true), app_id).map(|a| a.name.clone()))
    }

    /// The icon `name` drawn `px` pixels square.
    pub fn picture(&mut self, name: &str, px: u32) -> Option<&edel::app_icons::Picture> {
        self.drawn.get(name, px)
    }
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
                    state.text_key += 1;
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
    /// its own, nor where `Settings::bars_in` gives no bars (M5.18b:
    /// `layout.title_bars` `never`, or `floating-only` in tiling), nor
    /// while it is fullscreen.
    pub fn insets(&self, window: &Window) -> Insets {
        if server_side(window) && self.bars_shown(window) && !self.is_fullscreen(window) {
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

    /// The buttons on `window`'s bar: the person's, but on a dialog (a
    /// window with a parent, such as Settings' questions) only close, as a
    /// dialog has nothing to minimize or to fill the screen with (M6.18).
    pub fn shown(&self, window: &Window) -> Shown {
        let mut shown = self.settings.buttons;
        if window.toplevel().is_some_and(|t| t.parent().is_some()) {
            shown.minimize = false;
            shown.maximize = false;
        }
        shown
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
