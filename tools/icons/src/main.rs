//! Renders Wisp's icon sources into the files the app embeds:
//!
//! * `assets/icons/wisp-<size>.png` for every size in `SIZES`
//! * `assets/wisp.ico`, the multi-size icon Windows shows for `wisp.exe`
//!
//! Sizes up to 32 px come from `icon-small.svg`, which is drawn on the 16 px grid
//! so tray and title-bar icons stay crisp; larger ones from `icon.svg`.
//!
//! Run after editing either SVG:
//!
//! ```sh
//! cargo run --release --manifest-path tools/icons/Cargo.toml
//! ```

use resvg::{tiny_skia, usvg};
use std::error::Error;
use std::path::Path;

const SIZES: [u32; 11] = [16, 20, 24, 32, 40, 48, 64, 128, 256, 512, 1024];
/// What Windows asks for at 100–200 % scaling in Explorer, Alt+Tab, the taskbar and the tray.
const ICO_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
const SMALL_MAX: u32 = 32;

fn render(svg: &usvg::Tree, size: u32) -> tiny_skia::Pixmap {
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("icon sizes are non-zero");
    let scale = size as f32 / svg.size().width();
    resvg::render(svg, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap
}

fn main() -> Result<(), Box<dyn Error>> {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let load = |name: &str| -> Result<usvg::Tree, Box<dyn Error>> {
        let data = std::fs::read(assets.join(name))?;
        Ok(usvg::Tree::from_data(&data, &usvg::Options::default())?)
    };
    let large = load("icon.svg")?;
    let small = load("icon-small.svg")?;
    let source = |size: u32| if size <= SMALL_MAX { &small } else { &large };

    std::fs::create_dir_all(assets.join("icons"))?;
    for size in SIZES {
        let path = assets.join(format!("icons/wisp-{size}.png"));
        render(source(size), size).save_png(&path)?;
        println!("wrote {}", path.display());
    }

    let mut icon = ico::IconDir::new(ico::ResourceType::Icon);
    for size in ICO_SIZES {
        let pixmap = render(source(size), size);
        // tiny-skia keeps premultiplied alpha; ICO wants it straight.
        let rgba: Vec<u8> = pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect();
        icon.add_entry(ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(size, size, rgba))?);
    }
    let path = assets.join("wisp.ico");
    icon.write(std::fs::File::create(&path)?)?;
    println!("wrote {}", path.display());
    Ok(())
}
