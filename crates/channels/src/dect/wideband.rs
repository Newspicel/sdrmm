use num_complex::Complex;
use sdrmm_dsp::{Channelizer, Nco};
use sdrmm_wire::{DECT_CARRIER_SPACING_HZ, DectBand};

use super::burst::{Burst, Detector, INPUT_RATE_HZ, OCCUPIED_BANDWIDTH_HZ};
use crate::ChannelError;

const CUTOFF_HZ: f64 = 920_000.0;
const BIN_MULTIPLE: u8 = 4;
const IMAGE_REJECT_DB: f32 = 30.0;
const IMAGE_WINDOW: u64 = 64;

#[must_use]
pub(crate) fn bins(band: DectBand) -> usize {
    usize::from((band.carriers() + 2).next_multiple_of(BIN_MULTIPLE))
}

#[must_use]
pub(crate) fn input_rate(band: DectBand) -> f64 {
    bins(band) as f64 * DECT_CARRIER_SPACING_HZ
}

#[must_use]
pub(crate) fn half_span(band: DectBand) -> f64 {
    f64::from(band.carriers()) * DECT_CARRIER_SPACING_HZ / 2.0
}

struct Plan {
    shift_hz: f64,
    lanes: Vec<(i32, u8)>,
}

fn plan(band: DectBand, center_hz: f64) -> Plan {
    let rate = input_rate(band);
    let reach = rate / 2.0 - OCCUPIED_BANDWIDTH_HZ / 2.0;
    let first = band.carrier_hz(0).unwrap_or(center_hz) - center_hz;
    let shift_hz = first - DECT_CARRIER_SPACING_HZ * (first / DECT_CARRIER_SPACING_HZ).floor();
    let lanes = (0..band.carriers())
        .filter_map(|carrier| {
            let offset = band.carrier_hz(carrier)? - center_hz;
            let bin = ((offset - shift_hz) / DECT_CARRIER_SPACING_HZ).round() as i32;
            (offset.abs() <= reach).then_some((bin, carrier))
        })
        .collect();
    Plan { shift_hz, lanes }
}

pub(crate) struct Wideband {
    shift: Option<Nco>,
    shifted: Vec<Complex<f32>>,
    channelizer: Channelizer,
    lanes: Vec<Vec<Complex<f32>>>,
    detectors: Vec<Detector>,
}

impl Wideband {
    pub fn new(
        band: DectBand,
        center_hz: f64,
        accept_rfp: bool,
        accept_pp: bool,
    ) -> Result<Self, ChannelError> {
        let bins = bins(band);
        let rate = input_rate(band);
        let plan = plan(band, center_hz);
        if plan.lanes.is_empty() {
            return Err(ChannelError::InvalidSettings(format!(
                "no {} carrier within {:.3} MHz of {:.3} MHz",
                band.label(),
                rate / 2e6,
                center_hz / 1e6
            )));
        }
        let wanted: Vec<i32> = plan.lanes.iter().map(|&(bin, _)| bin).collect();
        let decimation = (rate / INPUT_RATE_HZ).round() as usize;
        let channelizer = Channelizer::new(bins, decimation, CUTOFF_HZ / rate, &wanted)
            .map_err(|error| ChannelError::InvalidSettings(error.to_string()))?;
        let shift =
            (plan.shift_hz.abs() > 1.0).then(|| Nco::new(-plan.shift_hz as f32, rate as f32));
        Ok(Self {
            shift,
            shifted: Vec::new(),
            lanes: vec![Vec::new(); wanted.len()],
            channelizer,
            detectors: plan
                .lanes
                .iter()
                .map(|&(_, carrier)| Detector::new(accept_rfp, accept_pp, Some(carrier)))
                .collect(),
        })
    }

    pub fn set_sides(&mut self, accept_rfp: bool, accept_pp: bool) {
        for detector in &mut self.detectors {
            detector.set_sides(accept_rfp, accept_pp);
        }
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Burst>) {
        let input = match &mut self.shift {
            Some(nco) => {
                self.shifted.clear();
                self.shifted.extend_from_slice(iq);
                nco.mix(&mut self.shifted);
                &self.shifted[..]
            }
            None => iq,
        };
        self.channelizer.process(input, &mut self.lanes);
        for (detector, lane) in self.detectors.iter_mut().zip(&self.lanes) {
            detector.process(lane, out);
        }
        out.sort_unstable_by_key(|burst| burst.sample);
        drop_images(out);
    }
}

fn drop_images(bursts: &mut Vec<Burst>) {
    let mut index = 0;
    while index < bursts.len() {
        let burst = bursts[index];
        let image = bursts.iter().any(|other| {
            other.carrier != burst.carrier
                && other.sample.abs_diff(burst.sample) <= IMAGE_WINDOW
                && other.level_dbfs - burst.level_dbfs >= IMAGE_REJECT_DB
        });
        if image {
            bursts.remove(index);
        } else {
            index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::DectBand;

    use super::{bins, input_rate, plan};

    #[test]
    fn the_eu_band_fits_twelve_bins_at_twenty_point_seven_megasamples() {
        assert_eq!(bins(DectBand::Eu), 12);
        assert_eq!(input_rate(DectBand::Eu), 20_736_000.0);
        assert_eq!(bins(DectBand::Us), 8);
        assert_eq!(input_rate(DectBand::Us), 13_824_000.0);
    }

    #[test]
    fn centred_on_the_band_every_carrier_gets_a_bin() {
        let eu = plan(DectBand::Eu, DectBand::Eu.center_hz());
        assert_eq!(eu.lanes.len(), 10);
        assert!((eu.shift_hz - 864_000.0).abs() < 1.0, "{}", eu.shift_hz);
        assert_eq!(eu.lanes.first(), Some(&(4, 0)));
        assert_eq!(eu.lanes.last(), Some(&(-5, 9)));
        let us = plan(DectBand::Us, DectBand::Us.center_hz());
        assert_eq!(us.lanes.len(), 5);
        assert!(us.shift_hz.abs() < 1.0);
        assert_eq!(us.lanes.first(), Some(&(-2, 0)));
    }

    #[test]
    fn a_window_off_the_band_centre_keeps_only_the_carriers_it_reaches() {
        let edge = DectBand::Eu.carrier_hz(0).unwrap_or_default();
        let lanes = plan(DectBand::Eu, edge).lanes;
        let carriers: Vec<u8> = lanes.iter().map(|&(_, carrier)| carrier).collect();
        assert_eq!(carriers, vec![0, 1, 2, 3, 4, 5]);
        assert!(lanes.iter().all(|&(bin, _)| (-6..6).contains(&bin)));
    }
}
