use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{Control, KindSpec, SettingField};
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;
pub(crate) mod ad;
mod beacon;
mod names;
mod repeats;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_modem::ble::{
    self as phy, Front, Packet, Sink,
    pdu::{self, Adi, AdvPdu},
};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};

use self::{
    ad::Fields,
    repeats::{Fnv, Repeats},
};
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, SymbolTap, check_rate,
};
use sdrmm_modem::cpm::Mapping;

const REPEAT_HOLD_S: f64 = 1.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "ble".to_owned(),
    name: "BLE advertisements".to_owned(),
    summary: "Bluetooth LE devices, beacons and trackers".to_owned(),
    family: DecoderFamily::UTILITY,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: BleLink::default().bandwidth_hz(),
    input_rate_hz: BleLink::default().input_rate_hz(),
    has_audio: false,
    emits: vec![EventKind::named("ble")],
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<BleParams, ChannelError> {
    sdrmm_decoder::settings_of::<BleParams>(settings)
}

pub(crate) fn occupied_band(link: BleLink) -> (f64, f64) {
    let half = link.bandwidth_hz() / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(link: BleLink) -> FilterSpec {
    match link {
        BleLink::Channel => FilterSpec::channel(phy::CHANNEL_HALF_PASSBAND_HZ),
        BleLink::Band => FilterSpec::Passthrough,
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

pub(crate) fn front(link: BleLink, frequency_hz: f64) -> Result<Front, ChannelError> {
    match link {
        BleLink::Channel => Front::channel(frequency_hz),
        BleLink::Band => Front::band(frequency_hz),
    }
    .map_err(|error| ChannelError::InvalidSettings(error.to_string()))
}

pub struct BleChannel {
    link: BleLink,
    frequency_hz: f64,
    front: Front,
    repeats: Repeats,
    unreadable: u32,
    mapping: Mapping,
}

impl NativeChannel for BleChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?;
        check_rate(ctx, &DESCRIPTOR, params.link.input_rate_hz())?;
        Ok(Self {
            link: params.link,
            frequency_hz: settings.frequency_hz,
            front: front(params.link, settings.frequency_hz)?,
            repeats: Repeats::new((phy::RATE_HZ * REPEAT_HOLD_S) as u64),
            unreadable: 0,
            mapping: Mapping::natural(2),
        })
    }
}

impl Channel for BleChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        if params.link.input_rate_hz() != self.link.input_rate_hz() {
            return Err(ChannelError::InvalidSettings(
                "this BLE link changes the input rate; rebuild the channel".to_owned(),
            ));
        }
        if params.link != self.link || settings.frequency_hz != self.frequency_hz {
            self.front = front(params.link, settings.frequency_hz)?;
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
        let mut events = EventSink {
            repeats: &mut self.repeats,
            events: &mut out.events,
            unreadable: &mut self.unreadable,
            tap: &mut out.symbols,
            mapping: &self.mapping,
        };
        self.front.process(iq, &mut events);
    }

    fn rejected(&self) -> u32 {
        self.front.rejected().saturating_add(self.unreadable)
    }
}

struct EventSink<'a> {
    repeats: &'a mut Repeats,
    events: &'a mut Vec<DecoderEvent>,
    unreadable: &'a mut u32,
    tap: &'a mut SymbolTap,
    mapping: &'a Mapping,
}

impl Sink for EventSink<'_> {
    fn packet(&mut self, packet: Packet<'_>) {
        let channel = packet.rf.map(phy::channel_index);
        let Some(pdu) = pdu::parse(packet.pdu, channel) else {
            *self.unreadable = self.unreadable.saturating_add(1);
            return;
        };
        let Some(repeats) = self.repeats.admit(key(&pdu), content(&pdu), packet.sample) else {
            return;
        };
        let fields = Fields::read(pdu.data);
        *self.unreadable = self.unreadable.saturating_add(fields.truncated);
        self.events
            .push(DecoderEvent::of(&advert(&pdu, &packet, repeats, fields)));
    }

    fn symbols(&mut self, soft: &[f32], offset_hz: f64) {
        self.tap
            .levels(soft, &[], self.mapping, phy::SYMBOL_RATE_HZ, offset_hz);
    }
}

fn adi(pdu: &AdvPdu<'_>) -> Option<Adi> {
    pdu.extended.and_then(|extended| extended.adi)
}

fn key(pdu: &AdvPdu<'_>) -> u64 {
    let set = adi(pdu).map_or(u8::MAX, |adi| adi.set);
    let kind = Fnv::new().bytes(&[pdu.kind as u8, set]);
    match pdu.sender {
        Some(sender) => kind
            .bytes(&sender.bytes)
            .bytes(&[u8::from(sender.random)])
            .finish(),
        None => kind.bytes(&data_id(pdu)).bytes(pdu.data).finish(),
    }
}

fn content(pdu: &AdvPdu<'_>) -> u64 {
    let target = pdu
        .target
        .map_or([0; pdu::ADDRESS_BYTES], |target| target.bytes);
    Fnv::new()
        .bytes(&target)
        .bytes(&data_id(pdu))
        .bytes(pdu.data)
        .finish()
}

fn data_id(pdu: &AdvPdu<'_>) -> [u8; 2] {
    adi(pdu).map_or([0xFF; 2], |adi| adi.data_id.to_le_bytes())
}

pub(crate) fn advert(
    pdu: &AdvPdu<'_>,
    packet: &Packet<'_>,
    repeats: u32,
    fields: Fields,
) -> BleAdvert {
    let extended = pdu.extended.unwrap_or_default();
    BleAdvert {
        pdu: pdu.kind.into(),
        phy: wire_phy(packet.phy),
        channel: packet.rf.map(phy::channel_index),
        address: pdu.sender.map(wire_address),
        target: pdu.target.map(wire_address),
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
        adi: extended.adi.map(Into::into),
        aux: extended.aux.map(Into::into),
        data: sdrmm_crypto::hex_string(pdu.data),
    }
}

pub(crate) fn ble_decoder() -> Native<BleParams> {
    let decoder = Native::new::<BleChannel>(|p: &BleParams| occupied_band(p.link))
        .filter(|p| Ok(channel_filter(p.link)))
        .input_rate(|p| p.link.input_rate_hz())
        .home(2_426_000_000.0)
        .form(ble_form)
        .kinds(ble_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn ble_form() -> Vec<SettingField> {
    vec![
        SettingField::new("link", "Link", Control::choice::<BleLink>())
            .title("Bluetooth channels to listen on"),
    ]
}

fn ble_kinds() -> Vec<KindSpec> {
    vec![crate::ble::ble_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
