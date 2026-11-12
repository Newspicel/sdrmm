use sdrmm_device::{DeviceError, check_stream_settings};
use sdrmm_wire::{Capabilities, DeviceSettings, ExtraValue, GainKind, GainValue, any_range_holds};

use crate::{
    api::{DevHandle, Rtsa},
    caps::{
        CENTER, CHANNEL, CLOCK, GAIN, Profile, RATE_PER_SPAN, REFERENCE, SPAN, TX_GAIN,
        reference_for_gain,
    },
    model::Mode,
};

/// Refuses what this SPECTRAN cannot take before anything reaches it.
pub fn validate(delta: &DeviceSettings, caps: &Capabilities) -> Result<(), DeviceError> {
    check_stream_settings(delta, caps)?;
    if let Some(hz) = delta.center_hz
        && !any_range_holds(&caps.freq_ranges, hz)
    {
        return Err(DeviceError::Unsupported(format!(
            "center_hz {hz} is outside the tuner range"
        )));
    }
    if let Some(rate) = delta.sample_rate
        && !any_range_holds(&caps.sample_rate_ranges, rate)
    {
        return Err(DeviceError::Unsupported(format!("sample_rate {rate}")));
    }
    refuse_absent(delta)?;
    if let Some(antenna) = &delta.antenna
        && !caps.antennas.contains(antenna)
    {
        return Err(DeviceError::Unsupported(format!("antenna {antenna}")));
    }
    for gain in &delta.gains {
        let stage = caps
            .stage(&gain.stage)
            .ok_or_else(|| DeviceError::Unsupported(format!("gain stage {}", gain.stage)))?;
        if !stage.range.holds(gain.value_db) {
            return Err(DeviceError::Unsupported(format!(
                "{} gain {} dB is outside {}..{} dB",
                gain.stage, gain.value_db, stage.range.min, stage.range.max
            )));
        }
    }
    for extra in &delta.extra {
        validate_extra(extra, caps)?;
    }
    Ok(())
}

fn refuse_absent(delta: &DeviceSettings) -> Result<(), DeviceError> {
    let refused = if delta.bandwidth.is_some() {
        Some("bandwidth: the span sets the filter")
    } else if delta.bias_tee == Some(true) {
        Some("bias_tee: the SPECTRAN has none")
    } else if delta.ppm.is_some() {
        Some("ppm: the SPECTRAN corrects its own clock")
    } else if delta.agc.as_ref().is_some_and(|agc| agc.on) {
        Some("agc: the SPECTRAN has none")
    } else {
        None
    };
    refused.map_or(Ok(()), |reason| {
        Err(DeviceError::Unsupported(reason.to_string()))
    })
}

fn validate_extra(extra: &ExtraValue, caps: &Capabilities) -> Result<(), DeviceError> {
    let Some(sdrmm_wire::ExtraSetting::Enum { options, .. }) =
        caps.extra.iter().find(|known| known.name() == extra.name)
    else {
        return Err(DeviceError::Unsupported(format!(
            "{}: not a SPECTRAN setting",
            extra.name
        )));
    };
    let value = extra.value.as_str().unwrap_or_default();
    if !options.iter().any(|option| option.value == value) {
        return Err(DeviceError::Unsupported(format!(
            "{}: {value} is not one of its options",
            extra.name
        )));
    }
    Ok(())
}

/// Writes `delta` into the tree of the mode the device is open in. The receiver takes tuning,
/// span, reference level, input and extras; the transmitter takes tuning, span and its gain.
pub fn push(
    api: &dyn Rtsa,
    dev: DevHandle,
    mode: Mode,
    profile: &Profile,
    delta: &DeviceSettings,
) -> Result<(), DeviceError> {
    if let Some(hz) = delta.center_hz {
        api.set_float(dev, CENTER, hz)?;
    }
    match mode {
        Mode::Receiver => push_receiver(api, dev, profile, delta),
        Mode::Transmitter => push_transmitter(api, dev, profile, delta),
    }
}

fn push_receiver(
    api: &dyn Rtsa,
    dev: DevHandle,
    profile: &Profile,
    delta: &DeviceSettings,
) -> Result<(), DeviceError> {
    if let Some(rate) = delta.sample_rate {
        if let Some(clock) = profile.clock_for(rate)
            && api.get_string(dev, CLOCK)? != clock
        {
            api.set_string(dev, CLOCK, clock)?;
        }
        api.set_float(dev, SPAN, rate / RATE_PER_SPAN)?;
    }
    if let Some(gain) = stage_value(&delta.gains, GAIN) {
        api.set_float(dev, REFERENCE, reference_for_gain(gain))?;
    }
    if let Some(antenna) = &delta.antenna
        && profile.selects_input()
    {
        api.set_string(dev, CHANNEL, antenna)?;
    }
    for extra in &delta.extra {
        let Some(known) = profile.extras.iter().find(|known| known.name == extra.name) else {
            continue;
        };
        api.set_string(dev, known.path, extra.value.as_str().unwrap_or_default())?;
    }
    Ok(())
}

fn push_transmitter(
    api: &dyn Rtsa,
    dev: DevHandle,
    profile: &Profile,
    delta: &DeviceSettings,
) -> Result<(), DeviceError> {
    if let Some(rate) = delta.sample_rate
        && let Some(max) = profile.tx_span_max()
    {
        api.set_float(dev, SPAN, rate.min(max))?;
    }
    if let Some(gain) = stage_value(&delta.gains, GainKind::Tx.name()) {
        api.set_float(dev, TX_GAIN, gain)?;
    }
    Ok(())
}

fn stage_value(gains: &[GainValue], stage: &str) -> Option<f64> {
    gains
        .iter()
        .find(|gain| gain.stage == stage)
        .map(|gain| gain.value_db)
}

/// What the receiver actually runs at, read back after the API had its say.
pub fn read_receiver(api: &dyn Rtsa, dev: DevHandle) -> Result<DeviceSettings, DeviceError> {
    let center_hz = api.get_float(dev, CENTER)?;
    let span = api.get_float(dev, SPAN)?;
    let reference = api.get_float(dev, REFERENCE)?;
    Ok(DeviceSettings {
        center_hz: Some(center_hz),
        sample_rate: Some(span * RATE_PER_SPAN),
        gains: vec![GainValue {
            stage: GAIN.to_string(),
            value_db: -reference,
        }],
        ..DeviceSettings::default()
    })
}

/// Everything remembered, as one delta to replay into a freshly opened tree.
#[must_use]
pub fn replay(settings: &DeviceSettings) -> DeviceSettings {
    DeviceSettings {
        center_hz: settings.center_hz,
        sample_rate: settings.sample_rate,
        antenna: settings.antenna.clone(),
        gains: settings.gains.clone(),
        extra: settings.extra.clone(),
        ..DeviceSettings::default()
    }
}
