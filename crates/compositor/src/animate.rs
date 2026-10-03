//! Window animations on screen (roadmap M5.11b): a window fades in when it
//! opens, growing a little on Balanced and Full; a closed window's last
//! picture fades out, shrinking a little; a window the layout moves slides
//! to its new place; on a workspace switch (M5.2f) the shown windows'
//! pictures slide off the screen while the other workspace's slide in. `edel_compositor::animation` holds the rules (how
//! long and how far, by tier and `appearance.motion`); this keeps what is
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
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use smithay::backend::renderer::{ContextId, Renderer};
use smithay::desktop::Window;
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};
use smithay::wayland::compositor::{TraversalAction, with_surface_tree_downward};

use edel_compositor::animation::{Anim, Motion, Slide, Style, closing, opening, style};
use edel_compositor::effects::Tier;

use crate::decoration::data;
use crate::state::Edel;

/// What is animating, on the clock frames are drawn by.
pub struct Animations {
    clock: Instant,
    pub style: Style,
    opening: HashMap<Window, Anim>,
    slides: HashMap<Window, Slide>,
    pub closing: Vec<Closing>,
    /// The renderer whose textures a closed window's picture is made of.
    context: Option<ContextId<GlesTexture>>,
    /// The tier and motion last logged.
    logged: Option<(Tier, Motion)>,
}

/// A closed window's last picture, fading out, or a window's picture
/// sliding away with the workspace it is on.
pub struct Closing {
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
    /// Its border, left, right and bottom.
    pub borders: Vec<(SolidColorBuffer, Rectangle<i32, Logical>)>,
    /// How far it slides when its workspace is left; none when it closed.
    pub leave: Option<Point<i32, Logical>>,
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
            closing: Vec::new(),
            context: None,
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
        self.slides.retain(|_, slide| !slide.anim.done(now));
        self.closing.retain(|c| !c.anim.done(now));
    }

    /// Whether anything still animates, so another frame is wanted.
    pub fn running(&self) -> bool {
        !(self.opening.is_empty() && self.slides.is_empty() && self.closing.is_empty())
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
        match Slide::start(self.slides.get(window), from, to, now, self.style.slide) {
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

    /// How `window` is drawn at `now`.
    pub fn look(&self, window: &Window, now: Duration) -> Look {
        let (alpha, zoom) = self
            .opening
            .get(window)
            .map_or((1.0, 1.0), |anim| opening(&self.style, anim.eased(now)));
        let offset = self
            .slides
            .get(window)
            .map_or_else(Point::default, |slide| slide.offset_at(now));
        Look {
            alpha,
            zoom,
            offset,
        }
    }

    /// How a closed or leaving window is drawn at `now`.
    pub fn closing_look(&self, closed: &Closing, now: Duration) -> Look {
        let progress = closed.anim.eased(now);
        if let Some(by) = closed.leave {
            return Look {
                alpha: 1.0,
                zoom: 1.0,
                offset: (f64::from(by.x) * progress, f64::from(by.y) * progress).into(),
            };
        }
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

    /// The tier or `appearance.motion` may have changed: the style follows,
    /// and the log says so whenever either changes, even when the style
    /// stays (Lite already only fades, so reduced motion changes nothing
    /// there).
    pub fn restyle(&mut self) {
        let tier = self.deadline.tier();
        let motion = self.settings.motion;
        let new = style(tier, motion);
        // Said once the renderer, and so the tier, is known, then on change.
        let known = self.animations.context.is_some();
        if known && self.animations.logged != Some((tier, motion)) {
            self.animations.logged = Some((tier, motion));
            eprintln!(
                "edel-compositor: animations at tier {tier}, motion {}: open {} ms, close {} ms, slide {} ms",
                motion.name(),
                new.open.as_millis(),
                new.close.as_millis(),
                new.slide.as_millis()
            );
        }
        self.animations.style = new;
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

    /// `window`'s workspace is being left: its picture slides `by` off
    /// the screen, at the tier's slide length. The window keeps its
    /// picture, for when its workspace is shown again.
    pub fn snapshot_leaving(&mut self, window: &Window, by: Point<i32, Logical>) {
        self.snapshot(window, Some(by));
    }

    fn snapshot(&mut self, window: &Window, leave: Option<Point<i32, Logical>>) {
        let style = self.animations.style;
        let Some(place) = self.space.element_geometry(window) else {
            return;
        };
        let length = if leave.is_some() {
            style.slide
        } else {
            style.close
        };
        if length.is_zero() {
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
        let borders = if insets.top > 0 && (insets.side > 0 || insets.bottom > 0) {
            frame_data
                .borders
                .iter()
                .cloned()
                .zip(crate::render::border_places(frame, insets))
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
        self.animations.closing.push(Closing {
            anim: Anim::new(self.animations.now(), length),
            above,
            frame,
            parts,
            bar,
            borders,
            leave,
        });
        self.dirty = true;
    }
}
