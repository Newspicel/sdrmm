use crate::registry::Native;
use sdrmm_wire::{Control, KindSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
pub use model::*;
pub(crate) mod coding;
pub(crate) mod link;
mod phy;
mod records;
mod transport;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};

use self::{
    phy::{Burst, Family, Lane},
    transport::Keys,
};
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel,
    check_input_rate, datalink,
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
    emits: vec![EventKind::named("wmbus")],
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<WmbusParams, ChannelError> {
    let params: WmbusParams = crate::settings_of(settings)?;
    params.validate().map_err(ChannelError::InvalidSettings)?;
    Ok(params)
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
            Some(message) => out.events.push(DecoderEvent::of(&message)),
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

impl NativeChannel for WmbusChannel {
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
}

impl ChannelRx for WmbusChannel {
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

const WMBUS_T_C: &[HzRange] = &[HzRange::mhz(868.85, 869.05)];

const WMBUS_S: &[HzRange] = &[HzRange::mhz(868.2, 868.4)];

fn wmbus_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "Wireless M-Bus T/C",
            &[Modulation::Fsk2],
            HzRange::new(120_000.0, 300_000.0),
            "100 kchip/s 2-FSK on the 868.95 MHz meter channel",
        )
        .symbol_rate(HzRange::new(90_000.0, 110_000.0))
        .deviation(HzRange::new(30_000.0, 90_000.0))
        .on(WMBUS_T_C),
        SignalSignature::new(
            "Wireless M-Bus S",
            &[Modulation::Fsk2],
            HzRange::new(80_000.0, 250_000.0),
            "32.768 kchip/s Manchester 2-FSK on the 868.3 MHz meter channel",
        )
        .symbol_rate(HzRange::new(30_000.0, 35_500.0))
        .deviation(HzRange::new(30_000.0, 90_000.0))
        .on(WMBUS_S),
    ]
}

fn wmbus_trial() -> sdrmm_wire::SignalTrial {
    sdrmm_wire::SignalTrial::proven_by(EventKind::named("wmbus"))
}

pub(crate) fn wmbus_decoder() -> Native<WmbusParams> {
    Native::new::<WmbusChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .home(crate::wmbus::WMBUS_T_C_HZ)
        .signatures(wmbus_signatures)
        .trial(wmbus_trial)
        .form(wmbus_form)
        .kinds(wmbus_kinds)
}

pub(crate) fn wmbus_form() -> Vec<SettingField> {
    let key = vec![
        SettingField::new(
            "meter",
            "Meter ID",
            Control::bounded_text("Meter ID", METER_DIGITS, false),
        ),
        SettingField::new(
            "key",
            "AES key",
            Control::bounded_text("AES key", KEY_DIGITS, true),
        ),
    ];
    vec![
        SettingField::new("keys", "Keys", Control::list(key, "none"))
            .title("AES keys per meter ID; frames from other meters stay locked")
            .quiet(Vec::<WmbusKey>::new()),
    ]
}

fn wmbus_kinds() -> Vec<KindSpec> {
    vec![crate::wmbus::wmbus_kind()]
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::fixture;
