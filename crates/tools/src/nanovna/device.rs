use std::time::Instant;

use sdrmm_wire::{
    MIN_NANOVNA_POINTS, NanoVnaCalStep, NanoVnaCalibration, NanoVnaComplex,
    NanoVnaConfigureRequest, NanoVnaDeviceReport, NanoVnaPoint, NanoVnaPower, NanoVnaStandard,
    NanoVnaSweep, NanoVnaSweepRequest, NanoVnaSweepState,
};

use super::shell::{
    Session, Trace, first_number, parse_complex, parse_frequencies, parse_scan_rows, reported_value,
};

const LEGACY_SEGMENT_POINTS: u32 = 101;

const SCAN_EVERYTHING: u8 = 0b111;

const AUTO_POWER: u16 = 255;

const APPLIED_TOKEN: &str = "cal'ed";

pub fn paused<T>(
    session: &mut Session<'_>,
    work: impl FnOnce(&mut Session<'_>) -> Result<T, String>,
) -> Result<T, String> {
    session.silent("pause")?;
    let outcome = work(session);
    let resumed = session.silent("resume");
    let value = outcome?;
    resumed.map(|()| value)
}

pub fn describe(session: &mut Session<'_>, port: &str) -> Result<NanoVnaDeviceReport, String> {
    let firmware = session.command("version")?.join(" ");
    if firmware.is_empty() {
        return Err("the device returned no firmware version".to_owned());
    }
    let info = session.command("info")?;
    let board = info
        .iter()
        .find_map(|line| line.strip_prefix("Board:"))
        .map(|board| board.trim().to_owned());
    let max_points = max_points(&info);
    let battery_mv = value(session, "vbat")?
        .as_deref()
        .and_then(first_number)
        .and_then(|value| u32::try_from(value).ok());
    let bandwidth_hz = bandwidth(session)?;
    let power = value(session, "power")?
        .as_deref()
        .and_then(first_number)
        .and_then(|value| u16::try_from(value).ok());
    let tcxo_hz = value(session, "tcxo")?.as_deref().and_then(first_number);
    let harmonic_threshold_hz = value(session, "threshold")?
        .as_deref()
        .and_then(first_number);
    let electrical_delay_s = value(session, "edelay")?.and_then(|text| text.parse().ok());
    let s21_offset_db = value(session, "s21offset")?.and_then(|text| text.parse().ok());
    let sweep = sweep_state(session)?;
    let calibration = calibration_status(session, port)?;
    let commands = session
        .command("help")?
        .iter()
        .find_map(|line| line.strip_prefix("Commands:"))
        .map(|list| list.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    Ok(NanoVnaDeviceReport {
        port: port.to_owned(),
        firmware,
        board,
        info,
        max_points,
        battery_mv,
        bandwidth_hz,
        power,
        tcxo_hz,
        harmonic_threshold_hz,
        electrical_delay_s,
        s21_offset_db,
        sweep,
        calibration,
        commands,
    })
}

fn max_points(info: &[String]) -> Option<u32> {
    info.iter()
        .find_map(|line| line.split_once("[p:").map(|(_, rest)| rest))
        .and_then(first_number)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|points| *points >= MIN_NANOVNA_POINTS)
}

pub fn configure(
    session: &mut Session<'_>,
    request: &NanoVnaConfigureRequest,
) -> Result<NanoVnaDeviceReport, String> {
    if let Some(hz) = request.bandwidth_hz {
        set_bandwidth(session, hz)?;
    }
    if let Some(power) = request.power {
        session.silent(&format!("power {}", power_code(power)))?;
    }
    describe(session, &request.port)
}

fn set_bandwidth(session: &mut Session<'_>, hz: u32) -> Result<(), String> {
    let request = format!("bandwidth {hz} Hz");
    let lines = session.command(&request)?;
    if lines.iter().any(|line| line.starts_with("bandwidth ")) {
        return Ok(());
    }
    Err(format!("device refused `{request}`: {}", lines.join(" ")))
}

fn power_code(power: NanoVnaPower) -> u16 {
    match power {
        NanoVnaPower::Auto => AUTO_POWER,
        NanoVnaPower::Level(level) => level,
    }
}

fn value(session: &mut Session<'_>, request: &str) -> Result<Option<String>, String> {
    let lines = session.command(request)?;
    Ok(reported_value(&lines).map(str::to_owned))
}

fn bandwidth(session: &mut Session<'_>) -> Result<Option<u32>, String> {
    let lines = session.command("bandwidth")?;
    let Some(line) = lines.iter().find(|line| line.contains('(')) else {
        return Ok(None);
    };
    let inside = line
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(inside, _)| inside);
    Ok(inside
        .and_then(first_number)
        .and_then(|value| u32::try_from(value).ok()))
}

fn sweep_state(session: &mut Session<'_>) -> Result<Option<NanoVnaSweepState>, String> {
    let lines = session.command("sweep")?;
    let Some(fields) = lines
        .iter()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .find(|fields| fields.len() == 3)
    else {
        return Ok(None);
    };
    let (Ok(start_hz), Ok(stop_hz), Ok(points)) = (
        fields[0].parse::<u64>(),
        fields[1].parse::<u64>(),
        fields[2].parse::<u32>(),
    ) else {
        return Ok(None);
    };
    Ok(Some(NanoVnaSweepState {
        start_hz,
        stop_hz,
        points,
    }))
}

pub fn calibration_status(
    session: &mut Session<'_>,
    port: &str,
) -> Result<NanoVnaCalibration, String> {
    let raw = session.command("cal")?.join(" ");
    let mut standards = Vec::new();
    let mut error_terms = Vec::new();
    let mut applied = false;
    for token in raw.split_whitespace() {
        if token == APPLIED_TOKEN {
            applied = true;
        } else if let Some(standard) = NanoVnaStandard::from_token(token) {
            standards.push(standard);
        } else {
            error_terms.push(token.to_owned());
        }
    }
    Ok(NanoVnaCalibration {
        port: port.to_owned(),
        standards,
        error_terms,
        applied,
        raw: raw.trim().to_owned(),
    })
}

pub fn calibrate(
    session: &mut Session<'_>,
    port: &str,
    step: &NanoVnaCalStep,
    range: Option<NanoVnaSweepState>,
) -> Result<NanoVnaCalibration, String> {
    if matches!(step, NanoVnaCalStep::Status) {
        return calibration_status(session, port);
    }
    if let (NanoVnaCalStep::Reset, Some(range)) = (step, range) {
        session.silent(&format!(
            "sweep {} {} {}",
            range.start_hz, range.stop_hz, range.points
        ))?;
    }
    session.silent(&command_for(step))?;
    calibration_status(session, port)
}

fn command_for(step: &NanoVnaCalStep) -> String {
    match step {
        NanoVnaCalStep::Status => "cal".to_owned(),
        NanoVnaCalStep::Reset => "cal reset".to_owned(),
        NanoVnaCalStep::Open => "cal open".to_owned(),
        NanoVnaCalStep::Short => "cal short".to_owned(),
        NanoVnaCalStep::Load => "cal load".to_owned(),
        NanoVnaCalStep::Thru => "cal thru".to_owned(),
        NanoVnaCalStep::Isolation => "cal isoln".to_owned(),
        NanoVnaCalStep::Finish => "cal done".to_owned(),
        NanoVnaCalStep::Enable => "cal on".to_owned(),
        NanoVnaCalStep::Disable => "cal off".to_owned(),
        NanoVnaCalStep::Save { slot } => format!("save {slot}"),
        NanoVnaCalStep::Recall { slot } => format!("recall {slot}"),
    }
}

pub fn acquire(
    session: &mut Session<'_>,
    request: &NanoVnaSweepRequest,
) -> Result<NanoVnaSweep, String> {
    let started = Instant::now();
    let device = describe(session, &request.port)?;
    let segment_limit = device.max_points.unwrap_or(LEGACY_SEGMENT_POINTS);
    let mut points = Vec::with_capacity(request.points as usize);
    let mut point_offset = 0;
    for size in segment_sizes(request.points, segment_limit) {
        let segment_start = frequency_at(request, point_offset);
        let segment_stop = frequency_at(request, point_offset + size - 1);
        let values = acquire_segment(session, segment_start, segment_stop, size, request.averages)?;
        points.extend(values);
        point_offset += size;
    }
    session.silent(&format!(
        "sweep {} {} {}",
        request.start_hz,
        request.stop_hz,
        request.points.min(segment_limit)
    ))?;
    if points.len() != request.points as usize {
        return Err(format!(
            "requested {} points but the device returned {}",
            request.points,
            points.len()
        ));
    }
    Ok(NanoVnaSweep {
        device,
        requested_points: request.points,
        averages: request.averages,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        points,
    })
}

fn acquire_segment(
    session: &mut Session<'_>,
    start_hz: u64,
    stop_hz: u64,
    points: u32,
    averages: u16,
) -> Result<Vec<NanoVnaPoint>, String> {
    let mut frequencies = Vec::new();
    let mut s11_sum = vec![NanoVnaComplex { re: 0.0, im: 0.0 }; points as usize];
    let mut s21_sum = vec![NanoVnaComplex { re: 0.0, im: 0.0 }; points as usize];
    for average in 0..averages {
        let trace = measure(session, start_hz, stop_hz, points)?;
        ensure_lengths(points, &trace)?;
        if average == 0 {
            frequencies = trace.frequencies;
        } else if frequencies != trace.frequencies {
            return Err("device frequencies changed while averaging".to_owned());
        }
        accumulate(&mut s11_sum, &trace.s11);
        accumulate(&mut s21_sum, &trace.s21);
    }
    let divisor = f64::from(averages);
    Ok(frequencies
        .into_iter()
        .zip(s11_sum)
        .zip(s21_sum)
        .map(|((frequency_hz, s11), s21)| NanoVnaPoint {
            frequency_hz,
            s11: divide(s11, divisor),
            s21: divide(s21, divisor),
        })
        .collect())
}

fn measure(
    session: &mut Session<'_>,
    start_hz: u64,
    stop_hz: u64,
    points: u32,
) -> Result<Trace, String> {
    let request = format!("scan {start_hz} {stop_hz} {points} {SCAN_EVERYTHING}");
    let rows = session.command(&request)?;
    if rows.is_empty() {
        return read_back(session);
    }
    parse_scan_rows(&request, &rows)
}

fn read_back(session: &mut Session<'_>) -> Result<Trace, String> {
    Ok(Trace {
        frequencies: parse_frequencies(session.command("frequencies")?)?,
        s11: parse_complex(session.command("data 0")?)?,
        s21: parse_complex(session.command("data 1")?)?,
    })
}

fn segment_sizes(points: u32, limit: u32) -> Vec<u32> {
    let segment_count = points.div_ceil(limit.max(1));
    let base_size = points / segment_count;
    let extra = points % segment_count;
    (0..segment_count)
        .map(|segment| base_size + u32::from(segment < extra))
        .collect()
}

fn ensure_lengths(expected: u32, trace: &Trace) -> Result<(), String> {
    let expected = expected as usize;
    if trace.frequencies.len() == expected
        && trace.s11.len() == expected
        && trace.s21.len() == expected
    {
        return Ok(());
    }
    Err(format!(
        "device returned {} frequencies, {} S11 values, and {} S21 values; expected {expected}",
        trace.frequencies.len(),
        trace.s11.len(),
        trace.s21.len()
    ))
}

fn accumulate(sum: &mut [NanoVnaComplex], values: &[NanoVnaComplex]) {
    for (sum, value) in sum.iter_mut().zip(values) {
        sum.re += value.re;
        sum.im += value.im;
    }
}

fn divide(value: NanoVnaComplex, divisor: f64) -> NanoVnaComplex {
    NanoVnaComplex {
        re: value.re / divisor,
        im: value.im / divisor,
    }
}

fn frequency_at(request: &NanoVnaSweepRequest, index: u32) -> u64 {
    let span = u128::from(request.stop_hz - request.start_hz);
    let numerator = span * u128::from(index);
    let denominator = u128::from(request.points - 1);
    request.start_hz + ((numerator + denominator / 2) / denominator) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_distribution_keeps_every_chunk_within_device_limits() {
        assert_eq!(segment_sizes(102, 101), vec![51, 51]);
        assert_eq!(segment_sizes(11, 101), vec![11]);
        assert_eq!(segment_sizes(401, 401), vec![401]);
        assert_eq!(segment_sizes(1001, 401), vec![334, 334, 333]);
        for limit in [101, 401] {
            let sizes = segment_sizes(10_001, limit);
            assert_eq!(sizes.iter().sum::<u32>(), 10_001);
            assert!(sizes.into_iter().all(|size| size <= limit));
        }
    }

    #[test]
    fn the_point_limit_is_read_from_the_version_line() {
        let info = vec![
            "Board: NanoVNA-H 4".to_owned(),
            "Version: 1.2.46 [p:401, IF:12k, ADC:384k, Lcd:480x320]".to_owned(),
        ];
        assert_eq!(max_points(&info), Some(401));
        assert_eq!(max_points(&["Version: 0.4.5".to_owned()]), None);
        assert_eq!(max_points(&["Version: x [p:3]".to_owned()]), None);
    }

    #[test]
    fn auto_power_is_the_firmware_sentinel() {
        assert_eq!(power_code(NanoVnaPower::Auto), 255);
        assert_eq!(power_code(NanoVnaPower::Level(2)), 2);
    }

    #[test]
    fn each_calibration_step_names_its_own_shell_command() {
        assert_eq!(command_for(&NanoVnaCalStep::Isolation), "cal isoln");
        assert_eq!(command_for(&NanoVnaCalStep::Finish), "cal done");
        assert_eq!(command_for(&NanoVnaCalStep::Save { slot: 3 }), "save 3");
    }
}
