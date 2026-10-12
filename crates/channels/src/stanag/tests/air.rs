use num_complex::Complex;
use sdrmm_wire::{StanagInterleave, StanagModem, StanagRate};

use super::{identify, run};
use crate::ChannelRx;

fn ci16_le(bytes: &[u8]) -> Vec<Complex<f32>> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|s| {
            Complex::new(
                f32::from(i16::from_le_bytes([s[0], s[1]])) / 32_768.0,
                f32::from(i16::from_le_bytes([s[2], s[3]])) / 32_768.0,
            )
        })
        .collect()
}

const fn rate(bps: u16, interleave: StanagInterleave) -> Option<StanagRate> {
    Some(StanagRate {
        bps,
        interleave,
        voice: false,
    })
}

struct Recording {
    data: &'static [u8],
    modem: StanagModem,
    rate: Option<StanagRate>,
}

const RECORDINGS: &[Recording] = &[
    Recording {
        data: include_bytes!("../../../../../fixtures/stanag/stanag_4285_9k6.sigmf-data"),
        modem: StanagModem::Stanag4285,
        rate: None,
    },
    Recording {
        data: include_bytes!("../../../../../fixtures/stanag/stanag_4529_9k6.sigmf-data"),
        modem: StanagModem::Stanag4529,
        rate: None,
    },
    Recording {
        data: include_bytes!("../../../../../fixtures/stanag/stanag_110a_600s_9k6.sigmf-data"),
        modem: StanagModem::MilStd110a,
        rate: rate(600, StanagInterleave::Short),
    },
    Recording {
        data: include_bytes!("../../../../../fixtures/stanag/stanag_110a_2400l_9k6.sigmf-data"),
        modem: StanagModem::MilStd110a,
        rate: rate(2_400, StanagInterleave::Long),
    },
    Recording {
        data: include_bytes!("../../../../../fixtures/stanag/stanag_110a_150l_9k6.sigmf-data"),
        modem: StanagModem::MilStd110a,
        rate: rate(150, StanagInterleave::Long),
    },
    Recording {
        data: include_bytes!("../../../../../fixtures/stanag/stanag_4539_9k6.sigmf-data"),
        modem: StanagModem::Stanag4539,
        rate: None,
    },
    Recording {
        data: include_bytes!("../../../../../fixtures/stanag/stanag_39tone_9k6.sigmf-data"),
        modem: StanagModem::ThirtyNineTone,
        rate: None,
    },
    Recording {
        data: include_bytes!("../../../../../fixtures/stanag/stanag_16tone_9k6.sigmf-data"),
        modem: StanagModem::SixteenTone,
        rate: None,
    },
    Recording {
        data: include_bytes!("../../../../../fixtures/stanag/stanag_ale_9k6.sigmf-data"),
        modem: StanagModem::Ale2g,
        rate: None,
    },
];

#[test]
fn names_recorded_modems() {
    for recording in RECORDINGS {
        let (reports, _) = identify(&ci16_le(recording.data));
        let last = reports
            .last()
            .unwrap_or_else(|| panic!("{:?} not named", recording.modem));
        assert!(
            reports
                .iter()
                .all(|report| report.modem == Some(recording.modem)),
            "{:?}: {reports:?}",
            recording.modem
        );
        assert_eq!(last.rate, recording.rate, "{:?}", recording.modem);
    }
}

type Sample = (
    &'static str,
    Option<StanagModem>,
    Option<StanagRate>,
    &'static [StanagModem],
);

const LIBRARY: &[Sample] = &[
    ("STANAG4285_TFC", Some(StanagModem::Stanag4285), None, &[]),
    ("STANAG4285_IDLE", Some(StanagModem::Stanag4285), None, &[]),
    ("4285STNG", Some(StanagModem::Stanag4285), None, &[]),
    (
        "16.4732_MHz_STANAG",
        Some(StanagModem::Stanag4285),
        None,
        &[],
    ),
    (
        "SYS3000-FEC_STANAG_4285_FEC-VARIANT",
        Some(StanagModem::Stanag4285),
        None,
        &[],
    ),
    (
        "STANAG_4285_2016-07-24T00-01-05Z_5155.2kHz",
        Some(StanagModem::Stanag4285),
        None,
        &[],
    ),
    (
        "STANAG_4285_8PSK_January_29_2016_5758.6kHz",
        Some(StanagModem::Stanag4285),
        None,
        &[],
    ),
    ("Stanag-4481-psk", Some(StanagModem::Stanag4285), None, &[]),
    ("Stanag-4529", Some(StanagModem::Stanag4529), None, &[]),
    ("STANAG-4529", Some(StanagModem::Stanag4529), None, &[]),
    ("4529-cl", Some(StanagModem::Stanag4529), None, &[]),
    ("4529-cs", Some(StanagModem::Stanag4529), None, &[]),
    ("4529-u", Some(StanagModem::Stanag4529), None, &[]),
    (
        "MIL188-110A_75S",
        Some(StanagModem::MilStd110a),
        rate(75, StanagInterleave::Short),
        &[],
    ),
    (
        "MIL188-110A_75L",
        Some(StanagModem::MilStd110a),
        rate(75, StanagInterleave::Long),
        &[],
    ),
    (
        "150S",
        Some(StanagModem::MilStd110a),
        rate(150, StanagInterleave::Short),
        &[],
    ),
    (
        "150L",
        Some(StanagModem::MilStd110a),
        rate(150, StanagInterleave::Long),
        &[],
    ),
    (
        "MIL188-110A_300S",
        Some(StanagModem::MilStd110a),
        rate(300, StanagInterleave::Short),
        &[],
    ),
    (
        "MIL188-110A_300L",
        Some(StanagModem::MilStd110a),
        rate(300, StanagInterleave::Long),
        &[],
    ),
    (
        "MIL188-110A_600S",
        Some(StanagModem::MilStd110a),
        rate(600, StanagInterleave::Short),
        &[],
    ),
    (
        "MIL188-110A_600L",
        Some(StanagModem::MilStd110a),
        rate(600, StanagInterleave::Long),
        &[],
    ),
    (
        "MIL188-110A_1200S",
        Some(StanagModem::MilStd110a),
        rate(1_200, StanagInterleave::Short),
        &[],
    ),
    (
        "MIL188-110A_1200L",
        Some(StanagModem::MilStd110a),
        rate(1_200, StanagInterleave::Long),
        &[],
    ),
    (
        "MIL188-110A_2400S",
        Some(StanagModem::MilStd110a),
        rate(2_400, StanagInterleave::Short),
        &[],
    ),
    (
        "MIL188-110A_2400L",
        Some(StanagModem::MilStd110a),
        rate(2_400, StanagInterleave::Long),
        &[],
    ),
    (
        "MIL188-110A_4800_No_Interleave",
        Some(StanagModem::MilStd110a),
        rate(4_800, StanagInterleave::Zero),
        &[],
    ),
    ("Mil-188-110a", Some(StanagModem::MilStd110a), None, &[]),
    (
        "Mil_188-110_Serial_(ryryry_de_rc97)",
        Some(StanagModem::MilStd110a),
        None,
        &[],
    ),
    (
        "188-110A_(serial)-1",
        Some(StanagModem::MilStd110a),
        None,
        &[StanagModem::Ale2g],
    ),
    (
        "188-110A_(serial)-2",
        Some(StanagModem::MilStd110a),
        None,
        &[],
    ),
    ("S4539-S9600", Some(StanagModem::Stanag4539), None, &[]),
    (
        "Mil-188-110b_Med_QAM_8000",
        Some(StanagModem::Stanag4539),
        None,
        &[],
    ),
    ("App_C_sound", Some(StanagModem::Stanag4539), None, &[]),
    (
        "MIL188-110B_AppC_S_3200_QPSK",
        Some(StanagModem::Stanag4539),
        None,
        &[],
    ),
    (
        "Mil-std-188-110b_(app.C)_QAM_8000_VS_",
        Some(StanagModem::Stanag4539),
        None,
        &[],
    ),
    (
        "Mil-188-110-39tone",
        Some(StanagModem::ThirtyNineTone),
        None,
        &[],
    ),
    (
        "39tone-l",
        Some(StanagModem::ThirtyNineTone),
        None,
        &[StanagModem::MilStd110a],
    ),
    ("39tone-s", Some(StanagModem::ThirtyNineTone), None, &[]),
    (
        "MIL188-110A_AppA",
        Some(StanagModem::SixteenTone),
        None,
        &[],
    ),
    (
        "MIL188-110A_AppA_pilot",
        Some(StanagModem::SixteenTone),
        None,
        &[],
    ),
    ("App.A_16-tone", Some(StanagModem::SixteenTone), None, &[]),
    ("NATO16-TONE", Some(StanagModem::SixteenTone), None, &[]),
    ("2G_ALEaudio", Some(StanagModem::Ale2g), None, &[]),
    (
        "Mil-std-188-110A_(serial)_1",
        Some(StanagModem::MilStd110a),
        None,
        &[StanagModem::Ale2g],
    ),
    ("3GALE-S4539", Some(StanagModem::Stanag4539), None, &[]),
    (
        "MIL188-110A_AppB",
        Some(StanagModem::ThirtyNineTone),
        None,
        &[],
    ),
    ("S4197", Some(StanagModem::ThirtyNineTone), None, &[]),
    ("Stanag4481", None, None, &[]),
];

fn audio(path: &std::path::Path) -> Vec<Complex<f32>> {
    std::fs::read(path)
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|s| Complex::new(f32::from_le_bytes(*s), 0.0))
        .collect()
}

fn expected(
    reports: &[sdrmm_wire::StanagReport],
    modem: Option<StanagModem>,
    rate: Option<StanagRate>,
    also: &[StanagModem],
) -> bool {
    let Some(modem) = modem else {
        return reports.is_empty();
    };
    let named = reports.iter().any(|report| report.modem == Some(modem));
    let wrong = reports.iter().any(|report| {
        report
            .modem
            .is_some_and(|other| other != modem && !also.contains(&other))
    });
    let rated = reports.iter().rev().find_map(|report| report.rate);
    named && !wrong && (rate.is_none() || rated == rate)
}

#[test]
#[ignore = "needs STANAG_SIGID_DIR=<sigidwiki samples as 9.6 kHz mono f32le, one <stem>.f32 each>"]
fn names_the_sigidwiki_library() {
    let dir = std::path::PathBuf::from(std::env::var("STANAG_SIGID_DIR").unwrap());
    let window = usize::from(sdrmm_wire::STANAG_WINDOW_S) * 9_600;
    let mut misses = Vec::new();
    for &(stem, modem, rate, also) in LIBRARY {
        let iq = audio(&dir.join(format!("{stem}.f32")));
        let mut channel = super::channel();
        let reports = run(&mut channel, &iq);
        let ok = if iq.len() < window {
            reports.is_empty()
        } else {
            expected(&reports, modem, rate, also)
        };
        let seen: Vec<_> = reports
            .iter()
            .map(|report| report.modem.map_or("gone", |modem| modem.label()))
            .collect();
        println!(
            "{stem:<44} {:<4} rejected {:<3} {}",
            if ok { "ok" } else { "MISS" },
            channel.rejected(),
            seen.join(", ")
        );
        if !ok {
            misses.push((stem, reports));
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}
