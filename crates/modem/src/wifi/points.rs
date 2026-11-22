use num_complex::Complex;

use super::Sink;
use crate::constellation::{Constellation, ConstellationError, tables};

/// The grid a frame's decision points lie on, at the scale of its table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grid {
    /// DBPSK Barker symbols and OFDM BPSK carriers, on the real axis.
    Bpsk,
    /// DQPSK Barker symbols and CCK codeword phases, on the axes.
    Quadrants,
    /// OFDM QPSK carriers, on the diagonals.
    Qpsk,
    Qam16,
    Qam64,
}

impl Grid {
    pub const ALL: [Self; 5] = [
        Self::Bpsk,
        Self::Quadrants,
        Self::Qpsk,
        Self::Qam16,
        Self::Qam64,
    ];

    pub fn table(self) -> Result<Constellation, ConstellationError> {
        match self {
            Self::Bpsk => Ok(tables::bpsk()),
            Self::Quadrants => tables::psk(4),
            Self::Qpsk => tables::psk_rotated(4, tables::PI_4_ROTATION),
            Self::Qam16 => tables::qam_square(16),
            Self::Qam64 => tables::qam_square(64),
        }
    }
}

/// Every [`Grid`]'s table, built once so a sink hands one out without allocating.
pub struct Grids([Constellation; Grid::ALL.len()]);

impl Grids {
    pub fn new() -> Result<Self, ConstellationError> {
        Ok(Self([
            Grid::Bpsk.table()?,
            Grid::Quadrants.table()?,
            Grid::Qpsk.table()?,
            Grid::Qam16.table()?,
            Grid::Qam64.table()?,
        ]))
    }

    #[must_use]
    pub fn table(&self, grid: Grid) -> &Constellation {
        &self.0[grid as usize]
    }
}

/// The decision points a frame was read from, carrier and channel taken off.
#[derive(Clone, Copy, Debug)]
pub struct Points<'a> {
    pub symbols: &'a [Complex<f32>],
    pub grid: Grid,
    pub rate_hz: f64,
    pub offset_hz: f64,
    /// The points come from many subcarriers at once, so they fold into no eye.
    pub multicarrier: bool,
}

pub(crate) struct Cloud {
    points: Vec<Complex<f32>>,
    multicarrier: bool,
    grid: Grid,
    rate_hz: f64,
    offset_hz: f64,
}

impl Cloud {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            points: Vec::with_capacity(capacity),
            multicarrier: false,
            grid: Grid::Bpsk,
            rate_hz: 0.0,
            offset_hz: 0.0,
        }
    }

    pub(crate) fn across_carriers(capacity: usize) -> Self {
        Self {
            multicarrier: true,
            ..Self::with_capacity(capacity)
        }
    }

    pub(crate) fn start(&mut self, grid: Grid, rate_hz: f64) {
        self.points.clear();
        self.grid = grid;
        self.rate_hz = rate_hz;
        self.offset_hz = 0.0;
    }

    pub(crate) fn push(&mut self, point: Complex<f32>) {
        self.points.push(point);
    }

    pub(crate) fn extend(&mut self, points: impl Iterator<Item = Complex<f32>>) {
        self.points.extend(points);
    }

    pub(crate) fn heard_at(&mut self, offset_hz: f64) {
        self.offset_hz = offset_hz;
    }

    /// Scales the points to a mean magnitude of one, for a detector that reads them at the
    /// carrier's own level.
    pub(crate) fn unit_mean(&mut self) {
        let total: f32 = self.points.iter().map(|point| point.norm()).sum();
        let mean = total / self.points.len().max(1) as f32;
        if mean > 0.0 {
            self.points.iter_mut().for_each(|point| *point /= mean);
        }
    }

    pub(crate) fn hand(&self, sink: &mut impl Sink) {
        sink.points(Points {
            symbols: &self.points,
            grid: self.grid,
            rate_hz: self.rate_hz,
            offset_hz: self.offset_hz,
            multicarrier: self.multicarrier,
        });
    }
}
