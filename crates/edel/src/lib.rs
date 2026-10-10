//! The library half of `edel`: the settings file parser, which the compositor,
//! shell-ui and Settings link so every part reads the settings file with the
//! same code (ADR-008), the feature file reader, which tells them what a
//! machine has, the install plan Settings' installer page shows, the
//! keyboard shortcuts every part agrees on (M5.13), the keyboard layouts
//! (M5.21), the design tokens the
//! compositor and shell-ui draw with (M5.1b) and the layout presets
//! (M5.1c), and the apps people have with their icons, which shell-ui
//! lists and the compositor shows in title bars (M5.4c, M5.6a), and the
//! session's log, which the compositor writes and `edel` reads (M5.28a). It needs no
//! network or signing; those sit behind the binary's `cli` feature.

#[cfg(feature = "icons")]
pub mod app_icons;
pub mod apps;
pub mod backlight;
pub mod bluetooth;
pub mod features;
pub mod i18n;
#[cfg(feature = "icons")]
pub mod icons;
pub mod install;
pub mod keyboard;
pub mod network;
pub mod panel_edit;
pub mod places;
pub mod power;
pub mod presets;
pub mod scripts;
pub mod session_log;
pub mod settings;
pub mod shortcuts;
pub mod sound;
pub mod tokens;
pub mod tool;
pub mod users;
pub mod version;
