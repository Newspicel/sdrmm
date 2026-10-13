#![allow(clippy::expect_used)]

use num_complex::Complex;
use sdrmm_wire::{
    AprsMode, AprsParams, AtvParams, ChannelParams, ChannelSettings, SelcallSystem, Squelch,
    SstvMode,
};

use super::{
    acars, adsb, ais, atv, dect, dv, ermes, flex, gnss, morse, navtex, pocsag, radio_clock, rds,
    rtty, selcall, sstv,
};
use crate::{AprsTx, ChannelCtx, ChannelTx, TxPayload};

const NARROW: f64 = 240_000.0;
const AUDIO: f64 = 48_000.0;
const ADSB_RATE: f64 = 2_000_000.0;
const RDS_RATE: f64 = 960_000.0;
const DECT_RATE: f64 = 2_304_000.0;
const DECT_CALL_FRAMES: usize = 100;
const ATV_RATE: f64 = 2_400_000.0;
const SSTV_RATE: f64 = 16_000.0;
const SSTV_MODE: SstvMode = SstvMode::Robot36;

pub struct Recipe {
    pub stem: &'static str,
    pub rate: f64,
    pub note: &'static str,
    pub iq: fn() -> Vec<Complex<f32>>,
}

#[must_use]
pub fn recipe(stem: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|recipe| recipe.stem == stem)
}

pub const RECIPES: &[Recipe] = &[
    Recipe {
        stem: "pocsag_1200_240k",
        rate: NARROW,
        note: "pocsag channel at +50 kHz -> address 1234567 \"SDR-- FIXTURE\"",
        iq: pocsag_page,
    },
    Recipe {
        stem: "flex_1600_2_240k",
        rate: NARROW,
        note: "flex channel at +30 kHz -> address 1234567 \"SDR-- FLEX FIXTURE\", cycle 7 frame 83",
        iq: flex_page,
    },
    Recipe {
        stem: "ermes_alpha_240k",
        rate: NARROW,
        note: "ermes channel at -30 kHz -> address 234567 \"SDR-- ERMES FIXTURE\", urgent alert 5",
        iq: ermes_page,
    },
    Recipe {
        stem: "selcall_ccir1_48k",
        rate: AUDIO,
        note: "selcall CCIR-1 channel at +5 kHz -> 12234 (repeat marker expanded)",
        iq: selcall_ccir1,
    },
    Recipe {
        stem: "selcall_zvei1_48k",
        rate: AUDIO,
        note: "selcall ZVEI-1 channel at -5 kHz -> A11D0 (group symbols and repeat marker)",
        iq: selcall_zvei1,
    },
    Recipe {
        stem: "ais_position_240k",
        rate: NARROW,
        note: "ais channel at +25 kHz -> MMSI 211234560 at 53.5413, 9.9846",
        iq: ais_position,
    },
    Recipe {
        stem: "aprs_afsk1200_240k",
        rate: NARROW,
        note: "aprs channel at -40 kHz -> DL1ABC-9>APRS,WIDE1-1 at 52.5, 13.4",
        iq: aprs_packet,
    },
    Recipe {
        stem: "rtty_45_170_48k",
        rate: AUDIO,
        note: "rtty channel at +5 kHz (45.45 baud, 170 Hz) -> \"CQ CQ DE DL1ABC K\"",
        iq: rtty_call,
    },
    Recipe {
        stem: "morse_20wpm_48k",
        rate: AUDIO,
        note: "morse channel at -5 kHz -> \"CQ DE DL1ABC K\" at 20 wpm",
        iq: morse_call,
    },
    Recipe {
        stem: "cw_skimmer_dual_48k",
        rate: AUDIO,
        note: "cw_skimmer channel -> simultaneous DL1AAA at -3.5 kHz/18 wpm and G4BBB at +4.2 kHz/27 wpm",
        iq: cw_pair,
    },
    Recipe {
        stem: "adsb_squitters_2m",
        rate: ADSB_RATE,
        note: "adsb channel at 0 Hz, device at 2 MS/s -> 3C6444/DLH123 at FL380",
        iq: adsb_squitters,
    },
    Recipe {
        stem: "dcf77_2026_2k",
        rate: radio_clock::RATE,
        note: "radio_clock (DCF77) -> 2026-08-15 12:34 CET with valid parity",
        iq: radio_clock::dcf77,
    },
    Recipe {
        stem: "gps_l1_ca_prn7_2m048",
        rate: gnss::RATE,
        note: "gnss channel -> GPS L1 C/A PRN 7, +1000 Hz Doppler, code phase 158.3 chips",
        iq: gps_prn7,
    },
    Recipe {
        stem: "rds_station_960k",
        rate: RDS_RATE,
        note: "wfm channel at +200 kHz with rds on -> PI D3C2 \"SDR-M4\" + a 1 kHz tone",
        iq: rds_station,
    },
    Recipe {
        stem: "navtex_518_48k",
        rate: AUDIO,
        note: "navtex channel at +3 kHz -> DA07 navigational warning, \"GALE WARNING\"",
        iq: navtex_warning,
    },
    Recipe {
        stem: "acars_downlink_240k",
        rate: NARROW,
        note: "acars channel at -40 kHz -> D-AIBC / LH0400 [H1] \"SDR-- FIXTURE\"",
        iq: acars_downlink,
    },
    Recipe {
        stem: "ysf_callsigns_48k",
        rate: AUDIO,
        note: "ysf channel at 0 Hz -> DL1ABC to ALL via DB0XYZ and DB0ABC",
        iq: ysf_callsigns,
    },
    Recipe {
        stem: "dect_base_2m304",
        rate: DECT_RATE,
        note: "dect channel -> RFPI 01234D5E6D, carrier 4, standard authentication and ciphering",
        iq: dect_base,
    },
    Recipe {
        stem: "dect_call_2m304",
        rate: DECT_RATE,
        note: "dect channel -> clear call on RFPI 01234D5E6D, 1 kHz from the base, 600 Hz from the handset",
        iq: dect_call,
    },
    Recipe {
        stem: "atv_ccir625_2m4",
        rate: ATV_RATE,
        note: "atv channel at +200 kHz -> 625/25 AM, five vertical bars black to white",
        iq: atv_bars,
    },
    Recipe {
        stem: "sstv_robot36_48k",
        rate: AUDIO,
        note: "sstv channel at +4 kHz -> Robot 36, eight colour bars white to black",
        iq: sstv_bars,
    },
];

fn at(mut iq: Vec<Complex<f32>>, offset: f64, rate: f64) -> Vec<Complex<f32>> {
    super::shift(&mut iq, offset, rate);
    iq
}

fn pocsag_page() -> Vec<Complex<f32>> {
    let page = pocsag::Page {
        address: 1_234_567,
        function: 3,
        text: "SDR-- FIXTURE".to_string(),
        numeric: false,
    };
    at(
        pocsag::transmission(&[page], 1_200, 4_500.0, NARROW),
        50_000.0,
        NARROW,
    )
}

fn flex_page() -> Vec<Complex<f32>> {
    let page = flex::Page {
        address: 1_234_567,
        text: "SDR-- FLEX FIXTURE".to_string(),
    };
    at(flex::transmission(&page, 7, 83, NARROW), 30_000.0, NARROW)
}

fn ermes_page() -> Vec<Complex<f32>> {
    let page = ermes::Page {
        local_address: 234_567,
        message_number: 3,
        text: "SDR-- ERMES FIXTURE".to_string(),
        urgent: true,
        alert: 5,
    };
    at(ermes::transmission(&page, NARROW), -30_000.0, NARROW)
}

fn selcall_ccir1() -> Vec<Complex<f32>> {
    at(
        selcall::transmission(SelcallSystem::Ccir1, "12234", AUDIO)
            .expect("CCIR-1 fixture code is valid"),
        5_000.0,
        AUDIO,
    )
}

fn selcall_zvei1() -> Vec<Complex<f32>> {
    at(
        selcall::transmission(SelcallSystem::Zvei1, "A11D0", AUDIO)
            .expect("ZVEI-1 fixture code is valid"),
        -5_000.0,
        AUDIO,
    )
}

fn ais_position() -> Vec<Complex<f32>> {
    let report = ais::PositionReport {
        mmsi: 211_234_560,
        lat: 53.5413,
        lon: 9.9846,
        sog_kt: 12.3,
        cog_deg: 178.4,
        heading_deg: 179,
        nav_status: 0,
    };
    at(
        ais::burst(&ais::position_payload(&report), NARROW),
        25_000.0,
        NARROW,
    )
}

fn aprs_packet() -> Vec<Complex<f32>> {
    let settings = ChannelSettings {
        frequency_hz: 0.0,
        squelch: Squelch::Off,
        params: ChannelParams::Aprs(AprsParams {
            mode: AprsMode::Afsk1200,
            ..AprsParams::default()
        }),
        blanker: Default::default(),
    };
    let mut tx = AprsTx::new(
        ChannelCtx {
            input_rate: AprsTx::descriptor().input_rate_hz,
        },
        settings,
    )
    .expect("aprs modulator at its own channel rate");
    tx.submit(TxPayload::Frame(AprsTx::ui_frame(
        "DL1ABC-9",
        "APRS",
        &["WIDE1-1"],
        "!5230.00N/01324.00E>SDR-- fixture",
    )))
    .expect("a ui frame is a payload the modulator carries");
    at(
        super::resample(&super::burst(&mut tx), AUDIO, NARROW),
        -40_000.0,
        NARROW,
    )
}

fn rtty_call() -> Vec<Complex<f32>> {
    at(
        rtty::transmission("CQ CQ DE DL1ABC K\r\n", 45.45, 170.0, 1.5, AUDIO),
        5_000.0,
        AUDIO,
    )
}

fn morse_call() -> Vec<Complex<f32>> {
    at(
        morse::transmission("CQ DE DL1ABC K", 20.0, 0.0, AUDIO),
        -5_000.0,
        AUDIO,
    )
}

fn cw_pair() -> Vec<Complex<f32>> {
    let first = morse::transmission("VVV CQ DE DL1AAA K", 18.0, -3_500.0, AUDIO);
    let second = morse::transmission("VVV CQ DE G4BBB K", 27.0, 4_200.0, AUDIO);
    let mut iq = vec![Complex::new(0.0, 0.0); first.len().max(second.len()) + AUDIO as usize * 4];
    for (destination, source) in iq.iter_mut().zip(first) {
        *destination += source * 0.55;
    }
    for (destination, source) in iq.iter_mut().zip(second) {
        *destination += source * 0.35;
    }
    iq
}

fn adsb_squitters() -> Vec<Complex<f32>> {
    let icao = 0x3C_6444;
    adsb::transmission(
        &[
            adsb::squitter(icao, adsb::me_identification("DLH123")),
            adsb::squitter(
                icao,
                adsb::me_airborne_position(38_000, 52.2572, 3.9190, false),
            ),
            adsb::squitter(
                icao,
                adsb::me_airborne_position(38_000, 52.2657, 3.9184, true),
            ),
            adsb::squitter(icao, adsb::me_velocity(450.0, 275.0, -1_024)),
        ],
        500.0,
        0.8,
        ADSB_RATE,
    )
}

fn gps_prn7() -> Vec<Complex<f32>> {
    gnss::acquisition(7, 1_000.0, 317, 2)
}

fn rds_station() -> Vec<Complex<f32>> {
    let station = rds::Station {
        pi: 0xD3C2,
        ps: "SDR-M4  ".to_string(),
        radiotext: "SDR-- reference fixture".to_string(),
        pty: 10,
        tp: true,
        ta: false,
        music: true,
        alt_freqs_hz: vec![89_800_000.0, 95_500_000.0],
    };
    at(
        rds::transmission(&station, 8.0, Some(1_000.0), RDS_RATE),
        200_000.0,
        RDS_RATE,
    )
}

fn navtex_warning() -> Vec<Complex<f32>> {
    at(
        navtex::transmission("ZCZC DA07\r\nGALE WARNING\r\nGERMAN BIGHT\r\nNNNN", AUDIO),
        3_000.0,
        AUDIO,
    )
}

fn acars_downlink() -> Vec<Complex<f32>> {
    let block = acars::Block {
        mode: '2',
        registration: ".D-AIBC",
        ack: '\x15',
        label: "H1",
        block_id: '3',
        seq_no: Some("M01A"),
        flight: Some("LH0400"),
        text: "SDR-- FIXTURE",
        more: false,
    };
    at(acars::transmission(&block, NARROW), -40_000.0, NARROW)
}

fn ysf_callsigns() -> Vec<Complex<f32>> {
    dv::ysf::transmission_with_callsigns(
        &dv::ysf::Fich::default(),
        &dv::ysf::Call::default(),
        AUDIO,
    )
}

fn dect_base() -> Vec<Complex<f32>> {
    dect::dummy_bearer(
        &dect::Station {
            rfpi: 0x0001_234D_5E6D,
            carrier: 4,
            slot: 2,
            slot_pair: 2,
            capabilities: dect::capability_bits(&[17, 33, 36, 37, 38]),
            ..dect::Station::default()
        },
        60,
    )
}

fn dect_call() -> Vec<Complex<f32>> {
    let base = dect::tone(1_000.0, 0.3, DECT_CALL_FRAMES * 80);
    let handset = dect::tone(600.0, 0.3, DECT_CALL_FRAMES * 80);
    let mut air = dect::Air::carrier(DECT_CALL_FRAMES);
    dect::Call {
        station: dect::Station {
            rfpi: 0x0001_234D_5E6D,
            carrier: 4,
            slot: 2,
            ..dect::Station::default()
        },
        pmid: 0x1_2345,
        first_frame: 0,
        base: &base,
        handset: &handset,
        grant_at: None,
    }
    .transmit(&mut air, DECT_CALL_FRAMES);
    air.into_iq()
}

fn atv_bars() -> Vec<Complex<f32>> {
    at(
        atv::bars(&atv::AtvSource::new(&AtvParams::default(), ATV_RATE), 2),
        200_000.0,
        ATV_RATE,
    )
}

fn sstv_bars() -> Vec<Complex<f32>> {
    let sstv = sstv::transmission(SSTV_MODE, &sstv::bars(SSTV_MODE), SSTV_RATE);
    at(super::resample(&sstv, SSTV_RATE, AUDIO), 4_000.0, AUDIO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stem_is_unique() {
        for (index, recipe) in RECIPES.iter().enumerate() {
            assert!(
                RECIPES[index + 1..]
                    .iter()
                    .all(|other| other.stem != recipe.stem),
                "{}",
                recipe.stem
            );
        }
    }

    #[test]
    fn a_recipe_is_found_by_its_stem() {
        let found = recipe("dect_call_2m304").expect("dect call recipe");
        assert_eq!(found.rate, DECT_RATE);
        assert!(recipe("missing").is_none());
    }
}
