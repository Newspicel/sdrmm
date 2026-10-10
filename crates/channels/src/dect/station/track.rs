use sdrmm_wire::{
    DectBand, DectCapability, DectCipherState, DectFrame, DectIdentity, DectSecurity, DectSide,
    DectUpdate, DectVoice,
};

use crate::dect::{
    burst::FRAME_SAMPLES,
    capabilities::{self, PARTS, Part},
    mac::{self, EncryptionCommand, EncryptionPhase, StaticInfo},
    voice::BearerVoice,
};

const SLOT_TOLERANCE: u64 = 200;
const MAX_HANDSETS: usize = 16;
const MULTIFRAME: u64 = 16;
const QT_FRAME: u64 = 8;
const ANCHOR_LIFETIME: u64 = FRAME_SAMPLES * 400;
const SLOT_JITTER: f64 = 4.0;
const DRIFT_PER_FRAME: f64 = FRAME_SAMPLES as f64 * 50e-6;
const EXPECTED_FRAMES: f64 = 8.0;

pub(super) fn set<T: PartialEq>(slot: &mut T, value: T, dirty: &mut bool) {
    if *slot != value {
        *slot = value;
        *dirty = true;
    }
}

pub(super) fn slot_distance(a: u64, b: u64) -> u64 {
    let delta = a.wrapping_sub(b) % FRAME_SAMPLES;
    delta.min(FRAME_SAMPLES - delta)
}

#[derive(Default)]
pub(super) struct Track {
    pub used: bool,
    pub anchor: u64,
    pub last: u64,
    pub last_emit: u64,
    pub emitted: bool,
    pub side: DectSide,
    pub tuned_carrier: Option<u8>,
    pub identity: Option<DectIdentity>,
    carrier: Option<u8>,
    slot_pair: Option<u8>,
    rf_carriers: Option<u16>,
    transceivers: Option<u8>,
    pscn: Option<u8>,
    extended_carriers: bool,
    pub multiframe: Option<u32>,
    parts: [Vec<DectCapability>; PARTS],
    capabilities: Vec<DectCapability>,
    pub security: DectSecurity,
    fmid: Option<u16>,
    pmid: Option<u32>,
    handsets: Vec<u32>,
    pub bursts: u32,
    pub level_dbfs: f32,
    qt_sample: Option<u64>,
    pub voice: Option<DectVoice>,
    pub bearer: Option<BearerVoice>,
}

impl Track {
    pub fn reset(&mut self, sample: u64, side: DectSide, carrier: Option<u8>) {
        *self = Self {
            used: true,
            anchor: sample,
            last: sample,
            side,
            tuned_carrier: carrier,
            ..Self::default()
        };
    }

    pub fn matches(&self, sample: u64, side: DectSide, carrier: Option<u8>) -> bool {
        self.used
            && self.side == side
            && self.tuned_carrier == carrier
            && slot_distance(sample, self.anchor) <= SLOT_TOLERANCE
    }

    pub fn expects(&self, sample: u64, side: DectSide, carrier: Option<u8>) -> bool {
        let frames = (sample.saturating_sub(self.anchor) as f64 / FRAME_SAMPLES as f64).round();
        self.used
            && self.bursts > 0
            && self.side == side
            && self.tuned_carrier == carrier
            && frames <= EXPECTED_FRAMES
            && slot_distance(sample, self.anchor) as f64 <= SLOT_JITTER + frames * DRIFT_PER_FRAME
    }

    pub fn mark_multiframe_start(&mut self, sample: u64) {
        self.qt_sample = Some(sample);
    }

    pub fn frame_number(&self, sample: u64) -> Option<u8> {
        let elapsed = (sample + FRAME_SAMPLES / 2).checked_sub(self.qt_sample?)?;
        if elapsed > ANCHOR_LIFETIME {
            return None;
        }
        Some(((QT_FRAME + elapsed / FRAME_SAMPLES) % MULTIFRAME) as u8)
    }

    pub fn encrypted(&self) -> bool {
        self.security.cipher_state == DectCipherState::Active
    }

    fn note_handset(&mut self, pmid: u32, dirty: &mut bool) {
        if self.handsets.contains(&pmid) {
            return;
        }
        if self.handsets.len() == MAX_HANDSETS {
            self.handsets.remove(0);
        }
        self.handsets.push(pmid);
        *dirty = true;
    }

    pub fn apply_static(&mut self, info: StaticInfo, band: DectBand, dirty: &mut bool) {
        set(&mut self.slot_pair, Some(info.slot_pair), dirty);
        set(&mut self.transceivers, Some(info.transceivers + 1), dirty);
        set(&mut self.rf_carriers, Some(info.rf_carriers), dirty);
        set(&mut self.pscn, Some(info.pscn), dirty);
        set(&mut self.extended_carriers, info.extended_carriers, dirty);
        if band.carrier_hz(info.carrier).is_some() {
            set(&mut self.carrier, Some(info.carrier), dirty);
        }
    }

    pub fn apply_capabilities(&mut self, part: Part, a_field: u64, dirty: &mut bool) {
        let mut found = Vec::new();
        let next = capabilities::decode(part, a_field, &mut found);
        if self.parts[part.index()] != found {
            self.parts[part.index()] = found;
            self.capabilities = self.parts.concat();
            *dirty = true;
        }
        let has = |capability| self.parts[part.index()].contains(&capability);
        match part {
            Part::Fixed => {
                let authentication = has(DectCapability::StandardAuthentication);
                let ciphering = has(DectCapability::StandardCiphering);
                set(
                    &mut self.security.authentication_supported,
                    Some(authentication),
                    dirty,
                );
                set(
                    &mut self.security.ciphering_supported,
                    Some(ciphering),
                    dirty,
                );
            }
            Part::Extended2 => {
                let dsaa2 = has(DectCapability::Dsaa2);
                let dsc2 = has(DectCapability::Dsc2);
                set(&mut self.security.dsaa2_supported, Some(dsaa2), dirty);
                set(&mut self.security.dsc2_supported, Some(dsc2), dirty);
            }
            Part::Extended | Part::Extended3 => {}
        }
        let part_two_absent =
            matches!(part, Part::Fixed | Part::Extended) && next.announced == Some(false);
        if part_two_absent {
            set(&mut self.security.dsaa2_supported, Some(false), dirty);
            set(&mut self.security.dsc2_supported, Some(false), dirty);
        }
    }

    pub fn apply_encryption(&mut self, a_field: u64, dirty: &mut bool) {
        let message = mac::encryption(a_field);
        let state = match (message.command, message.phase) {
            (EncryptionCommand::Stop, _) => DectCipherState::Stopped,
            (EncryptionCommand::Reserved, _) => return,
            (_, EncryptionPhase::Request) => DectCipherState::Requested,
            (_, EncryptionPhase::Confirm) => DectCipherState::Confirmed,
            (_, EncryptionPhase::Grant) => DectCipherState::Active,
            (_, EncryptionPhase::Reject) => DectCipherState::Clear,
        };
        let command = match message.command {
            EncryptionCommand::Start => "start encryption",
            EncryptionCommand::Stop => "stop encryption",
            EncryptionCommand::StartWithKeyIndex => "start encryption with key index",
            EncryptionCommand::Reserved => "reserved",
        };
        let phase = match message.phase {
            EncryptionPhase::Request => "request",
            EncryptionPhase::Confirm => "confirm",
            EncryptionPhase::Grant => "grant",
            EncryptionPhase::Reject => "reject",
        };
        set(&mut self.security.cipher_state, state, dirty);
        set(
            &mut self.security.last_command,
            Some(format!("{command}: {phase}")),
            dirty,
        );
        if let Some(index) = message.key_index {
            set(&mut self.security.cipher_key_index, Some(index), dirty);
        }
        if let Some(fmid) = message.fmid {
            set(&mut self.fmid, Some(fmid), dirty);
        }
        if let Some(pmid) = message.pmid {
            set(&mut self.pmid, Some(pmid), dirty);
            self.note_handset(pmid, dirty);
        }
        self.security.encryption_events += 1;
        *dirty = true;
    }

    pub fn voice_mut(&mut self, dirty: &mut bool) -> &mut DectVoice {
        if self.voice.is_none() {
            *dirty = true;
        }
        self.voice.get_or_insert_with(DectVoice::default)
    }

    pub fn frame(&self, update: DectUpdate, band: DectBand) -> DectFrame {
        let carrier = self.tuned_carrier.or(self.carrier);
        DectFrame {
            side: self.side,
            update,
            identity: self.identity.clone(),
            carrier,
            carrier_hz: carrier.and_then(|c| band.carrier_hz(c)),
            slot_pair: self.slot_pair,
            rf_carriers: self.rf_carriers,
            transceivers: self.transceivers,
            pscn: self.pscn,
            extended_carriers: self.extended_carriers,
            multiframe: self.multiframe,
            security: self.security.clone(),
            capabilities: self.capabilities.clone(),
            fmid: self.fmid,
            pmid: self.pmid,
            handsets: self.handsets.clone(),
            voice: self.voice,
            bursts: self.bursts,
            level_dbfs: self.level_dbfs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard_at(anchor: u64) -> Track {
        let mut track = Track::default();
        track.reset(anchor, DectSide::Rfp, None);
        track.bursts = 1;
        track
    }

    #[test]
    fn a_known_slot_is_expected_only_close_by_and_soon() {
        let track = heard_at(1_000);
        let at = |frames: u64, skew: u64| 1_000 + frames * FRAME_SAMPLES + skew;
        assert!(track.expects(at(1, 4), DectSide::Rfp, None));
        assert!(track.expects(at(8, 12), DectSide::Rfp, None));
        assert!(!track.expects(at(1, 20), DectSide::Rfp, None));
        assert!(!track.expects(at(9, 0), DectSide::Rfp, None));
        assert!(!track.expects(at(1, 0), DectSide::Pp, None));
        assert!(!track.expects(at(1, 0), DectSide::Rfp, Some(3)));
    }

    #[test]
    fn a_slot_never_decoded_is_not_expected() {
        let mut track = heard_at(1_000);
        track.bursts = 0;
        assert!(!track.expects(1_000 + FRAME_SAMPLES, DectSide::Rfp, None));
    }
}
