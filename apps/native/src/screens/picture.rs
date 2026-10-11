use std::{
    collections::HashSet,
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
};

use anyhow::Context;
use zgui_render_wgpu::Pixels;

const VARIED: usize = 8;

pub fn folder() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            PathBuf::from,
        )
        .join("screens")
}

pub fn rgb(pixels: &Pixels) -> Vec<u8> {
    let swap = pixels.is_bgra();
    pixels
        .bytes()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| {
            if swap {
                [px[2], px[1], px[0]]
            } else {
                [px[0], px[1], px[2]]
            }
        })
        .collect()
}

pub fn varied(rgb: &[u8]) -> bool {
    let colours: HashSet<&[u8; 3]> = rgb.as_chunks::<3>().0.iter().step_by(97).collect();
    colours.len() >= VARIED
}

pub fn write(pixels: &Pixels, rgb: &[u8], path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let size = pixels.size();
    let file = File::create(path).with_context(|| format!("cannot create {}", path.display()))?;
    let mut encoder = png::Encoder::new(
        BufWriter::new(file),
        u32::try_from(size.width)?,
        u32::try_from(size.height)?,
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgb)?;
    writer.finish()?;
    Ok(())
}
