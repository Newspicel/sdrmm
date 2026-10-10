const LOW_DB: f32 = -160.0;
const STEP_DB: f32 = 0.5;
const CELLS: usize = 400;
const QUIET: f64 = 0.05;
const Z_QUIET: f64 = -1.644_853_6;
const MIN_SEEN: u32 = 64;

pub struct QuietFloor {
    cells: Vec<u32>,
    seen: u32,
    offset_db: f32,
    rise_db: f32,
    floor_db: Option<f32>,
}

impl QuietFloor {
    #[must_use]
    pub fn new(degrees: f32, rise_db: f32) -> Self {
        Self {
            cells: vec![0; CELLS],
            seen: 0,
            offset_db: quantile_offset_db(f64::from(degrees)),
            rise_db,
            floor_db: None,
        }
    }

    pub fn observe(&mut self, db: f32) {
        if !db.is_finite() {
            return;
        }
        let cell = ((db - LOW_DB) / STEP_DB).clamp(0.0, (CELLS - 1) as f32) as usize;
        self.cells[cell] += 1;
        self.seen += 1;
    }

    pub fn settle(&mut self) -> Option<f32> {
        if self.seen >= MIN_SEEN {
            let quiet = self.quiet_db() + self.offset_db;
            self.floor_db = Some(match self.floor_db {
                Some(old) => quiet.min(old + self.rise_db),
                None => quiet,
            });
        }
        self.cells.fill(0);
        self.seen = 0;
        self.floor_db
    }

    #[must_use]
    pub fn floor_db(&self) -> Option<f32> {
        self.floor_db
    }

    pub fn reset(&mut self) {
        self.cells.fill(0);
        self.seen = 0;
        self.floor_db = None;
    }

    fn quiet_db(&self) -> f32 {
        let wanted = (f64::from(self.seen) * QUIET).ceil().max(1.0) as u32;
        let mut total = 0;
        let cell = self
            .cells
            .iter()
            .position(|&count| {
                total += count;
                total >= wanted
            })
            .unwrap_or(CELLS - 1);
        LOW_DB + (cell as f32 + 0.5) * STEP_DB
    }
}

fn quantile_offset_db(degrees: f64) -> f32 {
    let ratio = if degrees <= 2.0 {
        -(1.0 - QUIET).ln()
    } else {
        let k = 2.0 / (9.0 * degrees);
        (1.0 - k + Z_QUIET * k.sqrt()).max(1e-6).powi(3)
    };
    (-10.0 * ratio.log10()) as f32
}
