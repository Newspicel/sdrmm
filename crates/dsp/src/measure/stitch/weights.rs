use super::{STITCH_FFT, StitchOptions, Stitcher, floor::POWER_FLOOR_DB};
use crate::{math::db::Decibels, math::stats::median};

const UPDATE_FRAMES: u64 = 64;
const FLATTEN_DB: f32 = 6.0;
const MAX_EQUALISE_DB: f32 = 30.0;
const MAX_QUALITY_DB: f32 = 60.0;
const SPUR_ABOVE_DB: f32 = 12.0;
const PARTNER_ABOVE_DB: f32 = 3.0;
const SPUR_FRAMES: u32 = 8;
const SPUR_WINDOW: u16 = (1 << 10) - 1;
/// A lane whose noise reads this low is dead: no input, nothing to equalise against.
const DEAD_DB: f32 = -250.0;

/// What a tap's gain and blend weight follow from, once the floors are known.
#[derive(Clone, Copy)]
struct Shaping {
    options: StitchOptions,
    ready: bool,
    reference_db: f32,
}

impl Shaping {
    fn gain_db(self, lane_gain_db: f32, floor_db: f32, live: bool) -> f32 {
        if self.ready && live && self.options.equalise && self.options.flatten {
            (self.reference_db - floor_db)
                .clamp(lane_gain_db - FLATTEN_DB, lane_gain_db + FLATTEN_DB)
        } else {
            lane_gain_db
        }
    }

    fn quality(self, floor_db: f32, equalised_db: f32) -> f32 {
        if !self.ready || !self.options.snr_blend {
            return 1.0;
        }
        if floor_db <= DEAD_DB {
            return 0.0;
        }
        (self.reference_db - equalised_db)
            .clamp(-MAX_QUALITY_DB, MAX_QUALITY_DB)
            .db_to_power()
    }
}

impl Stitcher {
    pub(super) fn restart_floors(&mut self) {
        self.tracked = 0;
        self.ready = false;
        for entry in &mut self.overlaps {
            entry.low_seen = 0;
            entry.high_seen = 0;
        }
    }

    pub(super) fn update_weights(&mut self) {
        if self.ready {
            self.estimate_noise();
            self.flag_spurs();
        } else {
            for lane in &mut self.lanes {
                lane.forget_estimates();
            }
        }
        self.shape_taps();
        self.normalise_taps();
    }

    pub(super) fn track_floors(&mut self) {
        for lane in &mut self.lanes {
            lane.floor.track(&lane.spectrum, self.tracked);
        }
        if self.ready {
            self.watch_spurs();
        }
        self.tracked += 1;
        if self.tracked.is_multiple_of(UPDATE_FRAMES) {
            self.ready = true;
            self.update_weights();
        }
    }

    fn estimate_noise(&mut self) {
        for lane in &mut self.lanes {
            lane.floor.smooth(&mut self.ordered);
            self.scratch.clear();
            self.scratch.extend(
                lane.taps
                    .iter()
                    .map(|tap| lane.floor.smooth_db[tap.lane_bin]),
            );
            lane.noise_db = median(&mut self.scratch).unwrap_or(POWER_FLOOR_DB);
        }
        let (sum, count) = self
            .lanes
            .iter()
            .filter(|lane| lane.noise_db > DEAD_DB)
            .fold((0.0f32, 0u32), |(sum, count), lane| {
                (sum + lane.noise_db, count + 1)
            });
        self.reference_db = if count == 0 { 0.0 } else { sum / count as f32 };
        let equalise = self.options.equalise;
        for lane in &mut self.lanes {
            lane.gain_db = if equalise && lane.noise_db > DEAD_DB {
                (self.reference_db - lane.noise_db).clamp(-MAX_EQUALISE_DB, MAX_EQUALISE_DB)
            } else {
                0.0
            };
        }
    }

    /// Flags a shared bin as a spur in the lane that kept hearing it while its partner did not.
    fn flag_spurs(&mut self) {
        let (lanes, matches, overlaps) = (&mut self.lanes, &self.matches, &self.overlaps);
        for lane in lanes.iter_mut() {
            lane.spur_bins = 0;
            lane.floor.spur.fill(false);
        }
        if !self.options.spur_reject {
            return;
        }
        for pair in matches {
            for entry in &overlaps[pair.start..pair.end] {
                if entry.low_seen.count_ones() >= SPUR_FRAMES {
                    lanes[pair.low].flag_spur(entry.low_bin);
                }
                if entry.high_seen.count_ones() >= SPUR_FRAMES {
                    lanes[pair.high].flag_spur(entry.high_bin);
                }
            }
        }
    }

    fn watch_spurs(&mut self) {
        let (lanes, matches, overlaps) = (&self.lanes, &self.matches, &mut self.overlaps);
        for pair in matches {
            let (low, high) = (&lanes[pair.low].floor, &lanes[pair.high].floor);
            for entry in &mut overlaps[pair.start..pair.end] {
                let low_above = low.above_db(entry.low_bin);
                let high_above = high.above_db(entry.high_bin);
                entry.low_seen = remember(
                    entry.low_seen,
                    low_above > SPUR_ABOVE_DB && high_above < PARTNER_ABOVE_DB,
                );
                entry.high_seen = remember(
                    entry.high_seen,
                    high_above > SPUR_ABOVE_DB && low_above < PARTNER_ABOVE_DB,
                );
            }
        }
    }

    fn shape_taps(&mut self) {
        let shaping = Shaping {
            options: self.options,
            ready: self.ready,
            reference_db: self.reference_db,
        };
        for lane in &mut self.lanes {
            let live = lane.noise_db > DEAD_DB;
            for tap in &mut lane.taps {
                let floor_db = lane.floor.smooth_db[tap.lane_bin];
                let gain_db = shaping.gain_db(lane.gain_db, floor_db, live);
                tap.gain = gain_db.db_to_amplitude();
                tap.quality = if lane.floor.spur[tap.lane_bin] {
                    0.0
                } else {
                    shaping.quality(floor_db, floor_db + gain_db)
                };
            }
        }
    }

    /// Scales every tap so the lanes reaching one output bin sum to the crossfade, split by
    /// quality when SNR blending weighs them.
    fn normalise_taps(&mut self) {
        self.sums.fill(0.0);
        self.blend_sums.fill(0.0);
        for tap in self.lanes.iter().flat_map(|lane| &lane.taps) {
            self.sums[tap.out_bin] += tap.mask;
            self.blend_sums[tap.out_bin] += tap.mask * tap.quality;
        }
        let scale = 1.0 / STITCH_FFT as f32;
        for tap in self.lanes.iter_mut().flat_map(|lane| &mut lane.taps) {
            let (mask_sum, blend_sum) = (self.sums[tap.out_bin], self.blend_sums[tap.out_bin]);
            tap.share = if blend_sum > f32::MIN_POSITIVE {
                mask_sum.min(1.0) * tap.mask * tap.quality / blend_sum
            } else {
                tap.mask / mask_sum.max(1.0)
            };
            tap.weight = tap.share * tap.gain * scale;
        }
    }
}

fn remember(seen: u16, now: bool) -> u16 {
    ((seen << 1) | u16::from(now)) & SPUR_WINDOW
}
