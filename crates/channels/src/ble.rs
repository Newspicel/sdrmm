pub(crate) mod ad;
mod beacon;
mod names;
pub(crate) mod pdu;
mod repeats;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_modem::ble::{self as phy, Lane, Packet, Sink, band::Band};
use sdrmm_wire::{
    BleAdvert, BleLink, BleParams, BlePhy, ChannelDescriptor, ChannelParams, ChannelSettings,
    DecoderEvent, DecoderFamily,
};

use self::{
    ad::Fields,
    pdu::AdvPdu,
    repeats::{Fnv, Repeats},
};
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_rate, datalink,
};

const PASSBAND: f64 = 0.55;
const REPEAT_HOLD_S: f64 = 1.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "ble".to_owned(),
    name: "BLE advertisements".to_owned(),
    summary: "Bluetooth LE devices, beacons and trackers".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: BleLink::default().bandwidth_hz(),
    input_rate_hz: BleLink::default().input_rate_hz(),
    has_audio: false,
    decoder_kind: Some("ble".to_owned()),
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<BleParams, ChannelError> {
    match &settings.params {
        ChannelParams::Ble(params) => Ok(*params),
        other => Err(ChannelError::InvalidSettings(format!(
            "BLE channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn occupied_band(link: BleLink) -> (f64, f64) {
    let half = link.bandwidth_hz() / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(link: BleLink) -> ChannelFilter {
    match link {
        BleLink::Channel => {
            let rate = link.input_rate_hz();
            datalink::channel_filter(rate, rate * PASSBAND / 2.0)
        }
        BleLink::Band => ChannelFilter::Passthrough,
    }
}

#[must_use]
pub(crate) fn wire_phy(phy: phy::BlePhy) -> BlePhy {
    match phy {
        phy::BlePhy::Le1m => BlePhy::Le1m,
        phy::BlePhy::CodedS8 => BlePhy::LeCodedS8,
        phy::BlePhy::CodedS2 => BlePhy::LeCodedS2,
    }
}

fn invalid(error: impl std::fmt::Display) -> ChannelError {
    ChannelError::InvalidSettings(error.to_string())
}

pub(crate) enum Front {
    Channel(Box<Lane>),
    Band(Box<Band>),
}

impl Front {
    pub(crate) fn new(link: BleLink, frequency_hz: f64) -> Result<Self, ChannelError> {
        Ok(match link {
            BleLink::Channel => {
                let rf = phy::rf_channel_at(frequency_hz).ok_or_else(|| {
                    ChannelError::InvalidSettings(format!(
                        "{:.3} MHz is not a Bluetooth channel: 2402 to 2480 MHz in 2 MHz steps",
                        frequency_hz / 1e6
                    ))
                })?;
                Self::Channel(Box::new(
                    Lane::new(Some(rf), phy::channel_index(rf)).map_err(invalid)?,
                ))
            }
            BleLink::Band => Self::Band(Box::new(
                Band::new(link.input_rate_hz(), frequency_hz).map_err(invalid)?,
            )),
        })
    }

    pub(crate) fn reset(&mut self) {
        match self {
            Self::Channel(lane) => lane.reset(),
            Self::Band(band) => band.reset(),
        }
    }

    #[must_use]
    pub(crate) fn rejected(&self) -> u32 {
        match self {
            Self::Channel(lane) => lane.rejected(),
            Self::Band(band) => band.rejected(),
        }
    }

    pub(crate) fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        match self {
            Self::Channel(lane) => lane.process(iq, sink),
            Self::Band(band) => band.process(iq, sink),
        }
    }
}

pub struct BleChannel {
    link: BleLink,
    frequency_hz: f64,
    front: Front,
    repeats: Repeats,
    unreadable: u32,
}

impl ChannelRx for BleChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?;
        check_rate(ctx, &DESCRIPTOR, params.link.input_rate_hz())?;
        Ok(Self {
            link: params.link,
            frequency_hz: settings.frequency_hz,
            front: Front::new(params.link, settings.frequency_hz)?,
            repeats: Repeats::new((phy::RATE_HZ * REPEAT_HOLD_S) as u64),
            unreadable: 0,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        if params.link.input_rate_hz() != self.link.input_rate_hz() {
            return Err(ChannelError::InvalidSettings(
                "this BLE link changes the input rate; rebuild the channel".to_owned(),
            ));
        }
        if params.link != self.link || settings.frequency_hz != self.frequency_hz {
            self.front = Front::new(params.link, settings.frequency_hz)?;
            self.repeats.reset();
            self.unreadable = 0;
            self.link = params.link;
            self.frequency_hz = settings.frequency_hz;
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.front.reset();
        self.repeats.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let rejected = self.front.rejected();
        let mut events = Events {
            repeats: &mut self.repeats,
            events: &mut out.events,
            unreadable: &mut self.unreadable,
            rejected,
        };
        self.front.process(iq, &mut events);
    }
}

struct Events<'a> {
    repeats: &'a mut Repeats,
    events: &'a mut Vec<DecoderEvent>,
    unreadable: &'a mut u32,
    rejected: u32,
}

impl Sink for Events<'_> {
    fn packet(&mut self, packet: Packet<'_>) {
        let channel = packet.rf.map(phy::channel_index);
        let Some(pdu) = pdu::parse(packet.pdu, channel) else {
            *self.unreadable = self.unreadable.saturating_add(1);
            return;
        };
        let Some(repeats) = self.repeats.admit(key(&pdu), content(&pdu), packet.sample) else {
            return;
        };
        let rejected = self.rejected.saturating_add(*self.unreadable);
        self.events
            .push(DecoderEvent::Ble(advert(&pdu, &packet, repeats, rejected)));
    }
}

fn key(pdu: &AdvPdu<'_>) -> u64 {
    let set = pdu
        .extended
        .and_then(|extended| extended.adi)
        .map_or(u8::MAX, |adi| adi.set);
    let kind = Fnv::new().bytes(&[pdu.kind as u8, set]);
    match pdu.sender {
        Some(sender) => kind
            .bytes(&sender.bytes)
            .bytes(&[u8::from(sender.random)])
            .finish(),
        None => kind.bytes(pdu.data).finish(),
    }
}

fn content(pdu: &AdvPdu<'_>) -> u64 {
    let target = pdu
        .target
        .map_or([0; pdu::ADDRESS_BYTES], |target| target.bytes);
    Fnv::new().bytes(&target).bytes(pdu.data).finish()
}

pub(crate) fn advert(
    pdu: &AdvPdu<'_>,
    packet: &Packet<'_>,
    repeats: u32,
    rejected: u32,
) -> BleAdvert {
    let fields = Fields::read(pdu.data);
    let extended = pdu.extended.unwrap_or_default();
    BleAdvert {
        pdu: pdu.kind,
        phy: wire_phy(packet.phy),
        channel: packet.rf.map(phy::channel_index),
        address: pdu.sender.map(|sender| sender.wire()),
        target: pdu.target.map(|target| target.wire()),
        level_dbfs: packet.level_dbfs,
        repeats,
        name: fields.name,
        flags: fields.flags,
        tx_power_dbm: fields.tx_power_dbm.or(extended.tx_power_dbm),
        appearance: fields.appearance,
        services: fields.services,
        manufacturer: fields.manufacturer,
        beacon: fields.beacon,
        uri: fields.uri,
        adi: extended.adi,
        aux: extended.aux,
        data: ad::hex(pdu.data),
        rejected,
    }
}

#[cfg(test)]
mod tests;
