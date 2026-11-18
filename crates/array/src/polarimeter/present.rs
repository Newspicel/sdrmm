use sdrmm_wire::channel::ChannelStatus;
use sdrmm_wire::present::{Unit, fields, number, percent, text_of};
use sdrmm_wire::processor::view::{EllipseView, ProcessorView};

use super::{Hand, PolarimeterReading};
use crate::Report;
use crate::present::{db, snr_health};

const fn hand_text(hand: Hand) -> &'static str {
    match hand {
        Hand::Right => "RH",
        Hand::Left => "LH",
        Hand::Linear => "Lin",
    }
}

impl Report for PolarimeterReading {
    fn status(&self) -> ChannelStatus {
        let readouts = fields(&[
            ("i", "I", db(Some(self.i_db), 1)),
            ("degree", "Pol", percent(self.degree.clamp(0.0, 1.0))),
            ("q", "Q", number(Some(self.q), Unit::None, 2)),
            (
                "tilt",
                "Tilt",
                number(Some(self.angle_deg), Unit::Degrees, 0),
            ),
            ("u", "U", number(Some(self.u), Unit::None, 2)),
            (
                "ellipticity",
                "Ellip",
                number(Some(self.ellipticity_deg), Unit::Degrees, 0),
            ),
            ("v", "V", number(Some(self.v), Unit::None, 2)),
            ("hand", "Hand", text_of(hand_text(self.hand))),
            ("snr", "SNR", db(self.snr_db, 0)),
        ]);
        ChannelStatus {
            health: snr_health(self.snr_db),
            readouts,
            choices: Vec::new(),
        }
    }

    fn view(&self) -> ProcessorView {
        ProcessorView {
            ellipse: Some(EllipseView {
                angle_deg: self.angle_deg,
                ellipticity_deg: self.ellipticity_deg,
                v: self.v,
            }),
            ..ProcessorView::default()
        }
    }
}
