//! What a frame draws, for both backends (M4.4): the cursor on top
//! (M4.6b), then the windows from the top down, each with its popups above
//! it and its title bar and border round it. Bars, borders and cursor
//! images keep their ids between frames, so damage tracking redraws only
//! what changed and an idle desktop draws nothing.

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::{
    WaylandSurfaceRenderElement, render_elements_from_surface_tree,
};
use smithay::backend::renderer::element::{AsRenderElements, Kind};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::input::pointer::CursorImageStatus;
use smithay::output::Output;
use smithay::utils::{Physical, Point, Rectangle, Scale, Size};

use edel_compositor::frame::Look;

use crate::decoration::{data, title};
use crate::pointer::{SIZE, surface_hotspot};
use crate::state::Edel;

smithay::backend::renderer::element::render_elements! {
    pub Element<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Bar=MemoryRenderBufferRenderElement<GlesRenderer>,
    Border=SolidColorRenderElement,
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
    pub fn elements(&mut self, renderer: &mut GlesRenderer, output: &Output) -> Vec<Element> {
        let Some(screen) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let focused = self.focused_window();
        let windows: Vec<_> = self.space.elements().rev().cloned().collect();
        let mut elements = self.cursor_elements(renderer, screen.loc, scale);
        for window in windows {
            let Some(place) = self.space.element_geometry(&window) else {
                continue;
            };
            let origin =
                (place.loc - window.geometry().loc - screen.loc).to_physical_precise_round(scale);
            elements.extend(
                window
                    .render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                        renderer,
                        origin,
                        Scale::from(scale),
                        1.0,
                    )
                    .into_iter()
                    .map(Element::Surface),
            );
            let insets = self.insets(&window);
            if insets.top == 0 {
                continue;
            }
            let outer = insets.frame(place);
            let is_focused = focused.as_ref() == Some(&window);
            // The bar is drawn in the screen's own pixels, so it stays sharp
            // at any scale, and shown at its logical size.
            let bar_size = Size::<i32, smithay::utils::Logical>::from((outer.size.w, insets.top));
            let pixels = bar_size.to_f64().to_physical(scale).to_i32_round::<i32>();
            let bar_at = (outer.loc - screen.loc)
                .to_f64()
                .to_physical(scale)
                .to_i32_round::<i32>()
                .to_f64();
            let look = Look {
                width: pixels.w,
                height: pixels.h,
                scale_120: (scale * 120.0).round() as u32,
                title: title(&window),
                focused: is_focused,
                maximized: self.is_maximized(&window),
                hovered: self
                    .hover
                    .as_ref()
                    .filter(|(w, _)| *w == window)
                    .map(|(_, b)| *b),
                text: self.text.is_some(),
            };
            let mut frame = data(&window).borrow_mut();
            let at = |p: Point<i32, smithay::utils::Logical>| {
                (p - screen.loc).to_f64().to_physical(scale)
            };
            let bar = frame.bar(look, &self.tokens, self.text.as_mut());
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                bar_at,
                bar,
                None,
                Some(Rectangle::from_size(
                    (f64::from(pixels.w), f64::from(pixels.h)).into(),
                )),
                Some(bar_size),
                Kind::Unspecified,
            ) {
                Ok(element) => elements.push(Element::Bar(element)),
                Err(e) => eprintln!("edel-compositor: drawing a title bar failed: {e}"),
            }
            if insets.side == 0 && insets.bottom == 0 {
                continue;
            }
            let colour = if is_focused {
                self.tokens.title_bar_focused
            } else {
                self.tokens.title_bar
            };
            let side_h = outer.size.h - insets.top - insets.bottom;
            let parts = [
                (
                    outer.loc + Point::from((0, insets.top)),
                    Size::from((insets.side, side_h)),
                ),
                (
                    outer.loc + Point::from((outer.size.w - insets.side, insets.top)),
                    Size::from((insets.side, side_h)),
                ),
                (
                    outer.loc + Point::from((0, outer.size.h - insets.bottom)),
                    Size::from((outer.size.w, insets.bottom)),
                ),
            ];
            for (buffer, (loc, size)) in frame.borders.iter_mut().zip(parts) {
                buffer.update(size, colour.rgba());
                elements.push(Element::Border(SolidColorRenderElement::from_buffer(
                    buffer,
                    at(loc).to_i32_round(),
                    scale,
                    1.0,
                    Kind::Unspecified,
                )));
            }
        }
        elements
    }
}
