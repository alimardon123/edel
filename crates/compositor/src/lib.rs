//! The Edel OS compositor's parts that need no display (roadmap M4.2a):
//! design tokens, frame telemetry, outputs, the window policy trait and
//! window frames (M4.4).
//! The binary (`main.rs`) adds the Wayland state and the backends.

pub mod frame;
pub mod layout;
pub mod telemetry;
pub mod tokens;
