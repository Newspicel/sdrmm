mod dji;
mod french;

use sdrmm_modem::wifi::{self as phy, WifiPhy};
use sdrmm_wire::{DecoderEvent, RemoteIdFrame, RemoteIdPhy, RemoteIdTransport};

use super::{
    odid::APP_CODE,
    tracker::{Heard, Tracker},
};
use crate::wifi::mac::{
    ACTION, BEACON, SSID, TYPE_MASK, VENDOR, WFA_OUI, address_text, element, management, text,
};

const ASTM_OUI: [u8; 3] = [0xFA, 0x0B, 0xBC];
const NAN_TYPE: u8 = 0x13;
const PUBLIC_ACTION: u8 = 4;
const VENDOR_ACTION: u8 = 9;
const SERVICE_DESCRIPTOR: u8 = 0x03;
pub(crate) const REMOTE_ID_SERVICE: [u8; 6] = [0x88, 0x69, 0x19, 0x9D, 0x92, 0x09];
const BINDING_BITMAP: u8 = 0x40;
const MATCHING_FILTER: u8 = 0x04;
const RESPONSE_FILTER: u8 = 0x08;
const SERVICE_INFO: u8 = 0x10;

#[must_use]
pub(crate) fn wanted_control(control: u8) -> bool {
    matches!(u16::from(control) & TYPE_MASK, BEACON | ACTION)
}

pub(crate) struct Mpdu<'a> {
    pub bytes: &'a [u8],
    pub phy: RemoteIdPhy,
    pub channel: Option<u8>,
    pub level_dbfs: f32,
}

pub(crate) fn frame(mpdu: &Mpdu<'_>, tracker: &mut Tracker) -> Option<RemoteIdFrame> {
    let parsed = management(mpdu.bytes)?;
    let heard = |transport, counter, ssid: Option<&[u8]>, payload| Heard {
        transport,
        phy: mpdu.phy,
        address: address_text(parsed.transmitter),
        channel: mpdu.channel,
        counter,
        ssid: ssid.map(text),
        level_dbfs: mpdu.level_dbfs,
        payload,
    };
    match parsed.kind {
        BEACON => {
            let elements = parsed.elements()?;
            let ssid = element(elements, |id, _| id == SSID);
            if let Some(found) = element(elements, |id, data| {
                id == VENDOR && data.starts_with(&ASTM_OUI) && data.get(3) == Some(&APP_CODE)
            }) {
                let (&counter, pack) = found.get(4..)?.split_first()?;
                return tracker.frame(heard(
                    RemoteIdTransport::WifiBeacon,
                    Some(counter),
                    ssid,
                    pack,
                ));
            }
            if let Some(drone_id) =
                element(elements, |id, data| id == VENDOR && dji::is_drone_id(data))
            {
                let messages = dji::messages(drone_id)?;
                return Some(tracker.direct(
                    heard(RemoteIdTransport::WifiBeaconDji, None, ssid, &[]),
                    messages,
                ));
            }
            let french = element(elements, |id, data| {
                id == VENDOR && data.starts_with(&french::OUI) && data.get(3) == Some(&french::TYPE)
            })?;
            let messages = french::messages(french.get(4..)?)?;
            Some(tracker.direct(
                heard(RemoteIdTransport::WifiBeaconFrench, None, ssid, &[]),
                messages,
            ))
        }
        ACTION => {
            let (counter, pack) = nan_service_info(parsed.body)?;
            tracker.frame(heard(RemoteIdTransport::WifiNan, Some(counter), None, pack))
        }
        _ => None,
    }
}

fn nan_service_info(body: &[u8]) -> Option<(u8, &[u8])> {
    let [
        PUBLIC_ACTION,
        VENDOR_ACTION,
        o1,
        o2,
        o3,
        NAN_TYPE,
        attributes @ ..,
    ] = body
    else {
        return None;
    };
    if [*o1, *o2, *o3] != WFA_OUI {
        return None;
    }
    let mut rest = attributes;
    while let [id, low, high, tail @ ..] = rest {
        let (data, next) = tail.split_at_checked(usize::from(u16::from_le_bytes([*low, *high])))?;
        if *id == SERVICE_DESCRIPTOR
            && data.starts_with(&REMOTE_ID_SERVICE)
            && let Some(info) = service_info(data)
        {
            let (&counter, pack) = info.split_first()?;
            return Some((counter, pack));
        }
        rest = next;
    }
    None
}

fn service_info(descriptor: &[u8]) -> Option<&[u8]> {
    let control = *descriptor.get(8)?;
    let mut at = 9;
    if control & BINDING_BITMAP != 0 {
        at += 2;
    }
    for flag in [MATCHING_FILTER, RESPONSE_FILTER] {
        if control & flag != 0 {
            at += 1 + usize::from(*descriptor.get(at)?);
        }
    }
    if control & SERVICE_INFO == 0 {
        return None;
    }
    let length = usize::from(*descriptor.get(at)?);
    descriptor.get(at + 1..at + 1 + length)
}

fn wire_phy(phy: WifiPhy) -> RemoteIdPhy {
    match phy {
        WifiPhy::Dsss1m => RemoteIdPhy::Dsss1m,
        WifiPhy::Dsss2m => RemoteIdPhy::Dsss2m,
        WifiPhy::Cck5m5 => RemoteIdPhy::Cck5m5,
        WifiPhy::Cck11m => RemoteIdPhy::Cck11m,
        WifiPhy::Ofdm { .. } => RemoteIdPhy::Ofdm,
    }
}

pub(crate) struct Events<'a> {
    pub tracker: &'a mut Tracker,
    pub events: &'a mut Vec<DecoderEvent>,
    pub channel: Option<u8>,
}

impl phy::Sink for Events<'_> {
    fn accepts(&self, frame_control: u8) -> bool {
        wanted_control(frame_control)
    }

    fn frame(&mut self, frame: phy::Frame<'_>) {
        let mpdu = Mpdu {
            bytes: frame.mpdu,
            phy: wire_phy(frame.phy),
            channel: self.channel,
            level_dbfs: frame.level_dbfs,
        };
        if let Some(event) = self::frame(&mpdu, self.tracker) {
            self.events.push(DecoderEvent::RemoteId(event));
        }
    }
}

#[cfg(any(test, feature = "synth"))]
pub mod build;

#[cfg(test)]
mod tests;
