use sdrmm_wire::{DeviceBackend, DeviceInfo};

pub const DRIVER_ID: DeviceBackend = DeviceBackend::Aaronia;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    V6,
    V6Eco,
}

impl Kind {
    pub const ALL: [Self; 2] = [Self::V6, Self::V6Eco];

    #[must_use]
    pub const fn type_name(self) -> &'static str {
        match self {
            Self::V6 => "spectranv6",
            Self::V6Eco => "spectranv6eco",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::V6 => "SPECTRAN V6",
            Self::V6Eco => "SPECTRAN V6 ECO",
        }
    }

    /// The clock an ECO runs at, which it does not let the host choose.
    #[must_use]
    pub const fn fixed_clock_hz(self) -> Option<f64> {
        match self {
            Self::V6 => None,
            Self::V6Eco => Some(61.44e6),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Mode {
    Receiver,
    Transmitter,
}

impl Mode {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Receiver => "iqreceiver",
            Self::Transmitter => "iqtransmitter",
        }
    }

    #[must_use]
    pub fn device_type(self, kind: Kind) -> String {
        format!("{}/{}", kind.type_name(), self.name())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Found {
    pub kind: Kind,
    pub serial: String,
    pub ready: bool,
    pub active: bool,
}

impl Found {
    #[must_use]
    pub fn describe(&self) -> DeviceInfo {
        DeviceInfo {
            driver: DRIVER_ID,
            key: self.serial.clone(),
            label: format!("{} {}", self.kind.label(), self.serial),
            serial: Some(self.serial.clone()),
            profile: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemKind {
    Number,
    Bool,
    Enum,
    String,
    Other,
}

/// What the device's configuration tree says one setting accepts.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub options: Vec<String>,
    pub disabled: u64,
}

impl Item {
    #[must_use]
    pub fn number(min: f64, max: f64, step: f64) -> Self {
        Self {
            kind: ItemKind::Number,
            min,
            max,
            step,
            options: Vec::new(),
            disabled: 0,
        }
    }

    #[must_use]
    pub fn choice(options: &[&str]) -> Self {
        Self {
            kind: ItemKind::Enum,
            min: 0.0,
            max: 0.0,
            step: 0.0,
            options: options.iter().map(ToString::to_string).collect(),
            disabled: 0,
        }
    }

    /// The options the device offers right now, without those it marks disabled.
    #[must_use]
    pub fn enabled_options(&self) -> Vec<&str> {
        self.options
            .iter()
            .enumerate()
            .filter(|(index, _)| *index >= 64 || self.disabled & (1 << index) == 0)
            .map(|(_, option)| option.as_str())
            .collect()
    }

    #[must_use]
    pub fn has_range(&self) -> bool {
        self.kind == ItemKind::Number && self.max > self.min
    }
}

/// One received packet as the API lends it: the samples stay valid until it is consumed.
#[derive(Clone, Copy, Debug)]
pub struct PacketView {
    pub flags: u64,
    pub start_time: f64,
    pub end_time: f64,
    pub step_hz: f64,
    pub num: i64,
    pub size: i64,
    pub stride: i64,
    pub data: *const f32,
}

unsafe impl Send for PacketView {}

#[derive(Clone, Copy, Debug)]
pub struct Outgoing<'a> {
    pub flags: u64,
    pub start_time: f64,
    pub end_time: f64,
    pub start_hz: f64,
    pub step_hz: f64,
    pub iq: &'a [f32],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_type_joins_the_family_and_the_mode() {
        assert_eq!(
            Mode::Receiver.device_type(Kind::V6),
            "spectranv6/iqreceiver"
        );
        assert_eq!(
            Mode::Transmitter.device_type(Kind::V6Eco),
            "spectranv6eco/iqtransmitter"
        );
    }

    #[test]
    fn disabled_options_are_left_off_the_menu() {
        let mut item = Item::choice(&["46MHz", "92MHz", "245MHz"]);
        item.disabled = 0b100;
        assert_eq!(item.enabled_options(), vec!["46MHz", "92MHz"]);
    }

    #[test]
    fn a_found_radio_is_listed_by_serial_with_its_family_in_the_label() {
        let found = Found {
            kind: Kind::V6Eco,
            serial: "123456".to_string(),
            ready: true,
            active: false,
        };
        let info = found.describe();
        assert_eq!(info.key, "123456");
        assert_eq!(info.label, "SPECTRAN V6 ECO 123456");
        assert_eq!(info.serial.as_deref(), Some("123456"));
    }
}
