//! The Edel OS compositor's parts that need no display (roadmap M4.2a):
//! design tokens, frame telemetry, outputs, the window policies (floating,
//! tiling, M4.5), window frames (M4.4), the settings from the system
//! file (M4.5), effect tiers (M5.11a) and window animations (M5.11b).
//! The binary (`main.rs`) adds the Wayland state and the backends.

pub mod animation;
pub mod cursor;
pub mod effects;
pub mod frame;
pub mod layout;
pub mod settings;
pub mod telemetry;
pub mod tiling;
/// The design tokens live in `edel::tokens`, which shell-ui reads too
/// (M5.1b).
pub use edel::tokens;
