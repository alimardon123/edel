//! Live blur behind shell-ui's popups (roadmap M5.14, built in M8.21 part
//! 21d): the rules `backdrop.rs` follows and the two shaders of a dual
//! Kawase blur. Pure and tested; `backdrop.rs` compiles the shaders once and
//! draws the blur with them.
//!
//! The blur is made at half the screen's size and less, so its cost is
//! that of a small picture: the picture behind is drawn once, halved
//! `passes` times with `DOWN_SHADER`, then brought back up with `UP_SHADER`
//! into a texture half the screen's size. How far each pass samples grows
//! with `blur`, so a strong blur takes few passes and a weak one many more
//! samples (`offset`).

/// How many times the picture is halved for `blur` percent of blur
/// (`appearance.blur`, `tokens.blur`): none at 0, once up to 33, twice up
/// to 66, and three times from 67 on.
pub fn passes(blur: u32) -> u32 {
    match blur {
        0 => 0,
        1..=33 => 1,
        34..=66 => 2,
        _ => 3,
    }
}

/// How far each pass samples, in source pixels, for `blur` percent (at
/// most 100, which gives 3.0).
pub fn offset(blur: u32) -> f32 {
    1.0 + blur.min(100) as f32 / 50.0
}

/// How many pixels a blur reaches past what it covers, for `blur` percent:
/// the sampling offset grown by each pass, rounded up, with a small margin.
/// A change farther away than this does not change the blur.
pub fn reach(blur: u32) -> i32 {
    (offset(blur) * 2f32.powi(passes(blur) as i32)).ceil() as i32 + 2
}

/// Half of `size` on each side, rounded up and at least 1 pixel: the next
/// level down of the blur's pictures.
pub fn halved(size: (i32, i32)) -> (i32, i32) {
    (((size.0 + 1) / 2).max(1), ((size.1 + 1) / 2).max(1))
}

/// The sizes of the blur's pictures: `size`, then each halving, `passes + 1`
/// entries in all.
pub fn levels(size: (i32, i32), passes: u32) -> Vec<(i32, i32)> {
    let mut sizes = vec![size];
    for _ in 0..passes {
        let last = sizes[sizes.len() - 1];
        sizes.push(halved(last));
    }
    sizes
}

/// The downsampling pass (dual Kawase), a texture shader for
/// `GlesRenderer::compile_custom_texture_shader`: the centre sample weighs
/// four times the four diagonal ones, averaged over eight. Its uniforms are
/// `BLUR_UNIFORMS`.
pub const DOWN_SHADER: &str = r#"#version 100

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

uniform vec2 half_pixel;
uniform float offset;

vec4 sample(vec2 uv) {
#if defined(NO_ALPHA)
    return vec4(texture2D(tex, uv).rgb, 1.0);
#else
    return texture2D(tex, uv);
#endif
}

void main() {
    vec2 uv = v_coords;
    vec4 sum = sample(uv) * 4.0
        + sample(uv - half_pixel * offset)
        + sample(uv + half_pixel * offset)
        + sample(uv + vec2(half_pixel.x, -half_pixel.y) * offset)
        + sample(uv - vec2(half_pixel.x, -half_pixel.y) * offset);
    gl_FragColor = sum / 8.0 * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        gl_FragColor = vec4(0.0, 0.2, 0.0, 0.2) + gl_FragColor * 0.8;
#endif
}
"#;

/// The upsampling pass (dual Kawase), the counterpart of `DOWN_SHADER`: eight
/// samples around the pixel, the edges weighing one and the corners two,
/// averaged over twelve. Its uniforms are `BLUR_UNIFORMS`.
pub const UP_SHADER: &str = r#"#version 100

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

uniform vec2 half_pixel;
uniform float offset;

vec4 sample(vec2 uv) {
#if defined(NO_ALPHA)
    return vec4(texture2D(tex, uv).rgb, 1.0);
#else
    return texture2D(tex, uv);
#endif
}

void main() {
    vec2 uv = v_coords;
    vec2 o = half_pixel * offset;
    vec4 sum = sample(uv + vec2(-o.x * 2.0, 0.0))
        + sample(uv + vec2(-o.x, o.y)) * 2.0
        + sample(uv + vec2(0.0, o.y * 2.0))
        + sample(uv + vec2(o.x, o.y)) * 2.0
        + sample(uv + vec2(o.x * 2.0, 0.0))
        + sample(uv + vec2(o.x, -o.y)) * 2.0
        + sample(uv + vec2(0.0, -o.y * 2.0))
        + sample(uv + vec2(-o.x, -o.y)) * 2.0;
    gl_FragColor = sum / 12.0 * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        gl_FragColor = vec4(0.0, 0.2, 0.0, 0.2) + gl_FragColor * 0.8;
#endif
}
"#;

/// The uniform names both shaders take, in the order `backdrop.rs` sets
/// them: the size of a source texel (`half_pixel`) and the sampling offset.
pub const BLUR_UNIFORMS: [&str; 2] = ["half_pixel", "offset"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_follow_the_blur_percent_in_thirds() {
        assert_eq!(passes(0), 0);
        assert_eq!(passes(1), 1);
        assert_eq!(passes(33), 1);
        assert_eq!(passes(34), 2);
        assert_eq!(passes(66), 2);
        assert_eq!(passes(67), 3);
        assert_eq!(passes(100), 3);
    }

    #[test]
    fn offset_grows_with_the_blur_and_stops_at_three() {
        assert_eq!(offset(0), 1.0);
        assert_eq!(offset(50), 2.0);
        assert_eq!(offset(200), 3.0);
    }

    #[test]
    fn reach_covers_the_blur_past_its_passes() {
        assert_eq!(reach(0), 3);
        assert_eq!(reach(100), 26);
    }

    #[test]
    fn halved_rounds_up_and_stays_at_least_one() {
        assert_eq!(halved((1281, 801)), (641, 401));
        assert_eq!(halved((1, 1)), (1, 1));
    }

    #[test]
    fn levels_halve_the_screen_once_per_pass() {
        assert_eq!(
            levels((1280, 800), 3),
            vec![(1280, 800), (640, 400), (320, 200), (160, 100)]
        );
    }

    #[test]
    fn both_shaders_keep_the_frame_and_the_uniforms() {
        for shader in [DOWN_SHADER, UP_SHADER] {
            assert!(shader.contains("//_DEFINES_"));
            assert!(shader.contains("#version 100"));
            assert!(shader.contains("half_pixel"));
            assert!(shader.contains("offset"));
        }
    }
}
