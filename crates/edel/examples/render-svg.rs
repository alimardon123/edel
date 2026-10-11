//! Draws an SVG file into a PNG of the SVG's own size, with the resvg the
//! icons are drawn with: `design/wallpapers/make.py` makes our own
//! wallpapers with it (M5.10c4b). A tool for the repository, not shipped.
//!
//! `cargo run -q -p edel --features icons --example render-svg -- IN.svg OUT.png`

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [svg, png] = args.as_slice() else {
        return Err("give an SVG file to read and a PNG file to write".into());
    };
    let data = std::fs::read(svg).map_err(|e| format!("{svg} could not be read: {e}"))?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default())
        .map_err(|e| format!("{svg} is not an SVG resvg reads: {e}"))?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| format!("{svg} has no size to draw"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap
        .save_png(png)
        .map_err(|e| format!("{png} could not be written: {e}"))
}
