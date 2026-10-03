//! The pointer's own arrow (roadmap M4.6b): drawn here, black with a white
//! edge, at any scale, for when no xcursor theme is installed. The images
//! use ARGB8888 with premultiplied alpha, in memory order (blue, green,
//! red, alpha), as cursor themes and Wayland buffers do.

/// The arrow's outline, in units of a 24-unit cursor; the tip, at 1,1, is
/// the hotspot.
const ARROW: [(f32, f32); 7] = [
    (1.0, 1.0),
    (1.0, 18.0),
    (5.0, 14.0),
    (8.0, 21.0),
    (10.5, 20.0),
    (7.5, 13.2),
    (13.0, 13.2),
];

/// The width of the white edge, in units.
const EDGE: f32 = 1.0;

/// A cursor image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Where the pointer is, from the image's top left.
    pub hotspot: (u32, u32),
    /// ARGB8888, premultiplied, in memory order.
    pub pixels: Vec<u8>,
}

/// The arrow for a cursor `size` pixels tall (24 at scale 1), smoothed by
/// sampling each pixel 16 times.
pub fn arrow(size: u32) -> Image {
    let unit = size.max(8) as f32 / 24.0;
    let width = (14.0 * unit).ceil() as u32;
    let height = (22.0 * unit).ceil() as u32;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let (mut white, mut black) = (0u32, 0u32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let p = (
                        (x as f32 + (sx as f32 + 0.5) / 4.0) / unit,
                        (y as f32 + (sy as f32 + 0.5) / 4.0) / unit,
                    );
                    if !inside(p) {
                        continue;
                    }
                    if edge_distance(p) < EDGE {
                        white += 1;
                    } else {
                        black += 1;
                    }
                }
            }
            let alpha = ((white + black) * 255 + 8) / 16;
            let grey = (white * 255 + 8) / 16;
            pixels.extend_from_slice(&[grey as u8, grey as u8, grey as u8, alpha as u8]);
        }
    }
    Image {
        width,
        height,
        hotspot: (
            (ARROW[0].0 * unit).round() as u32,
            (ARROW[0].1 * unit).round() as u32,
        ),
        pixels,
    }
}

/// Whether `p` is inside the arrow (even-odd rule).
fn inside(p: (f32, f32)) -> bool {
    let mut odd = false;
    for i in 0..ARROW.len() {
        let (a, b) = (ARROW[i], ARROW[(i + 1) % ARROW.len()]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            odd = !odd;
        }
    }
    odd
}

/// How far `p` is from the arrow's outline, in units.
fn edge_distance(p: (f32, f32)) -> f32 {
    (0..ARROW.len())
        .map(|i| {
            let (a, b) = (ARROW[i], ARROW[(i + 1) % ARROW.len()]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            let (nx, ny) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
            (nx * nx + ny * ny).sqrt()
        })
        .fold(f32::INFINITY, f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * image.width + x) * 4) as usize;
        image.pixels[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn the_arrow_is_black_inside_white_at_its_edge_and_clear_around() {
        let arrow = arrow(24);
        assert_eq!((arrow.width, arrow.height), (14, 22));
        assert_eq!(arrow.hotspot, (1, 1));
        assert_eq!(arrow.pixels.len(), 14 * 22 * 4);
        assert_eq!(pixel(&arrow, 3, 10), [0, 0, 0, 255], "the body");
        assert_eq!(pixel(&arrow, 1, 10), [255, 255, 255, 255], "the left edge");
        assert_eq!(pixel(&arrow, 13, 2), [0, 0, 0, 0], "beside the tip");
        assert_eq!(pixel(&arrow, 0, 21), [0, 0, 0, 0], "below the tail");
    }

    #[test]
    fn the_arrow_scales_with_its_size() {
        let big = arrow(48);
        assert_eq!((big.width, big.height), (28, 44));
        assert_eq!(big.hotspot, (2, 2));
        assert_eq!(pixel(&big, 6, 20), [0, 0, 0, 255]);
        // Premultiplied: no pixel is brighter than it is opaque.
        assert!(big.pixels.chunks(4).all(|p| p[0] <= p[3]));
    }
}
