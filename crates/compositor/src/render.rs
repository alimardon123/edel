//! What a frame draws, for both backends (M4.4): the windows from the top
//! down, each with its popups above it and its title bar and border round
//! it. Bars and borders keep their ids between frames, so damage tracking
//! redraws only what changed and an idle desktop draws nothing.

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::{AsRenderElements, Kind};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::output::Output;
use smithay::utils::{Point, Scale, Size};

use edel_compositor::frame::Look;

use crate::decoration::{data, title};
use crate::state::Edel;

smithay::backend::renderer::element::render_elements! {
    pub Element<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Bar=MemoryRenderBufferRenderElement<GlesRenderer>,
    Border=SolidColorRenderElement,
}

impl Edel {
    /// Everything on `output`, front to back.
    pub fn elements(&mut self, renderer: &mut GlesRenderer, output: &Output) -> Vec<Element> {
        let Some(screen) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let focused = self.focused_window();
        let windows: Vec<_> = self.space.elements().rev().cloned().collect();
        let mut elements = Vec::new();
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
            let look = Look {
                width: outer.size.w,
                height: insets.top,
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
                at(outer.loc),
                bar,
                None,
                None,
                None,
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
