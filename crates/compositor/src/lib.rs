//! The Edel OS compositor's parts that need no display (roadmap M4.2a):
//! design tokens, frame telemetry, outputs and the window policy trait.
//! The binary (`main.rs`) adds the Wayland state and the backends.

pub mod layout;
pub mod telemetry;
pub mod tokens;
