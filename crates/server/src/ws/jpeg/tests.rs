use zune_jpeg::{JpegDecoder, zune_core::bytestream::ZCursor};

use super::*;

fn decoded(jpeg: &[u8]) -> (usize, usize, Vec<u8>) {
    let mut decoder = JpegDecoder::new(ZCursor::new(jpeg));
    let pixels = decoder.decode().expect("a decodable JPEG");
    let info = decoder.info().expect("headers");
    (usize::from(info.width), usize::from(info.height), pixels)
}

fn decoded_gray(jpeg: &[u8]) -> (usize, usize, Vec<u8>) {
    let (width, height, pixels) = decoded(jpeg);
    let expanded = pixels.len() / (width * height);
    (
        width,
        height,
        pixels.into_iter().step_by(expanded).collect(),
    )
}

fn psnr_db(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let error: f64 = a
        .iter()
        .zip(b)
        .map(|(&a, &b)| (f64::from(a) - f64::from(b)).powi(2))
        .sum::<f64>()
        / a.len() as f64;
    10.0 * (255.0f64.powi(2) / error.max(1e-9)).log10()
}

fn gray(width: u16, height: u16) -> VideoPicture {
    let luma = (0..usize::from(height))
        .flat_map(|y| {
            (0..usize::from(width)).map(move |x| {
                let wave = ((x as f32 * 0.21).sin() + (y as f32 * 0.13).cos()) * 50.0;
                (128.0 + wave + x as f32 * 0.4) as u8
            })
        })
        .collect();
    VideoPicture {
        width,
        height,
        luma,
        rgb: Vec::new(),
    }
}

fn color(width: u16, height: u16) -> VideoPicture {
    let rgb = (0..usize::from(height))
        .flat_map(|y| {
            (0..usize::from(width)).flat_map(move |x| {
                [
                    (40.0 + x as f32 * 1.5) as u8,
                    (200.0 - y as f32 * 2.0) as u8,
                    (128.0 + ((x + y) as f32 * 0.1).sin() * 90.0) as u8,
                ]
            })
        })
        .collect();
    VideoPicture {
        width,
        height,
        luma: Vec::new(),
        rgb,
    }
}

#[test]
fn a_gray_picture_decodes_to_nearly_itself_at_any_size() {
    for (width, height) in [(64, 48), (37, 21), (8, 8), (1, 1), (129, 3)] {
        let picture = gray(width, height);
        let jpeg = encode(&picture).expect("encodes");
        let (wide, high, pixels) = decoded_gray(&jpeg);
        assert_eq!((wide, high), (usize::from(width), usize::from(height)));
        let fidelity = psnr_db(&pixels, &picture.luma);
        assert!(fidelity > 36.0, "{width}x{height}: {fidelity:.1} dB");
    }
}

#[test]
fn a_colour_picture_decodes_to_nearly_itself_at_any_size() {
    for (width, height) in [(64, 48), (37, 21), (16, 16), (1, 1), (130, 5)] {
        let picture = color(width, height);
        let jpeg = encode(&picture).expect("encodes");
        let (wide, high, pixels) = decoded(&jpeg);
        assert_eq!((wide, high), (usize::from(width), usize::from(height)));
        let fidelity = psnr_db(&pixels, &picture.rgb);
        assert!(fidelity > 32.0, "{width}x{height}: {fidelity:.1} dB");
    }
}

#[test]
fn noise_and_extremes_stay_decodable() {
    let mut state = 0x2545_f491u32;
    let mut noise = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state >> 24) as u8
    };
    let noisy = VideoPicture {
        width: 48,
        height: 40,
        luma: Vec::new(),
        rgb: (0..48 * 40 * 3).map(|_| noise()).collect(),
    };
    let (wide, high, _) = decoded(&encode(&noisy).expect("encodes"));
    assert_eq!((wide, high), (48, 40));

    let checker = VideoPicture {
        width: 32,
        height: 32,
        luma: (0..32 * 32)
            .map(|at| if (at / 32 + at % 32) % 2 == 0 { 0 } else { 255 })
            .collect(),
        rgb: Vec::new(),
    };
    let (_, _, pixels) = decoded_gray(&encode(&checker).expect("encodes"));
    assert!(psnr_db(&pixels, &checker.luma) > 20.0);
}

#[test]
fn a_picture_is_far_smaller_than_its_pixels() {
    let picture = color(640, 480);
    let jpeg = encode(&picture).expect("encodes");
    assert!(jpeg.len() * 10 < picture.rgb.len(), "{} bytes", jpeg.len());
}

#[test]
fn a_picture_without_the_pixels_its_size_asks_for_is_refused() {
    assert_eq!(encode(&VideoPicture::default()), None);
    let mut short = gray(16, 16);
    short.luma.pop();
    assert_eq!(encode(&short), None);
    let mut long = color(16, 16);
    long.rgb.push(0);
    assert_eq!(encode(&long), None);
}

#[test]
fn the_fast_transform_matches_the_textbook_one() {
    let mut cells: [f32; CELLS] =
        std::array::from_fn(|cell| ((cell * 37 + 11) % 255) as f32 - 128.0);
    let source = cells;
    transform(&mut cells);
    for v in 0..BLOCK {
        for u in 0..BLOCK {
            let norm = |k: usize| {
                if k == 0 {
                    std::f32::consts::FRAC_1_SQRT_2
                } else {
                    1.0
                }
            };
            let mut sum = 0.0;
            for y in 0..BLOCK {
                for x in 0..BLOCK {
                    let angle = |at: usize, k: usize| {
                        ((2 * at + 1) as f32 * k as f32 * std::f32::consts::PI / 16.0).cos()
                    };
                    sum += source[y * BLOCK + x] * angle(x, u) * angle(y, v);
                }
            }
            let textbook = 0.25 * norm(u) * norm(v) * sum;
            let fast = cells[v * BLOCK + u] / (TRANSFORM_SCALE[u] * TRANSFORM_SCALE[v] * 8.0);
            assert!(
                (fast - textbook).abs() < 0.01,
                "({u},{v}): {fast} vs {textbook}"
            );
        }
    }
}

#[test]
fn the_zigzag_visits_every_cell_once_along_the_diagonals() {
    let mut seen = [false; CELLS];
    let mut diagonal = 0;
    for &cell in &ZIGZAG {
        assert!(!std::mem::replace(&mut seen[cell], true));
        let now = cell / BLOCK + cell % BLOCK;
        assert!(now == diagonal || now == diagonal + 1);
        diagonal = now;
    }
}

#[test]
fn every_coefficient_a_block_can_hold_has_a_code() {
    for table in [&TABLES.luma.ac, &TABLES.chroma.ac] {
        assert_eq!(
            table
                .bits
                .iter()
                .map(|&count| usize::from(count))
                .sum::<usize>(),
            table.values.len()
        );
        for run in 0..16u8 {
            for size in 1..=10u8 {
                assert!(
                    table.size[usize::from((run << 4) | size)] > 0,
                    "{run}/{size}"
                );
            }
        }
        assert!(table.size[usize::from(ZERO_RUN)] > 0);
        assert!(table.size[usize::from(END_OF_BLOCK)] > 0);
    }
    for table in [&TABLES.luma.dc, &TABLES.chroma.dc] {
        for size in 0..=11 {
            assert!(table.size[size] > 0, "{size}");
        }
    }
}

#[test]
fn clients_watching_one_picture_share_one_encoding() {
    let memo = Memo::default();
    let picture = Arc::new(color(32, 32));
    let first = memo.encoded(&picture).expect("encodes");
    let again = memo.encoded(&picture).expect("encodes");
    assert_eq!(first.as_ptr(), again.as_ptr());

    let other = Arc::new(color(32, 32));
    let separate = memo.encoded(&other).expect("encodes");
    assert_ne!(first.as_ptr(), separate.as_ptr());
    assert_eq!(first, separate);
    assert_eq!(memo.encoded(&Arc::new(VideoPicture::default())), None);
}

#[test]
fn only_the_latest_pictures_are_remembered() {
    let memo = Memo::default();
    for _ in 0..REMEMBERED * 3 {
        memo.encoded(&Arc::new(gray(8, 8)));
    }
    let recent = memo.recent.lock().expect("lock");
    assert_eq!(recent.len(), REMEMBERED);
}
