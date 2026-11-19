/// One image segment as a file delivers it: `lines` rows of `columns` eight-bit pixels starting
/// at `start_line` of a `width` by `height` image.
#[derive(Clone, Copy, Debug)]
pub struct ImageSegment<'a> {
    pub key: u64,
    pub width: usize,
    pub height: usize,
    pub start_line: usize,
    pub columns: usize,
    pub pixels: &'a [u8],
    pub expected: u16,
    pub lines_missing: u32,
}

impl ImageSegment<'_> {
    fn lines(&self) -> usize {
        self.pixels.len() / self.columns.max(1)
    }

    fn valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.columns > 0
            && self.columns <= self.width
            && self.start_line < self.height
    }
}

/// An image being drawn, scaled down so its longer side fits the mosaic's limit.
#[derive(Clone, Debug, Default)]
pub struct Canvas {
    pub key: u64,
    /// The file name of the first segment that came in.
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub luma: Vec<u8>,
    pub segments: u16,
    pub expected: u16,
    pub lines_missing: u32,
    scale: usize,
    used: u64,
    busy: bool,
}

impl Canvas {
    #[must_use]
    pub fn complete(&self) -> bool {
        self.segments >= self.expected
    }

    fn open(&mut self, segment: &ImageSegment<'_>, name: &str, max_side: usize) {
        let scale = segment.width.max(segment.height).div_ceil(max_side).max(1);
        self.key = segment.key;
        self.name.clear();
        self.name.push_str(name);
        self.scale = scale;
        self.width = segment.width.div_ceil(scale);
        self.height = segment.height.div_ceil(scale);
        self.luma.clear();
        self.luma.resize(self.width * self.height, 0);
        self.segments = 0;
        self.expected = segment.expected.max(1);
        self.lines_missing = 0;
        self.busy = true;
    }

    fn draw(&mut self, segment: &ImageSegment<'_>) {
        let scale = self.scale;
        for (index, row) in segment.pixels.chunks_exact(segment.columns).enumerate() {
            let line = segment.start_line + index;
            if !line.is_multiple_of(scale) || line / scale >= self.height {
                continue;
            }
            let out = &mut self.luma[line / scale * self.width..][..self.width];
            for (slot, cell) in out.iter_mut().zip(row.chunks(scale)) {
                let sum: usize = cell.iter().map(|&value| usize::from(value)).sum();
                *slot = (sum / cell.len()) as u8;
            }
        }
        self.segments = self.segments.saturating_add(1);
        self.lines_missing = self.lines_missing.saturating_add(segment.lines_missing);
    }
}

/// Draws the segments of several images at once, as a downlink interleaves them.
pub struct Mosaic {
    canvases: Vec<Canvas>,
    max_side: usize,
    clock: u64,
}

impl Mosaic {
    #[must_use]
    pub fn new(slots: usize, max_side: usize) -> Self {
        Self {
            canvases: (0..slots.max(1)).map(|_| Canvas::default()).collect(),
            max_side: max_side.max(1),
            clock: 0,
        }
    }

    #[must_use]
    pub fn canvas(&self, index: usize) -> &Canvas {
        &self.canvases[index]
    }

    /// The canvas that must be given up before `key` can be drawn, if every one is in use.
    #[must_use]
    pub fn victim_for(&self, key: u64) -> Option<usize> {
        let open = |canvas: &&Canvas| canvas.busy;
        if self
            .canvases
            .iter()
            .filter(open)
            .any(|canvas| canvas.key == key)
            || self.canvases.iter().any(|canvas| !canvas.busy)
        {
            return None;
        }
        self.canvases
            .iter()
            .enumerate()
            .min_by_key(|(_, canvas)| canvas.used)
            .map(|(index, _)| index)
    }

    pub fn release(&mut self, index: usize) {
        if let Some(canvas) = self.canvases.get_mut(index) {
            canvas.busy = false;
        }
    }

    /// Indices of the canvases still being drawn.
    pub fn open(&self) -> impl Iterator<Item = usize> + '_ {
        self.canvases
            .iter()
            .enumerate()
            .filter(|(_, canvas)| canvas.busy)
            .map(|(index, _)| index)
    }

    /// Draws `segment`; returns the canvas it went to, or `None` when its geometry is unusable
    /// or every canvas is taken (see [`Self::victim_for`]).
    pub fn place(&mut self, segment: &ImageSegment<'_>, name: &str) -> Option<usize> {
        if !segment.valid() || segment.lines() == 0 {
            return None;
        }
        self.clock += 1;
        let found = self
            .canvases
            .iter()
            .position(|canvas| canvas.busy && canvas.key == segment.key);
        let index = if let Some(index) = found {
            index
        } else {
            let index = self.canvases.iter().position(|canvas| !canvas.busy)?;
            self.canvases[index].open(segment, name, self.max_side);
            index
        };
        let canvas = &mut self.canvases[index];
        canvas.used = self.clock;
        canvas.draw(segment);
        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(key: u64, start_line: usize, lines: usize, pixels: &[u8]) -> ImageSegment<'_> {
        ImageSegment {
            key,
            width: 40,
            height: 30,
            start_line,
            columns: 40,
            pixels: &pixels[..lines * 40],
            expected: 3,
            lines_missing: 0,
        }
    }

    #[test]
    fn segments_fill_a_scaled_canvas() {
        let pixels: Vec<u8> = (0..400).map(|index| (index % 40 * 5) as u8).collect();
        let mut mosaic = Mosaic::new(2, 20);
        for start in [0, 20, 10] {
            let index = mosaic
                .place(&segment(7, start, 10, &pixels), "a")
                .expect("placed");
            assert_eq!(mosaic.canvas(index).name, "a");
        }
        let canvas = mosaic.canvas(0);
        assert!(canvas.complete());
        assert_eq!((canvas.width, canvas.height), (20, 15));
        assert_eq!(&canvas.luma[..3], &[2, 12, 22]);
        assert_eq!(canvas.luma[14 * 20 + 19], 192);
    }

    #[test]
    fn a_full_mosaic_names_its_oldest_canvas() {
        let pixels = vec![9u8; 400];
        let mut mosaic = Mosaic::new(2, 100);
        mosaic.place(&segment(1, 0, 10, &pixels), "one");
        mosaic.place(&segment(2, 0, 10, &pixels), "two");
        mosaic.place(&segment(1, 10, 10, &pixels), "one");
        assert_eq!(mosaic.victim_for(2), None);
        let victim = mosaic.victim_for(3).expect("full");
        assert_eq!(mosaic.canvas(victim).key, 2);
        assert!(mosaic.place(&segment(3, 0, 10, &pixels), "three").is_none());
        mosaic.release(victim);
        assert!(mosaic.place(&segment(3, 0, 10, &pixels), "three").is_some());
        assert_eq!(mosaic.open().count(), 2);
    }
}
