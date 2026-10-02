//! The library half of `edel`: the system file parser, which the compositor,
//! shell-ui and Settings link so every part reads `system.toml` with the
//! same code (ADR-008), and the install plan Settings' installer page shows. It needs no network or signing; those sit behind
//! the binary's `cli` feature.

pub mod install;
pub mod system;
