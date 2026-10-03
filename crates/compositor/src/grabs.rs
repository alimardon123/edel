//! Moving and resizing a window with the pointer (M4.3): one grab for
//! both, started by the window itself (dragging its own title bar or
//! edges, xdg-shell's move and resize requests) or by Super with the left
//! button (move) or the right button (resize from the nearest corner).
//! While it lasts, the pointer belongs to the grab and no client sees it;
//! when the button is released, the floating policy records the new place
//! and the state file is written once, never during the drag (ADR-002).

use smithay::desktop::Window;
use smithay::input::SeatHandler;
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
    GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
    GestureSwipeUpdateEvent, GrabStartData, MotionEvent, PointerGrab, PointerInnerHandle,
    RelativeMotionEvent,
};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::{ResizeEdge, State};
use smithay::utils::{Logical, Point, Rectangle, Size};

use crate::state::Edel;

/// The smallest a window can be made by resizing it, wide and high.
const MIN_W: i32 = 96;
const MIN_H: i32 = 64;

/// What the grab does with the pointer's motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Move,
    Resize(ResizeEdge),
}

pub struct WindowGrab {
    pub start: GrabStartData<Edel>,
    pub window: Window,
    pub kind: Kind,
    /// The window's place when the grab started.
    pub initial: Rectangle<i32, Logical>,
    /// Its place now.
    pub current: Rectangle<i32, Logical>,
}

/// The place a resize from `edges` gives a window that started at
/// `initial`, after the pointer moved by `delta`: the opposite edges stay
/// put, and the size never goes below [`MIN_W`] by [`MIN_H`].
pub fn resized(
    initial: Rectangle<i32, Logical>,
    edges: ResizeEdge,
    delta: Point<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let left = matches!(
        edges,
        ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft
    );
    let right = matches!(
        edges,
        ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight
    );
    let top = matches!(
        edges,
        ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight
    );
    let bottom = matches!(
        edges,
        ResizeEdge::Bottom | ResizeEdge::BottomLeft | ResizeEdge::BottomRight
    );
    let mut w = initial.size.w;
    let mut h = initial.size.h;
    if left {
        w -= delta.x;
    } else if right {
        w += delta.x;
    }
    if top {
        h -= delta.y;
    } else if bottom {
        h += delta.y;
    }
    let size: Size<i32, Logical> = Size::from((w.max(MIN_W), h.max(MIN_H)));
    let mut loc = initial.loc;
    if left {
        loc.x += initial.size.w - size.w;
    }
    if top {
        loc.y += initial.size.h - size.h;
    }
    Rectangle::new(loc, size)
}

/// The corner nearest to `point` in `rect`, for a Super+right-drag resize.
pub fn nearest_corner(rect: Rectangle<i32, Logical>, point: Point<f64, Logical>) -> ResizeEdge {
    let centre = rect.loc.to_f64() + rect.size.to_f64().to_point().downscale(2.0);
    match (point.x < centre.x, point.y < centre.y) {
        (true, true) => ResizeEdge::TopLeft,
        (false, true) => ResizeEdge::TopRight,
        (true, false) => ResizeEdge::BottomLeft,
        (false, false) => ResizeEdge::BottomRight,
    }
}

impl PointerGrab<Edel> for WindowGrab {
    fn motion(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        _focus: Option<(<Edel as SeatHandler>::PointerFocus, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        // The window under the pointer sees nothing while it is dragged.
        handle.motion(data, None, event);
        let delta = (event.location - self.start.location).to_i32_round();
        match self.kind {
            Kind::Move => {
                self.current.loc = self.initial.loc + delta;
                data.space
                    .map_element(self.window.clone(), self.current.loc, true);
            }
            Kind::Resize(edges) => {
                let place = resized(self.initial, edges, delta);
                if place.size != self.current.size {
                    if let Some(toplevel) = self.window.toplevel() {
                        toplevel.with_pending_state(|state| {
                            state.states.set(State::Resizing);
                            state.size = Some(place.size);
                        });
                        toplevel.send_pending_configure();
                    }
                }
                self.current = place;
                data.space.map_element(self.window.clone(), place.loc, true);
            }
        }
        data.dirty = true;
    }

    fn relative_motion(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        focus: Option<(<Edel as SeatHandler>::PointerFocus, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn button(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if !handle.current_pressed().contains(&self.start.button) {
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn axis(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        details: AxisFrame,
    ) {
        handle.axis(data, details);
    }

    fn frame(&mut self, data: &mut Edel, handle: &mut PointerInnerHandle<'_, Edel>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event);
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event);
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event);
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event);
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event);
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event);
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event);
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut Edel,
        handle: &mut PointerInnerHandle<'_, Edel>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event);
    }

    fn start_data(&self) -> &GrabStartData<Edel> {
        &self.start
    }

    /// The button is up: the window keeps its new place, the client hears
    /// that the resize ended, and the state file is written.
    fn unset(&mut self, data: &mut Edel) {
        if let (Kind::Resize(_), Some(toplevel)) = (self.kind, self.window.toplevel()) {
            toplevel.with_pending_state(|state| {
                state.states.unset(State::Resizing);
            });
            toplevel.send_pending_configure();
        }
        data.placed(&self.window, self.current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn resizing_keeps_the_opposite_edges() {
        let start = rect(100, 100, 400, 300);
        assert_eq!(
            resized(start, ResizeEdge::BottomRight, (50, 20).into()),
            rect(100, 100, 450, 320)
        );
        assert_eq!(
            resized(start, ResizeEdge::TopLeft, (50, 20).into()),
            rect(150, 120, 350, 280)
        );
        assert_eq!(
            resized(start, ResizeEdge::Left, (-30, 99).into()),
            rect(70, 100, 430, 300)
        );
        assert_eq!(
            resized(start, ResizeEdge::Bottom, (99, -50).into()),
            rect(100, 100, 400, 250)
        );
    }

    #[test]
    fn a_window_never_shrinks_below_the_minimum() {
        let start = rect(100, 100, 400, 300);
        let tiny = resized(start, ResizeEdge::TopLeft, (1000, 1000).into());
        assert_eq!(
            tiny,
            rect(404, 336, 96, 64),
            "the bottom right corner stays put"
        );
    }

    #[test]
    fn super_right_drag_takes_the_nearest_corner() {
        let r = rect(0, 0, 100, 100);
        assert_eq!(nearest_corner(r, (10.0, 10.0).into()), ResizeEdge::TopLeft);
        assert_eq!(nearest_corner(r, (90.0, 10.0).into()), ResizeEdge::TopRight);
        assert_eq!(
            nearest_corner(r, (10.0, 90.0).into()),
            ResizeEdge::BottomLeft
        );
        assert_eq!(
            nearest_corner(r, (90.0, 90.0).into()),
            ResizeEdge::BottomRight
        );
    }
}
