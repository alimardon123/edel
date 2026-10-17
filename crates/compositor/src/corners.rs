//! Rounded window corners (roadmap M5.14, built in M8.21): which windows
//! round their corners and by how much, the corners each part of a window
//! clips, and the two shaders that draw them on the GPU (`rounded.rs`
//! compiles and uses them). Pure and tested.

use crate::effects::Tier;

/// The corner radius a window is drawn with, in logical pixels: the
/// `radius_window` token on Full and Balanced, and none (square) on Lite,
/// for a maximized or fullscreen window, and for a tiled one whose
/// neighbours touch it (`gap` 0), as corners there would only show the
/// wallpaper in slivers between windows.
pub fn window_radius(
    tier: Tier,
    maximized: bool,
    fullscreen: bool,
    tiled: bool,
    gap: u32,
    token: u32,
) -> u32 {
    if tier == Tier::Lite || maximized || fullscreen || (tiled && gap == 0) {
        0
    } else {
        token
    }
}

/// The radii a window's own surfaces are clipped with, in the order the
/// clip shader takes them: top left, top right, bottom right, bottom left.
/// Under a title bar the top corners are the bar's, so they stay square;
/// inside a border the curve is the border's inner one, `radius - border`.
pub fn surface_radii(radius: f32, border: f32, bar: bool) -> [f32; 4] {
    let inner = (radius - border).max(0.0);
    let top = if bar { 0.0 } else { inner };
    [top, top, inner, inner]
}

/// The four corner squares of a `w` by `h` rectangle whose corners have
/// `radii` (top left, top right, bottom right, bottom left), each as
/// (x, y, w, h) from the rectangle's top left, rounded up to whole pixels;
/// a corner of radius 0 gives none. A clipped surface is not opaque in
/// these, so whatever lies under the corner is drawn.
pub fn corner_squares(w: i32, h: i32, radii: [f32; 4]) -> Vec<(i32, i32, i32, i32)> {
    let mut squares = Vec::new();
    for (corner, radius) in radii.into_iter().enumerate() {
        if radius <= 0.0 {
            continue;
        }
        let s = (radius.ceil() as i32).min(w).min(h);
        squares.push(match corner {
            0 => (0, 0, s, s),
            1 => (w - s, 0, s, s),
            2 => (w - s, h - s, s, s),
            _ => (0, h - s, s, s),
        });
    }
    squares
}

/// The clip shader's `uv_to_px`: how a texture coordinate maps to a pixel
/// of the clip rectangle, for a texture `tex` (w, h) whose part `src`
/// (x, y, w, h, in the texture's pixels) is drawn into a destination
/// `dst` (w, h) pixels big, the clip rectangle lying `clip` (x, y) pixels
/// from the destination's top left. Only for a texture drawn untransformed
/// (the caller checks): pixel = uv * (a, b) + (c, d).
pub fn uv_to_px(
    tex: (f64, f64),
    src: (f64, f64, f64, f64),
    dst: (f64, f64),
    clip: (f64, f64),
) -> [f32; 4] {
    let sx = dst.0 / src.2;
    let sy = dst.1 / src.3;
    [
        (tex.0 * sx) as f32,
        (tex.1 * sy) as f32,
        (-src.0 * sx - clip.0) as f32,
        (-src.1 * sy - clip.1) as f32,
    ]
}

/// The clip shader (a texture shader for `GlesRenderer::compile_custom_texture_shader`):
/// draws a texture as smithay's own shader does, then fades out what lies
/// outside a rectangle `size` pixels big with corners of `radii` (top left,
/// top right, bottom right, bottom left), smoothed over one pixel. Its
/// uniforms are `uv_to_px` (see [`uv_to_px`]), `clip_size` and `radii`.
pub const CLIP_SHADER: &str = r#"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

uniform vec4 uv_to_px;
uniform vec2 clip_size;
uniform vec4 radii;

float corner(vec2 p, vec2 size, vec4 r) {
    vec2 half_size = size * 0.5;
    vec2 q = p - half_size;
    float radius = q.x < 0.0 ? (q.y < 0.0 ? r.x : r.w) : (q.y < 0.0 ? r.y : r.z);
    radius = min(radius, min(half_size.x, half_size.y));
    vec2 d = abs(q) - half_size + vec2(radius);
    return min(max(d.x, d.y), 0.0) + length(max(d, 0.0)) - radius;
}

void main() {
    vec4 color = texture2D(tex, v_coords);

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

    vec2 p = v_coords * uv_to_px.xy + uv_to_px.zw;
    color = color * clamp(0.5 - corner(p, clip_size, radii), 0.0, 1.0);

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// The ring shader (a pixel shader for `GlesRenderer::compile_custom_pixel_shader`):
/// a window's border below its title bar, its left side in `start`, its
/// right in `end`, its bottom a blend of the two across, `edge` pixels
/// wide, its bottom corners of `radius` and its top open, smoothed over
/// one pixel. Colours are straight (not premultiplied) RGBA. Lengths are
/// logical pixels; `scale` is the physical pixels per logical pixel.
pub const RING_SHADER: &str = r#"#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform float alpha;
uniform vec2 size;
varying vec2 v_coords;

uniform vec4 start;
uniform vec4 end;
uniform float edge;
uniform float radius;
uniform float scale;

float bottom_round(vec2 p, vec2 lo, vec2 hi, float r) {
    vec2 c = (lo + hi) * 0.5;
    vec2 h = (hi - lo) * 0.5;
    vec2 q = p - c;
    float rr = q.y > 0.0 ? r : 0.0;
    vec2 d = abs(q) - h + vec2(rr);
    return min(max(d.x, d.y), 0.0) + length(max(d, 0.0)) - rr;
}

void main() {
    vec2 p = v_coords * size;
    float outer = bottom_round(p, vec2(0.0, -radius - 1.0), size, radius);
    float inner = bottom_round(p, vec2(edge, -radius - 1.0), size - vec2(edge), max(radius - edge, 0.0));
    float cover = clamp(0.5 - outer * scale, 0.0, 1.0) * clamp(0.5 + inner * scale, 0.0, 1.0);
    vec4 c = p.x < edge ? start : (p.x > size.x - edge ? end : mix(start, end, p.x / size.x));
    gl_FragColor = vec4(c.rgb * c.a, c.a) * cover * alpha;
}
"#;

/// The clip shader's uniform names, in the order `rounded.rs` sets them.
pub const CLIP_UNIFORMS: [&str; 3] = ["uv_to_px", "clip_size", "radii"];
/// The ring shader's uniform names.
pub const RING_UNIFORMS: [&str; 5] = ["start", "end", "edge", "radius", "scale"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_radius_is_the_token_on_full_and_balanced_and_square_otherwise() {
        assert_eq!(window_radius(Tier::Full, false, false, false, 0, 10), 10);
        assert_eq!(
            window_radius(Tier::Balanced, false, false, false, 0, 10),
            10
        );
        assert_eq!(window_radius(Tier::Lite, false, false, false, 0, 10), 0);
        assert_eq!(window_radius(Tier::Full, true, false, false, 0, 10), 0);
        assert_eq!(window_radius(Tier::Full, false, true, false, 0, 10), 0);
        assert_eq!(window_radius(Tier::Full, false, false, true, 0, 10), 0);
        assert_eq!(window_radius(Tier::Full, false, false, true, 8, 10), 10);
    }

    #[test]
    fn surface_radii_keep_square_top_corners_under_a_bar() {
        assert_eq!(surface_radii(10.0, 1.0, true), [0.0, 0.0, 9.0, 9.0]);
        assert_eq!(surface_radii(10.0, 1.0, false), [9.0; 4]);
        assert_eq!(surface_radii(0.5, 1.0, false), [0.0; 4]);
    }

    #[test]
    fn corner_squares_skip_square_corners_and_fit_small_rectangles() {
        assert_eq!(
            corner_squares(100, 50, [0.0, 0.0, 9.0, 9.0]),
            vec![(91, 41, 9, 9), (0, 41, 9, 9)]
        );
        let small = corner_squares(6, 6, [9.0; 4]);
        assert_eq!(small.len(), 4);
        assert!(small.iter().all(|s| s.2 == 6 && s.3 == 6));
    }

    #[test]
    fn uv_to_px_maps_texture_coordinates_to_clip_pixels() {
        let whole = uv_to_px(
            (200.0, 100.0),
            (0.0, 0.0, 200.0, 100.0),
            (400.0, 200.0),
            (10.0, 0.0),
        );
        assert_eq!(whole, [400.0, 200.0, -10.0, 0.0]);
        let middle = (0.5 * whole[0] + whole[2], 0.5 * whole[1] + whole[3]);
        assert_eq!(middle, (190.0, 100.0));
        let cropped = uv_to_px(
            (200.0, 100.0),
            (50.0, 0.0, 100.0, 100.0),
            (100.0, 100.0),
            (0.0, 0.0),
        );
        assert_eq!(cropped, [200.0, 100.0, -50.0, 0.0]);
        assert_eq!(0.25 * cropped[0] + cropped[2], 0.0, "the crop's left edge");
    }

    #[test]
    fn the_clip_shader_has_its_defines_version_and_uniforms() {
        assert!(CLIP_SHADER.contains("//_DEFINES_"));
        assert!(CLIP_SHADER.contains("#version 100"));
        for name in CLIP_UNIFORMS {
            assert!(CLIP_SHADER.contains(name), "{name}");
        }
    }

    #[test]
    fn the_ring_shader_leaves_its_version_to_smithay_and_has_its_uniforms() {
        assert!(!RING_SHADER.contains("#version"));
        assert!(!RING_SHADER.contains("//_DEFINES_"));
        for name in RING_UNIFORMS {
            assert!(RING_SHADER.contains(name), "{name}");
        }
    }
}
