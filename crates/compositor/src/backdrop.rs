//! ext-background-effect-v1 and the blur drawn behind a surface on the Full
//! tier (roadmap M5.14, built in M8.21 part 21d), as the roadmap says: made
//! again only where what is behind changed, and frozen while the surface
//! pops up. `blur.rs` holds the rules and the shaders; `layers.rs` finds the
//! surfaces that ask for a blur (`Spot`); `render.rs` calls `add_backdrops`,
//! which draws the blurred picture under them.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::Frame as _;
use smithay::backend::renderer::Renderer as _;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::{Element, Id, Kind};
use smithay::backend::renderer::gles::{
    GlesError, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType,
};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::backend::renderer::{Bind as _, Offscreen as _, Texture as _};
use smithay::output::Output;
use smithay::reexports::wayland_protocols::ext::background_effect::v1::server::{
    ext_background_effect_manager_v1::{self, Capability, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::{self, ExtBackgroundEffectSurfaceV1},
};
use smithay::reexports::wayland_server::protocol::wl_region::WlRegion;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, Weak,
};
use smithay::utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform};
use smithay::wayland::compositor::{Cacheable, RectangleKind, get_region_attributes, with_states};

use edel_compositor::blur::{self, BLUR_UNIFORMS, DOWN_SHADER, UP_SHADER};
use edel_compositor::effects::Tier;
use edel_compositor::messages;

use crate::layers::Spot;
use crate::render::{Drawn, Element as Part, zoomed};
use crate::state::Edel;

const VERSION: u32 = 1;

/// The two blur programs, compiled once for the renderer.
#[derive(Clone)]
pub struct Programs {
    /// The downsampling pass.
    down: GlesTexProgram,
    /// The upsampling pass.
    up: GlesTexProgram,
}

/// A surface's blur region: the rectangles it asked to blur, in its own
/// pixels, or none when it asked for no blur. Smithay keeps it double
/// buffered, so it applies on the surface's next commit.
#[derive(Default, Clone)]
pub struct BlurRegion(pub Option<Vec<Rectangle<i32, Logical>>>);

impl Cacheable for BlurRegion {
    /// The state a commit takes over: a copy of the pending region.
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        self.clone()
    }

    /// The region a commit makes current replaces the one before it.
    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

/// Marks a surface that has its blur object (one per surface, as the
/// protocol says).
struct BlurClaimed(Cell<bool>);

/// One element behind a blur, as a blur key sees it: what makes its
/// picture, so the blur is made again when any of it changes.
#[derive(PartialEq)]
struct Seen {
    /// Its id, which tells one element from another.
    id: Id,
    /// Its commit counter, which changes with its content.
    commit: CommitCounter,
    /// Where it is, in the output's physical pixels.
    geometry: Rectangle<i32, Physical>,
    /// Its alpha, as bits, so a fade behind makes the blur again.
    alpha: u32,
    /// Its source rectangle, formatted, as it is not hashable.
    src: String,
    /// Its transform.
    transform: Transform,
}

/// A blurred picture of what lies behind the popups on one output, kept
/// until what is behind it changes.
pub struct Kept {
    /// The output's mode size the picture was made for.
    size: (i32, i32),
    /// The blur percent it was made with.
    blur: u32,
    /// What was behind the popups when it was made.
    key: Vec<Seen>,
    /// Every level's texture: the screen's picture first, then each
    /// halving; the last of the upsampling is level 1, half the screen.
    levels: Vec<GlesTexture>,
    /// The ids of the blur elements drawn from it, one per rectangle, kept
    /// while the picture is kept so damage tracking draws only the change.
    generation_ids: Vec<Id>,
}

/// What the blur keeps on the compositor's state.
#[derive(Default)]
pub struct Backdrops {
    /// The clients that bound the global and are still alive.
    managers: Vec<ExtBackgroundEffectManagerV1>,
    /// The capabilities last sent to them.
    told: Option<Capability>,
    /// The programs, once they compiled (`start_backdrop`).
    programs: Option<Programs>,
    /// The blurred pictures, by output name.
    kept: HashMap<String, Kept>,
    /// The outputs whose blur failed once, so the failure is logged once.
    failed: HashSet<String>,
}

pub fn create_global(display: &DisplayHandle) {
    display.create_global::<Edel, ExtBackgroundEffectManagerV1, ()>(VERSION, ());
}

/// The rectangles of `surface`'s blur region, in its own pixels; none when
/// it asked for no blur.
pub fn blur_rects(surface: &WlSurface) -> Vec<Rectangle<i32, Logical>> {
    with_states(surface, |states| {
        let mut region = states.cached_state.get::<BlurRegion>();
        region.current().0.clone().unwrap_or_default()
    })
}

/// The corners of `rect` moved inward to whole texels, as the opaque part
/// of a blur element: what lies inside its edges is the blurred picture.
fn inward(rect: Rectangle<f64, Logical>) -> Rectangle<i32, Buffer> {
    let x0 = rect.loc.x.ceil() as i32;
    let y0 = rect.loc.y.ceil() as i32;
    let x1 = (rect.loc.x + rect.size.w).floor() as i32;
    let y1 = (rect.loc.y + rect.size.h).floor() as i32;
    Rectangle::new(
        Point::from((x0, y0)),
        Size::from(((x1 - x0).max(0), (y1 - y0).max(0))),
    )
}

/// Draws `source` into `target` through `program`, halved or doubled from
/// `from` pixels to `to` pixels, with the offset of `blur`.
fn pass(
    renderer: &mut GlesRenderer,
    program: &GlesTexProgram,
    source: &GlesTexture,
    from: (i32, i32),
    target: &mut GlesTexture,
    to: (i32, i32),
    offset: f32,
) -> Result<(), String> {
    let mut framebuffer = renderer
        .bind(target)
        .map_err(|e| format!("binding a blur level: {e}"))?;
    let size = Size::<i32, Physical>::from(to);
    let mut frame = renderer
        .render(&mut framebuffer, size, Transform::Normal)
        .map_err(|e| format!("starting a blur pass: {e}"))?;
    let dest = Rectangle::<i32, Physical>::from_size(size);
    let texel = [0.5 / from.0 as f32, 0.5 / from.1 as f32];
    frame
        .render_texture_from_to(
            source,
            Rectangle::<f64, Buffer>::from_size(Size::from((from.0 as f64, from.1 as f64))),
            dest,
            &[dest],
            &[],
            Transform::Normal,
            1.0,
            Some(program),
            &[
                Uniform::new(BLUR_UNIFORMS[0], texel),
                Uniform::new(BLUR_UNIFORMS[1], offset),
            ],
        )
        .map_err(|e| format!("drawing a blur pass: {e}"))?;
    // The GL calls of one context run in order, so the next pass reads what
    // this one drew without waiting on its sync point.
    let _sync = frame
        .finish()
        .map_err(|e| format!("finishing a blur pass: {e}"))?;
    Ok(())
}

/// An output as the blur sees it: its mode size in pixels, its scale, the
/// colour behind everything and the blur percent.
#[derive(Clone, Copy)]
struct Screen {
    /// The size in pixels, turned as the output is.
    size: (i32, i32),
    /// Physical pixels per logical pixel.
    scale: f64,
    /// The clear colour behind everything.
    background: [f32; 4],
    /// The blur percent (`tokens.blur`).
    blur: u32,
}

/// Makes the blurred picture of `behind` on `screen`: `behind` drawn once
/// into the first level, halved `passes` times with the down pass, then
/// brought back up into level 1. `levels` are reused when they fit, and
/// drawn into again.
fn draw_blur(
    renderer: &mut GlesRenderer,
    programs: &Programs,
    behind: &[Drawn],
    screen: Screen,
    mut levels: Vec<GlesTexture>,
) -> Result<Vec<GlesTexture>, String> {
    let Screen {
        size,
        scale,
        background,
        blur,
    } = screen;
    let sizes = blur::levels(size, blur::passes(blur));
    if levels.len() != sizes.len() {
        levels.clear();
        for &(w, h) in &sizes {
            let texture = renderer
                .create_buffer(Fourcc::Argb8888, Size::from((w, h)))
                .map_err(|e| format!("no texture for the blur: {e}"))?;
            levels.push(texture);
        }
    }
    // The screen as it is behind the popups, drawn without reading it back.
    let mut damage =
        OutputDamageTracker::new(Size::<i32, Physical>::from(size), scale, Transform::Normal);
    {
        let mut framebuffer = renderer
            .bind(&mut levels[0])
            .map_err(|e| format!("binding the blur's screen picture: {e}"))?;
        damage
            .render_output(renderer, &mut framebuffer, 0, behind, background)
            .map_err(|e| format!("drawing what is behind the popups: {e}"))?;
    }
    for i in 1..levels.len() {
        let (done, rest) = levels.split_at_mut(i);
        pass(
            renderer,
            &programs.down,
            &done[i - 1],
            sizes[i - 1],
            &mut rest[0],
            sizes[i],
            blur::offset(blur),
        )?;
    }
    // Upward: each level takes the one below it, down to level 1. The
    // screen's picture and the passes' draws make an even number of
    // draws, so level 1 comes out upright and is drawn as it is.
    for i in (1..levels.len() - 1).rev() {
        let (keep, below) = levels.split_at_mut(i + 1);
        pass(
            renderer,
            &programs.up,
            &below[0],
            sizes[i + 1],
            &mut keep[i],
            sizes[i],
            blur::offset(blur),
        )?;
    }
    Ok(levels)
}

/// What is behind the popups, as a blur key: each element that lies within
/// the blur's reach of a popup's rectangle, with what makes its picture.
fn behind_key(behind: &[Drawn], spots: &[Spot], blur: u32, scale: f64) -> Vec<Seen> {
    let reach = blur::reach(blur);
    let around: Vec<Rectangle<i32, Physical>> = spots
        .iter()
        .flat_map(|spot| spot.rects.iter())
        .map(|rect| {
            Rectangle::new(
                rect.loc - Point::from((reach, reach)),
                Size::from((rect.size.w + 2 * reach, rect.size.h + 2 * reach)),
            )
        })
        .collect();
    let scale = Scale::from(scale);
    behind
        .iter()
        .filter_map(|element| {
            let geometry = element.geometry(scale);
            if !around.iter().any(|area| area.overlaps(geometry)) {
                return None;
            }
            Some(Seen {
                id: element.id().clone(),
                commit: element.current_commit(),
                geometry,
                alpha: element.alpha().to_bits(),
                src: format!("{:?}", element.src()),
                transform: element.transform(),
            })
        })
        .collect()
}

impl Edel {
    /// Compiles the blur's programs once the renderer exists, only when the
    /// session starts on the Full tier: the tier never rises again, so on
    /// Balanced and Lite they would only cost memory. On failure the popups
    /// keep their colour, and the log says why.
    pub fn start_backdrop(&mut self, renderer: &mut GlesRenderer) {
        if self.deadline.tier() != Tier::Full {
            self.tell_blur();
            return;
        }
        match Programs::compile(renderer) {
            Ok(programs) => {
                self.backdrops.programs = Some(programs);
                eprintln!("edel-compositor: live blur ready");
            }
            Err(e) => eprintln!("edel-compositor: {}", messages::blur_off(&e)),
        }
        self.tell_blur();
    }

    /// The capability the blur has now: `blur` on the Full tier with the
    /// programs made, none otherwise.
    fn blur_capabilities(&self) -> Capability {
        if self.backdrops.programs.is_some() && self.deadline.tier() == Tier::Full {
            Capability::Blur
        } else {
            Capability::empty()
        }
    }

    /// Tells every client when the blur's capability changed. Once it is
    /// gone, the pictures are dropped and the screens drawn again, so no
    /// blur stays on screen.
    pub fn tell_blur(&mut self) {
        let now = self.blur_capabilities();
        if self.backdrops.told == Some(now) {
            return;
        }
        self.backdrops.managers.retain(|manager| manager.is_alive());
        for manager in &self.backdrops.managers {
            manager.capabilities(now);
        }
        self.backdrops.told = Some(now);
        if now.is_empty() {
            self.backdrops.kept.clear();
            self.dirty = true;
        }
    }

    /// Forgets `output`'s blurred picture, so the next frame that blurs
    /// makes it again.
    pub fn drop_backdrop(&mut self, output: &Output) {
        self.backdrops.kept.remove(&output.name());
    }

    /// Draws the blur under the surfaces in `spots`, into `elements` (front
    /// to back) at each surface's place among them. The picture of what lies
    /// behind is made again only when it changed where the blur reaches, and
    /// kept while a surface pops up.
    pub fn add_backdrops(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        elements: &mut Vec<Drawn>,
        spots: Vec<Spot>,
    ) {
        let name = output.name();
        let blur = self.tokens.blur;
        let wanted = !spots.is_empty()
            && self.deadline.tier() == Tier::Full
            && blur > 0
            && !self.backdrops.failed.contains(&name);
        let programs = match (wanted, self.backdrops.programs.clone()) {
            (true, Some(programs)) => programs,
            _ => {
                self.drop_backdrop(output);
                return;
            }
        };
        let (Some(mode), scale) = (output.current_mode(), output.current_scale()) else {
            self.drop_backdrop(output);
            return;
        };
        let scale = scale.fractional_scale();
        let size = output.current_transform().transform_size(mode.size);
        let size = (size.w, size.h);
        let background = self.tokens.background.rgba();
        let lowest = spots.iter().map(|spot| spot.after).max().unwrap_or(0);
        let behind = &elements[lowest..];
        let frozen = spots.iter().any(|spot| spot.frozen);
        let key = behind_key(behind, &spots, blur, scale);
        let mut kept = match self.backdrops.kept.remove(&name) {
            Some(kept) if kept.size == size && kept.blur == blur && (kept.key == key || frozen) => {
                kept
            }
            old => {
                let levels = match old {
                    Some(old) if old.size == size && old.blur == blur => old.levels,
                    _ => Vec::new(),
                };
                let screen = Screen {
                    size,
                    scale,
                    background,
                    blur,
                };
                match draw_blur(renderer, &programs, behind, screen, levels) {
                    Ok(levels) => Kept {
                        size,
                        blur,
                        key,
                        levels,
                        generation_ids: Vec::new(),
                    },
                    Err(e) => {
                        if self.backdrops.failed.insert(name.clone()) {
                            eprintln!("edel-compositor: {}", messages::blur_off(&e));
                        }
                        return;
                    }
                }
            }
        };
        let picture = kept.levels[1].clone();
        // The picture is half the screen, so a rectangle of the screen is
        // half of it in the picture's own pixels.
        let texel = (
            kept.levels[1].size().w as f64 / size.0 as f64,
            kept.levels[1].size().h as f64 / size.1 as f64,
        );
        let context = renderer.context_id();
        let mut next = 0usize;
        let mut groups: Vec<(usize, Vec<Drawn>)> = Vec::new();
        for spot in &spots {
            let mut drawn = Vec::new();
            for rect in &spot.rects {
                if kept.generation_ids.len() <= next {
                    kept.generation_ids.push(Id::new());
                }
                let id = kept.generation_ids[next].clone();
                next += 1;
                let src = Rectangle::<f64, Logical>::new(
                    Point::from((rect.loc.x as f64 * texel.0, rect.loc.y as f64 * texel.1)),
                    Size::from((rect.size.w as f64 * texel.0, rect.size.h as f64 * texel.1)),
                );
                let logical = Size::<i32, Logical>::from((
                    (rect.size.w as f64 / scale).round() as i32,
                    (rect.size.h as f64 / scale).round() as i32,
                ));
                let opaque = (spot.alpha >= 1.0).then(|| vec![inward(src)]);
                // Orientation checked by the 21d screenshot: the picture is
                // drawn as it is, since level 1 is upright (see `draw_blur`).
                let element = TextureRenderElement::from_static_texture(
                    id,
                    context.clone(),
                    rect.loc.to_f64(),
                    picture.clone(),
                    1,
                    Transform::Normal,
                    Some(spot.alpha),
                    Some(src),
                    Some(logical),
                    opaque,
                    Kind::Unspecified,
                );
                drawn.push(zoomed(Part::Picture(element), spot.centre, spot.zoom));
            }
            groups.push((spot.after, drawn));
        }
        // The lowest first, so each insertion leaves the ones above it in place.
        groups.sort_by_key(|(after, _)| std::cmp::Reverse(*after));
        for (after, drawn) in groups {
            let tail = elements.split_off(after);
            elements.extend(drawn);
            elements.extend(tail);
        }
        self.backdrops.kept.insert(name, kept);
    }
}

impl Programs {
    /// Compiles the down and up passes with their uniforms.
    fn compile(renderer: &mut GlesRenderer) -> Result<Programs, GlesError> {
        let uniforms = [
            UniformName::new(BLUR_UNIFORMS[0], UniformType::_2f),
            UniformName::new(BLUR_UNIFORMS[1], UniformType::_1f),
        ];
        Ok(Programs {
            down: renderer.compile_custom_texture_shader(DOWN_SHADER, &uniforms)?,
            up: renderer.compile_custom_texture_shader(UP_SHADER, &uniforms)?,
        })
    }
}

impl GlobalDispatch<ExtBackgroundEffectManagerV1, ()> for Edel {
    /// A client bound the global: it hears the capability at once.
    fn bind(
        state: &mut Edel,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtBackgroundEffectManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Edel>,
    ) {
        let manager = data_init.init(resource, ());
        manager.capabilities(state.blur_capabilities());
        state.backdrops.managers.push(manager);
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for Edel {
    fn request(
        _: &mut Edel,
        _: &Client,
        manager: &ExtBackgroundEffectManagerV1,
        request: ext_background_effect_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Edel>,
    ) {
        let ext_background_effect_manager_v1::Request::GetBackgroundEffect { id, surface } =
            request
        else {
            return;
        };
        let claimed = with_states(&surface, |states| {
            states
                .data_map
                .insert_if_missing(|| BlurClaimed(Cell::new(false)));
            states
                .data_map
                .get::<BlurClaimed>()
                .is_some_and(|claim| claim.0.replace(true))
        });
        if claimed {
            manager.post_error(
                ext_background_effect_manager_v1::Error::BackgroundEffectExists,
                "the surface already has a background effect object",
            );
            return;
        }
        data_init.init(id, surface.downgrade());
    }
}

impl Dispatch<ExtBackgroundEffectSurfaceV1, Weak<WlSurface>> for Edel {
    fn request(
        _: &mut Edel,
        _: &Client,
        object: &ExtBackgroundEffectSurfaceV1,
        request: ext_background_effect_surface_v1::Request,
        surface: &Weak<WlSurface>,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Edel>,
    ) {
        match request {
            ext_background_effect_surface_v1::Request::SetBlurRegion { region } => {
                let Ok(surface) = surface.upgrade() else {
                    object.post_error(
                        ext_background_effect_surface_v1::Error::SurfaceDestroyed,
                        "the surface has been destroyed",
                    );
                    return;
                };
                let pending = region.map(|region| blur_areas(&region));
                with_states(&surface, |states| {
                    let mut region = states.cached_state.get::<BlurRegion>();
                    region.pending().0 = pending;
                });
            }
            ext_background_effect_surface_v1::Request::Destroy => {
                // Inert once its surface is gone: nothing to remove.
                let Ok(surface) = surface.upgrade() else {
                    return;
                };
                with_states(&surface, |states| {
                    let mut region = states.cached_state.get::<BlurRegion>();
                    region.pending().0 = None;
                    if let Some(claim) = states.data_map.get::<BlurClaimed>() {
                        claim.0.set(false);
                    }
                });
            }
            _ => {}
        }
    }
}

/// The `add` rectangles of a client's region, in its own pixels. Subtracted
/// rectangles are left out: the blur is drawn over each `add` rectangle
/// whole, so a hole in the region is blurred too.
fn blur_areas(region: &WlRegion) -> Vec<Rectangle<i32, Logical>> {
    get_region_attributes(region)
        .rects
        .into_iter()
        .filter_map(|(kind, rect)| matches!(kind, RectangleKind::Add).then_some(rect))
        .collect()
}
