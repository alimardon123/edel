//! The Edel OS compositor's parts that need no display (roadmap M4.2a):
//! design tokens, frame telemetry, outputs, the window policies (floating,
//! tiling, M4.5), window frames (M4.4) and the settings from the system
//! file (M4.5).
//! The binary (`main.rs`) adds the Wayland state and the backends.

pub mod frame;
pub mod layout;
pub mod settings;
pub mod telemetry;
pub mod tiling;
pub mod tokens;
