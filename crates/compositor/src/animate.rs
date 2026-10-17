//! Window animations on screen (roadmap M5.11b): a window fades in when it
//! opens, growing a little on Balanced and Full; a closed window's last
//! picture fades out, shrinking a little; a window the layout moves slides
//! to its new place; a workspace switch (M5.2f) is one spring per screen
//! (M5.14) that the shown windows' pictures and the other workspace's windows
//! share, so the pictures slide off the screen as the others slide in, and
//! three fingers on a touchpad can hold it. `edel_compositor::animation` holds the rules (how
//! long and how far, by tier and `appearance.animations`); this keeps what is
//! animating, starts it from the window events and gives `render.rs` each
//! window's look. While anything animates the backends draw a frame every
//! screen refresh; when it ends they stop, so an idle desktop still draws
//! nothing.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use smithay::backend::renderer::element::Id;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::backend::renderer::element::solid::SolidColorBuffer;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::utils::{CommitCounter, RendererSurfaceStateUserData};
use smithay::backend::renderer::{ContextId, Renderer};
use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};
use smithay::wayland::compositor::{TraversalAction, with_surface_tree_downward};

use edel_compositor::animation::{
    Anim, Curve, Motion, Slide, Style, Switch, closing, opening, style,
};
use edel_compositor::effects::Tier;

use crate::decoration::data;
use crate::state::Edel;

/// What is animating, on the clock frames are drawn by.
pub struct Animations {
    clock: Instant,
    pub style: Style,
    opening: HashMap<Window, Anim>,
    slides: HashMap<Window, Slide>,
    /// Each screen's workspace switch, by screen name, or `None` for one
    /// switch across every screen when workspaces are not per screen (M5.14).
    pub switches: HashMap<Option<String>, Switch>,
    /// Three fingers hold the workspaces, so a switch is made even where
    /// slides take no time (M5.14).
    pub swiping: bool,
    pub closing: Vec<Closing>,
    /// The renderer whose textures a closed window's picture is made of.
    context: Option<ContextId<GlesTexture>>,
    /// The overlay surfaces that popped up, each with when it did (M5.12b4),
    /// kept until the surface hides or is destroyed.
    popups: HashMap<WlSurface, Anim>,
    /// The popups' namespaces already logged (M5.12b4): each is said once
    /// a session, as its line, written while shell-ui writes its own to the
    /// same log, could otherwise cut into shell-ui's lines CI reads.
    popups_said: std::collections::HashSet<String>,
    /// When the last frame was drawn (`prune`'s `now`): a popup whose end
    /// came after it still wants one more frame, drawn whole.
    drawn: Duration,
    /// The tier, motion, speed and curve last logged.
    logged: Option<(Tier, Motion, u32, Curve)>,
}

/// A closed window's last picture, fading out, or a window's picture
/// sliding away with the workspace it is on.
pub struct Closing {
    /// It times the fade of a closed window; a leaving picture uses its
    /// spring instead.
    pub anim: Anim,
    /// How many windows were above it: it fades out at that depth.
    pub above: usize,
    /// Its frame, title bar included; it shrinks to the frame's centre.
    pub frame: Rectangle<i32, Logical>,
    /// Its surfaces, as they were last drawn, each with an id of its own
    /// for the fade.
    pub parts: Vec<(Id, Part)>,
    /// Its title bar's last pixels.
    pub bar: Option<Bar>,
    /// Its border, left, right and bottom; two when the bottom is a
    /// gradient, drawn as `strip` (M5.12b2b).
    pub borders: Vec<(SolidColorBuffer, Rectangle<i32, Logical>)>,
    /// The bottom strip of a gradient border, its last pixels and place.
    pub strip: Option<Bar>,
    /// Its picture of a window whose workspace is being left, moving with
    /// that screen's switch; none when it closed.
    pub leave: Option<Leaving>,
}

/// A window's picture of a workspace being left (M5.14): it is drawn at
/// `base` from its workspace's place, and moves with the switch of its
/// screen, which is the same spring as the incoming windows', so both move
/// as one. The picture goes when its switch ends or the window comes back.
pub struct Leaving {
    /// Where the picture is drawn from the incoming workspace's place: a
    /// screen's width away, plus where it was sliding.
    pub base: Point<f64, Logical>,
    /// The switch it moves with: its screen's, or `None` for all screens.
    pub screen: Option<String>,
    /// The window it is a picture of, so the picture goes when the window
    /// itself comes back.
    pub window: Window,
}

impl Closing {
    /// Whether it has ended by `now`. A leaving picture is pruned by its
    /// switch's end instead (`Animations::prune`), so it never ends here.
    pub fn done(&self, now: Duration) -> bool {
        match &self.leave {
            Some(_) => false,
            None => self.anim.done(now),
        }
    }
}

/// A closed window's title bar: its last pixels, their size and the bar's
/// place.
pub struct Bar {
    pub buffer: MemoryRenderBuffer,
    pub pixels: (i32, i32),
    pub place: Rectangle<i32, Logical>,
}

/// One surface of a window as last drawn: the texture it showed, kept
/// alive for a fade, and where it was, from the window's origin until it
/// closes and from the layout's after.
#[derive(Clone)]
pub struct Part {
    pub texture: GlesTexture,
    pub at: Point<i32, Logical>,
    pub src: Rectangle<f64, Logical>,
    pub size: Size<i32, Logical>,
    pub scale: i32,
    pub transform: Transform,
    /// The surface's commit it shows, so a live picture in the overview
    /// is drawn anew when the window draws (M5.2j-b4).
    pub commit: CommitCounter,
}

/// How a window is drawn this frame: its alpha, its size as a share of its
/// own, and how far from its place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub alpha: f32,
    pub zoom: f64,
    pub offset: Point<f64, Logical>,
}

impl Animations {
    pub fn new(style: Style) -> Animations {
        Animations {
            clock: Instant::now(),
            style,
            opening: HashMap::new(),
            slides: HashMap::new(),
            switches: HashMap::new(),
            swiping: false,
            closing: Vec::new(),
            context: None,
            popups: HashMap::new(),
            popups_said: std::collections::HashSet::new(),
            drawn: Duration::ZERO,
            logged: None,
        }
    }

    /// The time on the animations' clock.
    pub fn now(&self) -> Duration {
        self.clock.elapsed()
    }

    /// Forgets what ended by `now`; called before each frame is drawn.
    pub fn prune(&mut self, now: Duration) {
        self.opening.retain(|_, anim| !anim.done(now));
        self.slides.retain(|_, slide| !slide.done(now));
        self.switches.retain(|_, switch| !switch.done(now));
        // A leaving picture lives as long as its switch does.
        let switches = &self.switches;
        self.closing.retain(|c| match &c.leave {
            Some(leave) => switches.contains_key(&leave.screen),
            None => !c.done(now),
        });
        self.drawn = now;
    }

    /// Whether anything still animates, so another frame is wanted. A
    /// popup counts until a frame was drawn after its end, so its last
    /// frame on screen is the whole one, not one a step short of it (a
    /// greeter left a shade transparent until the next damage). A held
    /// switch counts for nothing: a still hand on a touchpad draws no
    /// frames (M5.14).
    pub fn running(&self) -> bool {
        let held = |screen: &Option<String>| self.switches.get(screen).is_some_and(Switch::held);
        let closing = self.closing.iter().any(|c| match &c.leave {
            Some(leave) => !held(&leave.screen),
            None => true,
        });
        let switching = !self.switches.values().all(Switch::held);
        !(self.opening.is_empty()
            && self.slides.is_empty()
            && !closing
            && !switching
            && popups_drawn_whole(self.popups.values(), self.drawn))
    }

    /// `window` was shown for the first time.
    pub fn opened(&mut self, window: &Window) {
        if !self.style.open.is_zero() {
            let anim = Anim::new(self.now(), self.style.open);
            self.opening.insert(window.clone(), anim);
        }
    }

    /// The layout moved `window` from `from` to `to`. A window still
    /// opening does not slide: it grows in where it ends up.
    pub fn moved(&mut self, window: &Window, from: Point<i32, Logical>, to: Point<i32, Logical>) {
        if self.opening.contains_key(window) {
            return;
        }
        let now = self.now();
        match Slide::start(
            self.slides.get(window),
            from,
            to,
            now,
            self.style.slide,
            self.style.curve,
        ) {
            Some(slide) => self.slides.insert(window.clone(), slide),
            None => self.slides.remove(window),
        };
    }

    /// `window` follows a person's pointer from now on, not a slide.
    pub fn stop_slide(&mut self, window: &Window) {
        self.slides.remove(window);
    }

    /// `window` left the screen.
    pub fn forget(&mut self, window: &Window) {
        self.opening.remove(window);
        self.slides.remove(window);
    }

    /// How `window` is drawn at `now`, on the workspace switch of `screen`
    /// (M5.14) as well as its own slide.
    pub fn look(&self, window: &Window, now: Duration, screen: &Option<String>) -> Look {
        let (alpha, zoom) = self.opening.get(window).map_or((1.0, 1.0), |anim| {
            opening(&self.style, anim.eased(now, self.style.curve))
        });
        let slide = self
            .slides
            .get(window)
            .map_or_else(Point::default, |slide| slide.offset_at(now));
        let switch = self
            .switches
            .get(screen)
            .map_or_else(Point::default, |switch| switch.offset_at(now));
        Look {
            alpha,
            zoom,
            offset: slide + switch,
        }
    }

    /// A switch of `by` begins on `screen` at `now` (M5.14): the switch there
    /// before, if any, is carried on, and the pictures already leaving there
    /// move `by` further along, so nothing on the screen jumps. False when
    /// the switch takes no time (and there is no swipe): then the switch on
    /// that screen and its pictures are dropped, and the windows jump.
    pub fn begin_switch(&mut self, screen: &Option<String>, by: Point<f64, Logical>) -> bool {
        let now = self.now();
        let length = self.style.slide;
        if length.is_zero() && !self.swiping {
            self.switches.remove(screen);
            self.closing
                .retain(|c| c.leave.as_ref().is_none_or(|l| &l.screen != screen));
            return false;
        }
        for closed in &mut self.closing {
            if let Some(leave) = closed.leave.as_mut().filter(|l| &l.screen == screen) {
                leave.base += by;
            }
        }
        let next = Switch::start(self.switches.get(screen), by, now, length, self.style.curve);
        self.switches.insert(screen.clone(), next);
        true
    }

    /// `window` is shown again: its leaving pictures go, as a window shown
    /// again is drawn itself, not its picture.
    pub fn came_back(&mut self, window: &Window) {
        self.closing
            .retain(|c| c.leave.as_ref().is_none_or(|l| &l.window != window));
    }

    /// How an overlay surface that popped up is drawn at `now`: its alpha
    /// and its size as a share of its own; none when it did not pop up.
    pub fn popup_look(&self, surface: &WlSurface, now: Duration) -> Option<(f32, f64)> {
        self.popups
            .get(surface)
            .map(|anim| opening(&self.style, anim.eased(now, self.style.curve)))
    }

    /// How a closed or leaving window is drawn at `now`.
    pub fn closing_look(&self, closed: &Closing, now: Duration) -> Look {
        if let Some(leaving) = &closed.leave {
            let switch = self
                .switches
                .get(&leaving.screen)
                .map_or_else(Point::default, |switch| switch.offset_at(now));
            let at = leaving.base + switch;
            return Look {
                alpha: 1.0,
                zoom: 1.0,
                offset: at,
            };
        }
        let progress = closed.anim.eased(now, self.style.curve);
        let (alpha, zoom) = closing(&self.style, progress);
        Look {
            alpha,
            zoom,
            offset: Point::default(),
        }
    }
}

impl Edel {
    /// Remembers the renderer whose textures closed windows' pictures are
    /// made of; called once it exists.
    pub fn start_animations(&mut self, renderer: &GlesRenderer) {
        self.animations.context = Some(renderer.context_id());
        self.restyle();
    }

    /// The tier or `appearance.animations` may have changed: the style follows,
    /// and the log says so whenever either changes, even when the style
    /// stays (Lite already only fades, so reduced motion changes nothing
    /// there).
    pub fn restyle(&mut self) {
        let tier = self.deadline.tier();
        let motion = self.settings.motion;
        let speed = self
            .settings
            .animation_speed
            .unwrap_or(edel::settings::ANIMATION_SPEED_DEFAULT);
        let curve = self.settings.animation_curve;
        let new = style(tier, motion, speed, curve);
        // Said once the renderer, and so the tier, is known, then on change.
        let known = self.animations.context.is_some();
        if known && self.animations.logged != Some((tier, motion, speed, curve)) {
            self.animations.logged = Some((tier, motion, speed, curve));
            eprintln!(
                "edel-compositor: animations at tier {tier}, motion {}: open {} ms, close {} ms, slide {} ms, speed {speed} %, curve {}",
                motion.name(),
                new.open.as_millis(),
                new.close.as_millis(),
                new.slide.as_millis(),
                curve.name()
            );
        }
        self.animations.style = new;
    }

    /// An overlay surface, such as a popup, shows a picture for the first
    /// time: it pops up, fading in and growing over the style's open length
    /// (M5.12b4). Said once per popup; with no open length nothing happens.
    pub fn popup_mapped(&mut self, surface: &WlSurface, namespace: &str) {
        let open = self.animations.style.open;
        if open.is_zero() || self.animations.popups.contains_key(surface) {
            return;
        }
        let now = self.animations.now();
        self.animations
            .popups
            .insert(surface.clone(), Anim::new(now, open));
        if self.animations.popups_said.insert(namespace.to_string()) {
            eprintln!(
                "edel-compositor: popup {namespace} opens over {} ms",
                open.as_millis()
            );
        }
        self.dirty = true;
    }

    /// An overlay surface hid or was destroyed: its next showing pops up
    /// again.
    pub fn popup_gone(&mut self, surface: &WlSurface) {
        self.animations.popups.remove(surface);
    }

    /// Keeps the textures `window`'s surfaces showed in the frame just
    /// drawn, for its close animation: a client that exits takes its
    /// surfaces with it, often before it says its window is gone.
    pub fn remember_picture(&self, window: &Window) {
        let Some(context) = &self.animations.context else {
            return;
        };
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let mut frame = data(window).borrow_mut();
        let picture = &mut frame.picture;
        picture.clear();
        with_surface_tree_downward(
            toplevel.wl_surface(),
            Point::default(),
            |_, states, at| {
                let view = states
                    .data_map
                    .get::<RendererSurfaceStateUserData>()
                    .and_then(|data| data.lock().ok().and_then(|d| d.view()));
                match view {
                    Some(view) => TraversalAction::DoChildren(*at + view.offset),
                    None => TraversalAction::SkipChildren,
                }
            },
            |_, states, at| {
                let Some(data) = states.data_map.get::<RendererSurfaceStateUserData>() else {
                    return;
                };
                let Ok(data) = data.lock() else {
                    return;
                };
                let (Some(view), Some(texture)) = (data.view(), data.texture(context.clone()))
                else {
                    return;
                };
                picture.push(Part {
                    texture: texture.clone(),
                    at: *at + view.offset,
                    src: view.src,
                    size: view.dst,
                    scale: data.buffer_scale(),
                    transform: data.buffer_transform(),
                    commit: data.current_commit(),
                });
            },
            |_, _, _| true,
        );
    }

    /// `window` closed: its last picture fades out where it was, if it
    /// was ever drawn.
    pub fn snapshot_closing(&mut self, window: &Window) {
        self.snapshot(window, None);
    }

    /// `window`'s workspace is being left on `screen`'s switch (M5.14), which
    /// began `by` away: its picture is drawn from where it is, moving with
    /// that switch. The window keeps its picture, for when its workspace is
    /// shown again. The caller snapshots only when `begin_switch` said the
    /// switch takes time, so slides taking none does not skip it here.
    pub fn snapshot_leaving(
        &mut self,
        window: &Window,
        by: Point<f64, Logical>,
        screen: Option<String>,
    ) {
        self.snapshot(window, Some((by, screen)));
    }

    /// Keeps `window`'s picture for a close, or for a workspace being left
    /// (`leave`: where its switch began, and the switch's screen). A closed
    /// window's picture is skipped when its close takes no time; a leaving
    /// one is not (see `snapshot_leaving`).
    fn snapshot(&mut self, window: &Window, leave: Option<(Point<f64, Logical>, Option<String>)>) {
        let style = self.animations.style;
        let Some(place) = self.space.element_geometry(window) else {
            return;
        };
        if leave.is_none() && style.close.is_zero() {
            return;
        }
        let origin = place.loc - window.geometry().loc;
        let insets = self.insets(window);
        let frame = insets.frame(place);
        let mut frame_data = data(window).borrow_mut();
        let picture = if leave.is_some() {
            frame_data.picture.clone()
        } else {
            std::mem::take(&mut frame_data.picture)
        };
        let parts: Vec<_> = picture
            .into_iter()
            .map(|part| {
                let at = origin + part.at;
                (Id::new(), Part { at, ..part })
            })
            .collect();
        if parts.is_empty() {
            return;
        }
        let bar = (insets.top > 0)
            .then(|| frame_data.last_bar())
            .flatten()
            .map(|(buffer, pixels)| Bar {
                buffer,
                pixels,
                place: Rectangle::new(frame.loc, (frame.size.w, insets.top).into()),
            });
        let strip = frame_data
            .last_strip()
            .filter(|(_, _, border)| border.start != border.end)
            .map(|(buffer, pixels, _)| Bar {
                buffer,
                pixels,
                place: crate::render::border_places(frame, insets)[2],
            });
        let sides = if strip.is_some() { 2 } else { 3 };
        let borders = if insets.top > 0 && (insets.side > 0 || insets.bottom > 0) {
            frame_data
                .borders
                .iter()
                .cloned()
                .zip(crate::render::border_places(frame, insets))
                .take(sides)
                .collect()
        } else {
            Vec::new()
        };
        drop(frame_data);
        let above = self
            .space
            .elements()
            .rev()
            .position(|w| w == window)
            .unwrap_or(0);
        let now = self.animations.now();
        // A picture still sliding is drawn where the slide has it, so it does
        // not jump; the slide is still here, as `hide` comes after.
        let leave = leave.map(|(by, screen)| {
            let slide = self
                .animations
                .slides
                .get(window)
                .map_or_else(Point::default, |slide| slide.offset_at(now));
            Leaving {
                base: by + slide,
                screen,
                window: window.clone(),
            }
        });
        let length = if leave.is_some() {
            Duration::ZERO
        } else {
            style.close
        };
        self.animations.closing.push(Closing {
            anim: Anim::new(now, length),
            above,
            frame,
            parts,
            bar,
            borders,
            strip,
            leave,
        });
        self.dirty = true;
    }
}

/// Whether every popup's end came by `drawn`, the time of the last frame
/// drawn, so each shows whole and no further frame is wanted for them.
fn popups_drawn_whole<'a>(popups: impl IntoIterator<Item = &'a Anim>, drawn: Duration) -> bool {
    popups.into_iter().all(|anim| anim.done(drawn))
}

#[cfg(test)]
mod popup_tests {
    use super::*;

    #[test]
    fn a_popup_wants_one_frame_drawn_after_its_end() {
        let anim = Anim::new(Duration::from_millis(0), Duration::from_millis(200));
        // The last frame was drawn at 190 ms: the popup was not yet whole.
        assert!(!popups_drawn_whole([&anim], Duration::from_millis(190)));
        // A frame drawn at 210 ms shows it whole; nothing more is wanted.
        assert!(popups_drawn_whole([&anim], Duration::from_millis(210)));
    }
}
