pub(crate) mod coding;
pub(crate) mod link;
mod phy;
mod records;
mod transport;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily, EventKind,
    WMBUS_BANDWIDTH_HZ, WMBUS_SAMPLE_RATE_HZ, WmbusMessage, WmbusParams,
};

use self::{
    phy::{Burst, Family, Lane},
    transport::Keys,
};
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate, datalink,
};

const RATE: f64 = WMBUS_SAMPLE_RATE_HZ;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "wmbus".to_owned(),
    name: "Wireless M-Bus".to_owned(),
    summary: "Water, heat and energy meters in modes S, T and C".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: WMBUS_BANDWIDTH_HZ,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![EventKind::Wmbus],
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<WmbusParams, ChannelError> {
    match &settings.params {
        ChannelParams::Wmbus(params) => {
            params.validate().map_err(ChannelError::InvalidSettings)?;
            Ok(params.clone())
        }
        other => Err(ChannelError::InvalidSettings(format!(
            "wmbus channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn occupied_band() -> (f64, f64) {
    let half = WMBUS_BANDWIDTH_HZ / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    datalink::channel_filter(RATE, WMBUS_BANDWIDTH_HZ / 2.0)
}

struct Decoder {
    keys: Keys,
    rejected: u32,
}

impl Decoder {
    fn burst(&mut self, burst: &Burst<'_>, out: &mut ChannelOutputs) {
        match self.message(burst) {
            Some(message) => out.events.push(DecoderEvent::Wmbus(message)),
            None => self.rejected = self.rejected.saturating_add(1),
        }
    }

    fn message(&self, burst: &Burst<'_>) -> Option<WmbusMessage> {
        let payload = link::unpack(burst.format, burst.raw)?;
        let frame = payload.as_slice();
        let reading = transport::read(frame, &self.keys)?;
        let via = (reading.meter != reading.link).then(|| reading.link.station());
        Some(WmbusMessage {
            mode: burst.mode,
            format: burst.format,
            control: frame[1],
            manufacturer: reading.meter.manufacturer(),
            id: reading.meter.id(),
            version: reading.meter.version,
            device_type: reading.meter.device_type,
            via,
            ci: reading.ci,
            access: reading.access,
            status: reading.status,
            security: reading.security,
            records: reading.records,
            frame: datalink::hex(frame),
            corrected: burst.repaired,
            snr_db: burst.snr_db,
        })
    }
}

pub struct WmbusChannel {
    fast: Box<Lane>,
    slow: Box<Lane>,
    decoder: Decoder,
}

impl ChannelRx for WmbusChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        Ok(Self {
            fast: Box::new(Lane::new(Family::Fast)),
            slow: Box::new(Lane::new(Family::Slow)),
            decoder: Decoder {
                keys: Keys::new(&params.keys),
                rejected: 0,
            },
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.decoder.keys = Keys::new(&params(&settings)?.keys);
        Ok(())
    }

    fn retuned(&mut self) {
        self.fast.reset();
        self.slow.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let decoder = &mut self.decoder;
        let mut sink = |burst: Burst<'_>| decoder.burst(&burst, out);
        for &sample in iq {
            self.fast.push(sample, &mut sink);
            self.slow.push(sample, &mut sink);
        }
    }

    fn rejected(&self) -> u32 {
        self.decoder.rejected
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::fixture;
