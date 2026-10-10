pub(crate) const FCS_BYTES: usize = 4;
pub(crate) const HEADER_BYTES: usize = 24;
pub(crate) const BEACON_FIXED: usize = 12;
pub(crate) const TYPE_MASK: u16 = 0x00FC;
pub(crate) const BEACON: u16 = 0x0080;
pub(crate) const PROBE_RESPONSE: u16 = 0x0050;
pub(crate) const ACTION: u16 = 0x00D0;
pub(crate) const SSID: u8 = 0;
pub(crate) const VENDOR: u8 = 221;
pub(crate) const WFA_OUI: [u8; 3] = [0x50, 0x6F, 0x9A];
const TRANSMITTER: std::ops::Range<usize> = 10..16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Management<'a> {
    pub kind: u16,
    pub transmitter: &'a [u8],
    pub body: &'a [u8],
}

impl<'a> Management<'a> {
    #[must_use]
    pub(crate) fn elements(&self) -> Option<&'a [u8]> {
        match self.kind {
            BEACON | PROBE_RESPONSE => self.body.get(BEACON_FIXED..),
            _ => None,
        }
    }
}

#[must_use]
pub(crate) fn management(mpdu: &[u8]) -> Option<Management<'_>> {
    let body_end = mpdu.len().checked_sub(FCS_BYTES)?;
    let header = mpdu.get(..HEADER_BYTES)?;
    Some(Management {
        kind: u16::from_le_bytes([header[0], header[1]]) & TYPE_MASK,
        transmitter: &header[TRANSMITTER],
        body: mpdu.get(HEADER_BYTES..body_end)?,
    })
}

pub(crate) fn elements(bytes: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    let mut rest = bytes;
    std::iter::from_fn(move || {
        let [id, length, tail @ ..] = rest else {
            return None;
        };
        let (data, next) = tail.split_at_checked(usize::from(*length))?;
        rest = next;
        Some((*id, data))
    })
}

#[must_use]
pub(crate) fn element(bytes: &[u8], wanted: impl Fn(u8, &[u8]) -> bool) -> Option<&[u8]> {
    elements(bytes).find_map(|(id, data)| wanted(id, data).then_some(data))
}

#[must_use]
pub(crate) fn address_text(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(17);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            text.push(':');
        }
        text.push_str(&format!("{byte:02X}"));
    }
    text
}

#[must_use]
pub(crate) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beacon_header_and_its_elements_read_back() {
        let mut mpdu = vec![0x80, 0x00, 0, 0];
        mpdu.extend([0xFF; 6]);
        mpdu.extend([0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
        mpdu.extend([0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
        mpdu.extend([0, 0]);
        mpdu.extend([0; BEACON_FIXED]);
        mpdu.extend([SSID, 4, b'h', b'o', b'm', b'e', VENDOR, 3, 1, 2, 3]);
        mpdu.extend([0; FCS_BYTES]);
        let frame = management(&mpdu).unwrap();
        assert_eq!(frame.kind, BEACON);
        assert_eq!(address_text(frame.transmitter), "02:11:22:33:44:55");
        let elements = frame.elements().unwrap();
        assert_eq!(element(elements, |id, _| id == SSID), Some(&b"home"[..]));
        assert_eq!(
            element(elements, |id, _| id == VENDOR),
            Some(&[1, 2, 3][..])
        );
        assert_eq!(super::elements(&[SSID, 9, 1]).count(), 0);
    }
}
