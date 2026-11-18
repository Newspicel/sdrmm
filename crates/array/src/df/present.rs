use sdrmm_wire::channel::ChannelStatus;
use sdrmm_wire::present::{Unit, count, fields, number, percent};
use sdrmm_wire::processor::view::{BearingView, Needle, NeedleWeight, ProcessorView};

use crate::Report;
use crate::present::{db, degrees, flags, ranked, snr_health};
use sdrmm_wire::{DfPeak, DfReading};

fn df_peak_text(peak: &DfPeak) -> String {
    let deg = peak.true_deg.unwrap_or(peak.relative_deg).rem_euclid(360.0);
    format!(
        "{deg:.0}° {:.0}% ±{:.0}°",
        peak.confidence.clamp(0.0, 1.0) * 100.0,
        peak.sigma_deg
    )
}

fn df_needles(peaks: &[DfPeak]) -> Vec<Needle> {
    let mut needles = Vec::with_capacity(peaks.len() * 2);
    for (rank, peak) in peaks.iter().enumerate() {
        needles.push(Needle {
            relative_deg: peak.relative_deg,
            true_deg: peak.true_deg,
            weight: if rank == 0 {
                NeedleWeight::Primary
            } else {
                NeedleWeight::Secondary
            },
        });
        if let Some(mirror) = peak.mirror_deg {
            needles.push(Needle {
                relative_deg: mirror,
                true_deg: peak.mirror_true_deg,
                weight: NeedleWeight::Mirror,
            });
        }
    }
    needles
}

impl Report for DfReading {
    fn status(&self) -> ChannelStatus {
        let first = self.peaks.first();
        let mut readouts = fields(&[
            (
                "bearing",
                "Bearing",
                degrees(first.and_then(|peak| peak.true_deg)),
            ),
            (
                "relative",
                "Rel",
                degrees(first.map(|peak| peak.relative_deg)),
            ),
            (
                "sigma",
                "±",
                number(first.map(|peak| peak.sigma_deg), Unit::Degrees, 1),
            ),
            ("fit", "Fit", percent(self.fit)),
            ("snr", "SNR", db(Some(self.snr_db), 0)),
            ("sources", "Src", count(Some(self.sources))),
        ]);
        readouts.extend(ranked(&self.peaks, df_peak_text));
        ChannelStatus {
            health: snr_health(Some(self.snr_db)),
            readouts,
            choices: Vec::new(),
        }
    }

    fn view(&self) -> ProcessorView {
        ProcessorView {
            flags: flags(&[
                (self.squelched, "Squelch", "Peak below squelch"),
                (
                    self.azimuth_deg.is_none(),
                    "No heading",
                    "Wire a GPS with heading",
                ),
                (
                    self.station.is_none(),
                    "No position",
                    "Wire a GPS to the array to send bearings",
                ),
                (
                    self.aliasing,
                    "Aliased",
                    "Elements more than half a wavelength apart",
                ),
                (
                    self.mode_aliasing,
                    "Mode alias",
                    "Too high for phase modes on this circle",
                ),
                (
                    self.mirror,
                    "Mirror",
                    "A line array cannot tell front from back",
                ),
                (
                    self.rotating,
                    "Rotating",
                    "Turning too fast, blocks skipped",
                ),
                (
                    self.singular,
                    "Singular",
                    "Covariance could not be inverted",
                ),
                (
                    self.table_out_of_range,
                    "Table off",
                    "Outside the calibration table",
                ),
            ]),
            bearing: Some(BearingView {
                spectrum: self.pseudospectrum.clone(),
                heading_deg: self.azimuth_deg,
                needles: df_needles(&self.peaks),
                sigma_deg: self.peaks.first().map(|peak| peak.sigma_deg),
            }),
            ..ProcessorView::default()
        }
    }
}
