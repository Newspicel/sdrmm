mod track;

use sdrmm_wire::{DectBand, DectFrame, DectSide, DectUpdate};
use track::{Track, set, slot_distance};

use super::{
    bfield::BField,
    burst::{Burst, FRAME_SAMPLES, INPUT_RATE_HZ},
    capabilities::Part,
    identity::Rfpi,
    mac::{self, Header, Tail, VOICE_B_FIELD, a_field_crc_ok},
    voice::{BearerVoice, Playout},
};

const MAX_TRACKS: usize = 64;
const PAIR_TOLERANCE: u64 = 200;
const REFRESH_SAMPLES: u64 = (INPUT_RATE_HZ * 5.0) as u64;
const VOICE_REFRESH_SAMPLES: u64 = (INPUT_RATE_HZ * 0.25) as u64;
const DUPLEX_OFFSET: u64 = FRAME_SAMPLES / 2;

pub(crate) struct Tracker {
    tracks: Vec<Track>,
    band: DectBand,
}

impl Tracker {
    pub fn new(band: DectBand) -> Self {
        let mut tracks = Vec::with_capacity(MAX_TRACKS);
        tracks.resize_with(MAX_TRACKS, Track::default);
        Self { tracks, band }
    }

    pub fn set_band(&mut self, band: DectBand) {
        self.band = band;
    }

    pub fn clear(&mut self) {
        for track in &mut self.tracks {
            track.used = false;
        }
    }

    fn index_for(&mut self, burst: &Burst, side: DectSide) -> usize {
        if let Some(index) = self
            .tracks
            .iter()
            .position(|track| track.matches(burst.sample, side, burst.carrier))
        {
            return index;
        }
        let index = self
            .tracks
            .iter()
            .position(|track| !track.used)
            .unwrap_or_else(|| {
                self.tracks
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, track)| track.last)
                    .map_or(0, |(index, _)| index)
            });
        self.tracks[index].reset(burst.sample, side, burst.carrier);
        index
    }

    fn partner(&self, burst: &Burst) -> Option<usize> {
        let base_time = burst.sample.checked_sub(DUPLEX_OFFSET)?;
        self.tracks.iter().position(|track| {
            track.used
                && track.side == DectSide::Rfp
                && track.tuned_carrier == burst.carrier
                && slot_distance(base_time, track.anchor) <= PAIR_TOLERANCE
        })
    }

    pub fn apply(&mut self, burst: &Burst, playout: &mut Playout) -> Option<DectFrame> {
        let side = if burst.from_rfp {
            DectSide::Rfp
        } else {
            DectSide::Pp
        };
        let index = self.index_for(burst, side);
        let band = self.band;
        let track = &mut self.tracks[index];
        track.anchor = burst.sample;
        track.last = burst.sample;
        track.level_dbfs = burst.level_dbfs;

        if !a_field_crc_ok(burst.a_field) {
            track.crc_errors = track.crc_errors.saturating_add(1);
            track.pending_errors = true;
            return None;
        }
        track.bursts = track.bursts.saturating_add(1);

        let mut dirty = !track.emitted || track.pending_errors;
        track.pending_errors = false;
        let header = mac::header(burst.a_field, burst.from_rfp);
        let mut update = apply_tail(track, header, burst, band, &mut dirty);

        let carries_voice = burst.b_field.is_some() && header.ba == VOICE_B_FIELD;
        if let Some(field) = &burst.b_field
            && carries_voice
        {
            let mut voice_dirty = false;
            self.voice(index, burst, field, playout, &mut voice_dirty);
            if voice_dirty {
                update = DectUpdate::Voice;
                dirty = true;
            }
        }

        let track = &mut self.tracks[index];
        let refresh = if carries_voice {
            VOICE_REFRESH_SAMPLES
        } else {
            REFRESH_SAMPLES
        };
        let stale = burst.sample.saturating_sub(track.last_emit) >= refresh;
        if !dirty && !stale {
            return None;
        }
        track.last_emit = burst.sample;
        track.emitted = true;
        Some(track.frame(update, band))
    }

    fn voice(
        &mut self,
        index: usize,
        burst: &Burst,
        field: &BField,
        playout: &mut Playout,
        dirty: &mut bool,
    ) {
        let partner = if burst.from_rfp {
            Some(index)
        } else {
            self.partner(burst)
        };
        let base_time = if burst.from_rfp {
            Some(burst.sample)
        } else {
            burst.sample.checked_sub(DUPLEX_OFFSET)
        };
        let paired = partner.map(|at| &self.tracks[at]);
        let encrypted =
            self.tracks[index].encrypted() || paired.is_some_and(|track| track.encrypted());
        let frame = paired
            .zip(base_time)
            .and_then(|(track, at)| track.frame_number(at));
        let call = partner.unwrap_or(index);
        let track = &mut self.tracks[index];
        let Some(frame) = frame.filter(|_| !encrypted) else {
            let voice = track.voice_mut(dirty);
            if encrypted {
                voice.encrypted = voice.encrypted.saturating_add(1);
            } else {
                voice.unsynced = voice.unsynced.saturating_add(1);
            }
            set(&mut voice.playing, false, dirty);
            return;
        };
        let pcm =
            track
                .bearer
                .get_or_insert_with(BearerVoice::new)
                .decode(field, frame, burst.sample);
        let routed = playout.routes(call, burst.sample);
        let late = routed && !playout.place(burst.sample, &pcm);
        let voice = track.voice_mut(dirty);
        voice.frames = voice.frames.saturating_add(1);
        if !field.x_crc_ok() {
            voice.x_crc_errors = voice.x_crc_errors.saturating_add(1);
        }
        if late {
            voice.late = voice.late.saturating_add(1);
        }
        set(&mut voice.playing, routed, dirty);
    }
}

fn apply_tail(
    track: &mut Track,
    header: Header,
    burst: &Burst,
    band: DectBand,
    dirty: &mut bool,
) -> DectUpdate {
    match header.tail {
        Tail::Nt | Tail::NtConnectionless => {
            let identity = Rfpi::parse(mac::rfpi(burst.a_field)).describe();
            set(&mut track.identity, Some(identity), dirty);
            DectUpdate::Identity
        }
        Tail::Qt => {
            if burst.from_rfp {
                track.mark_multiframe_start(burst.sample);
            }
            apply_qt(track, burst.a_field, band, dirty)
        }
        Tail::Mt | Tail::MtFirst => {
            if mac::mt_head(burst.a_field) == 5 {
                track.apply_encryption(burst.a_field, dirty);
                DectUpdate::Encryption
            } else {
                DectUpdate::Bearer
            }
        }
        Tail::Pt => DectUpdate::Paging,
        Tail::Ct { .. } | Tail::Escape => DectUpdate::Bearer,
    }
}

fn apply_qt(track: &mut Track, a_field: u64, band: DectBand, dirty: &mut bool) -> DectUpdate {
    let qh = mac::qt_head(a_field);
    if let Some(part) = Part::from_qh(qh) {
        track.apply_capabilities(part, a_field, dirty);
        return DectUpdate::Capabilities;
    }
    match qh {
        0 | 1 => track.apply_static(mac::static_info(a_field), band, dirty),
        6 => {
            let multiframe = mac::multiframe_number(a_field);
            set(&mut track.multiframe, Some(multiframe), dirty);
        }
        _ => {}
    }
    DectUpdate::SystemInfo
}
