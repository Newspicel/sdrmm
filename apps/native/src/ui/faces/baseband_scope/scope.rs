use num_complex::Complex;
use sdrmm_dsp::spectrum::SpectrumAnalyzer;
use sdrmm_wire::frame::SymbolPlane;

use super::{
    frames::{Block, Burst},
    grid::{
        BASEBAND_DECAY, Eye, Grid, Trend, add_constellation, add_eye, eye_scale, iq_scale,
        samples_per_symbol, symbol_histogram, symbol_phase,
    },
    measure::{View, discriminator, tick_label},
};
use crate::ui::{
    faces::scope::{
        palette::Palette,
        plot::{TracePoints, columns_of, level_y, trace_points},
        traces::{DbWindow, trace_unit},
        view::{FULL_VIEW, decibel_ticks, frequency_ticks},
    },
    kit_raster::{PLOT_BG, PLOT_GRID, PLOT_HOLD, PLOT_INK, PLOT_TRACE, Pen, Raster, Rgba, Scene},
};

pub const GRID: usize = 320;
pub const FFT_SIZE: usize = 2048;
pub const SPECTRUM_RANGE_DB: f32 = 90.0;
pub const TREND_POINTS: usize = 240;
pub const AXIS_H: f32 = 16.0;
pub const CHAR_PX: f32 = 6.0;
const MINOR_TICKS: usize = 5;
const PAD_LEFT: f32 = 40.0;
const PAD_RIGHT: f32 = 8.0;
const PAD_TOP: f32 = 6.0;
const PAD_BOTTOM: f32 = 18.0;
const INSET_TOP: f32 = 10.0;
const INSET_BOTTOM: f32 = 4.0;
const SCATTER_PAD: f32 = 12.0;
const TICK_SPACING_PX: f32 = 110.0;
const EDGE_PX: f32 = 2.0;
const LEGEND_GAP_PX: f32 = 26.0;
const BAND_FILL: f32 = 0.3;
const TRACE_FILL: f32 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub view: View,
    pub eye: Eye,
    pub symbol_rate: f32,
    pub decimate: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            view: View::Spectrum,
            eye: Eye::Frequency,
            symbol_rate: 4800.0,
            decimate: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl PlotBox {
    fn device(self, scale: f32) -> (f32, f32, f32, f32) {
        let (x0, y0) = ((self.x * scale).round(), (self.y * scale).round());
        (
            x0,
            y0,
            ((self.x + self.w) * scale).round() - x0,
            ((self.y + self.h) * scale).round() - y0,
        )
    }

    fn usable(self) -> bool {
        self.w > 0.0 && self.h > 0.0
    }
}

#[must_use]
pub fn spectrum_box(width: f32, height: f32) -> PlotBox {
    PlotBox {
        x: 0.0,
        y: 0.0,
        w: width,
        h: (height - AXIS_H).max(1.0),
    }
}

#[must_use]
pub fn scatter_box(constellation: bool, width: f32, height: f32) -> PlotBox {
    let (w, h) = (width - 2.0 * SCATTER_PAD, height - 2.0 * SCATTER_PAD);
    if !constellation {
        return PlotBox {
            x: SCATTER_PAD,
            y: SCATTER_PAD,
            w,
            h,
        };
    }
    let side = w.min(h);
    PlotBox {
        x: (width - side) / 2.0,
        y: (height - side) / 2.0,
        w: side,
        h: side,
    }
}

#[must_use]
pub fn histogram_box(width: f32, height: f32) -> PlotBox {
    PlotBox {
        x: PAD_RIGHT,
        y: PAD_TOP + INSET_TOP,
        w: width - 2.0 * PAD_RIGHT,
        h: height - PAD_TOP - PAD_BOTTOM - INSET_TOP - INSET_BOTTOM,
    }
}

#[must_use]
pub fn trend_box(width: f32, height: f32) -> PlotBox {
    PlotBox {
        x: PAD_LEFT,
        y: PAD_TOP + INSET_TOP,
        w: width - PAD_LEFT - PAD_RIGHT,
        h: height - PAD_TOP - PAD_BOTTOM - INSET_TOP - INSET_BOTTOM,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Left,
    Centre,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub anchor: Anchor,
    pub backed: bool,
}

fn plot_label(x: f32, baseline: f32, text: impl Into<String>, anchor: Anchor) -> Label {
    Label {
        x,
        y: baseline - 3.0,
        text: text.into(),
        anchor,
        backed: true,
    }
}

fn axis_label(x: f32, middle: f32, text: impl Into<String>, anchor: Anchor) -> Label {
    Label {
        x,
        y: middle,
        text: text.into(),
        anchor,
        backed: false,
    }
}

#[must_use]
pub fn text_width(text: &str) -> f32 {
    text.chars().count() as f32 * CHAR_PX
}

pub struct Trends {
    pub mer: Trend,
    pub margin: Trend,
    pub drift: Trend,
}

#[derive(Default)]
struct Scratch {
    points: TracePoints,
    high: Vec<f32>,
    low: Vec<f32>,
    line: Vec<f32>,
}

pub struct Scope {
    pub settings: Settings,
    pub burst: Option<Burst>,
    pub block: Option<Block>,
    pub trends: Trends,
    grid: Grid,
    scale: f32,
    analyzer: SpectrumAnalyzer,
    input: Vec<Complex<f32>>,
    db: Vec<f32>,
    top: f32,
    lut: Vec<Rgba>,
    scratch: Scratch,
    stamp: u64,
}

impl Scope {
    #[must_use]
    pub fn new(palette: Palette) -> Self {
        Self {
            settings: Settings::default(),
            burst: None,
            block: None,
            trends: Trends {
                mer: Trend::new(TREND_POINTS),
                margin: Trend::new(TREND_POINTS),
                drift: Trend::new(TREND_POINTS),
            },
            grid: Grid::new(GRID, GRID),
            scale: 1.0,
            analyzer: SpectrumAnalyzer::new(FFT_SIZE),
            input: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            db: vec![0.0; FFT_SIZE],
            top: 0.0,
            lut: lut(palette),
            scratch: Scratch::default(),
            stamp: 0,
        }
    }

    #[must_use]
    pub fn shown(&self) -> View {
        self.settings.view.shown(self.block.is_some())
    }

    pub fn configure(&mut self, settings: Settings) {
        self.settings = settings;
        self.grid.clear();
        if self.shown() == View::Spectrum {
            self.analyse();
        }
        self.stamp += 1;
    }

    pub fn take_burst(&mut self, burst: Burst) {
        let settings = self.settings;
        let period = samples_per_symbol(burst.sample_rate, settings.symbol_rate);
        match settings.view {
            View::Constellation => {
                self.grid.decay(BASEBAND_DECAY);
                if self.block.is_none() {
                    let step = if settings.decimate {
                        period.round()
                    } else {
                        1.0
                    };
                    let offset = if settings.decimate {
                        symbol_phase(&burst.samples, period)
                    } else {
                        0
                    };
                    self.scale = iq_scale(&burst.samples);
                    add_constellation(&mut self.grid, &burst.samples, self.scale, step, offset);
                }
            }
            View::Eye => {
                self.grid.decay(BASEBAND_DECAY);
                let scale = eye_scale(&burst.samples, settings.eye);
                add_eye(&mut self.grid, &burst.samples, period, settings.eye, scale);
            }
            _ => {}
        }
        self.burst = Some(burst);
        if self.shown() == View::Spectrum {
            self.analyse();
        }
        self.stamp += 1;
    }

    pub fn take_block(&mut self, block: Block) {
        self.trends.mer.push(block.mer_db);
        self.trends.margin.push(block.margin);
        self.trends.drift.push(block.freq_error_hz);
        if self.settings.view == View::Constellation {
            self.grid.decay(BASEBAND_DECAY);
            self.scale = block.reference_scale();
            add_constellation(&mut self.grid, &block.paired(), self.scale, 1.0, 0);
        }
        self.block = Some(block);
        self.stamp += 1;
    }

    #[must_use]
    pub fn labels(&self, width: f32, height: f32) -> Vec<Label> {
        match self.shown() {
            View::Spectrum => self.burst.as_ref().map_or_else(Vec::new, |burst| {
                spectrum_labels(burst.sample_rate, self.window(), width, height)
            }),
            View::Constellation => {
                constellation_labels(scatter_box(true, width, height), self.scale)
            }
            View::Eye => eye_labels(scatter_box(false, width, height), self.settings.eye),
            View::Levels => self.levels_labels(histogram_box(width, height)),
            View::Quality => trend_labels(
                trend_box(width, height),
                &[&self.trends.mer, &self.trends.margin],
                &["MER dB", "margin"],
                "per block",
                false,
            ),
            View::Drift => trend_labels(
                trend_box(width, height),
                &[&self.trends.drift],
                &["carrier"],
                "Hz",
                true,
            ),
            View::States => Vec::new(),
        }
    }

    fn window(&self) -> DbWindow {
        DbWindow {
            min: f64::from(self.top - SPECTRUM_RANGE_DB),
            max: f64::from(self.top),
        }
    }

    fn levels_labels(&self, plot: PlotBox) -> Vec<Label> {
        let mut labels = vec![
            plot_label(plot.x + 4.0, plot.y + 11.0, "share", Anchor::Left),
            plot_label(plot.x + plot.w - 4.0, plot.y + 11.0, "level", Anchor::Right),
        ];
        if let Some(block) = &self.block {
            let scale = block.reference_scale();
            for level in reference_levels(block) {
                let x = (plot.x + histogram_x(level, scale) * plot.w).round() + 0.5;
                if (plot.x..=plot.x + plot.w).contains(&x) {
                    labels.push(plot_label(
                        x,
                        plot.y + plot.h + 11.0,
                        level_text(level),
                        Anchor::Centre,
                    ));
                }
            }
        }
        labels
    }

    fn analyse(&mut self) {
        let Some(burst) = &self.burst else {
            return;
        };
        let pairs = burst
            .samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[re, im]| Complex::new(*re, *im))
            .chain(std::iter::repeat(Complex::new(0.0, 0.0)));
        for (slot, pair) in self.input.iter_mut().zip(pairs) {
            *slot = pair;
        }
        self.analyzer.power_db(&self.input, &mut self.db);
        self.top = spectrum_top(&self.db);
    }

    fn paint_spectrum(&mut self, raster: &mut Raster) {
        let Some(burst) = &self.burst else {
            return;
        };
        let scale = raster.scale.max(0.01);
        let (width, height) = (raster.width as f32 / scale, raster.height as f32 / scale);
        let plot = spectrum_box(width, height);
        let window = self.window();
        let grid = Pen::new(PLOT_GRID, 1.0, 1.0);
        let foot = plot.h * scale;
        for db in decibel_ticks(window.min, window.max, 4.0) {
            let y = grid_y(db, plot.h, window) * scale;
            raster.line((0.0, y), (raster.width as f32, y), grid);
        }
        for at in spectrum_ticks(burst.sample_rate, width)
            .into_iter()
            .map(|(at, _)| at)
        {
            let x = ((at * width).round() + 0.5) * scale;
            raster.line((x, 0.0), (x, foot), grid);
        }
        let centre = (width / 2.0).round() + 0.5;
        let dash = Pen::new(PLOT_INK, 0.7, 1.0).dashed(2.0 * scale, 4.0 * scale);
        raster.line((centre * scale, 0.0), (centre * scale, foot), dash);
        self.paint_trace(raster, f64::from(foot), window);
    }

    fn paint_trace(&mut self, raster: &mut Raster, foot: f64, window: DbWindow) {
        let columns = raster.width as usize;
        let scratch = &mut self.scratch;
        trace_points(
            &self.db,
            FULL_VIEW,
            columns as f64,
            f64::NEG_INFINITY,
            &mut scratch.points,
        );
        columns_of(
            &scratch.points,
            columns,
            [&mut scratch.high, &mut scratch.low, &mut scratch.line],
        );
        let pen = Pen::new(PLOT_TRACE, 1.0, 1.25 * raster.scale);
        let mut last = None;
        for column in 0..columns {
            let x = column as f32;
            let high = level_y(scratch.high[column], foot, window);
            let low = level_y(scratch.low[column], foot, window);
            let line = level_y(scratch.line[column], foot, window);
            raster.rect(x, high, x + 1.0, low.max(high + 1.0), PLOT_TRACE, BAND_FILL);
            raster.rect(x, line, x + 1.0, foot as f32, PLOT_TRACE, TRACE_FILL);
            if let Some(previous) = last {
                raster.line(previous, (x + 0.5, line), pen);
            }
            last = Some((x + 0.5, line));
        }
    }

    fn paint_scatter(&self, raster: &mut Raster) {
        let constellation = self.shown() == View::Constellation;
        let scale = raster.scale.max(0.01);
        let plot = scatter_box(
            constellation,
            raster.width as f32 / scale,
            raster.height as f32 / scale,
        );
        if !plot.usable() {
            return;
        }
        let (x0, y0, w, h) = plot.device(scale);
        graticule(raster, (x0, y0, w, h), 8, if constellation { 8 } else { 6 });
        if constellation {
            let pen = Pen::new(PLOT_INK, 0.35, 1.0).dashed(2.0 * scale, 4.0 * scale);
            raster.circle((x0 + w / 2.0, y0 + h / 2.0), w / 2.0, pen);
        }
        self.recolour(raster, (x0, y0, w, h));
    }

    fn recolour(&self, raster: &mut Raster, (x0, y0, w, h): (f32, f32, f32, f32)) {
        if w < 1.0 || h < 1.0 {
            return;
        }
        for py in 0..h as usize {
            let gy = (py as f32 + 0.5) * self.grid.height as f32 / h - 0.5;
            for px in 0..w as usize {
                let gx = (px as f32 + 0.5) * self.grid.width as f32 / w - 0.5;
                let value = smoothed(&self.grid, gx, gy);
                if value <= 0.0 {
                    continue;
                }
                let colour = self.lut[((value * 255.0).round() as usize).min(255)];
                let alpha = (40.0 + value * 215.0).min(255.0) / 255.0;
                raster.blend(x0 as i64 + px as i64, y0 as i64 + py as i64, colour, alpha);
            }
        }
    }

    fn paint_levels(&self, raster: &mut Raster) {
        let (bins, reference, scale) = match (&self.block, &self.burst) {
            (Some(block), _) => {
                let scale = block.reference_scale();
                let stride = if block.plane == SymbolPlane::Complex {
                    2
                } else {
                    1
                };
                (
                    symbol_histogram(&block.symbols, stride, scale),
                    reference_levels(block),
                    scale,
                )
            }
            (None, Some(burst)) => {
                let period = samples_per_symbol(burst.sample_rate, self.settings.symbol_rate);
                let rail =
                    discriminator(&burst.samples, period, symbol_phase(&burst.samples, period));
                (symbol_histogram(&rail, 1, 1.0), Vec::new(), 1.0)
            }
            (None, None) => return,
        };
        let pixel = raster.scale.max(0.01);
        let plot = histogram_box(raster.width as f32 / pixel, raster.height as f32 / pixel);
        if !plot.usable() || bins.is_empty() {
            return;
        }
        let (x0, y0, w, h) = plot.device(pixel);
        graticule(raster, (x0, y0, w, h), 8, 4);
        let step = w / bins.len() as f32;
        let foot = y0 + h;
        for (index, value) in bins.iter().enumerate() {
            if *value > 0.0 {
                let left = x0 + index as f32 * step;
                raster.rect(
                    left,
                    foot - value * h * 0.92,
                    left + (step - 0.5).max(1.0),
                    foot,
                    PLOT_TRACE,
                    0.85,
                );
            }
        }
        let dash = Pen::new(PLOT_HOLD, 0.7, 1.0).dashed(3.0 * pixel, 3.0 * pixel);
        for level in reference {
            let x = x0 + histogram_x(level, scale) * w;
            if (x0..=x0 + w).contains(&x) {
                raster.line((x, y0), (x, foot), dash);
            }
        }
    }

    fn paint_trend(&self, raster: &mut Raster, series: &[(&Trend, Rgba, &str)], zero: bool) {
        let scale = raster.scale.max(0.01);
        let plot = trend_box(raster.width as f32 / scale, raster.height as f32 / scale);
        if !plot.usable() {
            return;
        }
        let (x0, y0, w, h) = plot.device(scale);
        graticule(raster, (x0, y0, w, h), 10, 4);
        let trends: Vec<&Trend> = series.iter().map(|(trend, _, _)| *trend).collect();
        let Some((min, max, longest)) = trend_span(&trends, zero) else {
            return;
        };
        let pen_width = 1.5 * scale;
        for (trend, colour, _) in series {
            if trend.len() < 2 {
                continue;
            }
            let mut last = None;
            for index in 0..trend.len() {
                let x = x0 + index as f32 / (longest - 1).max(1) as f32 * w;
                let y = y0 + (1.0 - (trend.sample(index) - min) / (max - min)) * h;
                if let Some(previous) = last {
                    raster.line(previous, (x, y), Pen::new(*colour, 1.0, pen_width));
                }
                last = Some((x, y));
            }
        }
        let names: Vec<&str> = series.iter().map(|(_, _, name)| *name).collect();
        for ((_, colour, _), at) in series.iter().zip(legend_offsets(plot, &names)) {
            let (left, top) = (at * scale, (plot.y + 7.0) * scale);
            raster.rect(
                left,
                top,
                left + 8.0 * scale,
                top + 2.0 * scale,
                *colour,
                1.0,
            );
        }
    }
}

impl Scene for Scope {
    fn stamp(&self) -> u64 {
        self.stamp
    }

    fn paint(&mut self, raster: &mut Raster) {
        raster.fill(PLOT_BG);
        match self.shown() {
            View::Spectrum => self.paint_spectrum(raster),
            View::Constellation | View::Eye => self.paint_scatter(raster),
            View::Levels => self.paint_levels(raster),
            View::Quality => self.paint_trend(
                raster,
                &[
                    (&self.trends.mer, PLOT_TRACE, "MER dB"),
                    (&self.trends.margin, PLOT_HOLD, "margin"),
                ],
                false,
            ),
            View::Drift => {
                self.paint_trend(raster, &[(&self.trends.drift, PLOT_TRACE, "carrier")], true);
            }
            View::States => {}
        }
    }
}

#[must_use]
pub fn smoothed(grid: &Grid, x: f32, y: f32) -> f32 {
    let cell = |gx: i64, gy: i64| {
        let gx = gx.clamp(0, grid.width as i64 - 1) as usize;
        let gy = gy.clamp(0, grid.height as i64 - 1) as usize;
        grid.cells[gy * grid.width + gx]
    };
    let (left, top) = (x.floor(), y.floor());
    let (fx, fy) = (x - left, y - top);
    let (left, top) = (left as i64, top as i64);
    let upper = cell(left, top) * (1.0 - fx) + cell(left + 1, top) * fx;
    let lower = cell(left, top + 1) * (1.0 - fx) + cell(left + 1, top + 1) * fx;
    upper * (1.0 - fy) + lower * fy
}

fn grid_y(db: f64, plot_h: f32, window: DbWindow) -> f32 {
    (f64::from(plot_h) * (1.0 - trace_unit(db, window))).round() as f32 + 0.5
}

fn lut(palette: Palette) -> Vec<Rgba> {
    (0..256)
        .map(|step| {
            let [r, g, b] = palette.sample(f64::from(step as f32 / 255.0));
            [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255]
        })
        .collect()
}

fn graticule(
    raster: &mut Raster,
    (x0, y0, w, h): (f32, f32, f32, f32),
    columns: usize,
    rows: usize,
) {
    let grid = Pen::new(PLOT_GRID, 0.55, 1.0);
    for column in 1..columns {
        let x = (x0 + w * column as f32 / columns as f32).round();
        raster.line((x, y0), (x, y0 + h), grid);
    }
    for row in 1..rows {
        let y = (y0 + h * row as f32 / rows as f32).round();
        raster.line((x0, y), (x0 + w, y), grid);
    }
    let edge = Pen::new(PLOT_GRID, 1.0, 1.0);
    raster.line((x0, y0), (x0 + w, y0), edge);
    raster.line((x0, y0 + h), (x0 + w, y0 + h), edge);
    raster.line((x0, y0), (x0, y0 + h), edge);
    raster.line((x0 + w, y0), (x0 + w, y0 + h), edge);
    let (cx, cy) = (x0 + w / 2.0, y0 + h / 2.0);
    let tick = Pen::new(PLOT_INK, 0.45, 1.0);
    let scale = raster.scale;
    let reach = |index: usize| if index.is_multiple_of(MINOR_TICKS) { 4.0 } else { 2.0 } * scale;
    for index in 1..columns * MINOR_TICKS {
        let x = (x0 + w * index as f32 / (columns * MINOR_TICKS) as f32).round();
        let size = reach(index);
        raster.line((x, cy - size), (x, cy + size), tick);
    }
    for index in 1..rows * MINOR_TICKS {
        let y = (y0 + h * index as f32 / (rows * MINOR_TICKS) as f32).round();
        let size = reach(index);
        raster.line((cx - size, y), (cx + size, y), tick);
    }
}

#[must_use]
pub fn spectrum_top(db: &[f32]) -> f32 {
    let peak = db.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if peak.is_finite() {
        (peak / 10.0).ceil() * 10.0
    } else {
        0.0
    }
}

#[must_use]
pub fn spectrum_ticks(sample_rate: f32, width: f32) -> Vec<(f32, String)> {
    let target = (width / TICK_SPACING_PX).floor().max(2.0);
    frequency_ticks(0.0, f64::from(sample_rate), FULL_VIEW, f64::from(target))
        .into_iter()
        .map(|tick| (tick.at as f32, format_offset(tick.hz)))
        .collect()
}

#[must_use]
pub fn format_offset(hz: f64) -> String {
    let khz = hz / 1e3;
    let shown = if khz.abs() < 1e-9 { 0.0 } else { khz };
    let decimals = if shown == 0.0 || shown.abs() >= 10.0 {
        0
    } else {
        1
    };
    let sign = if shown > 0.0 { "+" } else { "" };
    format!("{sign}{shown:.decimals$}")
}

#[must_use]
pub fn fits(x: f32, text: &str, width: f32) -> bool {
    let half = text_width(text) / 2.0;
    x - half >= EDGE_PX && x + half <= width - EDGE_PX
}

#[must_use]
pub fn spectrum_labels(sample_rate: f32, window: DbWindow, width: f32, height: f32) -> Vec<Label> {
    let plot = spectrum_box(width, height);
    let mut labels = vec![
        axis_label(4.0, 8.0, "dBFS", Anchor::Left),
        axis_label(width - 4.0, 8.0, "kHz", Anchor::Right),
    ];
    for (at, text) in spectrum_ticks(sample_rate, width) {
        let x = (at * width).round() + 0.5;
        if fits(x, &text, width) {
            labels.push(axis_label(x, height - AXIS_H / 2.0, text, Anchor::Centre));
        }
    }
    for db in decibel_ticks(window.min, window.max, 4.0) {
        let y = grid_y(db, plot.h, window);
        if y > 24.0 && y < plot.h - 4.0 {
            labels.push(Label {
                backed: true,
                ..axis_label(4.0, y - 7.0, format!("{db:.0}"), Anchor::Left)
            });
        }
    }
    labels
}

fn constellation_labels(plot: PlotBox, scale: f32) -> Vec<Label> {
    let half = scale / 2.0;
    let bottom = plot.y + plot.h - 5.0;
    vec![
        plot_label(
            plot.x + plot.w - 5.0,
            plot.y + plot.h / 2.0 - 6.0,
            "I",
            Anchor::Right,
        ),
        plot_label(
            plot.x + plot.w / 2.0 + 6.0,
            plot.y + 12.0,
            "Q",
            Anchor::Left,
        ),
        plot_label(
            plot.x + plot.w / 4.0,
            bottom,
            tick_label(-half),
            Anchor::Centre,
        ),
        plot_label(
            plot.x + plot.w * 0.75,
            bottom,
            tick_label(half),
            Anchor::Centre,
        ),
        plot_label(
            plot.x + 5.0,
            plot.y + plot.h / 4.0 + 4.0,
            tick_label(half),
            Anchor::Left,
        ),
        plot_label(
            plot.x + 5.0,
            plot.y + plot.h * 0.75 + 4.0,
            tick_label(-half),
            Anchor::Left,
        ),
    ]
}

fn eye_labels(plot: PlotBox, eye: Eye) -> Vec<Label> {
    vec![
        plot_label(plot.x + 5.0, plot.y + 12.0, eye.label(), Anchor::Left),
        plot_label(
            plot.x + plot.w - 5.0,
            plot.y + plot.h - 5.0,
            "2 symbols",
            Anchor::Right,
        ),
    ]
}

#[must_use]
pub fn reference_levels(block: &Block) -> Vec<f32> {
    match block.plane {
        SymbolPlane::Level => block.reference.clone(),
        SymbolPlane::Complex => block.reference.iter().step_by(2).copied().collect(),
    }
}

#[must_use]
pub fn histogram_x(level: f32, scale: f32) -> f32 {
    let span = if scale > 0.0 { scale } else { 1.0 };
    (level / span + 1.0) / 2.0
}

#[must_use]
pub fn level_text(level: f32) -> String {
    if level.fract() == 0.0 {
        format!("{level:.0}")
    } else {
        format!("{level:.2}")
    }
}

#[must_use]
pub fn trend_span(trends: &[&Trend], zero: bool) -> Option<(f32, f32, usize)> {
    let filled: Vec<&&Trend> = trends.iter().filter(|trend| trend.len() > 0).collect();
    let longest = filled.iter().map(|trend| trend.len()).max()?;
    let (mut min, mut max) = filled.iter().map(|trend| trend.range()).fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(min, max), (low, high)| (min.min(low), max.max(high)),
    );
    if zero {
        let reach = min.abs().max(max.abs()).max(1.0);
        (min, max) = (-reach, reach);
    }
    let pad = ((max - min) * 0.1).max(0.5);
    Some((min - pad, max + pad, longest))
}

#[must_use]
pub fn legend_offsets(plot: PlotBox, names: &[&str]) -> Vec<f32> {
    let mut at = plot.x + 6.0;
    names
        .iter()
        .map(|name| {
            let here = at;
            at += text_width(name) + LEGEND_GAP_PX;
            here
        })
        .collect()
}

fn trend_labels(
    plot: PlotBox,
    trends: &[&Trend],
    names: &[&str],
    unit: &str,
    zero: bool,
) -> Vec<Label> {
    let mut labels = vec![
        plot_label(plot.x + plot.w - 6.0, plot.y + 11.0, unit, Anchor::Right),
        plot_label(plot.x, plot.y + plot.h + 12.0, "older", Anchor::Left),
        plot_label(
            plot.x + plot.w,
            plot.y + plot.h + 12.0,
            "now",
            Anchor::Right,
        ),
    ];
    for (name, at) in names.iter().zip(legend_offsets(plot, names)) {
        labels.push(plot_label(at + 12.0, plot.y + 11.0, *name, Anchor::Left));
    }
    let Some((min, max, _)) = trend_span(trends, zero) else {
        return labels;
    };
    let decimals = if (max - min).abs() < 10.0 { 1 } else { 0 };
    for step in 0..=4 {
        let value = min + (max - min) * step as f32 / 4.0;
        let y = plot.y + (1.0 - (value - min) / (max - min)) * plot.h;
        labels.push(plot_label(
            plot.x - 5.0,
            y + 3.0,
            format!("{value:.decimals$}"),
            Anchor::Right,
        ));
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::super::frames::tests::block;
    use super::*;
    use crate::ui::faces::scope::colormap::Colormap;

    fn burst(samples: Vec<f32>) -> Burst {
        Burst {
            center_hz: 100e6,
            sample_rate: 48_000.0,
            samples,
        }
    }

    fn scope() -> Scope {
        Scope::new(Palette::Map(Colormap::Viridis))
    }

    #[test]
    fn a_burst_in_constellation_view_lights_the_grid_and_asks_for_a_repaint() {
        let mut scope = scope();
        scope.configure(Settings {
            view: View::Constellation,
            decimate: false,
            ..Settings::default()
        });
        let before = scope.stamp();
        scope.take_burst(burst(vec![1.0, 0.0, -1.0, 0.0]));
        assert!(scope.stamp() > before);
        assert!(scope.grid.cells.iter().any(|cell| *cell > 0.0));
        scope.configure(scope.settings);
        assert!(scope.grid.cells.iter().all(|cell| *cell == 0.0));
    }

    #[test]
    fn a_block_feeds_the_trends_and_unlocks_the_symbol_views() {
        let mut scope = scope();
        scope.configure(Settings {
            view: View::Quality,
            ..Settings::default()
        });
        assert_eq!(scope.shown(), View::Spectrum);
        scope.take_block(block());
        assert_eq!(scope.shown(), View::Quality);
        assert_eq!(scope.trends.mer.len(), 1);
        assert_eq!(scope.trends.drift.sample(0), -12.0);
    }

    #[test]
    fn every_view_paints_the_whole_plot() {
        let mut scope = scope();
        scope.take_burst(burst((0..256).map(|i| (i as f32 * 0.3).sin()).collect()));
        let mut raster = Raster::sized(200, 100);
        scope.paint(&mut raster);
        assert_eq!((raster.width, raster.height), (200, 100));
        assert!(
            raster
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| *pixel != PLOT_BG)
        );
        scope.configure(Settings {
            view: View::Constellation,
            ..Settings::default()
        });
        let mut square = Raster::sized(200, 100);
        scope.paint(&mut square);
        assert_eq!((square.width, square.height), (200, 100));
    }

    #[test]
    fn the_constellation_sits_in_a_centred_square() {
        let plot = scatter_box(true, 200.0, 100.0);
        assert_eq!((plot.w, plot.h), (76.0, 76.0));
        assert_eq!((plot.x, plot.y), (62.0, 12.0));
        let eye = scatter_box(false, 200.0, 100.0);
        assert_eq!((eye.w, eye.h), (176.0, 76.0));
    }

    #[test]
    fn the_spectrum_window_sits_below_the_rounded_peak() {
        assert_eq!(spectrum_top(&[-43.0, -71.0]), -40.0);
        assert_eq!(spectrum_top(&[]), 0.0);
    }

    #[test]
    fn spectrum_ticks_space_about_one_per_110_px_and_read_in_khz() {
        let narrow = spectrum_ticks(48_000.0, 220.0);
        let wide = spectrum_ticks(48_000.0, 880.0);
        assert!(wide.len() > narrow.len());
        assert!(
            wide.iter()
                .any(|(at, text)| (*at - 0.5).abs() < 1e-6 && text == "0")
        );
        assert_eq!(format_offset(12_000.0), "+12");
        assert_eq!(format_offset(-2_500.0), "-2.5");
        assert_eq!(format_offset(1e-7), "0");
    }

    #[test]
    fn spectrum_labels_name_the_axes_and_skip_what_would_clip() {
        let window = DbWindow {
            min: -130.0,
            max: -40.0,
        };
        let labels = spectrum_labels(48_000.0, window, 400.0, 200.0);
        assert!(labels.iter().any(|label| label.text == "dBFS"));
        let khz = labels
            .iter()
            .find(|label| label.text == "kHz")
            .expect("a unit");
        assert_eq!((khz.x, khz.anchor), (396.0, Anchor::Right));
        assert!(
            labels
                .iter()
                .filter(|label| label.anchor == Anchor::Centre)
                .all(|label| fits(label.x, &label.text, 400.0))
        );
        assert!(
            labels
                .iter()
                .any(|label| label.backed && label.text == "-80")
        );
    }

    #[test]
    fn a_smoothed_cell_blends_its_neighbours() {
        let mut grid = Grid::new(2, 1);
        grid.cells = vec![0.0, 1.0];
        assert!((smoothed(&grid, 0.5, 0.0) - 0.5).abs() < 1e-6);
        assert_eq!(smoothed(&grid, -3.0, 0.0), 0.0);
        assert_eq!(smoothed(&grid, 9.0, 0.0), 1.0);
    }

    #[test]
    fn a_trend_span_pads_its_range_and_centres_on_zero_when_asked() {
        let mut trend = Trend::new(4);
        trend.push(2.0);
        trend.push(6.0);
        let (min, max, longest) = trend_span(&[&trend], false).expect("a span");
        assert_eq!(longest, 2);
        assert!(min < 2.0 && max > 6.0);
        let (min, max, _) = trend_span(&[&trend], true).expect("a span");
        assert!((min + max).abs() < 1e-5);
        assert_eq!(trend_span(&[&Trend::new(4)], false), None);
    }

    #[test]
    fn the_trend_legend_spaces_by_its_measured_names() {
        let plot = trend_box(400.0, 200.0);
        let offsets = legend_offsets(plot, &["MER dB", "margin"]);
        assert_eq!(offsets[0], plot.x + 6.0);
        assert_eq!(offsets[1], plot.x + 6.0 + 6.0 * CHAR_PX + 26.0);
    }

    #[test]
    fn reference_levels_are_labelled_where_the_histogram_puts_them() {
        assert_eq!(reference_levels(&block()), vec![1.0, 3.0, -1.0, -3.0]);
        assert!((histogram_x(0.0, 4.2) - 0.5).abs() < 1e-6);
        assert_eq!(level_text(3.0), "3");
        assert_eq!(level_text(0.33), "0.33");
    }

    #[test]
    fn every_view_but_the_states_carries_backed_labels() {
        let mut scope = scope();
        scope.take_burst(burst(vec![1.0, 0.0]));
        scope.take_block(block());
        for view in [
            View::Spectrum,
            View::Constellation,
            View::Eye,
            View::Levels,
            View::Quality,
            View::Drift,
        ] {
            scope.configure(Settings {
                view,
                ..Settings::default()
            });
            assert!(!scope.labels(400.0, 240.0).is_empty(), "{view:?}");
        }
        scope.configure(Settings {
            view: View::Levels,
            ..Settings::default()
        });
        assert!(
            scope
                .labels(400.0, 240.0)
                .iter()
                .filter(|label| label.anchor == Anchor::Centre)
                .all(|label| label.backed)
        );
        scope.configure(Settings {
            view: View::States,
            ..Settings::default()
        });
        assert!(scope.labels(400.0, 240.0).is_empty());
    }
}
