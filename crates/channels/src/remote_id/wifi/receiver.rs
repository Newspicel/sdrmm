use num_complex::Complex;
use sdrmm_wire::{RemoteIdFrame, RemoteIdPhy};

use super::{Mpdu, channel_number, dsss::Dsss, frame, ofdm::Ofdm};
use crate::remote_id::tracker::Tracker;

const DC_ALPHA: f32 = 1.0 / 4_096.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Burst {
    pub bytes: Vec<u8>,
    pub phy: RemoteIdPhy,
    pub level_dbfs: f32,
    pub sample: u64,
}

pub(crate) fn level_dbfs(samples: &[Complex<f32>]) -> f32 {
    let mean = samples.iter().map(Complex::norm_sqr).sum::<f32>() / samples.len().max(1) as f32;
    10.0 * mean.max(1e-20).log10()
}

pub(crate) struct Wifi {
    channel: Option<u8>,
    dc: Complex<f32>,
    blocked: Vec<Complex<f32>>,
    dsss: Dsss,
    ofdm: Ofdm,
    bursts: Vec<Burst>,
    reported: u32,
}

impl Wifi {
    pub(crate) fn new(frequency_hz: f64) -> Self {
        Self {
            channel: channel_number(frequency_hz),
            dc: Complex::new(0.0, 0.0),
            blocked: Vec::new(),
            dsss: Dsss::new(),
            ofdm: Ofdm::new(),
            bursts: Vec::new(),
            reported: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.dc = Complex::new(0.0, 0.0);
        self.dsss.reset();
        self.ofdm.reset();
    }

    fn rejected(&self) -> u32 {
        self.dsss.rejected.saturating_add(self.ofdm.rejected)
    }

    pub(crate) fn process(
        &mut self,
        iq: &[Complex<f32>],
        tracker: &mut Tracker,
        out: &mut Vec<RemoteIdFrame>,
    ) {
        self.blocked.clear();
        let dc = &mut self.dc;
        self.blocked.extend(iq.iter().map(|&sample| {
            *dc += (sample - *dc) * DC_ALPHA;
            sample - *dc
        }));
        self.bursts.clear();
        self.dsss.process(&self.blocked, &mut self.bursts);
        self.ofdm.process(&self.blocked, &mut self.bursts);
        self.bursts.sort_unstable_by_key(|burst| burst.sample);
        let rejected = self.rejected();
        tracker.reject(rejected.saturating_sub(self.reported));
        self.reported = rejected;
        for burst in &self.bursts {
            let mpdu = Mpdu {
                bytes: &burst.bytes,
                phy: burst.phy,
                channel: self.channel,
                level_dbfs: burst.level_dbfs,
            };
            out.extend(frame(&mpdu, tracker));
        }
    }
}
