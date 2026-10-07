//! What a frame draws, for both backends (M4.4): the cursor on top
//! (M4.6b), then the windows from the top down, each with its popups above
//! it and its title bar and border round it. Bars, borders and cursor
//! images keep their ids between frames, so damage tracking redraws only
//! what changed and an idle desktop draws nothing. While a window opens,
//! closes or slides (M5.11b), its parts are drawn as `animate.rs` says:
//! faded, grown or shrunk about its centre, or off its place.

use smithay::backend::renderer::Renderer;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::{
    WaylandSurfaceRenderElement, render_elements_from_surface_tree,
};
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::utils::RescaleRenderElement;
use smithay::backend::renderer::element::{AsRenderElements, Kind};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::desktop::Window;
use smithay::input::pointer::CursorImageStatus;
use smithay::output::Output;
use smithay::utils::{Logical, Physical, Point, Rectangle, Scale, Size};

use edel_compositor::frame::{Insets, Look};

use crate::animate::{Closing, Look as Seen};
use crate::decoration::{data, title};
use crate::pointer::{SIZE, surface_hotspot};
use crate::state::Edel;

smithay::backend::renderer::element::render_elements! {
    pub Element<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Bar=MemoryRenderBufferRenderElement<GlesRenderer>,
    Border=SolidColorRenderElement,
    Picture=TextureRenderElement<GlesTexture>,
}

// What a frame draws: elements as they are, or grown or shrunk about a
// window's centre while it opens or closes.
smithay::backend::renderer::element::render_elements! {
    pub Drawn<=GlesRenderer>;
    Plain=Element,
    Zoomed=RescaleRenderElement<Element>,
}

/// `element` at `zoom` times its size about `centre`.
fn zoomed(element: Element, centre: Point<i32, Physical>, zoom: f64) -> Drawn {
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

/// The centre of `frame` on a screen at `screen`, in its pixels, moved by
/// `shift`.
fn centre_of(
    frame: Rectangle<i32, Logical>,
    screen: Point<i32, Logical>,
    scale: f64,
    shift: Point<i32, Physical>,
) -> Point<i32, Physical> {
    let centre = (frame.loc - screen).to_f64() + frame.size.to_f64().downscale(2.0).to_point();
    centre.to_physical(scale).to_i32_round() + shift
}

impl Edel {
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
        let px = (f64::from(SIZE) * scale).round() as u32;
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
        let windows: Vec<_> = self.space.elements().rev().cloned().collect();
        let mut elements: Vec<Drawn> = self
            .cursor_elements(renderer, screen.loc, scale)
            .into_iter()
            .map(Drawn::Plain)
            .collect();
        // Panels over the windows, backgrounds under them (M5.1a); a dock
        // a window covers is not drawn (M5.4f).
        let hidden = self.hidden_layers();
        elements.extend(
            crate::layers::elements(renderer, output, self.layers_over(output), scale, &hidden)
                .into_iter()
                .map(|e| Drawn::Plain(Element::Surface(e))),
        );
        // Closed windows fading out where they were in the stack (M5.11b).
        let mut pictures: Vec<(usize, Vec<Drawn>)> = self
            .animations
            .closing
            .iter()
            .map(|closed| {
                let drawn = self.closing_elements(renderer, closed, screen.loc, scale, now);
                (closed.above, drawn)
            })
            .collect();
        for (index, window) in windows.into_iter().enumerate() {
            for (_, drawn) in pictures.iter_mut().filter(|(above, _)| *above == index) {
                elements.append(drawn);
            }
            let Some(place) = self.space.element_geometry(&window) else {
                continue;
            };
            let look = self.animations.look(&window, now);
            let shift = look.offset.to_physical(scale).to_i32_round::<i32>();
            let origin = (place.loc - window.geometry().loc - screen.loc)
                .to_physical_precise_round(scale)
                + shift;
            let mut parts: Vec<Element> = window
                .render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                    renderer,
                    origin,
                    Scale::from(scale),
                    look.alpha,
                )
                .into_iter()
                .map(Element::Surface)
                .collect();
            self.remember_picture(&window);
            parts.extend(self.frame_elements(renderer, &window, place, screen.loc, scale, look));
            let outer = self.insets(&window).frame(place);
            let centre = centre_of(outer, screen.loc, scale, shift);
            elements.extend(parts.into_iter().map(|e| zoomed(e, centre, look.zoom)));
        }
        for (_, drawn) in &mut pictures {
            elements.append(drawn);
        }
        elements.extend(
            crate::layers::elements(renderer, output, &crate::layers::BELOW, scale, &hidden)
                .into_iter()
                .map(|e| Drawn::Plain(Element::Surface(e))),
        );
        elements
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
        let mut parts: Vec<Element> = closed
            .parts
            .iter()
            .map(|(id, part)| {
                Element::Picture(TextureRenderElement::from_static_texture(
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
                ))
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
        for (buffer, place) in &closed.borders {
            parts.push(Element::Border(SolidColorRenderElement::from_buffer(
                buffer,
                at(place.loc).to_i32_round(),
                scale,
                look.alpha,
                Kind::Unspecified,
            )));
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
            side: self.settings.button_side(),
            shown: self.settings.buttons,
            scheme: self.settings.color_scheme,
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
        let colour = if is_focused {
            self.tokens.title_bar_focused
        } else {
            self.tokens.title_bar
        };
        for (buffer, place) in frame.borders.iter_mut().zip(border_places(outer, insets)) {
            buffer.update(place.size, colour.rgba());
            parts.push(Element::Border(SolidColorRenderElement::from_buffer(
                buffer,
                at(place.loc).to_i32_round(),
                scale,
                seen.alpha,
                Kind::Unspecified,
            )));
        }
        parts
    }
}
