use std::f32::consts::TAU;

use super::*;

const FIRST_LINE: usize = 10;
const ROWS: usize = 48;
const STRETCH: usize = 200;
const FRAME_STEP: u64 = 709_379;
const LINE_STEP: u64 = 1135;
const ORIGIN: u64 = 1000;

/// What sits at a sample of field line `line` in frame `frame`: luma and the parts of chroma.
struct Scene {
    luma: fn(usize) -> f32,
    u: f32,
    v: f32,
}

fn start(frame: u64, line: usize) -> u64 {
    ORIGIN + frame * FRAME_STEP + line as u64 * LINE_STEP
}

/// A 4x-subcarrier PAL stream of `frames` frames of one field parity, V switching each line.
fn stream(frames: u64, width: usize, scene: &Scene, chroma_only: bool) -> Samples {
    let mut data = vec![0.0f32; start(frames, FIRST_LINE + ROWS) as usize + width];
    for frame in 0..frames {
        for line in FIRST_LINE..FIRST_LINE + ROWS {
            let switch = if (frame * 625 + line as u64).is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            let from = start(frame, line);
            for k in 0..width {
                let index = from + k as u64;
                let phase = TAU * 0.25 * (index % 4) as f32;
                let luma = if chroma_only { 0.0 } else { (scene.luma)(k) };
                data[index as usize] =
                    luma + scene.u * phase.sin() + switch * scene.v * phase.cos();
            }
        }
    }
    let mut samples = Samples::default();
    samples.extend(&data);
    samples
}

/// Every line's split chroma, taken as soon as it is ready, as the picture does.
fn split(depth: Depth, frames: u64, scene: &Scene) -> Vec<Vec<f32>> {
    let mut transform = TransformPal::new(STRETCH, FIRST_LINE, ROWS, depth);
    let samples = stream(frames, transform.width(), scene, false);
    let mut keys = Vec::new();
    let mut out: Vec<Vec<f32>> = Vec::new();
    let take = |transform: &TransformPal, keys: &[(RowKey, usize)], out: &mut Vec<Vec<f32>>| {
        out.resize(keys.len(), Vec::new());
        for (n, &(key, offset)) in keys.iter().enumerate() {
            if let (true, Some((row_start, _, chroma))) = (out[n].is_empty(), transform.row(key)) {
                assert_eq!(row_start as usize % 4, offset % 4, "row off the lattice");
                out[n] = chroma.to_vec();
            }
        }
    };
    for frame in 0..frames {
        transform.begin_field(0);
        take(&transform, &keys, &mut out);
        for line in FIRST_LINE..FIRST_LINE + ROWS {
            let wanted = start(frame, line);
            let key = transform
                .push(line as u32, &samples, wanted)
                .expect("a held line");
            keys.push((key, wanted as usize));
        }
    }
    transform.finish();
    take(&transform, &keys, &mut out);
    assert!(
        out.iter().all(|line| !line.is_empty()),
        "every line is split"
    );
    out
}

fn worst(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// The middle frames and lines, which every tile around them reached.
fn settled(depth: Depth, frames: u64) -> impl Iterator<Item = usize> {
    let skip = if depth == Depth::Frames { 2 } else { 0 };
    (skip..frames as usize - skip)
        .flat_map(|frame| (16..ROWS - 16).map(move |line| frame * ROWS + line))
}

#[test]
fn a_flat_colour_comes_out_as_chroma_alone() {
    let scene = Scene {
        luma: |_| 0.5,
        u: 0.2,
        v: 0.15,
    };
    for depth in [Depth::Field, Depth::Frames] {
        let frames = 8;
        let chroma = split(depth, frames, &scene);
        let width = chroma[0].len();
        let want = stream(frames, width, &scene, true);
        for at in settled(depth, frames) {
            let (frame, line) = ((at / ROWS) as u64, FIRST_LINE + at % ROWS);
            let from = start(frame, line);
            let expect = want.span(from, from + width as u64);
            let err = worst(&chroma[at][32..width - 32], &expect[32..width - 32]);
            assert!(
                err < 0.01,
                "{depth:?} frame {frame} line {line}: off by {err}"
            );
        }
    }
}

#[test]
fn fine_grey_detail_stays_luma() {
    let scene = Scene {
        luma: |k| 0.5 + 0.3 * (TAU * 0.23 * k as f32).sin(),
        u: 0.0,
        v: 0.0,
    };
    for depth in [Depth::Field, Depth::Frames] {
        let frames = 8;
        let chroma = split(depth, frames, &scene);
        let width = chroma[0].len();
        for at in settled(depth, frames) {
            let leak = chroma[at][32..width - 32]
                .iter()
                .fold(0.0f32, |peak, c| peak.max(c.abs()));
            assert!(
                leak < 0.03,
                "{depth:?} line {at}: grey detail read as chroma {leak}"
            );
        }
    }
}

#[test]
fn rows_keep_the_subcarrier_lattice_across_lines_and_frames() {
    let mut transform = TransformPal::new(STRETCH, 0, 4, Depth::Frames);
    transform.begin_field(0);
    let samples = Samples::default();
    let anchor = 1_000;
    transform.push(0, &samples, anchor);
    let at = |transform: &mut TransformPal, frame, line, wanted| {
        transform.row_start(0, frame, line, wanted)
    };
    let line = at(&mut transform, 0, 1, anchor + LINE_STEP + 1);
    assert_eq!(line, anchor + LINE_STEP);
    let frame = at(&mut transform, 1, 0, anchor + FRAME_STEP - 1);
    assert_eq!(frame, anchor + FRAME_STEP);
}
