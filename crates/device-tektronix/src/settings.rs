use sdrmm_device::{DeviceError, check_stream_settings};
use sdrmm_wire::{
    Capabilities, DeviceSettings, ExtraSetting, ExtraValue, GainValue, any_range_holds,
};

use crate::{
    api::Rsa,
    caps::{
        ATTENUATOR, AUTO_ATTENUATION, FREQUENCY_SOURCE, GAIN, INPUT, PREAMP, bandwidth_for,
        reference_for_gain,
    },
    link::{Link, Pause},
    model::{FrequencySource, Limits},
};

const RATE_MATCH_HZ: f64 = 1.0;

/// Refuses what the analyser cannot take before anything reaches it.
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
        && !caps
            .sample_rates
            .iter()
            .any(|offered| (offered - rate).abs() < RATE_MATCH_HZ)
    {
        return Err(DeviceError::Unsupported(format!(
            "sample_rate {rate} is not one of the IQ stream's rates"
        )));
    }
    refuse_absent(delta)?;
    if let Some(antenna) = &delta.antenna
        && antenna != INPUT
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

fn validate_extra(extra: &ExtraValue, caps: &Capabilities) -> Result<(), DeviceError> {
    let Some(known) = caps.extra.iter().find(|known| known.name() == extra.name) else {
        return Err(DeviceError::Unsupported(format!(
            "{}: not a setting of this analyser",
            extra.name
        )));
    };
    let offered = match known {
        ExtraSetting::Bool { .. } => extra.value.as_bool().is_some(),
        ExtraSetting::Enum { options, .. } => extra
            .value
            .as_str()
            .is_some_and(|value| options.iter().any(|option| option.value == value)),
        ExtraSetting::Range { .. } | ExtraSetting::String { .. } => false,
    };
    if !offered {
        return Err(DeviceError::Unsupported(format!(
            "{}: {} is not offered",
            extra.name, extra.value
        )));
    }
    Ok(())
}

fn refuse_absent(delta: &DeviceSettings) -> Result<(), DeviceError> {
    let refused = if delta.bandwidth.is_some() {
        Some("bandwidth: the rate sets the filter")
    } else if delta.bias_tee == Some(true) {
        Some("bias_tee: the analyser has none")
    } else if delta.ppm.is_some() {
        Some("ppm: the analyser corrects its own clock")
    } else if delta.agc.as_ref().is_some_and(|agc| agc.on) {
        Some("agc: the analyser has none")
    } else {
        None
    };
    refused.map_or(Ok(()), |reason| {
        Err(DeviceError::Unsupported(reason.to_string()))
    })
}

/// What of `delta` differs from `current`, so a setting sent again does not pause the stream.
#[must_use]
pub fn changes(delta: &DeviceSettings, current: &DeviceSettings) -> DeviceSettings {
    let differs = |wanted: Option<f64>, held: Option<f64>| wanted.filter(|_| wanted != held);
    DeviceSettings {
        center_hz: differs(delta.center_hz, current.center_hz),
        sample_rate: differs(delta.sample_rate, current.sample_rate),
        gains: delta
            .gains
            .iter()
            .filter(|gain| current.gain(&gain.stage) != Some(gain.value_db))
            .cloned()
            .collect(),
        extra: delta
            .extra
            .iter()
            .filter(|extra| !current.extra.contains(extra))
            .cloned()
            .collect(),
        ..DeviceSettings::default()
    }
}

fn extra<'a>(delta: &'a DeviceSettings, name: &str) -> Option<&'a ExtraValue> {
    delta.extra.iter().find(|extra| extra.name == name)
}

fn switch(delta: &DeviceSettings, name: &str) -> Option<bool> {
    extra(delta, name).and_then(|extra| extra.value.as_bool())
}

/// What a delta asks of the analyser, in the API's own terms.
struct Wanted {
    center: Option<f64>,
    rate: Option<f64>,
    reference: Option<f64>,
    preamp: Option<bool>,
    auto_attenuation: Option<bool>,
    attenuation: Option<f64>,
    source: Option<FrequencySource>,
}

impl Wanted {
    fn of(delta: &DeviceSettings) -> Self {
        Self {
            center: delta.center_hz,
            rate: delta.sample_rate,
            reference: delta.gain(GAIN).map(reference_for_gain),
            preamp: switch(delta, PREAMP),
            auto_attenuation: switch(delta, AUTO_ATTENUATION),
            attenuation: delta.gain(ATTENUATOR),
            source: extra(delta, FREQUENCY_SOURCE)
                .and_then(|extra| extra.value.as_str())
                .and_then(FrequencySource::named),
        }
    }

    fn pause(&self) -> Option<Pause> {
        let rebuilds = self.rate.is_some()
            || self.reference.is_some()
            || self.preamp.is_some()
            || self.auto_attenuation.is_some()
            || self.attenuation.is_some()
            || self.source.is_some();
        if rebuilds {
            Some(Pause::Stream)
        } else {
            self.center.map(|_| Pause::Acquisition)
        }
    }

    /// The attenuator mode goes first: the API only takes a manual value with the mode off.
    fn write(&self, api: &dyn Rsa, limits: &Limits) -> Result<(), DeviceError> {
        if let Some(hz) = self.center {
            api.set_center(hz)?;
        }
        if let Some(dbm) = self.reference {
            api.set_reference(dbm)?;
        }
        if let Some(on) = self.preamp {
            api.set_preamp(on)?;
        }
        if let Some(on) = self.auto_attenuation {
            api.set_auto_attenuation(on)?;
        }
        if let Some(db) = self.attenuation {
            api.set_attenuation(db)?;
        }
        if let Some(source) = self.source {
            api.set_frequency_source(source)?;
        }
        if let Some(rate) = self.rate {
            api.set_bandwidth(bandwidth_for(rate, limits))?;
        }
        Ok(())
    }
}

/// Writes what `delta` carries, pausing a running stream as far as each setting needs.
pub fn push(link: &Link, delta: &DeviceSettings) -> Result<(), DeviceError> {
    let wanted = Wanted::of(delta);
    let Some(pause) = wanted.pause() else {
        return Ok(());
    };
    link.paused(pause, |api| wanted.write(api, &link.limits))
}

fn gain(stage: &str, value_db: f64) -> GainValue {
    GainValue {
        stage: stage.to_string(),
        value_db,
    }
}

fn held(name: &str, value: impl Into<serde_json::Value>) -> ExtraValue {
    ExtraValue {
        name: name.to_string(),
        value: value.into(),
    }
}

/// What the analyser actually runs at, read back after the API had its say. With the attenuator
/// on automatic this is the value the analyser picked.
pub fn read_back(link: &Link) -> Result<DeviceSettings, DeviceError> {
    let api = link.api.as_ref();
    let mut gains = vec![gain(GAIN, -api.reference()?)];
    let mut extra = vec![held(FREQUENCY_SOURCE, api.frequency_source()?.name())];
    if link.fitted.preamp {
        extra.push(held(PREAMP, api.preamp()?));
    }
    if link.fitted.attenuator {
        extra.push(held(AUTO_ATTENUATION, api.auto_attenuation()?));
        gains.push(gain(ATTENUATOR, api.attenuation()?));
    }
    Ok(DeviceSettings {
        center_hz: Some(api.center()?),
        sample_rate: Some(api.rate()?),
        gains,
        extra,
        ..DeviceSettings::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_moved_is_sent_again() {
        let current = DeviceSettings {
            center_hz: Some(100e6),
            sample_rate: Some(3.5e6),
            gains: vec![GainValue {
                stage: GAIN.to_string(),
                value_db: 30.0,
            }],
            ..DeviceSettings::default()
        };
        let same = changes(&current, &current);
        assert_eq!(same, DeviceSettings::default());
        let moved = changes(
            &DeviceSettings {
                center_hz: Some(101e6),
                ..current.clone()
            },
            &current,
        );
        assert_eq!(moved.center_hz, Some(101e6));
        assert_eq!(moved.sample_rate, None);
        assert!(moved.gains.is_empty());
    }
}
