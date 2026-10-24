use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    AeroChannel, AisChannel, AprsMode, AtvColor, AtvModulation, AtvStandard, ChannelParams,
    DabMode, DabTransmissionMode, DatvCodeRate, DatvRollOff, DatvStandard, DectBand, DectSides,
    DectSpan, DmrSlots, DrmMode, DvbtBandwidth, DvbtStandard, IlsComponent, IridiumSpan,
    NfmScramblerMode, NfmToneMode, NxdnBandwidth, PocsagBaud, PskBaud, RadioClockStandard,
    RttyStopBits, SelcallSystem, Sideband, SstvMode, SstvModulation,
};
use crate::{
    airtime::AirtimeSpan,
    ble::BleLink,
    lora::{LoraBandwidth, LoraCodingRate, LoraIq, LoraProtocol, LoraSpreadingFactor},
    measurement::PerBaud,
    remote_id::RemoteIdLink,
    tetrapol::TetrapolBand,
    weather::{LrptMode, SondeType, WefaxIoc, WefaxLpm},
};

pub const CTCSS_TONES_HZ: [f64; 50] = [
    67.0, 69.3, 71.9, 74.4, 77.0, 79.7, 82.5, 85.4, 88.5, 91.5, 94.8, 97.4, 100.0, 103.5, 107.2,
    110.9, 114.8, 118.8, 123.0, 127.3, 131.8, 136.5, 141.3, 146.2, 151.4, 156.7, 159.8, 162.2,
    165.5, 167.9, 171.3, 173.8, 177.3, 179.9, 183.5, 186.2, 189.9, 192.8, 196.6, 199.5, 203.5,
    206.5, 210.7, 218.1, 225.7, 229.1, 233.6, 241.8, 250.3, 254.1,
];

pub const DCS_CODES: [u16; 83] = [
    23, 25, 26, 31, 32, 43, 47, 51, 54, 65, 71, 72, 73, 74, 114, 115, 116, 125, 131, 132, 134, 143,
    152, 155, 156, 162, 165, 172, 174, 205, 223, 226, 243, 244, 245, 251, 261, 263, 265, 271, 306,
    311, 315, 331, 343, 346, 351, 364, 365, 371, 411, 412, 413, 423, 431, 432, 445, 464, 465, 466,
    503, 506, 516, 532, 546, 565, 606, 612, 624, 627, 631, 632, 654, 662, 664, 703, 712, 723, 731,
    732, 734, 743, 754,
];

pub const DEEMPHASIS_US: [f32; 2] = [50.0, 75.0];
pub const RTTY_BAUDS: [f64; 3] = [45.45, 50.0, 75.0];
pub const RTTY_SHIFTS_HZ: [f64; 3] = [170.0, 450.0, 850.0];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ParamChoice {
    pub name: String,
    pub options: Vec<ParamOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ParamOption {
    pub value: serde_json::Value,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

pub trait SettingChoice: Copy + Serialize + 'static {
    fn all() -> &'static [Self];
    fn choice_label(self) -> &'static str;
    fn choice_title(self) -> Option<&'static str> {
        None
    }
}

fn option(value: serde_json::Value, label: String, title: Option<&str>) -> ParamOption {
    ParamOption {
        value,
        label,
        title: title.map(str::to_owned),
    }
}

fn of<T: SettingChoice>(value: T) -> ParamOption {
    option(
        serde_json::to_value(value).unwrap_or_default(),
        value.choice_label().to_owned(),
        value.choice_title(),
    )
}

fn choice<T: SettingChoice>(name: &str) -> ParamChoice {
    ParamChoice {
        name: name.to_owned(),
        options: T::all().iter().copied().map(of).collect(),
    }
}

fn optional<T: SettingChoice>(name: &str, none: &str) -> ParamChoice {
    let mut options = vec![option(serde_json::Value::Null, none.to_owned(), None)];
    options.extend(T::all().iter().copied().map(of));
    ParamChoice {
        name: name.to_owned(),
        options,
    }
}

fn numbers<T: Copy + Serialize + Into<f64>>(
    name: &str,
    values: &[T],
    label: impl Fn(f64) -> String,
) -> ParamChoice {
    ParamChoice {
        name: name.to_owned(),
        options: values
            .iter()
            .map(|value| {
                option(
                    serde_json::to_value(value).unwrap_or_default(),
                    label((*value).into()),
                    None,
                )
            })
            .collect(),
    }
}

fn plain(value: f64) -> String {
    format!("{value}")
}

impl ChannelParams {
    #[must_use]
    pub fn choices(&self) -> Vec<ParamChoice> {
        match self {
            Self::Nfm(_) => nfm_choices(),
            Self::Selcall(_) => vec![choice::<SelcallSystem>("system")],
            Self::Ssb(_) | Self::Freedv(_) => vec![choice::<Sideband>("sideband")],
            Self::Wfm(_) => vec![numbers("deemphasis_us", &DEEMPHASIS_US, |us| {
                format!("{us} µs")
            })],
            Self::Pocsag(_) => vec![choice::<PocsagBaud>("baud")],
            Self::Tetrapol(_) => vec![choice::<TetrapolBand>("band")],
            Self::Ais(_) => vec![choice::<AisChannel>("ais_channel")],
            Self::InmarsatAero(_) => vec![choice::<AeroChannel>("channel")],
            Self::Iridium(_) => vec![choice::<IridiumSpan>("span")],
            Self::Aprs(_) => vec![choice::<AprsMode>("mode")],
            Self::Rtty(_) => rtty_choices(),
            Self::Psk(_) => vec![choice::<PskBaud>("baud")],
            Self::RadioClock(_) => vec![choice::<RadioClockStandard>("standard")],
            Self::Ils(_) => vec![choice::<IlsComponent>("component")],
            Self::Atv(_) => vec![
                choice::<AtvModulation>("modulation"),
                choice::<AtvStandard>("standard"),
                choice::<AtvColor>("color"),
            ],
            Self::Sstv(_) => vec![
                choice::<SstvModulation>("modulation"),
                optional::<SstvMode>("mode", "Follow VIS"),
            ],
            Self::Dab(_) => vec![
                choice::<DabMode>("mode"),
                choice::<DabTransmissionMode>("transmission_mode"),
            ],
            Self::Datv(_) => vec![
                choice::<DatvStandard>("standard"),
                choice::<DatvRollOff>("roll_off"),
                choice::<DatvCodeRate>("code_rate"),
            ],
            Self::Dvbt(_) => vec![
                choice::<DvbtStandard>("standard"),
                choice::<DvbtBandwidth>("bandwidth"),
            ],
            Self::Drm(_) => vec![choice::<DrmMode>("mode")],
            Self::Dmr(_) => vec![choice::<DmrSlots>("slots")],
            Self::Nxdn(_) => vec![choice::<NxdnBandwidth>("bandwidth")],
            Self::Dect(_) => vec![
                choice::<DectBand>("band"),
                choice::<DectSpan>("span"),
                choice::<DectSides>("sides"),
            ],
            Self::Lrpt(_) => vec![choice::<LrptMode>("mode")],
            Self::Wefax(_) => vec![choice::<WefaxIoc>("ioc"), choice::<WefaxLpm>("lpm")],
            Self::Radiosonde(_) => vec![optional::<SondeType>("sonde", "Auto")],
            Self::Lora(_) => lora_choices(),
            Self::RemoteId(_) => vec![choice::<RemoteIdLink>("link")],
            Self::Ble(_) => vec![choice::<BleLink>("link")],
            Self::WifiOccupancy(_) | Self::IsmSurvey(_) => vec![choice::<AirtimeSpan>("span")],
            Self::Per(_) => vec![choice::<PerBaud>("baud")],
            Self::Am(_)
            | Self::Flex(_)
            | Self::Ermes(_)
            | Self::Eot(_)
            | Self::Same(_)
            | Self::Sarsat(_)
            | Self::Adsb(_)
            | Self::Morse(_)
            | Self::CwSkimmer(_)
            | Self::Navtex(_)
            | Self::Acars(_)
            | Self::Dstar(_)
            | Self::Ysf(_)
            | Self::P25(_)
            | Self::Dpmr(_)
            | Self::M17(_)
            | Self::Tetra(_)
            | Self::Ft8(_)
            | Self::Ft4(_)
            | Self::Wspr(_)
            | Self::Ident(_)
            | Self::Gnss(_)
            | Self::Vor(_)
            | Self::Dsc(_)
            | Self::InmarsatStdc(_)
            | Self::Vdl2(_)
            | Self::Hfdl(_)
            | Self::Gsm(_)
            | Self::Apt(_)
            | Self::Ism(_)
            | Self::Wmbus(_)
            | Self::NoiseFigure(_)
            | Self::Sid(_)
            | Self::Stanag(_) => Vec::new(),
        }
    }
}

fn nfm_choices() -> Vec<ParamChoice> {
    vec![
        choice::<NfmToneMode>("tone_mode"),
        numbers("ctcss_hz", &CTCSS_TONES_HZ, |hz| format!("{hz:.1} Hz")),
        numbers("dcs_code", &DCS_CODES, |code| format!("{code:03}")),
        choice::<NfmScramblerMode>("scrambler_mode"),
    ]
}

fn rtty_choices() -> Vec<ParamChoice> {
    vec![
        numbers("baud", &RTTY_BAUDS, plain),
        numbers("shift_hz", &RTTY_SHIFTS_HZ, plain),
        choice::<RttyStopBits>("stop_bits"),
    ]
}

fn lora_choices() -> Vec<ParamChoice> {
    vec![
        choice::<LoraBandwidth>("bandwidth"),
        choice::<LoraSpreadingFactor>("spreading_factor"),
        choice::<LoraIq>("iq"),
        choice::<LoraProtocol>("protocol"),
        choice::<LoraCodingRate>("implicit_header.coding_rate"),
    ]
}

macro_rules! labelled {
    ($($ty:ty { $($variant:ident => $label:literal $(: $title:literal)?),+ $(,)? })+) => {
        $(
            impl SettingChoice for $ty {
                fn all() -> &'static [Self] {
                    &[$(<$ty>::$variant),+]
                }

                fn choice_label(self) -> &'static str {
                    match self {
                        $(<$ty>::$variant => $label),+
                    }
                }

                fn choice_title(self) -> Option<&'static str> {
                    match self {
                        $(<$ty>::$variant => labelled!(@title $($title)?)),+
                    }
                }
            }
        )+
    };
    (@title $title:literal) => { Some($title) };
    (@title) => { None };
}

macro_rules! named {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl SettingChoice for $ty {
                fn all() -> &'static [Self] {
                    &<$ty>::ALL
                }

                fn choice_label(self) -> &'static str {
                    self.label()
                }
            }
        )+
    };
}

named!(
    SelcallSystem,
    Sideband,
    PskBaud,
    RadioClockStandard,
    SstvMode,
    LrptMode,
    SondeType,
);

labelled! {
    NfmToneMode { Off => "off", Detect => "Detect", Ctcss => "CTCSS", Dcs => "DCS" }
    NfmScramblerMode { Off => "off", Inversion => "Inversion", Auto => "Auto" }
    PocsagBaud { Auto => "Auto", B512 => "512", B1200 => "1200", B2400 => "2400" }
    TetrapolBand { Uhf => "UHF", Vhf => "VHF": "70 to 87 MHz systems" }
    AisChannel { A => "A", B => "B" }
    AeroChannel {
        P => "P": "Forward channel to aircraft",
        Burst => "R/T": "Bursts from aircraft",
        C => "C": "Voice circuit",
    }
    IridiumSpan {
        Channel => "50 kHz": "One channel",
        Mhz1 => "1 MHz": "Radio at 1 MS/s, decodes the middle 800 kHz",
        Mhz2_5 => "2.5 MHz": "Radio at 2.5 MS/s, decodes the middle 2 MHz",
        Mhz5 => "5 MHz": "Radio at 5 MS/s, decodes the middle 4 MHz",
        Mhz10 => "10 MHz": "Radio at 10 MS/s, decodes the middle 8 MHz",
    }
    AprsMode { Afsk1200 => "AFSK 1200", G3ruh9600 => "G3RUH 9600" }
    RttyStopBits { One => "1", OneAndHalf => "1.5", Two => "2" }
    IlsComponent { Localizer => "Localizer", Glideslope => "Glideslope" }
    AtvModulation { Am => "AM", Fm => "FM" }
    AtvStandard { Ccir625 => "625 / 25", Eia525 => "525 / 30", SystemA405 => "405 / 25" }
    AtvColor { Monochrome => "Mono", Pal => "PAL", Ntsc => "NTSC" }
    SstvModulation { Usb => "USB", Lsb => "LSB", Fm => "FM", Am => "AM" }
    DabMode { Auto => "Auto", Dab => "DAB", DabPlus => "DAB+" }
    DabTransmissionMode { Auto => "Auto", I => "I", Ii => "II", Iii => "III", Iv => "IV" }
    DatvStandard { DvbS => "DVB-S", DvbS2 => "DVB-S2" }
    DatvRollOff {
        Pct35 => "0.35", Pct25 => "0.25", Pct20 => "0.20", Pct15 => "0.15", Pct10 => "0.10",
        Pct5 => "0.05",
    }
    DvbtStandard { DvbT => "DVB-T", DvbT2 => "DVB-T2" }
    DvbtBandwidth {
        Khz250 => "250 kHz", Khz333 => "333 kHz", Khz500 => "500 kHz", Mhz1 => "1 MHz",
        Mhz1_7 => "1.7 MHz", Mhz2 => "2 MHz", Mhz5 => "5 MHz", Mhz6 => "6 MHz", Mhz7 => "7 MHz",
        Mhz8 => "8 MHz", Mhz10 => "10 MHz",
    }
    DrmMode { Auto => "Auto", Drm30 => "DRM30", DrmPlus => "DRM+" }
    DmrSlots { Both => "Both", One => "TS1", Two => "TS2" }
    NxdnBandwidth { Narrow => "6.25", Wide => "12.5" }
    DectBand { Eu => "EU", Us => "US" }
    DectSpan {
        Carrier => "Carrier": "One carrier at 2.304 MS/s",
        Band => "Band":
            "Every carrier at once. Tune to the band centre, radio at 20 MS/s (EU) or 10 MS/s (US)",
    }
    DectSides { Both => "Both", Rfp => "Base", Pp => "Handset" }
    WefaxIoc { Ioc576 => "576", Ioc288 => "288" }
    WefaxLpm { Lpm60 => "60", Lpm90 => "90", Lpm120 => "120", Lpm240 => "240" }
    BleLink {
        Channel => "Channel": "One Bluetooth channel at 4 MS/s",
        Band => "Band": "Every Bluetooth channel in a 20 MS/s window",
    }
    AirtimeSpan {
        Mhz20 => "20": "20 MS/s",
        Mhz40 => "40": "40 MS/s",
        Mhz80 => "80": "80 MS/s, all of 2.4 GHz",
    }
    PerBaud { Baud1200 => "1200", Baud2400 => "2400", Baud4800 => "4800", Baud9600 => "9600" }
    RemoteIdLink {
        Bluetooth => "Bluetooth": "One Bluetooth channel at 4 MS/s",
        BluetoothBand => "BT band": "Every Bluetooth channel in a 20 MS/s window, for long range",
        Wifi => "Wi-Fi": "One 20 MHz Wi-Fi channel",
    }
    LoraBandwidth {
        Khz7_8 => "7.8 kHz", Khz10_4 => "10.4 kHz", Khz15_6 => "15.6 kHz", Khz20_8 => "20.8 kHz",
        Khz31_25 => "31.25 kHz", Khz41_7 => "41.7 kHz", Khz62_5 => "62.5 kHz",
        Khz125 => "125 kHz", Khz250 => "250 kHz", Khz500 => "500 kHz",
    }
    LoraSpreadingFactor {
        All => "All": "Search SF7 to SF12 at once",
        Sf7 => "SF7", Sf8 => "SF8", Sf9 => "SF9", Sf10 => "SF10", Sf11 => "SF11", Sf12 => "SF12",
    }
    LoraIq {
        Normal => "Normal",
        Inverted => "Inverted": "LoRaWAN downlinks",
        Both => "Both",
    }
}

impl SettingChoice for DatvCodeRate {
    fn all() -> &'static [Self] {
        &[
            Self::Auto,
            Self::Half,
            Self::TwoThirds,
            Self::ThreeQuarters,
            Self::FiveSixths,
            Self::SevenEighths,
        ]
    }

    fn choice_label(self) -> &'static str {
        self.label()
    }
}

impl SettingChoice for LoraCodingRate {
    fn all() -> &'static [Self] {
        &[Self::Cr45, Self::Cr46, Self::Cr47, Self::Cr48]
    }

    fn choice_label(self) -> &'static str {
        self.label()
    }
}

impl SettingChoice for LoraProtocol {
    fn all() -> &'static [Self] {
        &[
            Self::Auto,
            Self::Raw,
            Self::Lorawan,
            Self::Meshtastic,
            Self::Meshcore,
        ]
    }

    fn choice_label(self) -> &'static str {
        self.label()
    }

    fn choice_title(self) -> Option<&'static str> {
        (self == Self::Raw).then_some("Bytes only")
    }
}

#[cfg(test)]
mod tests;
