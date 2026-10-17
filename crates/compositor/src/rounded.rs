//! Rounded corners on screen (M5.14, built in M8.21 part 21c): the two
//! programs, compiled once before the first frame, and the elements drawn
//! with them. `edel_compositor::corners` holds the rules and the shaders'
//! text.

use smithay::backend::renderer::element::utils::{Relocate, RelocateRenderElement};
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::element::PixelShaderElement;
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesPixelProgram, GlesRenderer, GlesTexProgram, Uniform, UniformName,
    UniformType,
};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::desktop::Window;
use smithay::utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform};

use edel_compositor::corners::{self, CLIP_SHADER, CLIP_UNIFORMS, RING_SHADER, RING_UNIFORMS};
use edel_compositor::effects::Tier;
use edel_compositor::messages;

use crate::decoration::server_side;
use crate::state::Edel;

/// The rounded corners' programs, compiled once for the renderer.
pub struct Rounded {
    /// The clip shader, for windows' surfaces and closing pictures.
    pub clip: GlesTexProgram,
    /// The ring shader, for the border below the title bar.
    pub ring: GlesPixelProgram,
}

impl Rounded {
    /// Compiles both programs; an error leaves corners square.
    pub fn compile(renderer: &mut GlesRenderer) -> Result<Rounded, GlesError> {
        let clip = renderer.compile_custom_texture_shader(
            CLIP_SHADER,
            &[
                UniformName::new(CLIP_UNIFORMS[0], UniformType::_4f),
                UniformName::new(CLIP_UNIFORMS[1], UniformType::_2f),
                UniformName::new(CLIP_UNIFORMS[2], UniformType::_4f),
            ],
        )?;
        let ring = renderer.compile_custom_pixel_shader(
            RING_SHADER,
            &[
                UniformName::new(RING_UNIFORMS[0], UniformType::_4f),
                UniformName::new(RING_UNIFORMS[1], UniformType::_4f),
                UniformName::new(RING_UNIFORMS[2], UniformType::_1f),
                UniformName::new(RING_UNIFORMS[3], UniformType::_1f),
                UniformName::new(RING_UNIFORMS[4], UniformType::_1f),
            ],
        )?;
        Ok(Rounded { clip, ring })
    }
}

/// An element drawn through the clip shader: `inner`, a texture, faded out
/// outside a rectangle with rounded corners (M5.14).
pub struct Clipped<E> {
    inner: E,
    program: GlesTexProgram,
    /// The inner element's geometry when it was made, in the output's
    /// physical pixels: a zoom around it (the open and close animations'
    /// `RescaleRenderElement`) scales the clip with it.
    made: Rectangle<i32, Physical>,
    /// The clip rectangle, in the same pixels as `made`.
    clip: Rectangle<i32, Physical>,
    /// Its corner radii in physical pixels: top left, top right, bottom right, bottom left.
    radii: [f32; 4],
    /// The texture's size in its own pixels.
    texture: Size<i32, Buffer>,
    /// Whether the texture is drawn untransformed; a transformed texture is
    /// drawn unclipped.
    plain: bool,
}

impl<E: Element> Clipped<E> {
    /// `inner`, clipped to `clip` with `radii`, drawn through `program`. The
    /// caller says whether the texture is `plain`: untransformed and not
    /// upside down.
    pub fn new(
        inner: E,
        scale: Scale<f64>,
        program: &GlesTexProgram,
        clip: Rectangle<i32, Physical>,
        radii: [f32; 4],
        texture: Size<i32, Buffer>,
        plain: bool,
    ) -> Clipped<E> {
        let made = inner.geometry(scale);
        Clipped {
            inner,
            program: program.clone(),
            made,
            clip,
            radii,
            texture,
            plain,
        }
    }
}

impl<E: Element> Element for Clipped<E> {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    /// The inner element's opaque parts, where they lie inside the clip and
    /// outside its corners: what lies under a corner is drawn.
    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        let regions = self.inner.opaque_regions(scale);
        if !self.plain {
            return regions;
        }
        let geometry = self.inner.geometry(scale);
        // The clip in the element's own pixels, as opaque regions are.
        let clip = Rectangle::new(self.clip.loc - geometry.loc, self.clip.size);
        let corners = corners::corner_squares(clip.size.w, clip.size.h, self.radii)
            .into_iter()
            .map(|(x, y, w, h)| Rectangle::new(clip.loc + Point::from((x, y)), Size::from((w, h))));
        let inside: Vec<_> = regions
            .into_iter()
            .filter_map(|region| region.intersection(clip))
            .collect();
        Rectangle::subtract_rects_many(inside, corners)
            .into_iter()
            .collect()
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<E: RenderElement<GlesRenderer>> RenderElement<GlesRenderer> for Clipped<E> {
    /// Drawn through the clip shader, which a plane cannot do, so never
    /// straight to a plane.
    fn underlying_storage(&self, _renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        None
    }

    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        if !self.plain || self.made.is_empty() {
            return self.inner.draw(frame, src, dst, damage, opaque_regions);
        }
        // The clip follows the element when it is zoomed: its size in `dst`
        // over its size unzoomed.
        let kx = dst.size.w as f64 / self.made.size.w as f64;
        let ky = dst.size.h as f64 / self.made.size.h as f64;
        let cx = f64::from(self.clip.loc.x - self.made.loc.x) * kx;
        let cy = f64::from(self.clip.loc.y - self.made.loc.y) * ky;
        let cw = f64::from(self.clip.size.w) * kx;
        let ch = f64::from(self.clip.size.h) * ky;
        let k = kx.min(ky) as f32;
        let radii = self.radii.map(|r| r * k);
        let m = corners::uv_to_px(
            (f64::from(self.texture.w), f64::from(self.texture.h)),
            (src.loc.x, src.loc.y, src.size.w, src.size.h),
            (f64::from(dst.size.w), f64::from(dst.size.h)),
            (cx, cy),
        );
        frame.override_default_tex_program(
            self.program.clone(),
            vec![
                Uniform::new(CLIP_UNIFORMS[0], m),
                Uniform::new(CLIP_UNIFORMS[1], [cw as f32, ch as f32]),
                Uniform::new(CLIP_UNIFORMS[2], radii),
            ],
        );
        let result = self.inner.draw(frame, src, dst, damage, opaque_regions);
        frame.clear_tex_program_override();
        result
    }
}

/// The border below a rounded window's title bar (M5.14), one element of
/// the ring shader, kept in the window's frame data so its id and damage
/// carry over between frames; made anew only when what it shows changes.
pub struct Ring {
    element: PixelShaderElement,
    /// What it shows: its area, its look and its alpha.
    shows: (Rectangle<i32, Logical>, RingLook, f32),
}

/// What a ring shows, in logical pixels except `scale`: its colours
/// (straight RGBA), the edge's width, the outer radius of its bottom
/// corners and the physical pixels per logical pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingLook {
    /// The colour at its left end.
    pub start: [f32; 4],
    /// The colour at its right end.
    pub end: [f32; 4],
    /// The edge's width.
    pub edge: f32,
    /// The radius of its bottom corners.
    pub radius: f32,
    /// Physical pixels per logical pixel.
    pub scale: f32,
}

/// The ring shader's uniforms for `look`, in the order `RING_UNIFORMS` names.
fn ring_uniforms(look: &RingLook) -> Vec<Uniform<'static>> {
    vec![
        Uniform::new(RING_UNIFORMS[0], look.start),
        Uniform::new(RING_UNIFORMS[1], look.end),
        Uniform::new(RING_UNIFORMS[2], look.edge),
        Uniform::new(RING_UNIFORMS[3], look.radius),
        Uniform::new(RING_UNIFORMS[4], look.scale),
    ]
}

/// The ring for a window's border: `area` (logical, on the screen) drawn
/// as `look` says at `alpha`. `slot` keeps the element, so the same ring
/// comes back with the same id; its uniforms are replaced in place when
/// only the look or the area changes, and it is made anew when the alpha
/// does (a pixel shader element cannot take a new alpha).
pub fn ring_element(
    slot: &mut Option<Ring>,
    program: &GlesPixelProgram,
    area: Rectangle<i32, Logical>,
    look: RingLook,
    alpha: f32,
) -> PixelShaderElement {
    let shows = (area, look, alpha);
    if let Some(ring) = slot.as_mut() {
        if ring.shows == shows {
            return ring.element.clone();
        }
        if ring.shows.2 == alpha {
            ring.element.resize(area, None);
            ring.element.update_uniforms(ring_uniforms(&look));
            ring.shows = shows;
            return ring.element.clone();
        }
    }
    let element = PixelShaderElement::new(
        program.clone(),
        area,
        None,
        alpha,
        ring_uniforms(&look),
        Kind::Unspecified,
    );
    *slot = Some(Ring {
        element: element.clone(),
        shows,
    });
    element
}

/// The ring for a closing window's border: a fresh element each frame, as
/// a short fade needs no id kept between frames.
pub fn ring_once(
    program: &GlesPixelProgram,
    area: Rectangle<i32, Logical>,
    look: RingLook,
    alpha: f32,
    shift: Point<i32, Physical>,
) -> RelocateRenderElement<PixelShaderElement> {
    let element = PixelShaderElement::new(
        program.clone(),
        area,
        None,
        alpha,
        ring_uniforms(&look),
        Kind::Unspecified,
    );
    RelocateRenderElement::from_element(element, shift, Relocate::Relative)
}

impl Edel {
    /// Compiles the rounded corners' programs, once the renderer exists,
    /// except on the Lite tier, whose corners are square and which never
    /// rises again. On failure the corners stay square and the log says why.
    pub fn start_rounded(&mut self, renderer: &mut GlesRenderer) {
        if self.deadline.tier() == Tier::Lite {
            return;
        }
        match Rounded::compile(renderer) {
            Ok(rounded) => {
                self.rounded = Some(rounded);
                eprintln!("edel-compositor: rounded corners ready");
            }
            Err(e) => eprintln!("edel-compositor: {}", messages::corners_square(&e)),
        }
    }

    /// The radius `window` is drawn with, in logical pixels (M5.14): none
    /// while the corners are square, and none for a window that draws its
    /// own frame (its corners are its own), see `corners::window_radius`.
    pub fn corner_radius(&self, window: &Window) -> u32 {
        if self.rounded.is_none() || !server_side(window) {
            return 0;
        }
        corners::window_radius(
            self.deadline.tier(),
            self.is_maximized(window),
            self.is_fullscreen(window),
            self.desks.layout_of(window).rearranges(),
            self.tokens.gap,
            self.tokens.radius_window,
        )
    }
}
