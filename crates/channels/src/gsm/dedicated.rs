use sdrmm_wire::{GsmDedicated, GsmLapdmFrame, GsmProtocol};

use super::{
    coding::BLOCK_BYTES,
    identity::{sized_identity, tagged_identity},
};

const SACCH_HEADER: usize = 2;
const MOBILE_IDENTITY_TAG: u8 = 0x17;
const LAI_OCTETS: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Link<'a> {
    pub(crate) sapi: u8,
    pub(crate) frame: GsmLapdmFrame,
    pub(crate) info: &'a [u8],
}

pub(crate) fn link(block: &[u8; BLOCK_BYTES], sacch: bool) -> Option<Link<'_>> {
    let frame = block.get(if sacch { SACCH_HEADER } else { 0 }..)?;
    let [address, control, length, rest @ ..] = frame else {
        return None;
    };
    if address & 1 == 0 || length & 1 == 0 {
        return None;
    }
    let info = rest.get(..usize::from(length >> 2))?;
    Some(Link {
        sapi: address >> 2 & 7,
        frame: frame_kind(*control),
        info,
    })
}

fn frame_kind(control: u8) -> GsmLapdmFrame {
    if control & 1 == 0 {
        return GsmLapdmFrame::I;
    }
    if control & 3 == 1 {
        return match control >> 2 & 3 {
            0 => GsmLapdmFrame::Rr,
            1 => GsmLapdmFrame::Rnr,
            2 => GsmLapdmFrame::Rej,
            _ => GsmLapdmFrame::Unknown,
        };
    }
    match control & 0xEF {
        0x2F => GsmLapdmFrame::Sabm,
        0x0F => GsmLapdmFrame::Dm,
        0x03 => GsmLapdmFrame::Ui,
        0x43 => GsmLapdmFrame::Disc,
        0x63 => GsmLapdmFrame::Ua,
        0x87 => GsmLapdmFrame::Frmr,
        _ => GsmLapdmFrame::Unknown,
    }
}

pub(crate) fn protocol(discriminator: u8) -> GsmProtocol {
    match discriminator & 0x0F {
        0x06 => GsmProtocol::Rr,
        0x05 => GsmProtocol::Mm,
        0x03 => GsmProtocol::Cc,
        0x0B => GsmProtocol::Ss,
        0x09 => GsmProtocol::Sms,
        0x08 => GsmProtocol::Gmm,
        0x0A => GsmProtocol::Sm,
        _ => GsmProtocol::Other,
    }
}

pub(crate) fn message_type(protocol: GsmProtocol, octet: u8) -> u8 {
    match protocol {
        GsmProtocol::Mm | GsmProtocol::Cc | GsmProtocol::Ss => octet & 0x3F,
        _ => octet,
    }
}

pub(crate) fn describe(link: &Link<'_>) -> Option<GsmDedicated> {
    let mut dedicated = GsmDedicated {
        sapi: link.sapi,
        frame: link.frame,
        protocol: None,
        message: None,
        identity: None,
        cipher: None,
    };
    let Some(&[discriminator, octet]) = link.info.get(..2) else {
        return matches!(
            link.frame,
            GsmLapdmFrame::Sabm | GsmLapdmFrame::Ua | GsmLapdmFrame::Disc | GsmLapdmFrame::Dm
        )
        .then_some(dedicated);
    };
    let kind = protocol(discriminator);
    let code = message_type(kind, octet);
    dedicated.protocol = Some(kind);
    dedicated.message =
        Some(name(kind, code).map_or_else(|| format!("0x{code:02X}"), str::to_owned));
    dedicated.identity = identity(kind, code, link.info);
    dedicated.cipher = cipher(kind, code, link.info);
    Some(dedicated)
}

fn identity(protocol: GsmProtocol, code: u8, info: &[u8]) -> Option<sdrmm_wire::GsmIdentity> {
    let after_classmark = |at: usize| {
        let length = usize::from(*info.get(at)?);
        sized_identity(info.get(at + 1 + length..)?)
    };
    match (protocol, code) {
        (GsmProtocol::Rr, 0x27) => after_classmark(3),
        (GsmProtocol::Mm, 0x08) => sized_identity(info.get(3 + LAI_OCTETS + 1..)?),
        (GsmProtocol::Mm, 0x24) => after_classmark(3),
        (GsmProtocol::Mm, 0x19) => sized_identity(info.get(2..)?),
        (GsmProtocol::Mm, 0x1A) => sized_identity(info.get(2 + LAI_OCTETS..)?),
        (GsmProtocol::Mm, 0x02) => {
            tagged_identity(info.get(2 + LAI_OCTETS..)?, MOBILE_IDENTITY_TAG)
        }
        _ => None,
    }
}

fn cipher(protocol: GsmProtocol, code: u8, info: &[u8]) -> Option<u8> {
    if (protocol, code) != (GsmProtocol::Rr, 0x35) {
        return None;
    }
    let setting = info.get(2)? & 0x0F;
    Some(if setting & 1 == 1 {
        (setting >> 1 & 7) + 1
    } else {
        0
    })
}

pub(crate) const fn releases(protocol: GsmProtocol, code: u8) -> bool {
    matches!((protocol, code), (GsmProtocol::Rr, 0x0D | 0x2E | 0x2B))
}

fn name(protocol: GsmProtocol, code: u8) -> Option<&'static str> {
    match protocol {
        GsmProtocol::Rr => rr_name(code),
        GsmProtocol::Mm => mm_name(code),
        GsmProtocol::Cc => cc_name(code),
        GsmProtocol::Sms => match code {
            0x01 => Some("CP-DATA"),
            0x04 => Some("CP-ACK"),
            0x10 => Some("CP-ERROR"),
            _ => None,
        },
        GsmProtocol::Ss => match code {
            0x2A => Some("Release Complete"),
            0x3A => Some("Facility"),
            0x3B => Some("Register"),
            _ => None,
        },
        _ => None,
    }
}

fn rr_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x0D => "Channel Release",
        0x0A => "Partial Release",
        0x10 => "Channel Mode Modify",
        0x17 => "Channel Mode Modify Ack",
        0x12 => "Status",
        0x13 => "Classmark Enquiry",
        0x14 => "Frequency Redefinition",
        0x15 => "Measurement Report",
        0x16 => "Classmark Change",
        0x27 => "Paging Response",
        0x29 => "Assignment Complete",
        0x2B => "Handover Command",
        0x2C => "Handover Complete",
        0x2D => "Physical Information",
        0x2E => "Assignment Command",
        0x2F => "Assignment Failure",
        0x32 => "Ciphering Mode Complete",
        0x35 => "Ciphering Mode Command",
        0x1D => "System Information 5",
        0x05 => "System Information 5bis",
        0x06 => "System Information 5ter",
        0x1E => "System Information 6",
        _ => return None,
    })
}

fn mm_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x01 => "IMSI Detach",
        0x02 => "Location Updating Accept",
        0x04 => "Location Updating Reject",
        0x08 => "Location Updating Request",
        0x11 => "Authentication Reject",
        0x12 => "Authentication Request",
        0x14 => "Authentication Response",
        0x1C => "Authentication Failure",
        0x18 => "Identity Request",
        0x19 => "Identity Response",
        0x1A => "TMSI Reallocation Command",
        0x1B => "TMSI Reallocation Complete",
        0x21 => "CM Service Accept",
        0x22 => "CM Service Reject",
        0x23 => "CM Service Abort",
        0x24 => "CM Service Request",
        0x25 => "CM Service Prompt",
        0x28 => "CM Reestablishment Request",
        0x29 => "Abort",
        0x31 => "Status",
        0x32 => "Information",
        _ => return None,
    })
}

fn cc_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x01 => "Alerting",
        0x02 => "Call Proceeding",
        0x03 => "Progress",
        0x05 => "Setup",
        0x07 => "Connect",
        0x08 => "Call Confirmed",
        0x0E => "Emergency Setup",
        0x0F => "Connect Acknowledge",
        0x25 => "Disconnect",
        0x2A => "Release Complete",
        0x2D => "Release",
        0x34 => "Status Enquiry",
        0x3D => "Status",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::GsmIdentityKind;

    use super::*;

    fn block(hex: &str) -> [u8; BLOCK_BYTES] {
        let mut out = [0u8; BLOCK_BYTES];
        for (k, octet) in out.iter_mut().enumerate() {
            *octet = u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn a_contention_resolution_echoes_the_paging_response() {
        let block = block("017335062700035018a005f4015c57032b2b2b2b2b2b2b");
        let link = link(&block, false).unwrap();
        assert_eq!(link.frame, GsmLapdmFrame::Ua);
        let dedicated = describe(&link).unwrap();
        assert_eq!(dedicated.protocol, Some(GsmProtocol::Rr));
        assert_eq!(dedicated.message.as_deref(), Some("Paging Response"));
        let identity = dedicated.identity.unwrap();
        assert_eq!(identity.kind, GsmIdentityKind::Tmsi);
        assert_eq!(identity.value, "015C5703");
    }

    #[test]
    fn a_ciphering_mode_command_names_its_algorithm() {
        let block = block("03200d0635112b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b");
        let link = link(&block, false).unwrap();
        assert_eq!(link.frame, GsmLapdmFrame::I);
        let dedicated = describe(&link).unwrap();
        assert_eq!(dedicated.message.as_deref(), Some("Ciphering Mode Command"));
        assert_eq!(dedicated.cipher, Some(1));
    }

    #[test]
    fn sacch_frames_skip_the_layer_one_header() {
        let block = block("0001030349061d9f6d1810800000000000000000000000");
        let link = link(&block, true).unwrap();
        assert_eq!(link.frame, GsmLapdmFrame::Ui);
        assert_eq!(link.info[..2], [0x06, 0x1D]);
    }

    #[test]
    fn fill_frames_say_nothing() {
        let block = block("0103012b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b");
        assert_eq!(describe(&link(&block, false).unwrap()), None);
    }
}
