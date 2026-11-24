use super::*;
use air::{
    ISCH_DIBITS, PACKET_DIBITS, S_ISCH, SLOT_CHANNEL, SLOTS, duid_codeword, firm,
    info_isch_codeword,
};
use scrambler::Identity;
use sdrmm_codec::voice::testutil::half_rate_frames;
use sdrmm_decoder_test_support::cf32_le;
use sdrmm_decoder_test_support::frames::{assert_tone_audio, decode, decode_with_audio};
use sdrmm_decoder_test_support::settings;
use sdrmm_modem::linear::{LinearMod, LinearParams};
use sdrmm_modem::pulse::{self, Norm};
use sdrmm_wire::ChannelParams;

pub(crate) const RECORDING: &[u8] =
    include_bytes!("../../../../../fixtures/p25p2_mmr_idle_48k.sigmf-data");
const MMR: Identity = Identity {
    wacn: 0xBEE00,
    system: 0x164,
    nac: 0x164,
};
const SITE: Identity = Identity {
    wacn: 0x12345,
    system: 0x2A7,
    nac: 0x3C1,
};
const REGROUP: &str = "81900b0c8200c900c900c9";

const DUID_VOICE4: u8 = 0;
const DUID_SACCH: u8 = 3;
const DUID_VOICE2: u8 = 6;
const DUID_FACCH: u8 = 9;
const DUID_DATCH: u8 = 10;
const DUID_FACCH_CLEAR: u8 = 15;

fn channel(slots: P25p2Slots) -> P25p2Channel {
    P25p2Channel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(
            ChannelParams::of(&P25p2Params {
                slots,
                vocoder: None,
            })
            .expect("p25p2 settings"),
        ),
    )
    .expect("p25p2 channel")
}

#[test]
fn reads_the_site_and_a_patch_from_an_idle_carrier_off_the_air() {
    let frames = decode(&mut channel(P25p2Slots::Both), &cf32_le(RECORDING));
    let identity = frames
        .iter()
        .find(|frame| frame.opcode.as_deref() == Some("scrambling identity"))
        .expect("the scrambling seed is worked out");
    assert_eq!(identity.network_id, Some(0xBEE00));
    assert_eq!(identity.system_id, Some(0x164));
    assert_eq!(identity.color_code, Some(0x164));
    let patches: Vec<_> = frames
        .iter()
        .filter(|frame| frame.data.as_deref() == Some(REGROUP))
        .collect();
    let mut slots: Vec<_> = patches.iter().map(|patch| patch.slot).collect();
    slots.sort_unstable();
    assert_eq!(slots, [Some(1), Some(2)], "{frames:#?}");
    for patch in patches {
        assert_eq!(patch.opcode.as_deref(), Some("Motorola patch add"));
        assert_eq!(patch.destination, Some(3202));
        assert_eq!(patch.text.as_deref(), Some("TG 201"));
        assert_eq!(patch.crc_verified, Some(true));
        assert_eq!(patch.network_id, Some(0xBEE00));
    }
}

#[test]
fn only_the_sites_own_seed_decodes_the_recording() {
    let mut demod = Demod::new().expect("demod");
    let mut phasors = Vec::new();
    demod.process(&cf32_le(RECORDING), &mut phasors);
    let mut framer = Framer::new();
    let site = Masks::new(MMR);
    let other = Masks::new(Identity { nac: 0x165, ..MMR });
    let (mut right, mut wrong) = (0, 0);
    for &phasor in &phasors {
        if !framer.push(phasor) {
            continue;
        }
        let packet = framer.packet();
        if packet.burst() != (Burst::Sacch { scrambled: true }) {
            continue;
        }
        let reliability: Vec<f32> = packet
            .body()
            .iter()
            .map(|&phasor| demod::reliability(phasor))
            .collect();
        for (masks, count) in [(&site, &mut right), (&other, &mut wrong)] {
            let body: Vec<u8> = packet
                .body()
                .iter()
                .zip(masks.body(packet.slot))
                .map(|(&phasor, &scramble)| dibit(phasor) ^ scramble)
                .collect();
            if mac::decode(&body, &reliability, &SACCH).is_some() {
                *count += 1;
            }
        }
    }
    assert!(right >= 4, "{right} SACCH decoded");
    assert_eq!(wrong, 0);
}

#[test]
fn the_isch_and_duid_codes_match_the_published_tables() {
    for (value, codeword) in [
        (0, 0x18_4229_D461u64),
        (1, 0x18_7614_51F6),
        (7, 0x18_B37C_5AC9),
        (8, 0x11_46A4_4F13),
        (16, 0x1A_4A2E_239E),
        (53, 0x16_E36E_D6F6),
        (125, 0x0F_E8A8_5ADC),
        (127, 0x0F_B063_F092),
    ] {
        assert_eq!(info_isch_codeword(value), codeword, "{value}");
    }
    for slot in [0usize, 1, 4, 5, 8, 9] {
        let word = info_isch_codeword((slot % 4) << 5 | (slot / 4) << 3 | 2) ^ 0b1_0101 << 20;
        assert_eq!(
            air::info_isch(&firm(word)).map(air::InfoIsch::slot),
            Some(slot)
        );
    }
    assert!(air::info_isch(&firm(S_ISCH)).is_none());
    assert_eq!(air::burst_of(firm(0x9A)), Burst::Facch { scrambled: true });
    assert_eq!(
        air::burst_of(firm(0x9A ^ 0x40)),
        Burst::Facch { scrambled: true }
    );
    assert_eq!(air::burst_of(firm(0x03)), Burst::Unknown);
}

/// `word`'s soft bits with those under `flips` weakly wrong.
fn weakly_flipped<const N: usize>(word: u64, flips: u64) -> [i8; N] {
    let mut soft = firm(word);
    for (at, value) in soft.iter_mut().enumerate() {
        if flips >> (N - 1 - at) & 1 == 1 {
            *value = -*value / 4;
        }
    }
    soft
}

#[test]
fn soft_isch_and_duid_decode_past_the_hard_radius() {
    let slot = 5;
    let word = info_isch_codeword((slot % 4) << 5 | (slot / 4) << 3);
    let flips: u64 = 0x11_1111_1111 & !0x10;
    assert_eq!(flips.count_ones(), 9);
    assert_ne!(
        air::info_isch(&firm(word ^ flips)).map(air::InfoIsch::slot),
        Some(slot)
    );
    let soft = weakly_flipped(word, flips);
    assert_eq!(air::info_isch(&soft).map(air::InfoIsch::slot), Some(slot));
    let codeword = u64::from(duid_codeword(DUID_FACCH));
    let flips = 0b1000_0001;
    assert_eq!(air::burst_of(firm(codeword ^ flips)), Burst::Unknown);
    let soft = weakly_flipped(codeword, flips);
    assert_eq!(air::burst_of(soft), Burst::Facch { scrambled: true });
}

#[test]
fn a_scrambled_burst_gives_up_its_seed() {
    let masks = Masks::new(SITE);
    for (layout, duid, slot) in [(&SACCH, DUID_SACCH, 10), (&FACCH, DUID_FACCH, 3)] {
        let mut pdu = [0u8; 21];
        pdu[0] = 3 << 5;
        let body = scrambled(mac::encode(&pdu, layout), &masks, slot, duid);
        assert_eq!(solve_identity(&body, slot, layout), Some(SITE));
        let mut damaged = body;
        damaged[20] ^= 1;
        assert_ne!(solve_identity(&damaged, slot, layout), Some(SITE));
    }
}

fn scrambled(
    mut body: [u8; BODY_DIBITS],
    masks: &Masks,
    slot: usize,
    duid: u8,
) -> [u8; BODY_DIBITS] {
    for (dibit, &scramble) in body.iter_mut().zip(masks.body(slot)) {
        *dibit ^= scramble;
    }
    with_duid(body, duid)
}

fn with_duid(mut body: [u8; BODY_DIBITS], duid: u8) -> [u8; BODY_DIBITS] {
    let codeword = duid_codeword(duid);
    for (index, at) in [0usize, 37, 122, 159].into_iter().enumerate() {
        body[at] = codeword >> (6 - 2 * index) & 3;
    }
    body
}

/// A downlink built timeslot by timeslot, as dibits.
struct Air {
    masks: Masks,
    dibits: Vec<u8>,
}

impl Air {
    fn new(identity: Identity) -> Self {
        Self {
            masks: Masks::new(identity),
            dibits: Vec::new(),
        }
    }

    fn slot(&mut self, slot: usize, body: [u8; BODY_DIBITS]) {
        let isch = if matches!(slot, 2 | 3 | 6 | 7 | 10 | 11) {
            S_ISCH
        } else {
            info_isch_codeword((slot % 4) << 5 | (slot / 4) << 3)
        };
        self.dibits.extend(
            (0..ISCH_DIBITS)
                .rev()
                .map(|at| (isch >> (2 * at)) as u8 & 3),
        );
        self.dibits.extend_from_slice(&body);
    }

    fn idle(&self, slot: usize) -> [u8; BODY_DIBITS] {
        if matches!(slot, 10 | 11) {
            self.mac(slot, &[3 << 5], &SACCH, DUID_SACCH)
        } else {
            scrambled([0; BODY_DIBITS], &self.masks, slot, DUID_DATCH)
        }
    }

    fn mac(&self, slot: usize, pdu: &[u8], layout: &Layout, duid: u8) -> [u8; BODY_DIBITS] {
        let body = mac::encode(pdu, layout);
        if duid == DUID_FACCH_CLEAR {
            with_duid(body, duid)
        } else {
            scrambled(body, &self.masks, slot, duid)
        }
    }

    fn voice(&self, slot: usize, frames: &[[bool; 72]]) -> [u8; BODY_DIBITS] {
        let mut body = [0u8; BODY_DIBITS];
        for (&start, frame) in VOICE_FRAMES.iter().zip(frames) {
            for (offset, pair) in frame.as_chunks::<2>().0.iter().enumerate() {
                body[start + offset] = u8::from(pair[0]) << 1 | u8::from(pair[1]);
            }
        }
        let duid = if frames.len() == 4 {
            DUID_VOICE4
        } else {
            DUID_VOICE2
        };
        scrambled(body, &self.masks, slot, duid)
    }

    fn idle_superframe(&mut self) {
        for slot in 0..SLOTS {
            let body = self.idle(slot);
            self.slot(slot, body);
        }
    }

    /// A call on logical channel 0: the PTT, two superframes of voice and the end.
    fn call(&mut self, algorithm: u8, voice: &[[bool; 72]]) {
        let mut ptt = [0u8; 18];
        ptt[0] = 1 << 5;
        ptt[1..10].copy_from_slice(&[0x11; 9]);
        ptt[10] = algorithm;
        ptt[11..13].copy_from_slice(&0x1234u16.to_be_bytes());
        ptt[13..16].copy_from_slice(&[0x00, 0x30, 0x39]);
        ptt[16..18].copy_from_slice(&201u16.to_be_bytes());
        let mut active = [0u8; 21];
        active[0] = 4 << 5;
        let service = if algorithm == 0x80 { 0 } else { 0x40 };
        active[1..8].copy_from_slice(&[0x01, service, 0x00, 0xC9, 0x00, 0x30, 0x39]);
        let mut frames = voice.chunks(4);
        for superframe in 0..2 {
            for (slot, &lane) in SLOT_CHANNEL.iter().enumerate() {
                let body = match (lane, slot) {
                    (0, 0) if superframe == 0 => self.mac(slot, &ptt, &FACCH, DUID_FACCH),
                    (0, 11) => self.mac(slot, &active, &SACCH, DUID_SACCH),
                    (0, _) => self.voice(slot, frames.next().unwrap_or(&[])),
                    _ => self.idle(slot),
                };
                self.slot(slot, body);
            }
        }
        let mut end = [0u8; 18];
        end[0] = 2 << 5;
        end[16..18].copy_from_slice(&201u16.to_be_bytes());
        for slot in 0..SLOTS {
            let body = if slot == 0 {
                self.mac(slot, &end, &FACCH, DUID_FACCH)
            } else {
                self.idle(slot)
            };
            self.slot(slot, body);
        }
    }

    /// The dibits as H-DQPSK at 48 kHz, `offset_hz` off tune.
    fn iq(&self, offset_hz: f64) -> Vec<Complex<f32>> {
        const STEPS: [u32; 4] = [1, 3, 7, 5];
        let points = (0..8)
            .map(|step| Complex::from_polar(1.0, step as f32 * std::f32::consts::FRAC_PI_4))
            .collect();
        let table = Constellation::from_points(points, (0..8).collect()).expect("8 phases");
        let shape = pulse::root_raised_cosine(8.0, 0.2, 8, Norm::Energy);
        let params = LinearParams::new(table, shape, 8).expect("H-DQPSK");
        let mut phase = 0u32;
        let labels: Vec<u32> = self
            .dibits
            .iter()
            .map(|&dibit| {
                phase = (phase + STEPS[usize::from(dibit)]) % 8;
                phase
            })
            .collect();
        LinearMod::transmission(&params, &labels)
            .into_iter()
            .enumerate()
            .map(|(index, sample)| {
                let turn = std::f64::consts::TAU * offset_hz * index as f64 / INPUT_RATE_HZ;
                sample * Complex::from_polar(0.5, turn as f32)
            })
            .collect()
    }
}

/// Eight four-frame bursts and a two-frame one: logical channel 0 less the PTT and SACCH.
const CALL_FRAMES: usize = 8 * 4 + 2;

fn a_call(algorithm: u8) -> Air {
    let mut air = Air::new(SITE);
    for _ in 0..2 {
        air.idle_superframe();
    }
    air.call(algorithm, &half_rate_frames(CALL_FRAMES));
    air.idle_superframe();
    air
}

#[test]
fn a_clear_call_plays_its_voice() {
    for offset_hz in [-1_600.0, 0.0, 400.0] {
        let iq = a_call(0x80).iq(offset_hz);
        let (frames, audio) = decode_with_audio(&mut channel(P25p2Slots::Both), &iq);
        assert_tone_audio(&audio, CALL_FRAMES);
        let ptt = frames
            .iter()
            .find(|frame| frame.kind == DvFrameKind::Header)
            .unwrap_or_else(|| panic!("no PTT at {offset_hz} Hz: {frames:#?}"));
        assert_eq!(ptt.source, Some(12345));
        assert_eq!(ptt.destination, Some(201));
        assert_eq!(ptt.encrypted, Some(false));
        assert_eq!(ptt.slot, Some(1));
        assert_eq!(ptt.network_id, Some(SITE.wacn));
        assert_eq!(ptt.color_code, Some(SITE.nac));
        assert!(
            frames
                .iter()
                .any(|frame| frame.kind == DvFrameKind::Terminator)
        );
        let user = frames
            .iter()
            .find(|frame| frame.opcode.as_deref() == Some("group voice channel user"))
            .expect("the call's SACCH");
        assert_eq!(user.destination, Some(201));
    }
}

#[test]
fn an_encrypted_call_stays_muted() {
    let (frames, audio) = decode_with_audio(&mut channel(P25p2Slots::Both), &a_call(0x84).iq(0.0));
    let ptt = frames
        .iter()
        .find(|frame| frame.kind == DvFrameKind::Header)
        .expect("PTT");
    assert_eq!(ptt.algorithm_id, Some(0x84));
    assert_eq!(ptt.key_id, Some(0x1234));
    assert_eq!(ptt.encrypted, Some(true));
    assert_eq!(audio.len(), CALL_FRAMES * 960);
    assert!(audio.iter().all(|&sample| sample == 0.0));
}

#[test]
fn the_other_slot_is_left_out() {
    let (frames, audio) = decode_with_audio(&mut channel(P25p2Slots::Two), &a_call(0x80).iq(0.0));
    assert!(audio.is_empty());
    assert!(!frames.is_empty());
    assert!(
        frames.iter().all(|frame| frame.slot != Some(1)),
        "{frames:#?}"
    );
}

#[test]
fn clear_signalling_decodes_before_the_seed_is_known() {
    let mut air = Air::new(SITE);
    let mut pdu = [0u8; 18];
    pdu[0] = 4 << 5;
    pdu[1..10].copy_from_slice(&[0x40, 0x00, 0x10, 0x05, 0x00, 0xC9, 0x00, 0x30, 0x39]);
    for _ in 0..2 {
        for slot in 0..SLOTS {
            let body = if slot == 4 {
                air.mac(slot, &pdu, &FACCH, DUID_FACCH_CLEAR)
            } else {
                scrambled([0; BODY_DIBITS], &air.masks, slot, DUID_DATCH)
            };
            air.slot(slot, body);
        }
    }
    let frames = decode(&mut channel(P25p2Slots::Both), &air.iq(0.0));
    let grant = frames
        .iter()
        .find(|frame| frame.opcode.as_deref() == Some("group voice channel grant"))
        .expect("grant");
    assert_eq!(grant.channel, Some(0x1005));
    assert_eq!(grant.destination, Some(201));
    assert_eq!(grant.source, Some(12345));
    assert_eq!(grant.color_code, None);
}

#[test]
fn noise_decodes_to_nothing() {
    let noise = sdrmm_decoder_test_support::complex_noise(29, 0.5, 400_000);
    assert!(decode(&mut channel(P25p2Slots::Both), &noise).is_empty());
}

#[test]
fn a_superframe_is_twelve_thirty_millisecond_slots() {
    assert_eq!(PACKET_DIBITS as f64 / SYMBOL_RATE, 0.03);
    assert_eq!(SLOTS * PACKET_DIBITS * 2, 4_320);
}
