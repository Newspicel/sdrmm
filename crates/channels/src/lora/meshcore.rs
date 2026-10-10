mod advert_data;
#[cfg(any(test, feature = "synth"))]
mod encode;
mod frame;
mod group;
mod keys;
mod payload;
#[cfg(test)]
mod tests;

use sdrmm_wire::MeshcorePacket;

#[cfg(any(test, feature = "synth"))]
pub(crate) use encode::{advert, group_text};
pub(crate) use keys::Keys;
#[cfg(any(test, feature = "synth"))]
pub(crate) use keys::PUBLIC_SECRET;

pub(crate) fn decode(payload: &[u8], keys: &Keys) -> Option<MeshcorePacket> {
    let frame = frame::Frame::parse(payload)?;
    let content = payload::content(&frame, keys)?;
    Some(MeshcorePacket {
        route: frame.route,
        payload_type: frame.payload_type,
        version: 0,
        transport_codes: frame.transport_codes.to_vec(),
        path: frame.path_labels(),
        content,
    })
}
