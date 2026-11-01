//! Add-ons (M7.2a, docs/FEATURES.md): the rules of building, fetching and
//! mounting them. They are pure functions over paths and file stamps, so
//! the tests run without root, Alpine or a disk; `edel image build` and the
//! updater call them and do the copying, mounting and packing themselves.
//!
//! An add-on is a feature (`addon = true`) that a machine mounts beside the
//! slot's root, so a newer slot never has to ship it and an old one never
//! breaks it. What it installs lands in the folders below; anything it
//! would change in the slot itself is refused.
//!
//! `build` holds the image builder's rules and `machine` what a machine
//! does with the add-ons it fetches; both are reached as `addons::NAME`.

mod build;
mod machine;

pub use build::*;
pub use machine::*;
