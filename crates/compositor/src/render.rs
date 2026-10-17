//! What a frame draws, for both backends (M4.4): the cursor on top
//! (M4.6b), then the windows from the top down, each with its popups above
//! it and its title bar and border round it. Bars, borders and cursor
//! images keep their ids between frames, so damage tracking redraws only
//! what changed and an idle desktop draws nothing. While a window opens,
//! closes or slides (M5.11b), its parts are drawn as `animate.rs` says:
//! faded, grown or shrunk about its centre, or off its place.

use smithay::backend::renderer::Renderer;
use smithay::backend::renderer::Texture;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::{
    WaylandSurfaceRenderElement, WaylandSurfaceTexture, render_elements_from_surface_tree,
};
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::utils::{
    Relocate, RelocateRenderElement, RescaleRenderElement,
};
use smithay::backend::renderer::element::{AsRenderElements, Element as _, Kind};
use smithay::backend::renderer::gles::element::PixelShaderElement;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexProgram, GlesTexture};
use smithay::desktop::{PopupManager, Window};
use smithay::input::pointer::CursorImageStatus;
use smithay::output::Output;
use smithay::utils::{Logical, Physical, Point, Rectangle, Scale, Size, Transform};

use edel_compositor::corners;
use edel_compositor::effects::Tier;
use edel_compositor::frame::{Border, Insets, Look};
use edel_compositor::tokens::Tokens;

use crate::animate::{Closing, Look as Seen};
use crate::decoration::{data, title};
use crate::glance::painted;
use crate::glance_look;
use crate::pointer::surface_hotspot;
use crate::rounded::{Clipped, RingLook, ring_element, ring_once};
use crate::state::Edel;

smithay::backend::renderer::element::render_elements! {
    pub Element<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Bar=MemoryRenderBufferRenderElement<GlesRenderer>,
    Border=SolidColorRenderElement,
    Picture=TextureRenderElement<GlesTexture>,
    Clipped=crate::rounded::Clipped<WaylandSurfaceRenderElement<GlesRenderer>>,
    ClippedPicture=crate::rounded::Clipped<TextureRenderElement<GlesTexture>>,
    Ring=RelocateRenderElement<PixelShaderElement>,
}

// What a frame draws: elements as they are, or grown or shrunk about a
// window's centre while it opens or closes.
smithay::backend::renderer::element::render_elements! {
    pub Drawn<=GlesRenderer>;
    Plain=Element,
    Zoomed=RescaleRenderElement<Element>,
}

/// `element` at `zoom` times its size about `centre`.
pub(crate) fn zoomed(element: Element, centre: Point<i32, Physical>, zoom: f64) -> Drawn {
    if (zoom - 1.0).abs() < f64::EPSILON {
        Drawn::Plain(element)
    } else {
        Drawn::Zoomed(RescaleRenderElement::from_element(element, centre, zoom))
    }
}

/// Where a frame's border goes: left, right and bottom, between the bar
/// and the frame's edges.
pub fn border_places(
    outer: Rectangle<i32, Logical>,
    insets: Insets,
) -> [Rectangle<i32, Logical>; 3] {
    let side_h = outer.size.h - insets.top - insets.bottom;
    [
        Rectangle::new(
            outer.loc + Point::from((0, insets.top)),
            Size::from((insets.side, side_h)),
        ),
        Rectangle::new(
            outer.loc + Point::from((outer.size.w - insets.side, insets.top)),
            Size::from((insets.side, side_h)),
        ),
        Rectangle::new(
            outer.loc + Point::from((0, outer.size.h - insets.bottom)),
            Size::from((outer.size.w, insets.bottom)),
        ),
    ]
}

/// The opacity a window is drawn with (M5.12b3): the focused window's
/// `window_opacity` or the others' `window_opacity_unfocused`, as a share
/// of one. A fullscreen window is always opaque, as it covers the screen.
pub fn window_opacity(focused: bool, fullscreen: bool, tokens: &Tokens) -> f32 {
    if fullscreen {
        return 1.0;
    }
    let percent = if focused {
        tokens.window_opacity
    } else {
        tokens.window_opacity_unfocused
    };
    percent.min(100) as f32 / 100.0
}

/// The centre of `frame` on a screen at `screen`, in its pixels, moved by
/// `shift`.
pub(crate) fn centre_of(
    frame: Rectangle<i32, Logical>,
    screen: Point<i32, Logical>,
    scale: f64,
    shift: Point<i32, Physical>,
) -> Point<i32, Physical> {
    let centre = (frame.loc - screen).to_f64() + frame.size.to_f64().downscale(2.0).to_point();
    centre.to_physical(scale).to_i32_round() + shift
}

/// `window`'s surfaces drawn at `origin`, as smithay draws a window: its
/// popups above its toplevel's tree, unclipped (M5.14). With `clip` (the
/// clip shader, the rectangle in pixels and the corner radii) the toplevel's
/// textures are clipped to their rounded corners; a window with no
/// toplevel is drawn as smithay draws it.
fn surfaces(
    renderer: &mut GlesRenderer,
    window: &Window,
    origin: Point<i32, Physical>,
    scale: f64,
    alpha: f32,
    clip: Option<(GlesTexProgram, Rectangle<i32, Physical>, [f32; 4])>,
) -> Vec<Element> {
    let Some(toplevel) = window.toplevel() else {
        return window
            .render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                renderer,
                origin,
                Scale::from(scale),
                alpha,
            )
            .into_iter()
            .map(Element::Surface)
            .collect();
    };
    let surface = toplevel.wl_surface();
    let mut elements = Vec::new();
    for (popup, popup_offset) in PopupManager::popups_for_surface(surface) {
        let offset = (window.geometry().loc + popup_offset - popup.geometry().loc)
            .to_physical_precise_round(scale);
        elements.extend(
            render_elements_from_surface_tree(
                renderer,
                popup.wl_surface(),
                origin + offset,
                Scale::from(scale),
                alpha,
                Kind::Unspecified,
            )
            .into_iter()
            .map(Element::Surface),
        );
    }
    let tree: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = render_elements_from_surface_tree(
        renderer,
        surface,
        origin,
        Scale::from(scale),
        alpha,
        Kind::Unspecified,
    );
    let Some((program, rect, radii)) = clip else {
        elements.extend(tree.into_iter().map(Element::Surface));
        return elements;
    };
    for element in tree {
        // A texture drawn untransformed and not upside down is clipped; any
        // other is drawn as it is, unclipped (`Clipped::new`'s `plain`).
        let clipped = match element.texture() {
            WaylandSurfaceTexture::Texture(texture) => Some((
                texture.size(),
                element.transform() == Transform::Normal && !texture.is_y_inverted(),
            )),
            WaylandSurfaceTexture::SolidColor(_) => None,
        };
        match clipped {
            Some((size, plain)) => elements.push(Element::Clipped(Clipped::new(
                element,
                Scale::from(scale),
                &program,
                rect,
                radii,
                size,
                plain,
            ))),
            None => elements.push(Element::Surface(element)),
        }
    }
    elements
}

impl Edel {
    /// The opacity `window` is drawn with (M5.12b3), see [`window_opacity`].
    fn opacity_of(&self, window: &Window) -> f32 {
        let focused = self.focused_window().as_ref() == Some(window);
        window_opacity(focused, self.is_fullscreen(window), &self.tokens)
    }

    /// The cursor at the pointer, if it is on this screen.
    fn cursor_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        screen: Point<i32, smithay::utils::Logical>,
        scale: f64,
    ) -> Vec<Element> {
        let Some(pointer) = self.seat.get_pointer() else {
            return Vec::new();
        };
        let at = pointer.current_location() - screen.to_f64();
        let kind = if self.cursors.software {
            Kind::Unspecified
        } else {
            Kind::Cursor
        };
        if let Some(surface) = self.cursors.surface() {
            let origin = (at - surface_hotspot(&surface).to_f64())
                .to_physical(scale)
                .to_i32_round();
            return render_elements_from_surface_tree(
                renderer,
                &surface,
                origin,
                Scale::from(scale),
                1.0,
                kind,
            )
            .into_iter()
            .map(Element::Surface)
            .collect();
        }
        let CursorImageStatus::Named(icon) = self.cursors.status.clone() else {
            return Vec::new();
        };
        let px = (f64::from(self.cursors.size) * scale).round() as u32;
        let image = self.cursors.image(icon.name(), px);
        let origin = at.to_physical(scale).to_i32_round::<i32>() - image.hotspot;
        // Drawn in the screen's own pixels, as title bars are.
        let size = Size::<f64, Physical>::from((f64::from(image.width), f64::from(image.height)));
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            origin.to_f64(),
            &image.buffer,
            None,
            Some(Rectangle::from_size((size.w, size.h).into())),
            Some(size.to_logical(scale).to_i32_round()),
            kind,
        ) {
            Ok(element) => vec![Element::Bar(element)],
            Err(e) => {
                eprintln!("edel-compositor: drawing the cursor failed: {e}");
                Vec::new()
            }
        }
    }

    /// Everything on `output`, front to back.
    pub fn elements(&mut self, renderer: &mut GlesRenderer, output: &Output) -> Vec<Drawn> {
        let Some(screen) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let now = self.animations.now();
        self.animations.prune(now);
        let mut elements: Vec<Drawn> = self
            .cursor_elements(renderer, screen.loc, scale)
            .into_iter()
            .map(Drawn::Plain)
            .collect();
        // A locked session shows its lock screen alone (M5.10).
        if self.is_locked() {
            self.drop_backdrop(output);
            elements.extend(self.lock_elements(renderer, output, scale));
            return elements;
        }
        // The overview covers the screen while it is open (M5.2j).
        if self.overview.is_some() {
            self.drop_backdrop(output);
            elements.extend(self.overview_elements(renderer, output, scale));
            return elements;
        }
        // A game plays alone on its screen (M7.7c): its window is drawn,
        // the layers over it and the cursor, nothing else.
        let game = self.playing_on(output).cloned();
        let windows: Vec<_> = match &game {
            Some(top) => vec![top.clone()],
            None => self.space.elements().rev().cloned().collect(),
        };
        // Panels over the windows, backgrounds under them (M5.1a); a dock
        // a window covers is not drawn (M5.4f).
        let hidden = self.hidden_layers();
        // The blur behind the popups among them (`backdrop.rs`) lies under
        // each popup, so their places are noted as the parts are made.
        let base = elements.len();
        let mut spots = Vec::new();
        elements.extend(crate::layers::elements(
            renderer,
            output,
            self.layers_over(output),
            scale,
            &hidden,
            &self.animations,
            Some(&mut spots),
        ));
        for spot in &mut spots {
            spot.after += base;
        }
        // Closed windows fading out where they were in the stack (M5.11b),
        // none behind a game (M7.7c).
        let mut pictures: Vec<(usize, Vec<Drawn>)> = if game.is_some() {
            Vec::new()
        } else {
            self.animations
                .closing
                .iter()
                .map(|closed| {
                    let drawn = self.closing_elements(renderer, closed, screen.loc, scale, now);
                    (closed.above, drawn)
                })
                .collect()
        };
        for (index, window) in windows.into_iter().enumerate() {
            for (_, drawn) in pictures.iter_mut().filter(|(above, _)| *above == index) {
                elements.append(drawn);
            }
            let Some(place) = self.space.element_geometry(&window) else {
                continue;
            };
            // A switch's key is built only while one runs (M5.14).
            let switch = if self.animations.switches.is_empty() {
                None
            } else {
                self.window_switch_key(&window)
            };
            let mut look = self.animations.look(&window, now, &switch);
            // The window's opacity (M5.12b3) multiplies its own fade, so its
            // picture, title bar and border all show it.
            look.alpha *= self.opacity_of(&window);
            let shift = look.offset.to_physical(scale).to_i32_round::<i32>();
            let origin = (place.loc - window.geometry().loc - screen.loc)
                .to_physical_precise_round(scale)
                + shift;
            // Rounded corners (M5.14): the window's surfaces are clipped to
            // its place in pixels, with the radii its frame leaves.
            let radius = self.corner_radius(&window);
            let insets = self.insets(&window);
            let clip = self.rounded.as_ref().filter(|_| radius > 0).map(|rounded| {
                let rect = Rectangle::new(
                    origin + window.geometry().loc.to_physical_precise_round(scale),
                    place.size.to_physical_precise_round(scale),
                );
                let radii = corners::surface_radii(
                    radius as f32 * scale as f32,
                    insets.side as f32 * scale as f32,
                    insets.top > 0,
                );
                (rounded.clip.clone(), rect, radii)
            });
            // An input method's popup (M6.8) lies above the window it types
            // into, so it comes first, as the top of the window's parts.
            let mut parts: Vec<Element> = window
                .toplevel()
                .map(|toplevel| {
                    crate::ime::popup_elements(
                        renderer,
                        toplevel.wl_surface(),
                        origin,
                        scale,
                        look.alpha,
                    )
                })
                .unwrap_or_default()
                .into_iter()
                .map(Element::Surface)
                .collect();
            parts.extend(surfaces(renderer, &window, origin, scale, look.alpha, clip));
            self.remember_picture(&window);
            parts.extend(self.frame_elements(renderer, &window, place, screen.loc, scale, look));
            let outer = insets.frame(place);
            let centre = centre_of(outer, screen.loc, scale, shift);
            elements.extend(parts.into_iter().map(|e| zoomed(e, centre, look.zoom)));
            // Its shadow lies under the window's own parts (M5.12b3).
            elements
                .extend(self.shadow_elements(renderer, &window, outer, screen.loc, scale, look));
        }
        for (_, drawn) in &mut pictures {
            elements.append(drawn);
        }
        // The layers under the windows and the wallpaper are not drawn
        // behind a game (M7.7c), and nothing is blurred behind it.
        if game.is_some() {
            self.drop_backdrop(output);
            return elements;
        }
        elements.extend(crate::layers::elements(
            renderer,
            output,
            &crate::layers::BELOW,
            scale,
            &hidden,
            &self.animations,
            None,
        ));
        // The wallpaper lies under everything, so it comes last (M5.10c4b).
        elements.extend(self.wallpaper_elements(renderer, output, screen.size, scale));
        self.add_backdrops(renderer, output, &mut elements, spots);
        elements
    }

    /// The soft shadow under `window`'s frame `outer` (M5.12b3), drawn as
    /// the window's own parts are: faded with its look and grown about its
    /// centre. Only on Full and Balanced (ADR-002's tier table), never for a
    /// fullscreen window, and none at all for a shadow of no blur. The
    /// picture is painted again only when its size, scale or shadow changes.
    fn shadow_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        window: &Window,
        outer: Rectangle<i32, Logical>,
        screen: Point<i32, Logical>,
        scale: f64,
        look: Seen,
    ) -> Vec<Drawn> {
        if self.deadline.tier() == Tier::Lite
            || self.is_fullscreen(window)
            || self.tokens.shadow_blur == 0
        {
            return Vec::new();
        }
        // The shadow follows the window's corners (M5.14); square, the 6 it
        // always had.
        let radius = match self.corner_radius(window) {
            0 => glance_look::SQUARE_SHADOW_RADIUS,
            r => r as f32,
        };
        let shows = format!(
            "{:?} {scale} {:?} {} {} {radius}",
            outer.size, self.tokens.shadow, self.tokens.shadow_blur, self.tokens.shadow_offset
        );
        if self
            .window_shadows
            .get(window)
            .is_none_or(|(p, _)| p.shows != shows)
        {
            match glance_look::shadow(outer.size.w, outer.size.h, scale, &self.tokens, radius) {
                Some((pixmap, margin)) => {
                    self.window_shadows
                        .insert(window.clone(), (painted(shows, &pixmap), margin));
                }
                None => return Vec::new(),
            }
        }
        let Some((picture, margin)) = self.window_shadows.get(window) else {
            return Vec::new();
        };
        // The picture stands `margin` pixels out from the frame on each side.
        let margin = f64::from(*margin);
        let shift = look.offset.to_physical(scale);
        let at = (outer.loc - screen).to_f64().to_physical(scale) + shift
            - Point::from((margin, margin));
        let pixels = (f64::from(picture.pixels.0), f64::from(picture.pixels.1));
        let size = Size::<f64, Physical>::from(pixels);
        let element = match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            at,
            &picture.buffer,
            Some(look.alpha),
            Some(Rectangle::from_size(pixels.into())),
            Some(size.to_logical(scale).to_i32_round()),
            Kind::Unspecified,
        ) {
            Ok(element) => element,
            Err(e) => {
                eprintln!("edel-compositor: drawing a window's shadow failed: {e}");
                return Vec::new();
            }
        };
        let centre = centre_of(outer, screen, scale, shift.to_i32_round());
        vec![zoomed(Element::Bar(element), centre, look.zoom)]
    }

    /// A closed window's last picture, fading out (M5.11b).
    fn closing_elements(
        &self,
        renderer: &mut GlesRenderer,
        closed: &Closing,
        screen: Point<i32, Logical>,
        scale: f64,
        now: std::time::Duration,
    ) -> Vec<Drawn> {
        let look = self.animations.closing_look(closed, now);
        let context = renderer.context_id();
        let shift = look.offset.to_physical(scale);
        let centre = centre_of(closed.frame, screen, scale, shift.to_i32_round());
        let at = |p: Point<i32, Logical>| (p - screen).to_f64().to_physical(scale) + shift;
        // A rounded closing window's pictures are clipped to the place it
        // had (M5.14), with the radii its frame leaves.
        let clip = self
            .rounded
            .as_ref()
            .zip(closed.clip_frame)
            .filter(|_| closed.radius > 0)
            .map(|(rounded, place)| {
                let rect = Rectangle::new(
                    at(place.loc).to_i32_round::<i32>(),
                    place.size.to_physical_precise_round(scale),
                );
                let top = place.loc.y - closed.frame.loc.y;
                let radii = corners::surface_radii(
                    closed.radius as f32 * scale as f32,
                    closed.edge as f32 * scale as f32,
                    top > 0,
                );
                (&rounded.clip, rect, radii)
            });
        let mut parts: Vec<Element> = closed
            .parts
            .iter()
            .map(|(id, part)| {
                let texture = TextureRenderElement::from_static_texture(
                    id.clone(),
                    context.clone(),
                    at(part.at),
                    part.texture.clone(),
                    part.scale,
                    part.transform,
                    Some(look.alpha),
                    Some(part.src),
                    Some(part.size),
                    None,
                    Kind::Unspecified,
                );
                match clip {
                    Some((program, rect, radii)) => {
                        let plain =
                            part.transform == Transform::Normal && !part.texture.is_y_inverted();
                        Element::ClippedPicture(Clipped::new(
                            texture,
                            Scale::from(scale),
                            program,
                            rect,
                            radii,
                            part.texture.size(),
                            plain,
                        ))
                    }
                    None => Element::Picture(texture),
                }
            })
            .collect();
        if let Some(bar) = &closed.bar {
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                at(bar.place.loc),
                &bar.buffer,
                Some(look.alpha),
                Some(Rectangle::from_size(
                    (f64::from(bar.pixels.0), f64::from(bar.pixels.1)).into(),
                )),
                Some(bar.place.size),
                Kind::Unspecified,
            ) {
                Ok(element) => parts.push(Element::Bar(element)),
                Err(e) => eprintln!("edel-compositor: drawing a title bar failed: {e}"),
            }
        }
        // A rounded window's border is one ring (M5.14); a square one keeps
        // its sides and its strip.
        let ring = match (self.rounded.as_ref(), closed.clip_frame, closed.border) {
            (Some(rounded), Some(place), Some(border)) => {
                let top = place.loc.y - closed.frame.loc.y;
                let area = Rectangle::new(
                    Point::from((closed.frame.loc.x, closed.frame.loc.y + top)) - screen,
                    Size::from((closed.frame.size.w, closed.frame.size.h - top)),
                );
                let edge = RingLook {
                    start: border.start.rgba(),
                    end: border.end.rgba(),
                    edge: closed.edge as f32,
                    radius: closed.radius as f32,
                    scale: scale as f32,
                };
                Some(ring_once(
                    &rounded.ring,
                    area,
                    edge,
                    look.alpha,
                    shift.to_i32_round::<i32>(),
                ))
            }
            _ => None,
        };
        match ring {
            Some(ring) => parts.push(Element::Ring(ring)),
            None => {
                for (buffer, place) in &closed.borders {
                    parts.push(Element::Border(SolidColorRenderElement::from_buffer(
                        buffer,
                        at(place.loc).to_i32_round(),
                        scale,
                        look.alpha,
                        Kind::Unspecified,
                    )));
                }
                if let Some(strip) = &closed.strip {
                    match MemoryRenderBufferRenderElement::from_buffer(
                        renderer,
                        at(strip.place.loc),
                        &strip.buffer,
                        Some(look.alpha),
                        Some(Rectangle::from_size(
                            (f64::from(strip.pixels.0), f64::from(strip.pixels.1)).into(),
                        )),
                        Some(strip.place.size),
                        Kind::Unspecified,
                    ) {
                        Ok(element) => parts.push(Element::Bar(element)),
                        Err(e) => {
                            eprintln!("edel-compositor: drawing a window's border failed: {e}")
                        }
                    }
                }
            }
        }
        parts
            .into_iter()
            .map(|e| zoomed(e, centre, look.zoom))
            .collect()
    }

    /// `window`'s title bar and border, if it has our frame, drawn as
    /// `seen` says: faded and off its place while it animates.
    fn frame_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        window: &Window,
        place: Rectangle<i32, Logical>,
        screen: Point<i32, Logical>,
        scale: f64,
        seen: Seen,
    ) -> Vec<Element> {
        let is_focused = self.focused_window().as_ref() == Some(window);
        let mut parts = Vec::new();
        let insets = self.insets(window);
        if insets.top == 0 {
            return parts;
        }
        let border = Border::of(&self.tokens, is_focused);
        let radius = self.corner_radius(window);
        let outer = insets.frame(place);
        let shift = seen.offset.to_physical(scale);
        let at = |p: Point<i32, Logical>| (p - screen).to_f64().to_physical(scale) + shift;
        // The bar is drawn in the screen's own pixels, so it stays sharp
        // at any scale, and shown at its logical size.
        let bar_size = Size::<i32, Logical>::from((outer.size.w, insets.top));
        let pixels = bar_size.to_f64().to_physical(scale).to_i32_round::<i32>();
        let bar_at = at(outer.loc).to_i32_round::<i32>().to_f64();
        // The app's icon left of the title, 16 px as the mockups draw it.
        let icon_px = (16.0 * scale).round() as u32;
        let icon_name = self.app_icons.name(&crate::decoration::app_id(window));
        let look = Look {
            width: pixels.w,
            height: pixels.h,
            scale_120: (scale * 120.0).round() as u32,
            title: title(window),
            icon: icon_name.clone(),
            focused: is_focused,
            maximized: self.is_maximized(window),
            hovered: self
                .hover
                .as_ref()
                .filter(|(w, _)| w == window)
                .map(|(_, b)| *b),
            text: self.text.is_some(),
            text_key: self.text_key,
            side: self.settings.button_side(),
            shown: self.shown(window),
            scheme: self.settings.color_scheme,
            border,
            radius: (f64::from(radius) * scale).round() as u32,
        };
        let mut frame = data(window).borrow_mut();
        let icon = icon_name.and_then(|name| self.app_icons.picture(&name, icon_px));
        let bar = frame.bar(look, &self.tokens, self.text.as_mut(), icon);
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            bar_at,
            bar,
            Some(seen.alpha),
            Some(Rectangle::from_size(
                (f64::from(pixels.w), f64::from(pixels.h)).into(),
            )),
            Some(bar_size),
            Kind::Unspecified,
        ) {
            Ok(element) => parts.push(Element::Bar(element)),
            Err(e) => eprintln!("edel-compositor: drawing a title bar failed: {e}"),
        }
        if insets.side == 0 && insets.bottom == 0 {
            return parts;
        }
        // A rounded window's border is one ring (M5.14), its bottom corners
        // curved, drawn below the bar and moved with it as the bar is.
        if let Some(rounded) = self.rounded.as_ref().filter(|_| radius > 0) {
            let area = Rectangle::new(
                outer.loc + Point::from((0, insets.top)) - screen,
                Size::from((outer.size.w, outer.size.h - insets.top)),
            );
            let look = RingLook {
                start: border.start.rgba(),
                end: border.end.rgba(),
                edge: insets.side as f32,
                radius: radius as f32,
                scale: scale as f32,
            };
            let ring = ring_element(&mut frame.ring, &rounded.ring, area, look, seen.alpha);
            let relocated = RelocateRenderElement::from_element(
                ring,
                shift.to_i32_round::<i32>(),
                Relocate::Relative,
            );
            parts.push(Element::Ring(relocated));
            return parts;
        }
        // The border round the window, as the mockups draw it (M5.12b2b):
        // its left and right sides in the colours of its two ends, its
        // bottom in one colour, or a gradient of the two as a texture. The
        // bar draws its own part of it.
        let [left, right, bottom] = border_places(outer, insets);
        for (side, place) in [(0, left), (1, right)] {
            let colour = if side == 0 { border.start } else { border.end };
            let buffer = &mut frame.borders[side];
            buffer.update(place.size, colour.rgba());
            parts.push(Element::Border(SolidColorRenderElement::from_buffer(
                buffer,
                at(place.loc).to_i32_round(),
                scale,
                seen.alpha,
                Kind::Unspecified,
            )));
        }
        if border.start == border.end {
            let buffer = &mut frame.borders[2];
            buffer.update(bottom.size, border.start.rgba());
            parts.push(Element::Border(SolidColorRenderElement::from_buffer(
                buffer,
                at(bottom.loc).to_i32_round(),
                scale,
                seen.alpha,
                Kind::Unspecified,
            )));
        } else {
            let px = bottom
                .size
                .to_f64()
                .to_physical(scale)
                .to_i32_round::<i32>();
            let strip = frame.gradient((px.w, px.h), border);
            let bottom_at = at(bottom.loc).to_i32_round::<i32>().to_f64();
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                bottom_at,
                strip,
                Some(seen.alpha),
                Some(Rectangle::from_size(
                    (f64::from(px.w), f64::from(px.h)).into(),
                )),
                Some(bottom.size),
                Kind::Unspecified,
            ) {
                Ok(element) => parts.push(Element::Bar(element)),
                Err(e) => eprintln!("edel-compositor: drawing a window's border failed: {e}"),
            }
        }
        parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_focused_and_the_other_windows_take_their_own_opacity() {
        let mut tokens = Tokens::built_in();
        tokens.window_opacity = 90;
        tokens.window_opacity_unfocused = 70;
        assert_eq!(window_opacity(true, false, &tokens), 0.9);
        assert_eq!(window_opacity(false, false, &tokens), 0.7);
    }

    #[test]
    fn a_fullscreen_window_is_always_opaque() {
        let mut tokens = Tokens::built_in();
        tokens.window_opacity = 20;
        tokens.window_opacity_unfocused = 20;
        assert_eq!(window_opacity(true, true, &tokens), 1.0);
        assert_eq!(window_opacity(false, true, &tokens), 1.0);
    }

    #[test]
    fn the_release_draws_every_window_opaque() {
        let tokens = Tokens::built_in();
        assert_eq!(window_opacity(true, false, &tokens), 1.0);
        assert_eq!(window_opacity(false, false, &tokens), 1.0);
    }
}
