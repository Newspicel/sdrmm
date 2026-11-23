use sdrmm_wire::{DeviceBackend, DeviceInfo};

pub const DRIVER_ID: DeviceBackend = DeviceBackend::new("tektronix");

/// One analyser the API's search lists. The id only lives until the next search.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Found {
    pub id: i32,
    pub serial: String,
    pub model: String,
}

impl Found {
    #[must_use]
    pub fn describe(&self) -> DeviceInfo {
        DeviceInfo {
            driver: DRIVER_ID,
            key: self.serial.clone(),
            label: format!("Tektronix {} {}", self.model, self.serial),
            serial: Some(self.serial.clone()),
            profile: None,
        }
    }
}

/// What the connected analyser tunes and how wide its IQ stream goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub center_min: f64,
    pub center_max: f64,
    pub bandwidth_min: f64,
    pub bandwidth_max: f64,
}

/// What the connected analyser has beyond the tuner every model shares.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Fitted {
    pub preamp: bool,
    pub attenuator: bool,
    pub gnss: bool,
}

/// Where the analyser takes its frequency reference from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrequencySource {
    Internal,
    External,
    Gnss,
}

impl FrequencySource {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::External => "external",
            Self::Gnss => "gnss",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Internal => "Internal",
            Self::External => "External 10 MHz",
            Self::Gnss => "GNSS",
        }
    }

    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        [Self::Internal, Self::External, Self::Gnss]
            .into_iter()
            .find(|source| source.name() == name)
    }
}

/// One block of the IQ stream, already copied into the caller's buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Chunk {
    pub pairs: usize,
    pub timestamp: u64,
    pub status: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_found_analyser_is_listed_by_serial_with_its_model_in_the_label() {
        let found = Found {
            id: 0,
            serial: "B010123".to_string(),
            model: "RSA306".to_string(),
        };
        let info = found.describe();
        assert_eq!(info.key, "B010123");
        assert_eq!(info.label, "Tektronix RSA306 B010123");
        assert_eq!(info.serial.as_deref(), Some("B010123"));
    }

    #[test]
    fn a_frequency_source_is_found_again_by_its_name() {
        for source in [
            FrequencySource::Internal,
            FrequencySource::External,
            FrequencySource::Gnss,
        ] {
            assert_eq!(FrequencySource::named(source.name()), Some(source));
        }
        assert_eq!(FrequencySource::named("rubidium"), None);
    }
}
