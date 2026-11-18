//! AX.25 v2.2 addresses and UI frames.

pub const ADDRESS_LEN: usize = 7;
pub const CALL_LEN: usize = ADDRESS_LEN - 1;
pub const CONTROL_UI: u8 = 0x03;
pub const CONTROL_PF: u8 = 0x10;
pub const PID_NO_LAYER3: u8 = 0xF0;
const SSID_RESERVED: u8 = 0x60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    /// The callsign with `-ssid` when the SSID is not zero.
    pub call: String,
    /// The extension bit: no address follows.
    pub last: bool,
    /// The H bit of a digipeater, or the C bit of a destination or source.
    pub repeated: bool,
}

/// One seven octet address field, `None` unless it is a padded alphanumeric callsign.
#[must_use]
pub fn decode_address(field: &[u8]) -> Option<Address> {
    let (&ssid_octet, chars) = field.split_last()?;
    let mut call = String::with_capacity(9);
    let mut padded = false;
    for &octet in chars {
        if octet & 1 != 0 {
            return None;
        }
        let c = char::from(octet >> 1);
        if c == ' ' {
            padded = true;
            continue;
        }
        if padded || !c.is_ascii_alphanumeric() {
            return None;
        }
        call.push(c);
    }
    if call.is_empty() {
        return None;
    }
    let ssid = ssid_octet >> 1 & 0x0F;
    if ssid != 0 {
        call.push('-');
        call.push_str(&ssid.to_string());
    }
    Some(Address {
        call,
        last: ssid_octet & 1 != 0,
        repeated: ssid_octet & 0x80 != 0,
    })
}

/// Appends `call`, as `CALL` or `CALL-ssid`, as one address field. `flag` is the C or H bit.
pub fn encode_address(out: &mut Vec<u8>, call: &str, flag: bool, last: bool) {
    let (base, ssid) = match call.split_once('-') {
        Some((base, ssid)) => (base, ssid.parse::<u8>().unwrap_or(0)),
        None => (call, 0),
    };
    let mut chars = [b' '; CALL_LEN];
    for (slot, byte) in chars.iter_mut().zip(base.bytes()) {
        *slot = byte.to_ascii_uppercase();
    }
    for c in chars {
        out.push((c & 0x7F) << 1);
    }
    out.push(u8::from(flag) << 7 | SSID_RESERVED | (ssid & 0x0F) << 1 | u8::from(last));
}

/// Whether a frame with this control field carries a PID octet: I and UI frames.
#[must_use]
pub const fn carries_pid(control: u8) -> bool {
    control & 1 == 0 || control & !CONTROL_PF == CONTROL_UI
}

/// A UI command frame without FCS. A path hop ending in `*` has been repeated.
#[must_use]
pub fn ui_frame(source: &str, destination: &str, path: &[&str], info: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ADDRESS_LEN * (2 + path.len()) + 2 + info.len());
    encode_address(&mut out, destination, true, false);
    encode_address(&mut out, source, false, path.is_empty());
    for (i, hop) in path.iter().enumerate() {
        let repeated = hop.ends_with('*');
        encode_address(
            &mut out,
            hop.trim_end_matches('*'),
            repeated,
            i + 1 == path.len(),
        );
    }
    out.push(CONTROL_UI);
    out.push(PID_NO_LAYER3);
    out.extend_from_slice(info);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_round_trips_with_its_ssid_and_bits() {
        let mut field = Vec::new();
        encode_address(&mut field, "dl1abc-9", true, true);
        assert_eq!(field.len(), ADDRESS_LEN);
        let address = decode_address(&field).unwrap();
        assert_eq!(address.call, "DL1ABC-9");
        assert!(address.last && address.repeated);
        field.clear();
        encode_address(&mut field, "WIDE1", false, false);
        let address = decode_address(&field).unwrap();
        assert_eq!(address.call, "WIDE1");
        assert!(!address.last && !address.repeated);
    }

    #[test]
    fn padding_inside_a_call_or_an_extension_bit_is_rejected() {
        let mut field = Vec::new();
        encode_address(&mut field, "AB", false, false);
        field[3] = b'C' << 1;
        assert_eq!(decode_address(&field), None);
        field[3] = b' ' << 1 | 1;
        assert_eq!(decode_address(&field), None);
    }

    #[test]
    fn a_ui_frame_ends_its_address_list_on_the_last_hop() {
        let frame = ui_frame("N0CALL", "APRS", &["WIDE1-1*", "WIDE2-1"], b">hi");
        let hop = decode_address(&frame[2 * ADDRESS_LEN..3 * ADDRESS_LEN]).unwrap();
        assert_eq!(
            (hop.call.as_str(), hop.repeated, hop.last),
            ("WIDE1-1", true, false)
        );
        let hop = decode_address(&frame[3 * ADDRESS_LEN..4 * ADDRESS_LEN]).unwrap();
        assert!(hop.last && !hop.repeated);
        assert_eq!(
            &frame[4 * ADDRESS_LEN..],
            &[CONTROL_UI, PID_NO_LAYER3, b'>', b'h', b'i']
        );
        assert!(carries_pid(CONTROL_UI));
    }
}
