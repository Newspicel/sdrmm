use num_complex::Complex;
use sdrmm_dsp::{Channelizer, Nco};

use super::{
    CHANNEL_SPACING_HZ, channel_index, gfsk,
    receiver::{Lane, Packet, Syncs},
    rf_channel_hz,
};
use crate::ChannelError;

const CUTOFF_HZ: f64 = 1_100_000.0;
const EDGE_HZ: f64 = 1_500_000.0;
const RF_CHANNELS: u8 = 40;

struct Plan {
    shift_hz: f64,
    lanes: Vec<(i32, u8)>,
}

fn plan(rate: f64, center_hz: f64) -> Plan {
    let reach = rate / 2.0 - EDGE_HZ;
    let first = rf_channel_hz(0) - center_hz;
    let shift_hz = first - CHANNEL_SPACING_HZ * (first / CHANNEL_SPACING_HZ).round();
    let lanes = (0..RF_CHANNELS)
        .filter_map(|rf| {
            let offset = rf_channel_hz(rf) - center_hz;
            let bin = ((offset - shift_hz) / CHANNEL_SPACING_HZ).round() as i32;
            (offset.abs() <= reach).then_some((bin, rf))
        })
        .collect();
    Plan { shift_hz, lanes }
}

pub(crate) struct Band {
    shift: Option<Nco>,
    shifted: Vec<Complex<f32>>,
    channelizer: Channelizer,
    outputs: Vec<Vec<Complex<f32>>>,
    lanes: Vec<Lane>,
}

impl Band {
    pub(crate) fn new(rate: f64, center_hz: f64) -> Result<Self, ChannelError> {
        let bins = (rate / CHANNEL_SPACING_HZ).round() as usize;
        let plan = plan(rate, center_hz);
        if plan.lanes.is_empty() {
            return Err(ChannelError::InvalidSettings(format!(
                "no Bluetooth channel within {:.1} MHz of {:.3} MHz",
                rate / 2e6,
                center_hz / 1e6
            )));
        }
        let wanted: Vec<i32> = plan.lanes.iter().map(|&(bin, _)| bin).collect();
        let decimation = (rate / gfsk::RATE_HZ).round() as usize;
        let channelizer = Channelizer::new(bins, decimation, CUTOFF_HZ / rate, &wanted)
            .map_err(|error| ChannelError::InvalidSettings(error.to_string()))?;
        Ok(Self {
            shift: (plan.shift_hz.abs() > 1.0)
                .then(|| Nco::new(-plan.shift_hz as f32, rate as f32)),
            shifted: Vec::new(),
            outputs: vec![Vec::new(); wanted.len()],
            channelizer,
            lanes: plan
                .lanes
                .iter()
                .map(|&(_, rf)| Lane::new(Some(rf), channel_index(rf)))
                .collect(),
        })
    }

    pub(crate) fn reset(&mut self) {
        self.channelizer.reset();
        self.lanes.iter_mut().for_each(Lane::reset);
    }

    pub(crate) fn process(&mut self, iq: &[Complex<f32>], syncs: &Syncs, out: &mut Vec<Packet>) {
        let input = match &mut self.shift {
            Some(nco) => {
                self.shifted.clear();
                self.shifted.extend_from_slice(iq);
                nco.mix(&mut self.shifted);
                &self.shifted[..]
            }
            None => iq,
        };
        self.channelizer.process(input, &mut self.outputs);
        for (lane, samples) in self.lanes.iter_mut().zip(&self.outputs) {
            lane.process(samples, syncs, out);
        }
        out.sort_unstable_by_key(|packet| packet.sample);
    }
}

#[cfg(test)]
mod tests {
    use super::plan;

    #[test]
    fn twenty_megahertz_on_channel_38_reaches_nine_channels() {
        let plan = plan(20e6, 2_426e6);
        let channels: Vec<u8> = plan.lanes.iter().map(|&(_, rf)| rf).collect();
        assert_eq!(channels, (8..=16).collect::<Vec<u8>>());
        assert!(plan.shift_hz.abs() < 1.0);
        assert!(plan.lanes.iter().all(|&(bin, _)| (-5..5).contains(&bin)));
    }

    #[test]
    fn an_off_grid_centre_shifts_onto_the_channel_grid() {
        let plan = plan(20e6, 2_427e6);
        assert!((plan.shift_hz.abs() - 1e6).abs() < 1.0, "{}", plan.shift_hz);
        assert_eq!(plan.lanes.len(), 8);
    }
}
