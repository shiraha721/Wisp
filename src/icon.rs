//! The app icon, from the PNGs in `assets/icons`. Those are rendered from
//! `assets/icon.svg` and `assets/icon-small.svg` by `tools/icons`.

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

const PNGS: [(u32, &[u8]); 9] = [
    (16, include_bytes!("../assets/icons/wisp-16.png")),
    (20, include_bytes!("../assets/icons/wisp-20.png")),
    (24, include_bytes!("../assets/icons/wisp-24.png")),
    (32, include_bytes!("../assets/icons/wisp-32.png")),
    (40, include_bytes!("../assets/icons/wisp-40.png")),
    (48, include_bytes!("../assets/icons/wisp-48.png")),
    (64, include_bytes!("../assets/icons/wisp-64.png")),
    (128, include_bytes!("../assets/icons/wisp-128.png")),
    (256, include_bytes!("../assets/icons/wisp-256.png")),
];

/// The icon as a PNG file, for places that need one on disk (Windows notifications).
#[cfg_attr(not(windows), allow(dead_code))]
pub const PNG_256: &[u8] = include_bytes!("../assets/icons/wisp-256.png");

/// The icon drawn for `size` pixels: the exact size if there is one, otherwise
/// the next larger (which the caller's platform scales down).
pub fn image(size: u32) -> Image {
    rgba(size)
        .map(|(data, width, height)| {
            let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
            pixels.make_mut_bytes().copy_from_slice(&data);
            Image::from_rgba8(pixels)
        })
        .unwrap_or_default()
}

/// Like [`image`], as straight RGBA bytes with their width and height.
pub fn rgba(size: u32) -> Option<(Vec<u8>, u32, u32)> {
    let (_, png) = PNGS.iter().find(|(s, _)| *s >= size).unwrap_or(&PNGS[PNGS.len() - 1]);
    decode(png)
}

fn decode(png: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut data = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut data).ok()?;
    if frame.color_type != png::ColorType::Rgba || frame.bit_depth != png::BitDepth::Eight {
        return None;
    }
    data.truncate(frame.buffer_size());
    Some((data, frame.width, frame.height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_size_decodes() {
        for (size, png) in PNGS {
            let (data, width, height) = decode(png).unwrap_or_else(|| panic!("{size} px icon doesn't decode"));
            assert_eq!((width, height), (size, size));
            assert_eq!(data.len() as u32, size * size * 4);
        }
        assert_eq!(image(18).size().width, 20, "picks the next larger size");
        assert_eq!(image(1000).size().width, 256);
    }
}
