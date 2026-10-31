//! Where the library finds the owner files it builds in (ADR-010, the
//! architecture review of 2026-10-31): the design tokens, the icons, the
//! presets, themes, profiles and platform levels. They live at the
//! repository's root, beside the crates, so the library reads them through
//! `EDEL_SOURCE`, the folder that holds them: the repository by default,
//! or a copy named by the variable of that name when the crate is built
//! on its own.

fn main() {
    println!("cargo:rerun-if-env-changed=EDEL_SOURCE");
    let source = std::env::var("EDEL_SOURCE").unwrap_or_else(|_| {
        let crate_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
        format!("{crate_dir}/../..")
    });
    println!("cargo:rustc-env=EDEL_SOURCE={source}");
}
