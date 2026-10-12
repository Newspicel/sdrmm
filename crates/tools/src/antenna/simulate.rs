use num_complex::Complex64;
use sdrmm_wire::{
    AntennaGeometry, AntennaGround, AntennaMaterial, AntennaModel, AntennaPatternCut,
    AntennaPolarization, AntennaSegment, AntennaSegmentRole, AntennaSimulation,
    AntennaSimulationOutcome, AntennaSweepPoint, AntennaSweepRequest, AntennaWire, NanoVnaComplex,
};

use super::field::{Direction, FarField, Reflection, Sample};
use super::solver::{Medium, SPEED_OF_LIGHT_M_S, Singular, Solution, solve};
use super::structure::{Structure, StructureError};

const SEGMENTS_PER_WAVELENGTH: f64 = 20.0;
const MAX_SEGMENTS: f64 = 400.0;
const MAX_DIAMETER_WAVELENGTHS: f64 = 0.02;
const DEFAULT_SWEEP_SPAN: f64 = 0.1;
const DEFAULT_SWEEP_POINTS: u16 = 41;
const CUT_STEP_DEG: f64 = 2.0;
const SEARCH_STEP_DEG: f64 = 4.0;
const FINE_STEP_DEG: f64 = 0.5;
const VERTICAL_SHARE: f64 = 0.9;
const HORIZONTAL_SHARE: f64 = 0.1;
const TRIM_LOW: f64 = 0.2;
const TRIM_HIGH: f64 = 0.1;
const TRIM_STEP: f64 = 0.02;
const TRIM_BISECTIONS: usize = 12;
const MIN_GAIN: f64 = 1e-12;
const TIE: f64 = 1e-3;
const MAX_THREADS: usize = 8;

pub(super) struct Job<'a> {
    pub frequency_hz: f64,
    pub velocity_factor: f64,
    pub model: AntennaModel,
    pub sweep: Option<AntennaSweepRequest>,
    pub geometry: &'a AntennaGeometry,
    pub loading_coil: bool,
}

pub(super) fn simulate(
    job: &Job<'_>,
    trim: Option<&dyn Fn(f64) -> AntennaGeometry>,
) -> AntennaSimulationOutcome {
    match run(job, trim) {
        Ok(simulation) => AntennaSimulationOutcome::Solved(Box::new(simulation)),
        Err(reason) => AntennaSimulationOutcome::Unavailable { reason },
    }
}

fn run(
    job: &Job<'_>,
    trim: Option<&dyn Fn(f64) -> AntennaGeometry>,
) -> Result<AntennaSimulation, String> {
    let sweep = sweep_frequencies(job);
    let top_hz = sweep.iter().copied().fold(job.frequency_hz, f64::max);
    let top_wavelength = SPEED_OF_LIGHT_M_S / top_hz;
    let radius = job.model.wire_diameter_mm / 2_000.0;
    if 2.0 * radius > top_wavelength * MAX_DIAMETER_WAVELENGTHS {
        return Err(format!(
            "The wire is too thick to model at this frequency: keep it under {:.1} mm.",
            top_wavelength * MAX_DIAMETER_WAVELENGTHS * 1_000.0
        ));
    }
    let max_segment = segment_length(job.geometry, top_wavelength);
    let structure = build(job.geometry, job.model, max_segment)?;
    let at_center = medium(job.model, radius, job.frequency_hz);
    let center = solve(&structure, &at_center).map_err(|Singular| solver_failed())?;
    let coil = loading_coil(job, &center);
    let impedances = sweep_impedances(&structure, job.model, radius, &sweep)?;
    let k = std::f64::consts::TAU * job.frequency_hz / SPEED_OF_LIGHT_M_S;
    let field = FarField::new(
        &structure,
        &center,
        k,
        reflection(job.model.ground, job.frequency_hz),
    );
    let pattern = Pattern::measure(&field);
    let feed = center.impedance() + coil_reactance(coil, job.frequency_hz);
    Ok(AntennaSimulation {
        impedance: complex(feed),
        gain_dbi: round(decibels(pattern.peak.gain), 0.01),
        efficiency: round(efficiency(&structure, &center, &at_center), 1e-4),
        takeoff_deg: round(pattern.direction.elevation.to_degrees(), 0.1),
        bearing_deg: round(pattern.direction.azimuth.to_degrees(), 0.1).rem_euclid(360.0),
        front_to_back_db: round(pattern.front_to_back_db, 0.01),
        beamwidth_azimuth_deg: pattern.beamwidth_azimuth_deg,
        beamwidth_elevation_deg: pattern.beamwidth_elevation_deg,
        polarization: polarization(pattern.peak.vertical_share),
        azimuth: pattern.azimuth,
        elevation: pattern.elevation,
        sweep: sweep
            .iter()
            .zip(impedances)
            .map(|(&frequency_hz, impedance)| AntennaSweepPoint {
                frequency_hz,
                impedance: complex(impedance + coil_reactance(coil, frequency_hz)),
            })
            .collect(),
        wires: wires(&structure, &center),
        feed_wire: u32::try_from(structure.feed_wire).unwrap_or(u32::MAX),
        feed_segment: u32::try_from(structure.feed_segment).unwrap_or(u32::MAX),
        wire_radius_m: radius,
        loading_coil_h: coil,
        resonant_velocity_factor: trim.and_then(|rebuild| trimmed(job, rebuild, max_segment)),
    })
}

fn build(
    geometry: &AntennaGeometry,
    model: AntennaModel,
    max_segment: f64,
) -> Result<Structure, String> {
    let lift = if model.ground == AntennaGround::FreeSpace {
        0.0
    } else {
        model.height_m - geometry.feed.y_m
    };
    let structure = Structure::build(geometry, lift, max_segment).map_err(|error| match error {
        StructureError::Empty => "There is no wire to model.".to_owned(),
        StructureError::FeedOffAntenna => "The feedpoint is not on a wire.".to_owned(),
    })?;
    let radius = model.wire_diameter_mm / 2_000.0;
    if model.ground != AntennaGround::FreeSpace && structure.lowest_m <= radius {
        let needed = model.height_m - structure.lowest_m + radius.max(0.01);
        return Err(format!(
            "Part of the antenna is in the ground: raise the feedpoint to at least {needed:.2} m."
        ));
    }
    Ok(structure)
}

fn solver_failed() -> String {
    "The model has no solution: two wires may overlap.".to_owned()
}

fn segment_length(geometry: &AntennaGeometry, wavelength: f64) -> f64 {
    let lengths = geometry
        .segments
        .iter()
        .filter(|segment| {
            !matches!(
                segment.role,
                AntennaSegmentRole::Feedline | AntennaSegmentRole::Structure
            )
        })
        .map(AntennaSegment::length_m)
        .filter(|length| *length > 0.0);
    let (total, shortest) = lengths.fold((0.0, f64::INFINITY), |(total, shortest), length| {
        (total + length, f64::min(shortest, length))
    });
    (wavelength / SEGMENTS_PER_WAVELENGTH)
        .min(shortest)
        .max(total / MAX_SEGMENTS)
}

fn medium(model: AntennaModel, radius: f64, frequency_hz: f64) -> Medium {
    Medium {
        frequency_hz,
        radius_m: radius,
        conductivity_s_m: conductivity(model.material),
        image: model.ground != AntennaGround::FreeSpace,
    }
}

fn conductivity(material: AntennaMaterial) -> Option<f64> {
    match material {
        AntennaMaterial::Lossless => None,
        AntennaMaterial::Copper => Some(5.8e7),
        AntennaMaterial::Aluminium => Some(3.5e7),
        AntennaMaterial::StainlessSteel => Some(1.4e6),
    }
}

fn reflection(ground: AntennaGround, frequency_hz: f64) -> Reflection {
    match ground {
        AntennaGround::FreeSpace => Reflection::None,
        AntennaGround::Perfect => Reflection::Perfect,
        AntennaGround::Poor => Reflection::real(5.0, 0.001, frequency_hz),
        AntennaGround::Average => Reflection::real(13.0, 0.005, frequency_hz),
        AntennaGround::Good => Reflection::real(20.0, 0.03, frequency_hz),
        AntennaGround::Seawater => Reflection::real(81.0, 5.0, frequency_hz),
    }
}

fn sweep_frequencies(job: &Job<'_>) -> Vec<f64> {
    let (start, stop, points) = job.sweep.map_or(
        (
            job.frequency_hz * (1.0 - DEFAULT_SWEEP_SPAN),
            job.frequency_hz * (1.0 + DEFAULT_SWEEP_SPAN),
            DEFAULT_SWEEP_POINTS,
        ),
        |sweep| (sweep.start_hz, sweep.stop_hz, sweep.points),
    );
    let last = f64::from(points.max(2) - 1);
    (0..points.max(2))
        .map(|index| start + (stop - start) * f64::from(index) / last)
        .collect()
}

fn sweep_impedances(
    structure: &Structure,
    model: AntennaModel,
    radius: f64,
    frequencies: &[f64],
) -> Result<Vec<Complex64>, String> {
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, MAX_THREADS);
    let chunk = frequencies.len().div_ceil(threads).max(1);
    let results: Vec<Result<Vec<Complex64>, Singular>> = std::thread::scope(|scope| {
        let handles: Vec<_> = frequencies
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|&hz| {
                            solve(structure, &medium(model, radius, hz)).map(|s| s.impedance())
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(Err(Singular)))
            .collect()
    });
    let mut impedances = Vec::with_capacity(frequencies.len());
    for part in results {
        impedances.extend(part.map_err(|Singular| solver_failed())?);
    }
    Ok(impedances)
}

fn loading_coil(job: &Job<'_>, center: &Solution) -> Option<f64> {
    let reactance = center.impedance().im;
    (job.loading_coil && reactance < 0.0)
        .then(|| -reactance / (std::f64::consts::TAU * job.frequency_hz))
}

fn coil_reactance(coil: Option<f64>, frequency_hz: f64) -> Complex64 {
    Complex64::new(
        0.0,
        coil.map_or(0.0, |henry| std::f64::consts::TAU * frequency_hz * henry),
    )
}

fn efficiency(structure: &Structure, solution: &Solution, medium: &Medium) -> f64 {
    let lost: f64 = structure
        .pulses
        .iter()
        .zip(&solution.currents)
        .map(|(pulse, current)| {
            let length = structure.length_of(pulse);
            0.5 * current.norm_sqr() * medium.loss_ohms(length)
        })
        .sum();
    (1.0 - lost / solution.input_power_w()).clamp(0.0, 1.0)
}

fn wires(structure: &Structure, solution: &Solution) -> Vec<AntennaWire> {
    let mut halves = vec![Complex64::new(0.0, 0.0); structure.segments.len() * 2];
    for (pulse, current) in structure.pulses.iter().zip(&solution.currents) {
        for half in pulse.halves {
            halves[half.segment * 2 + usize::from(half.upper)] += current * half.along;
        }
    }
    let peak = halves.iter().map(|value| value.norm()).fold(0.0, f64::max);
    let peak = if peak > 0.0 { peak } else { 1.0 };
    structure
        .wires
        .iter()
        .map(|wire| AntennaWire {
            from: wire.from.to_point(structure.lift_m),
            to: wire.to.to_point(structure.lift_m),
            segments: u32::try_from(wire.segments).unwrap_or(u32::MAX),
            current: halves[wire.first_segment * 2..(wire.first_segment + wire.segments) * 2]
                .iter()
                .map(|value| round(value.norm() / peak, 1e-3))
                .collect(),
        })
        .collect()
}

fn trimmed(
    job: &Job<'_>,
    rebuild: &dyn Fn(f64) -> AntennaGeometry,
    max_segment: f64,
) -> Option<f64> {
    let radius = job.model.wire_diameter_mm / 2_000.0;
    let medium = medium(job.model, radius, job.frequency_hz);
    let reactance = |factor: f64| -> Option<f64> {
        let structure = build(&rebuild(factor), job.model, max_segment).ok()?;
        solve(&structure, &medium).ok().map(|s| s.impedance().im)
    };
    let low = (job.velocity_factor - TRIM_LOW).max(sdrmm_wire::MIN_VELOCITY_FACTOR);
    let high = (job.velocity_factor + TRIM_HIGH).min(sdrmm_wire::MAX_VELOCITY_FACTOR);
    let steps = ((high - low) / TRIM_STEP).ceil() as usize;
    let samples: Vec<(f64, f64)> = (0..=steps)
        .map(|index| low + (high - low) * index as f64 / steps.max(1) as f64)
        .filter_map(|factor| reactance(factor).map(|x| (factor, x)))
        .collect();
    let bracket = samples
        .windows(2)
        .filter(|pair| pair[0].1.signum() != pair[1].1.signum())
        .min_by(|a, b| {
            let distance =
                |pair: &[(f64, f64)]| (pair[0].0 + pair[1].0) / 2.0 - job.velocity_factor;
            distance(a).abs().total_cmp(&distance(b).abs())
        })?;
    let (mut low, mut high) = (bracket[0], bracket[1]);
    for _ in 0..TRIM_BISECTIONS {
        let middle = (low.0 + high.0) / 2.0;
        let x = reactance(middle)?;
        if x.signum() == low.1.signum() {
            low = (middle, x);
        } else {
            high = (middle, x);
        }
    }
    Some(round((low.0 + high.0) / 2.0, 1e-4))
}

struct Pattern {
    peak: Sample,
    direction: Direction,
    front_to_back_db: f64,
    beamwidth_azimuth_deg: Option<f64>,
    beamwidth_elevation_deg: Option<f64>,
    azimuth: AntennaPatternCut,
    elevation: AntennaPatternCut,
}

impl Pattern {
    fn measure(field: &FarField) -> Self {
        let lowest = if field.grounded() { 0.0 } else { -90.0 };
        let (direction, peak) = peak_direction(field, lowest);
        let azimuth_deg = direction.azimuth.to_degrees();
        let elevation_deg = direction.elevation.to_degrees();
        let back = field.sample(Direction::degrees(azimuth_deg + 180.0, elevation_deg));
        let around = |offset: f64| {
            field
                .sample(Direction::degrees(azimuth_deg + offset, elevation_deg))
                .gain
        };
        let tilt = |offset: f64| {
            field
                .sample(vertical_direction(azimuth_deg, elevation_deg + offset))
                .gain
        };
        let elevation_reach = if field.grounded() {
            (elevation_deg, 180.0 - elevation_deg)
        } else {
            (180.0, 180.0)
        };
        Self {
            front_to_back_db: decibels(peak.gain) - decibels(back.gain),
            beamwidth_azimuth_deg: beamwidth(peak.gain, around, (180.0, 180.0)),
            beamwidth_elevation_deg: beamwidth(peak.gain, tilt, elevation_reach),
            azimuth: cut(360.0, |angle| {
                field.sample(Direction::degrees(angle, elevation_deg))
            }),
            elevation: cut(if field.grounded() { 180.0 } else { 360.0 }, |angle| {
                field.sample(vertical_direction(azimuth_deg, angle))
            }),
            peak,
            direction,
        }
    }
}

fn vertical_direction(azimuth_deg: f64, angle_deg: f64) -> Direction {
    let angle = angle_deg.rem_euclid(360.0);
    if angle <= 90.0 {
        Direction::degrees(azimuth_deg, angle)
    } else if angle <= 270.0 {
        Direction::degrees(azimuth_deg + 180.0, 180.0 - angle)
    } else {
        Direction::degrees(azimuth_deg, angle - 360.0)
    }
}

fn peak_direction(field: &FarField, lowest: f64) -> (Direction, Sample) {
    let level = Direction::degrees(0.0, 0.0);
    let mut best = (level, field.sample(level));
    for elevation in search_elevations(lowest) {
        let mut azimuth = 0.0;
        while azimuth < 360.0 {
            best = better(field, best, Direction::degrees(azimuth, elevation));
            azimuth += SEARCH_STEP_DEG;
        }
    }
    let coarse = best.0;
    let steps = (SEARCH_STEP_DEG / FINE_STEP_DEG) as i32;
    let offsets = nearest_first(steps);
    for &de in &offsets {
        for &da in &offsets {
            let elevation =
                (coarse.elevation.to_degrees() + f64::from(de) * FINE_STEP_DEG).clamp(lowest, 90.0);
            let azimuth = coarse.azimuth.to_degrees() + f64::from(da) * FINE_STEP_DEG;
            best = better(field, best, Direction::degrees(azimuth, elevation));
        }
    }
    best
}

fn nearest_first(steps: i32) -> Vec<i32> {
    (0..=steps)
        .flat_map(|step| [step, -step])
        .fold(Vec::new(), |mut list, step| {
            if !list.contains(&step) {
                list.push(step);
            }
            list
        })
}

fn search_elevations(lowest: f64) -> Vec<f64> {
    let steps = (90.0 / SEARCH_STEP_DEG).round() as i32;
    (0..=steps)
        .flat_map(|step| {
            let rise = f64::from(step) * SEARCH_STEP_DEG;
            [rise, -rise]
        })
        .filter(|&elevation| {
            elevation >= lowest && !(elevation == 0.0 && elevation.is_sign_negative())
        })
        .collect()
}

fn better(
    field: &FarField,
    best: (Direction, Sample),
    direction: Direction,
) -> (Direction, Sample) {
    let sample = field.sample(direction);
    if sample.gain > best.1.gain * (1.0 + TIE) {
        (direction, sample)
    } else {
        best
    }
}

fn beamwidth(peak: f64, gain_at: impl Fn(f64) -> f64, reach: (f64, f64)) -> Option<f64> {
    let half = peak / 2.0;
    let edge = |sign: f64, limit: f64| -> Option<f64> {
        let mut previous = (0.0, peak);
        let mut offset = FINE_STEP_DEG;
        while offset <= limit {
            let gain = gain_at(sign * offset);
            if gain < half {
                let fraction = (previous.1 - half) / (previous.1 - gain);
                return Some(previous.0 + fraction * (offset - previous.0));
            }
            previous = (offset, gain);
            offset += FINE_STEP_DEG;
        }
        None
    };
    let width = edge(-1.0, reach.0)? + edge(1.0, reach.1)?;
    (width < 359.0).then_some(round(width, 0.1))
}

fn cut(span_deg: f64, sample: impl Fn(f64) -> Sample) -> AntennaPatternCut {
    let count = (span_deg / CUT_STEP_DEG).round() as usize + usize::from(span_deg < 360.0);
    AntennaPatternCut {
        step_deg: CUT_STEP_DEG,
        gain_dbi: (0..count)
            .map(|index| round(decibels(sample(index as f64 * CUT_STEP_DEG).gain), 0.01))
            .collect(),
    }
}

fn polarization(vertical_share: f64) -> AntennaPolarization {
    if vertical_share >= VERTICAL_SHARE {
        AntennaPolarization::Vertical
    } else if vertical_share <= HORIZONTAL_SHARE {
        AntennaPolarization::Horizontal
    } else {
        AntennaPolarization::Mixed
    }
}

fn decibels(gain: f64) -> f64 {
    10.0 * gain.max(MIN_GAIN).log10()
}

fn complex(value: Complex64) -> NanoVnaComplex {
    NanoVnaComplex {
        re: round(value.re, 1e-3),
        im: round(value.im, 1e-3),
    }
}

fn round(value: f64, step: f64) -> f64 {
    (value / step).round() * step
}

#[cfg(test)]
mod tests;
