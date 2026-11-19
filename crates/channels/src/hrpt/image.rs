use sdrmm_decoder::VideoPicture;

use super::HrptPicture;

pub const MAX_WIDTH: usize = 2_048;
pub const MAX_LINES: usize = 6_144;
pub const PLANES: usize = 3;
/// Planes a downlink fills, in this order.
pub const BLUE: usize = 0;
pub const RED: usize = 1;
pub const THERMAL: usize = 2;
/// Mean red level, of 255, below which the day side is too dark to show.
const DAYLIGHT: u64 = 24;
const STRETCH_CLIP: f64 = 0.005;
const MS_PER_DAY: u32 = 86_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Placed,
    Full,
}

/// One scan line as a downlink hands it over: three planes of 10-bit levels.
pub struct Scan {
    pub planes: [[u16; MAX_WIDTH]; PLANES],
    pub time_ms: Option<u32>,
}

impl Scan {
    #[must_use]
    pub fn new() -> Self {
        Self {
            planes: [[0; MAX_WIDTH]; PLANES],
            time_ms: None,
        }
    }
}

/// `ms` of the UTC day as `HH:MM:SS UTC`.
#[must_use]
pub fn time_label(ms: u32) -> String {
    let seconds = ms / 1_000;
    format!(
        "{:02}:{:02}:{:02} UTC",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

pub struct Imagery {
    planes: [Vec<u8>; PLANES],
    width: usize,
    line_ms: f64,
    lines: usize,
    previous_ms: Option<u32>,
    first_ms: Option<u32>,
}

impl Imagery {
    #[must_use]
    pub fn new(width: usize, line_ms: f64) -> Self {
        Self {
            planes: std::array::from_fn(|_| vec![0; MAX_WIDTH * MAX_LINES]),
            width: width.min(MAX_WIDTH),
            line_ms,
            lines: 0,
            previous_ms: None,
            first_ms: None,
        }
    }

    /// Empties the picture and takes the geometry of the next.
    pub fn reset(&mut self, width: usize, line_ms: f64) {
        for plane in &mut self.planes {
            plane[..self.lines * self.width].fill(0);
        }
        self.width = width.min(MAX_WIDTH);
        self.line_ms = line_ms;
        self.lines = 0;
        self.previous_ms = None;
        self.first_ms = None;
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines == 0
    }

    #[must_use]
    pub fn width(&self) -> u16 {
        self.width as u16
    }

    #[must_use]
    pub fn lines(&self) -> u16 {
        self.lines as u16
    }

    /// Onboard time of the first line whose stamp the next line bore out.
    #[must_use]
    pub fn first_line(&self) -> Option<String> {
        self.first_ms.map(time_label)
    }

    pub fn add(&mut self, scan: &Scan) -> Placement {
        if self.lines >= MAX_LINES {
            return Placement::Full;
        }
        self.confirm_time(scan.time_ms);
        let at = self.lines * self.width;
        for (plane, levels) in self.planes.iter_mut().zip(&scan.planes) {
            for (pixel, &level) in plane[at..at + self.width].iter_mut().zip(levels) {
                *pixel = (level >> 2) as u8;
            }
        }
        self.lines += 1;
        Placement::Placed
    }

    /// A stamp can carry bit errors, so the first line's time only counts once the next line's
    /// stamp follows it by about one line.
    fn confirm_time(&mut self, time_ms: Option<u32>) {
        if let (Some(previous), Some(now), None) = (self.previous_ms, time_ms, self.first_ms) {
            let step = f64::from(now.wrapping_sub(previous) % MS_PER_DAY);
            if (0.5 * self.line_ms..1.5 * self.line_ms).contains(&step) {
                self.first_ms = Some(previous);
            }
        }
        self.previous_ms = time_ms;
    }

    /// What `picture` draws for this pass: `Auto` turns to visible or infrared by daylight.
    #[must_use]
    pub fn resolve(&self, picture: HrptPicture) -> HrptPicture {
        match picture {
            HrptPicture::Auto if self.mean(RED) >= DAYLIGHT => HrptPicture::Visible,
            HrptPicture::Auto => HrptPicture::Infrared,
            chosen => chosen,
        }
    }

    fn filled(&self, plane: usize) -> &[u8] {
        &self.planes[plane][..self.lines * self.width]
    }

    fn mean(&self, plane: usize) -> u64 {
        let filled = self.filled(plane);
        let sum: u64 = filled.iter().map(|&value| u64::from(value)).sum();
        sum / (filled.len() as u64).max(1)
    }

    #[must_use]
    pub fn snapshot(&self, picture: HrptPicture) -> VideoPicture {
        let pixels = self.lines * self.width;
        let mut rgb = vec![0u8; pixels * 3];
        let mut luma = vec![0u8; pixels];
        if self.resolve(picture) == HrptPicture::Infrared {
            let stretch = Stretch::of(self.filled(THERMAL));
            for ((pixel, value), &raw) in rgb
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .zip(&mut luma)
                .zip(self.filled(THERMAL))
            {
                *value = 255 - stretch.apply(raw);
                *pixel = [*value; 3];
            }
        } else {
            let (red, blue) = (
                Stretch::of(self.filled(RED)),
                Stretch::of(self.filled(BLUE)),
            );
            for (((pixel, value), &r), &b) in rgb
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .zip(&mut luma)
                .zip(self.filled(RED))
                .zip(self.filled(BLUE))
            {
                let (r, b) = (red.apply(r), blue.apply(b));
                *pixel = [r, r, b];
                *value = ((u32::from(r) * 886 + u32::from(b) * 114) / 1000) as u8;
            }
        }
        VideoPicture {
            width: self.width(),
            height: self.lines(),
            luma,
            rgb,
        }
    }
}

/// A linear stretch of a channel's levels from its darkest to its brightest half percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stretch {
    low: u8,
    high: u8,
}

impl Stretch {
    fn of(values: &[u8]) -> Self {
        let mut histogram = [0u64; 256];
        for &value in values {
            histogram[usize::from(value)] += 1;
        }
        let clip = (values.len() as f64 * STRETCH_CLIP) as u64;
        Self {
            low: past_clip(&histogram, clip, 0..256),
            high: past_clip(&histogram, clip, (0..256).rev()),
        }
    }

    fn apply(self, value: u8) -> u8 {
        if self.high <= self.low {
            return value;
        }
        let span = u32::from(self.high - self.low);
        let above = u32::from(value.clamp(self.low, self.high) - self.low);
        (above * 255 / span) as u8
    }
}

/// The first level, walking `levels`, by which more than `clip` values have gone by.
fn past_clip(histogram: &[u64; 256], clip: u64, levels: impl Iterator<Item = usize>) -> u8 {
    let mut seen = 0;
    for level in levels {
        seen += histogram[level];
        if seen > clip {
            return level as u8;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE_MS: f64 = 150.0;

    fn scan(time_ms: Option<u32>, level: u16) -> Scan {
        let mut scan = Scan::new();
        scan.time_ms = time_ms;
        for plane in &mut scan.planes {
            plane.fill(level);
        }
        scan
    }

    #[test]
    fn the_first_time_waits_for_a_line_that_bears_it_out() {
        let mut imagery = Imagery::new(4, LINE_MS);
        let start = 5 * 3_600_000;
        for time in [
            Some(start + 99_000),
            None,
            Some(start + 150),
            Some(start + 300),
        ] {
            imagery.add(&scan(time, 100));
        }
        assert_eq!(imagery.lines(), 4);
        assert_eq!(imagery.first_line().as_deref(), Some("05:00:00 UTC"));
    }

    #[test]
    fn levels_keep_their_top_eight_bits() {
        let mut imagery = Imagery::new(4, LINE_MS);
        imagery.add(&scan(None, 1023));
        imagery.add(&scan(None, 4));
        assert_eq!(imagery.filled(THERMAL), [255, 255, 255, 255, 1, 1, 1, 1]);
    }

    #[test]
    fn a_stretch_spans_the_levels_in_use() {
        let values: Vec<u8> = (0..1000).map(|k| 100 + (k % 51) as u8).collect();
        let stretch = Stretch::of(&values);
        assert_eq!((stretch.low, stretch.high), (100, 150));
        assert_eq!(stretch.apply(100), 0);
        assert_eq!(stretch.apply(150), 255);
        assert_eq!(stretch.apply(200), 255);
    }
}
