//! The app icon, drawn in code so the binary needs no image files: a little index
//! card (red top rule, blue lines, a highlighter stroke) on ballpoint blue.

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

type Rgba = [f32; 4];

const BALLPOINT: Rgba = [0.165, 0.310, 0.800, 1.0];
const CARD: Rgba = [0.988, 0.992, 1.0, 1.0];
const RED: Rgba = [0.875, 0.310, 0.353, 1.0];
const LINE: Rgba = [0.76, 0.81, 0.93, 1.0];
const HIGHLIGHT: Rgba = [1.0, 0.89, 0.36, 0.9];

/// Signed distance from (px, py) to a rounded rectangle; negative inside.
fn rounded_rect(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (hx, hy) = ((x1 - x0) / 2.0 - r, (y1 - y0) / 2.0 - r);
    let (dx, dy) = ((px - cx).abs() - hx, (py - cy).abs() - hy);
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    outside + dx.max(dy).min(0.0) - r
}

fn over(dst: Rgba, src: Rgba, coverage: f32) -> Rgba {
    let a = src[3] * coverage;
    let out_a = a + dst[3] * (1.0 - a);
    if out_a <= 0.0 {
        return [0.0; 4];
    }
    let mix = |i: usize| (src[i] * a + dst[i] * dst[3] * (1.0 - a)) / out_a;
    [mix(0), mix(1), mix(2), out_a]
}

/// Colour of the icon at a point in unit coordinates (0..1).
fn sample(u: f32, v: f32) -> Rgba {
    let inside = |d: f32| if d <= 0.0 { 1.0 } else { 0.0 };
    let mut c = [0.0; 4];
    c = over(c, BALLPOINT, inside(rounded_rect(u, v, 0.02, 0.02, 0.98, 0.98, 0.22)));
    let card = inside(rounded_rect(u, v, 0.17, 0.22, 0.83, 0.80, 0.06));
    c = over(c, CARD, card);
    c = over(c, HIGHLIGHT, card * inside(rounded_rect(u, v, 0.24, 0.47, 0.64, 0.585, 0.03)));
    c = over(c, RED, card * inside(rounded_rect(u, v, 0.17, 0.335, 0.83, 0.375, 0.0)));
    for y in [0.53, 0.67] {
        c = over(c, LINE, card * inside(rounded_rect(u, v, 0.24, y - 0.014, 0.76, y + 0.014, 0.01)));
    }
    c
}

pub fn app_icon(size: u32) -> Image {
    const SS: u32 = 4; // supersampling per axis
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(size, size);
    let pixels = buffer.make_mut_slice();
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    let s = sample(u, v);
                    for i in 0..3 {
                        acc[i] += s[i] * s[3];
                    }
                    acc[3] += s[3];
                }
            }
            let n = (SS * SS) as f32;
            let alpha = acc[3] / n;
            let channel = |i: usize| if acc[3] > 0.0 { (acc[i] / acc[3] * 255.0).round() as u8 } else { 0 };
            pixels[(y * size + x) as usize] = Rgba8Pixel { r: channel(0), g: channel(1), b: channel(2), a: (alpha * 255.0).round() as u8 };
        }
    }
    Image::from_rgba8(buffer)
}
