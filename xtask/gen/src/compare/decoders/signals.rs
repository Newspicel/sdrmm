use sdrmm_wire::DecoderEvent;

use super::{
    convert::{Audio, Container, Demod, Format},
    noise::Ladder,
    parse,
    programs::{
        ACARSDEC, AIS_CATCHER, DIREWOLF, DSD_FME, DUMP978, DUMP1090, FT8_LIB, MINIMODEM, MULTIMON,
        Program, READSB, REDSEA, RTL_433,
    },
};

pub struct Signal {
    pub id: &'static str,
    pub name: &'static str,
    /// Captures played one after another, a short gap apart.
    pub fixtures: &'static [&'static str],
    pub channel: &'static str,
    /// The channel's settings as JSON; fields left out keep their defaults.
    pub settings: &'static str,
    pub offset_hz: f64,
    pub speed_seconds: f64,
    pub tail_seconds: f64,
    pub cu8_rate: Option<f64>,
    pub unique: bool,
    pub decodes: Show,
    pub speed: Show,
    pub noise: Ladder,
    pub key: Key,
    pub references: &'static [Reference],
}

pub enum Key {
    /// One key from an event, or none.
    One(fn(&DecoderEvent) -> Option<String>),
    /// Several keys from one event.
    Many(fn(&DecoderEvent) -> Vec<String>),
    /// Text spread over events, one key per word.
    Words(fn(&DecoderEvent) -> Option<String>),
}

impl Key {
    pub fn read(&self, event: &DecoderEvent, keys: &mut Vec<String>) {
        match self {
            Self::One(key) | Self::Words(key) => keys.extend(key(event)),
            Self::Many(key) => keys.extend(key(event)),
        }
    }

    pub fn finish(&self, keys: Vec<String>) -> Vec<String> {
        match self {
            Self::Words(_) => parse::words(&keys.concat()),
            Self::One(_) | Self::Many(_) => keys,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Show {
    Hidden,
    Plain,
    Noted(&'static str),
}

pub struct Reference {
    pub program: &'static Program,
    pub format: Format,
    pub args: &'static [&'static str],
    pub exit_codes: &'static [i32],
    pub tail: bool,
    pub keys: fn(&str) -> Vec<String>,
}

impl Signal {
    pub fn loops(&self, fixture_seconds: f64) -> usize {
        ((self.speed_seconds / fixture_seconds).ceil() as usize).max(1)
    }
}

pub const INPUT: &str = "{input}";

const DEFAULTS: &str = "{}";

/// The RTTY pair heard as audio: mark 1585 Hz, space 1415 Hz.
const RTTY_CENTRE_HZ: f64 = 1_500.0;

const MODE_S: Format = Format::Cu8 { rate: 2_400_000.0 };

const UAT_RATE: f64 = 2_083_334.0;

const ISM_RATE: f64 = 250_000.0;

/// The test captures of `rtl_433`, all those at 250 kHz.
const ISM_CAPTURES: &[&str] = &[
    "ism/acurite_5n1_433m92_250k",
    "ism/acurite_609txc_433m92_250k",
    "ism/acurite_tower_433m92_250k",
    "ism/ambient_f007th_433m92_250k",
    "ism/ambient_wh31e_915m_250k",
    "ism/bresser_5in1_868m3_250k",
    "ism/ev1527_remote_433m92_250k",
    "ism/fineoffset_wh1080_433m92_250k",
    "ism/fineoffset_wh2_433m92_250k",
    "ism/generic_remote_433m92_250k",
    "ism/honeywell_345m_250k",
    "ism/lacrosse_tx141_433m92_250k",
    "ism/lacrosse_tx29_tx35_868m2_250k",
    "ism/nexus_th_433m92_250k",
    "ism/oregon_thgr122n_433m92_250k",
    "ism/oregon_thn132n_433m92_250k",
    "ism/oregon_wgr800_433m92_250k",
    "ism/schrader_tpms_315m_250k",
    "ism/thermopro_tp12_433m92_250k",
];

const fn fm(deviation_hz: f64, rate: u32, container: Container) -> Format {
    Format::Audio(Audio {
        demod: Demod::Fm { deviation_hz },
        rate,
        container,
    })
}

const fn usb(tone_hz: f64, rate: u32, container: Container) -> Format {
    Format::Audio(Audio {
        demod: Demod::Usb { tone_hz },
        rate,
        container,
    })
}

pub const SIGNALS: &[Signal] = &[
    Signal {
        id: "adsb",
        name: "ADS-B",
        fixtures: &["adsb_offair_2m"],
        channel: "adsb",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 20.0,
        tail_seconds: 0.01,
        cu8_rate: Some(2_400_000.0),
        unique: true,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 20.0,
            low_db: -5.0,
        },
        key: Key::One(adsb),
        references: &[
            Reference {
                program: &DUMP1090,
                format: MODE_S,
                args: &[
                    "--device-type",
                    "ifile",
                    "--ifile",
                    INPUT,
                    "--iformat",
                    "UC8",
                    "--raw",
                ],
                exit_codes: &[0, 1],
                tail: true,
                keys: parse::mode_s,
            },
            Reference {
                program: &READSB,
                format: MODE_S,
                args: &[
                    "--device-type",
                    "ifile",
                    "--ifile",
                    INPUT,
                    "--iformat",
                    "UC8",
                    "--raw",
                ],
                exit_codes: &[0, 1],
                tail: true,
                keys: parse::mode_s,
            },
        ],
    },
    Signal {
        id: "ais",
        name: "AIS",
        fixtures: &["ais_position_240k"],
        channel: "ais",
        settings: DEFAULTS,
        offset_hz: 25_000.0,
        speed_seconds: 30.0,
        tail_seconds: 0.1,
        cu8_rate: None,
        unique: true,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: -2.0,
            low_db: -12.0,
        },
        key: Key::One(ais),
        references: &[Reference {
            program: &AIS_CATCHER,
            format: Format::Cf32,
            args: &["-r", "CF32", INPUT, "-s", "240000", "-n"],
            exit_codes: &[0],
            tail: true,
            keys: parse::nmea_payloads,
        }],
    },
    Signal {
        id: "aprs",
        name: "APRS",
        fixtures: &["aprs_afsk1200_240k"],
        channel: "aprs",
        settings: DEFAULTS,
        offset_hz: -40_000.0,
        speed_seconds: 300.0,
        tail_seconds: 0.2,
        cu8_rate: None,
        unique: true,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 5.0,
            low_db: -10.0,
        },
        key: Key::One(aprs),
        references: &[
            Reference {
                program: &DIREWOLF,
                format: fm(3_000.0, 48_000, Container::Wav),
                args: &[INPUT],
                exit_codes: &[0],
                tail: true,
                keys: parse::direwolf,
            },
            Reference {
                program: &MULTIMON,
                format: fm(3_000.0, 22_050, Container::Raw),
                args: &["-q", "-a", "AFSK1200", "-A", "-t", "raw", INPUT],
                exit_codes: &[0],
                tail: true,
                keys: parse::multimon_aprs,
            },
        ],
    },
    Signal {
        id: "pocsag",
        name: "POCSAG",
        fixtures: &["pocsag_1200_240k"],
        channel: "pocsag",
        settings: DEFAULTS,
        offset_hz: 50_000.0,
        speed_seconds: 300.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: true,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 5.0,
            low_db: -20.0,
        },
        key: Key::One(pocsag),
        references: &[Reference {
            program: &MULTIMON,
            format: fm(4_500.0, 22_050, Container::Raw),
            args: &["-q", "-a", "POCSAG1200", "-t", "raw", INPUT],
            exit_codes: &[0],
            tail: true,
            keys: parse::multimon_pocsag,
        }],
    },
    Signal {
        id: "flex",
        name: "FLEX",
        fixtures: &["flex_p2000_offair_48k"],
        channel: "flex",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 300.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: true,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 45.0,
            low_db: 20.0,
        },
        key: Key::One(flex),
        references: &[Reference {
            program: &MULTIMON,
            format: fm(4_800.0, 22_050, Container::Raw),
            args: &["-q", "-a", "FLEX_NEXT", "-t", "raw", INPUT],
            exit_codes: &[0],
            tail: true,
            keys: parse::multimon_flex,
        }],
    },
    Signal {
        id: "selcall",
        name: "Selcall",
        fixtures: &["selcall_ccir1_48k"],
        channel: "selcall",
        settings: DEFAULTS,
        offset_hz: 5_000.0,
        speed_seconds: 300.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: true,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 5.0,
            low_db: -10.0,
        },
        key: Key::One(selcall),
        references: &[Reference {
            program: &MULTIMON,
            format: fm(2_500.0, 22_050, Container::Raw),
            args: &["-q", "-a", "CCIR", "-t", "raw", INPUT],
            exit_codes: &[0],
            tail: true,
            keys: parse::multimon_ccir,
        }],
    },
    Signal {
        id: "acars",
        name: "ACARS",
        fixtures: &["acars_offair_48k"],
        channel: "acars",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 300.0,
        tail_seconds: 3.0,
        cu8_rate: None,
        unique: true,
        decodes: Show::Plain,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 30.0,
            low_db: 5.0,
        },
        key: Key::One(acars),
        references: &[Reference {
            program: &ACARSDEC,
            format: Format::Audio(Audio {
                demod: Demod::Am,
                rate: 12_500,
                container: Container::Wav,
            }),
            args: &["-o", "4", "-f", INPUT],
            exit_codes: &[0, 255],
            tail: true,
            keys: parse::acarsdec,
        }],
    },
    Signal {
        id: "ft8",
        name: "FT8",
        fixtures: &["ft8_20m_busy_12k"],
        channel: "ft8",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 0.0,
        tail_seconds: 4.0,
        cu8_rate: None,
        unique: true,
        decodes: Show::Noted("Two SDRmm decodes are unconfirmed."),
        speed: Show::Noted("SDRmm re-decodes overlapping windows."),
        noise: Ladder {
            high_db: 20.0,
            low_db: -5.0,
        },
        key: Key::One(ft8),
        references: &[Reference {
            program: &FT8_LIB,
            format: Format::Audio(Audio {
                demod: Demod::Real,
                rate: 12_000,
                container: Container::Wav,
            }),
            args: &[INPUT],
            exit_codes: &[0],
            tail: false,
            keys: parse::ft8_lib,
        }],
    },
    Signal {
        id: "dmr",
        name: "DMR",
        fixtures: &["dmr_call_48k"],
        channel: "dmr",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 30.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: false,
        decodes: Show::Noted("Counts frames carrying the call's addresses."),
        speed: Show::Plain,
        noise: Ladder {
            high_db: 20.0,
            low_db: 0.0,
        },
        key: Key::One(addresses),
        references: &[Reference {
            program: &DSD_FME,
            format: fm(2_400.0, 48_000, Container::Wav),
            args: &["-fs", "-i", INPUT, "-o", "null"],
            exit_codes: &[0],
            tail: true,
            keys: parse::dsd_fme,
        }],
    },
    Signal {
        id: "nxdn",
        name: "NXDN",
        fixtures: &["nxdn_addressed_48k"],
        channel: "nxdn",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 30.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: false,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 15.0,
            low_db: -10.0,
        },
        key: Key::One(addresses),
        references: &[Reference {
            program: &DSD_FME,
            format: fm(2_400.0, 48_000, Container::Wav),
            args: &["-fi", "-i", INPUT, "-o", "null"],
            exit_codes: &[0],
            tail: true,
            keys: parse::dsd_fme_nxdn,
        }],
    },
    Signal {
        id: "ysf",
        name: "YSF",
        fixtures: &["ysf_callsigns_48k"],
        channel: "ysf",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 30.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: false,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 25.0,
            low_db: 0.0,
        },
        key: Key::One(callsign),
        references: &[Reference {
            program: &DSD_FME,
            format: fm(2_400.0, 48_000, Container::Wav),
            args: &["-fy", "-i", INPUT, "-o", "null"],
            exit_codes: &[0],
            tail: true,
            keys: parse::dsd_fme_source,
        }],
    },
    Signal {
        id: "p25",
        name: "P25",
        fixtures: &["p25_calls_48k"],
        channel: "p25",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 30.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: false,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 20.0,
            low_db: 0.0,
        },
        key: Key::One(addresses),
        references: &[Reference {
            program: &DSD_FME,
            format: fm(1_944.0, 48_000, Container::Wav),
            args: &["-f1", "-i", INPUT, "-o", "null"],
            exit_codes: &[0],
            tail: true,
            keys: parse::dsd_fme_p25,
        }],
    },
    Signal {
        id: "m17",
        name: "M17",
        fixtures: &["m17_calls_48k"],
        channel: "m17",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 30.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: false,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 20.0,
            low_db: 0.0,
        },
        key: Key::One(callsign),
        references: &[Reference {
            program: &DSD_FME,
            format: fm(2_400.0, 48_000, Container::Wav),
            args: &["-fz", "-i", INPUT, "-o", "null"],
            exit_codes: &[0],
            tail: true,
            keys: parse::dsd_fme_source,
        }],
    },
    Signal {
        id: "dstar",
        name: "D-STAR",
        fixtures: &["dstar_calls_48k"],
        channel: "dstar",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 30.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: false,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 20.0,
            low_db: 0.0,
        },
        key: Key::One(header_callsign),
        references: &[Reference {
            program: &DSD_FME,
            format: fm(1_200.0, 48_000, Container::Wav),
            args: &["-fd", "-i", INPUT, "-o", "null"],
            exit_codes: &[0],
            tail: true,
            keys: parse::dsd_fme_source,
        }],
    },
    Signal {
        id: "dpmr",
        name: "dPMR",
        fixtures: &["dpmr_calls_48k"],
        channel: "dpmr",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 30.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: false,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 20.0,
            low_db: 0.0,
        },
        key: Key::One(addresses),
        references: &[Reference {
            program: &DSD_FME,
            format: fm(1_050.0, 48_000, Container::Wav),
            args: &["-fm", "-i", INPUT, "-o", "null"],
            exit_codes: &[0],
            tail: true,
            keys: parse::dsd_fme_dpmr,
        }],
    },
    Signal {
        id: "zvei",
        name: "ZVEI",
        fixtures: &["selcall_zvei1_48k"],
        channel: "selcall",
        settings: r#"{"system":"zvei1"}"#,
        offset_hz: -5_000.0,
        speed_seconds: 300.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: true,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 5.0,
            low_db: -10.0,
        },
        key: Key::One(selcall),
        references: &[Reference {
            program: &MULTIMON,
            format: fm(2_500.0, 22_050, Container::Raw),
            args: &["-q", "-a", "ZVEI1", "-t", "raw", INPUT],
            exit_codes: &[0],
            tail: true,
            keys: parse::multimon_zvei,
        }],
    },
    Signal {
        id: "uat",
        name: "UAT",
        fixtures: &["uat_reference_2m083"],
        channel: "uat",
        settings: DEFAULTS,
        offset_hz: 0.0,
        speed_seconds: 20.0,
        tail_seconds: 0.01,
        cu8_rate: Some(UAT_RATE),
        unique: true,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 12.0,
            low_db: 2.0,
        },
        key: Key::One(adsb),
        references: &[Reference {
            program: &DUMP978,
            format: Format::Cu8 { rate: UAT_RATE },
            args: &["--file", INPUT, "--format", "CU8", "--raw-stdout"],
            exit_codes: &[0],
            tail: true,
            keys: parse::dump978,
        }],
    },
    Signal {
        id: "rtty",
        name: "RTTY",
        fixtures: &["rtty_45_170_48k"],
        channel: "rtty",
        settings: DEFAULTS,
        offset_hz: 5_000.0,
        speed_seconds: 300.0,
        tail_seconds: 1.0,
        cu8_rate: None,
        unique: false,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: -10.0,
            low_db: -30.0,
        },
        key: Key::Words(rtty),
        references: &[Reference {
            program: &MINIMODEM,
            format: usb(RTTY_CENTRE_HZ, 48_000, Container::Wav),
            args: &[
                "--rx", "-q", "-M", "1585", "-S", "1415", "-f", INPUT, "rtty",
            ],
            exit_codes: &[0],
            tail: true,
            keys: parse::words,
        }],
    },
    Signal {
        id: "morse",
        name: "Morse",
        fixtures: &["morse_20wpm_48k"],
        channel: "morse",
        settings: DEFAULTS,
        offset_hz: -5_000.0,
        speed_seconds: 300.0,
        tail_seconds: 4.0,
        cu8_rate: None,
        unique: false,
        decodes: Show::Hidden,
        speed: Show::Plain,
        noise: Ladder {
            high_db: -6.0,
            low_db: -21.0,
        },
        key: Key::Words(morse),
        references: &[Reference {
            program: &MULTIMON,
            format: usb(800.0, 22_050, Container::Raw),
            args: &["-q", "-a", "MORSE_CW", "-t", "raw", INPUT],
            exit_codes: &[0],
            tail: true,
            keys: parse::words,
        }],
    },
    Signal {
        id: "rds",
        name: "RDS",
        fixtures: &["rds_station_960k"],
        channel: "wfm",
        settings: DEFAULTS,
        offset_hz: 200_000.0,
        speed_seconds: 60.0,
        tail_seconds: 0.5,
        cu8_rate: None,
        unique: true,
        decodes: Show::Hidden,
        speed: Show::Noted("SDRmm also demodulates the FM and stereo audio."),
        noise: Ladder {
            high_db: 12.0,
            low_db: 2.0,
        },
        key: Key::Many(rds),
        references: &[Reference {
            program: &REDSEA,
            format: fm(75_000.0, 171_000, Container::Wav),
            args: &["-f", INPUT],
            exit_codes: &[0],
            tail: true,
            keys: parse::redsea,
        }],
    },
    Signal {
        id: "ism",
        name: "ISM sensors",
        fixtures: ISM_CAPTURES,
        channel: "ism",
        settings: r#"{"unknown":false}"#,
        offset_hz: 0.0,
        speed_seconds: 60.0,
        tail_seconds: 0.1,
        cu8_rate: Some(ISM_RATE),
        unique: true,
        decodes: Show::Plain,
        speed: Show::Plain,
        noise: Ladder {
            high_db: 12.0,
            low_db: -3.0,
        },
        key: Key::One(ism),
        references: &[Reference {
            program: &RTL_433,
            format: Format::Cu8 { rate: ISM_RATE },
            args: &["-s", "250k", "-F", "json", "-r", INPUT],
            exit_codes: &[0],
            tail: true,
            keys: parse::rtl_433,
        }],
    },
];

fn adsb(event: &DecoderEvent) -> Option<String> {
    let message = event.read::<sdrmm_channels::AdsbMessage>()?;
    Some(message.raw.to_ascii_uppercase())
}

fn ais(event: &DecoderEvent) -> Option<String> {
    let message = event.read::<sdrmm_channels::AisMessage>()?;
    parse::nmea_payloads(&message.nmea).into_iter().next()
}

fn aprs(event: &DecoderEvent) -> Option<String> {
    Some(event.read::<sdrmm_channels::AprsPacket>()?.tnc2)
}

fn pocsag(event: &DecoderEvent) -> Option<String> {
    let message = event.read::<sdrmm_channels::PocsagMessage>()?;
    Some(format!("{} {}", message.address, message.text))
}

fn flex(event: &DecoderEvent) -> Option<String> {
    let message = event.read::<sdrmm_channels::FlexMessage>()?;
    Some(format!("{} {}", message.address, message.text))
}

fn selcall(event: &DecoderEvent) -> Option<String> {
    Some(event.read::<sdrmm_channels::SelcallSequence>()?.code)
}

fn acars(event: &DecoderEvent) -> Option<String> {
    let message = event.read::<sdrmm_channels::AcarsMessage>()?;
    Some(format!(
        "{} {} {}",
        message.registration, message.label, message.text
    ))
}

fn ft8(event: &DecoderEvent) -> Option<String> {
    if event.kind != "ft8" {
        return None;
    }
    Some(event.payload::<sdrmm_channels::WsjtMessage>()?.text)
}

fn rtty(event: &DecoderEvent) -> Option<String> {
    Some(event.read::<sdrmm_channels::RttyText>()?.text)
}

fn morse(event: &DecoderEvent) -> Option<String> {
    Some(event.read::<sdrmm_channels::MorseText>()?.text)
}

fn ism(event: &DecoderEvent) -> Option<String> {
    let message = event.read::<sdrmm_channels::IsmMessage>()?;
    Some(format!("{} {}", message.model, message.id))
}

fn rds(event: &DecoderEvent) -> Vec<String> {
    let Some(update) = event.read::<sdrmm_channels::RdsUpdate>() else {
        return Vec::new();
    };
    [
        update.pi.map(|pi| format!("PI {pi}")),
        update.ps.map(|ps| format!("PS {}", ps.trim())),
        update.radiotext.map(|text| format!("RT {}", text.trim())),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn addresses(event: &DecoderEvent) -> Option<String> {
    let frame = event.read::<sdrmm_wire::DvFrame>()?;
    let (source, destination) = (frame.source?, frame.destination?);
    Some(format!("{source} {destination}"))
}

fn header_callsign(event: &DecoderEvent) -> Option<String> {
    let frame = event.read::<sdrmm_wire::DvFrame>()?;
    (frame.kind == sdrmm_wire::DvFrameKind::Header).then_some(frame.source_call?)
}

fn callsign(event: &DecoderEvent) -> Option<String> {
    event.read::<sdrmm_wire::DvFrame>()?.source_call
}

#[cfg(test)]
mod tests {
    use super::super::ours::FrontEnd;
    use super::*;

    #[test]
    fn short_fixtures_loop_to_the_speed_length() {
        assert_eq!(SIGNALS[0].loops(0.2), 100);
        assert_eq!(SIGNALS[0].loops(30.0), 1);
        let ft8 = SIGNALS.iter().find(|s| s.id == "ft8").expect("ft8");
        assert_eq!(ft8.loops(15.0), 1);
    }

    #[test]
    fn words_are_read_across_events() {
        let events = [
            DecoderEvent::of(&sdrmm_channels::MorseText {
                text: "CQ D".to_owned(),
                wpm: 20.0,
            }),
            DecoderEvent::of(&sdrmm_channels::MorseText {
                text: "E DL1ABC".to_owned(),
                wpm: 20.0,
            }),
        ];
        let key = Key::Words(morse);
        let mut keys = Vec::new();
        for event in &events {
            key.read(event, &mut keys);
        }
        assert_eq!(key.finish(keys), ["CQ", "DE", "DL1ABC"]);
    }

    #[test]
    fn an_rds_update_gives_each_field_it_has() {
        let update = sdrmm_channels::RdsUpdate {
            pi: Some("D3C2".to_owned()),
            ps: Some("SDR-M4  ".to_owned()),
            ..Default::default()
        };
        assert_eq!(rds(&DecoderEvent::of(&update)), ["PI D3C2", "PS SDR-M4"]);
    }

    #[test]
    fn every_signal_has_a_front_end() {
        sdrmm_channels::builtin::install();
        for signal in SIGNALS {
            assert!(FrontEnd::new(signal).is_ok(), "{}", signal.id);
        }
    }

    #[test]
    fn every_ladder_steps_into_more_noise() {
        for signal in SIGNALS {
            assert!(signal.noise.high_db > signal.noise.low_db, "{}", signal.id);
        }
    }

    #[test]
    fn every_reference_reads_its_input() {
        for signal in SIGNALS {
            for reference in signal.references {
                assert!(reference.args.contains(&INPUT), "{}", signal.id);
            }
        }
    }
}
