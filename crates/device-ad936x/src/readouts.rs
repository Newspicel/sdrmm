use sdrmm_device::DeviceError;
use sdrmm_wire::Readout;

use crate::{
    iio::{Client, Context, Direction},
    layout::Layout,
};

const TEMPERATURE_CHANNEL: &str = "temp0";
const INPUT: &str = "input";
const RAW: &str = "raw";
const OFFSET: &str = "offset";
const SCALE: &str = "scale";
const RSSI: &str = "rssi";
const XADC: &str = "xadc";
const DAC: &str = "ad5660mp";
const DAC_CHANNEL: &str = "voltage";
const DAC_MODE: &str = "dac_mode";
const DAC_LOCKED: &str = "dac_locked";
const DAC_DISCIPLINED: &str = "0";

pub(crate) const TEMPERATURE: &str = "temperature";
pub(crate) const FPGA_TEMPERATURE: &str = "fpga_temperature";
pub(crate) const SIGNAL: &str = "rssi";
pub(crate) const REFERENCE: &str = "reference";

#[derive(Clone, Copy, Debug, PartialEq)]
struct Xadc {
    offset: f64,
    scale: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Probes {
    temperature: bool,
    fpga: Option<Xadc>,
    rssi: bool,
    reference: bool,
}

impl Probes {
    pub(crate) fn read(client: &Client, context: &Context, layout: &Layout) -> Self {
        let phy = context.device(&layout.phy);
        Self {
            temperature: phy
                .and_then(|device| device.channel(TEMPERATURE_CHANNEL, false))
                .is_some_and(|channel| channel.has(INPUT)),
            fpga: xadc(client, context),
            rssi: layout.port(false, 0).is_some_and(|port| {
                phy.and_then(|device| device.channel(port, false))
                    .is_some_and(|channel| channel.has(RSSI))
            }),
            reference: context
                .device(DAC)
                .and_then(|device| device.channel(DAC_CHANNEL, false))
                .is_some_and(|channel| channel.has(DAC_LOCKED) && channel.has(DAC_MODE)),
        }
    }

    pub(crate) fn sample(
        &self,
        client: &Client,
        layout: &Layout,
        lanes: usize,
    ) -> Result<Vec<Readout>, DeviceError> {
        let mut readouts = Vec::new();
        if self.temperature {
            readouts.push(Readout::number(
                TEMPERATURE,
                "Chip",
                chip_celsius(client, &layout.phy)?,
                "°C",
            ));
        }
        if let Some(xadc) = self.fpga {
            let raw = number(&client.read_channel_attr(
                XADC,
                Direction::In,
                TEMPERATURE_CHANNEL,
                RAW,
            )?)?;
            readouts.push(Readout::number(
                FPGA_TEMPERATURE,
                "FPGA",
                round_tenth((raw + xadc.offset) * xadc.scale / 1000.0),
                "°C",
            ));
        }
        if self.rssi {
            for lane in 0..lanes {
                let Some(port) = layout.port(false, lane) else {
                    continue;
                };
                let text = client.read_channel_attr(&layout.phy, Direction::In, port, RSSI)?;
                readouts.push(
                    Readout::number(SIGNAL, "RSSI", -number(&text)?, "dB").on_stream(lane as u32),
                );
            }
        }
        if self.reference
            && client
                .read_channel_attr(DAC, Direction::In, DAC_CHANNEL, DAC_MODE)?
                .trim()
                == DAC_DISCIPLINED
        {
            let locked = client.read_channel_attr(DAC, Direction::In, DAC_CHANNEL, DAC_LOCKED)?;
            readouts.push(Readout::flag(REFERENCE, "Ref", locked.trim() == "1"));
        }
        Ok(readouts)
    }
}

pub(crate) fn chip_celsius(client: &Client, phy: &str) -> Result<f64, DeviceError> {
    let millidegrees =
        number(&client.read_channel_attr(phy, Direction::In, TEMPERATURE_CHANNEL, INPUT)?)?;
    Ok(round_tenth(millidegrees / 1000.0))
}

fn xadc(client: &Client, context: &Context) -> Option<Xadc> {
    let channel = context.device(XADC)?.channel(TEMPERATURE_CHANNEL, false)?;
    if !(channel.has(RAW) && channel.has(OFFSET) && channel.has(SCALE)) {
        return None;
    }
    let read = |attr: &str| {
        client
            .read_channel_attr(XADC, Direction::In, TEMPERATURE_CHANNEL, attr)
            .inspect_err(|e| tracing::debug!("{XADC}.{TEMPERATURE_CHANNEL}.{attr}: {e}"))
            .ok()
            .and_then(|text| number(&text).ok())
    };
    Some(Xadc {
        offset: read(OFFSET)?,
        scale: read(SCALE)?,
    })
}

fn number(text: &str) -> Result<f64, DeviceError> {
    text.split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .ok_or_else(|| DeviceError::Io(format!("a readout reads {text:?}")))
}

fn round_tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_is_its_leading_number_and_anything_else_is_an_error() {
        assert_eq!(number("86.75 dB").expect("a number"), 86.75);
        assert_eq!(number("43860\n").expect("a number"), 43_860.0);
        assert!(number("ERROR").is_err());
        assert!(number("").is_err());
    }

    #[test]
    fn a_temperature_is_shown_to_a_tenth_of_a_degree() {
        assert_eq!(round_tenth(43.86), 43.9);
        assert_eq!(
            round_tenth((2609.0 - 2219.0) * 123.040_771_484 / 1000.0),
            48.0
        );
    }
}
