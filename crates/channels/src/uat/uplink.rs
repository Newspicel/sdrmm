//! The ground uplink (DO-282B section 2.2.3.2): its information frames, the FIS-B APDUs they
//! carry (DO-358), and the text inside them, either DLAC text reports (product 413) or the text
//! records of Text With Graphic Overlay products (NOTAMs, AIRMETs, SIGMETs, SUA, CWAs).

use super::fec::UPLINK_DATA;
use super::model::FisbReport;

const HEADER_BYTES: usize = 8;
const APP_DATA_VALID: u8 = 0x20;
const FRAME_HEADER: usize = 2;
const FISB_FRAME: u8 = 0;
const GENERIC_TEXT: u16 = 413;
const TWGO_TEXT_FORMAT: u8 = 2;
const TWGO_HEADER: usize = 6;
const RECORD_HEADER: usize = 5;
const RECORD_ACTIVE: u8 = 0x04;
pub(crate) const DLAC: &[u8; 64] =
    b"\x03ABCDEFGHIJKLMNOPQRSTUVWXYZ\x1A\t\x1E\n| !\"#$%&'()*+,-./0123456789:;<=>?";
const ETX: char = '\x03';
const NULL_CHAR: char = '\x1A';
const RECORD_SEPARATOR: char = '\x1E';
const TAB: u8 = 28;
/// Characters in a trailing group of 0, 1, 2 or 3 bytes.
const CODES_IN: [usize; 4] = [0, 1, 2, 4];

/// A FIS-B APDU: its product, when it was made and the bytes after its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Apdu<'a> {
    pub product_id: u16,
    pub segmented: bool,
    pub time: ProductTime,
    pub payload: &'a [u8],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProductTime {
    pub month_day: Option<(u8, u8)>,
    pub hour: u8,
    pub minute: u8,
}

impl ProductTime {
    fn label(self) -> String {
        match self.month_day {
            Some((month, day)) => {
                format!("{month:02}/{day:02} {:02}:{:02}", self.hour, self.minute)
            }
            None => format!("{:02}:{:02}", self.hour, self.minute),
        }
    }
}

/// Every FIS-B APDU in an uplink's application data, in order.
pub(crate) fn apdus(data: &[u8; UPLINK_DATA]) -> impl Iterator<Item = Apdu<'_>> {
    let valid = data[6] & APP_DATA_VALID != 0;
    let mut rest: &[u8] = if valid { &data[HEADER_BYTES..] } else { &[] };
    std::iter::from_fn(move || {
        loop {
            let (&[high, low], tail) = rest.split_first_chunk::<FRAME_HEADER>()?;
            let length = usize::from(high) << 1 | usize::from(low >> 7);
            let kind = low & 0x0F;
            if length == 0 && kind == FISB_FRAME {
                return None;
            }
            let (frame, after) = tail.split_at_checked(length)?;
            rest = after;
            if kind == FISB_FRAME
                && let Some(apdu) = apdu(frame)
            {
                return Some(apdu);
            }
        }
    })
}

fn apdu(frame: &[u8]) -> Option<Apdu<'_>> {
    let header = frame.get(..6)?;
    let product_id = u16::from(header[0] & 0x1F) << 6 | u16::from(header[1] >> 2);
    let segmented = header[1] & 0x02 != 0;
    let option = (header[1] & 0x01) << 1 | header[2] >> 7;
    let (time, skip) = match option {
        0 => (
            clock(
                header[2] >> 2 & 0x1F,
                (header[2] & 0x03) << 4 | header[3] >> 4,
            ),
            4,
        ),
        1 => (
            clock(
                header[2] >> 2 & 0x1F,
                (header[2] & 0x03) << 4 | header[3] >> 4,
            ),
            5,
        ),
        _ => {
            let month = header[2] >> 3 & 0x0F;
            let day = (header[2] & 0x07) << 2 | header[3] >> 6;
            let mut time = clock(
                header[3] >> 1 & 0x1F,
                (header[3] & 0x01) << 5 | header[4] >> 3,
            );
            time.month_day = Some((month, day));
            (time, if option == 2 { 5 } else { 6 })
        }
    };
    Some(Apdu {
        product_id,
        segmented,
        time,
        payload: frame.get(skip..)?,
    })
}

const fn clock(hour: u8, minute: u8) -> ProductTime {
    ProductTime {
        month_day: None,
        hour,
        minute,
    }
}

/// The text reports an APDU carries; graphics and segmented products give none.
pub(crate) fn reports(apdu: &Apdu<'_>, out: &mut Vec<FisbReport>) {
    if apdu.segmented {
        return;
    }
    match apdu.product_id {
        GENERIC_TEXT => generic_text(apdu, out),
        8 | 11 | 12 | 13 | 15 | 16 | 17 => twgo_text(apdu, out),
        _ => {}
    }
}

fn generic_text(apdu: &Apdu<'_>, out: &mut Vec<FisbReport>) {
    let text = dlac(apdu.payload);
    for report in text.split([RECORD_SEPARATOR, ETX]) {
        let report = report.trim();
        if report.is_empty() {
            continue;
        }
        let mut words = report.splitn(4, ' ');
        let kind = words.next().unwrap_or_default();
        let location = words.next();
        let issued = words.next();
        out.push(FisbReport {
            product_id: apdu.product_id,
            product: kind.to_owned(),
            location: location.map(str::to_owned),
            issued: issued.map(str::to_owned),
            text: words.next().unwrap_or_default().trim().to_owned(),
        });
    }
}

fn twgo_text(apdu: &Apdu<'_>, out: &mut Vec<FisbReport>) {
    let Some(header) = apdu.payload.get(..TWGO_HEADER) else {
        return;
    };
    if header[0] >> 4 != TWGO_TEXT_FORMAT {
        return;
    }
    let count = header[1] >> 4;
    let location = dlac(&header[2..5]);
    let location = before_etx(&location);
    let mut rest = &apdu.payload[TWGO_HEADER..];
    for _ in 0..count {
        let Some(record) = rest.get(..2) else {
            return;
        };
        let length = usize::from(u16::from_be_bytes([record[0], record[1]]));
        let Some(record) = rest.get(..length).filter(|_| length >= RECORD_HEADER) else {
            return;
        };
        rest = &rest[length..];
        if record[4] & RECORD_ACTIVE == 0 {
            continue;
        }
        let text = dlac(&record[RECORD_HEADER..]);
        let text = before_etx(&text);
        if text.is_empty() {
            continue;
        }
        out.push(FisbReport {
            product_id: apdu.product_id,
            product: twgo_name(apdu.product_id, text),
            location: (!location.is_empty()).then(|| location.to_owned()),
            issued: Some(apdu.time.label()),
            text: text.to_owned(),
        });
    }
}

/// A TWGO field up to its end of text, without the padding around it.
fn before_etx(text: &str) -> &str {
    text.split(ETX).next().unwrap_or_default().trim()
}

fn twgo_name(product_id: u16, text: &str) -> String {
    let first = text.split_whitespace().next().unwrap_or_default();
    let named = match product_id {
        8 | 16 | 17 if first.starts_with("NOTAM") => first,
        8 | 16 | 17 => "NOTAM",
        11 => "AIRMET",
        12 if first == "WST" => "WST",
        12 => "SIGMET",
        13 => "SUA",
        _ => "CWA",
    };
    named.to_owned()
}

/// DLAC, the six-bit alphabet of DO-358: four characters in three bytes, a tab followed by its
/// count of spaces, the null character dropped.
pub(crate) fn dlac(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 4 / 3);
    let mut tab = false;
    let codes = bytes.chunks(3).flat_map(|group| {
        let mut word = [0u8; 3];
        word[..group.len()].copy_from_slice(group);
        let all = [
            word[0] >> 2,
            (word[0] & 0x03) << 4 | word[1] >> 4,
            (word[1] & 0x0F) << 2 | word[2] >> 6,
            word[2] & 0x3F,
        ];
        all.into_iter().take(CODES_IN[group.len()])
    });
    for code in codes {
        if tab {
            text.extend(std::iter::repeat_n(' ', usize::from(code)));
            tab = false;
        } else if code == TAB {
            tab = true;
        } else {
            let letter = char::from(DLAC[usize::from(code)]);
            if letter != NULL_CHAR {
                text.push(letter);
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn dlac_packs_four_letters_in_three_bytes() {
        let packed = [0b0000_0100, 0b0010_0000, 0b1100_0000 | 32];
        assert_eq!(dlac(&packed), "ABC ");
    }

    #[test]
    fn a_dlac_tab_becomes_its_count_of_spaces() {
        let packed = [(1 << 2) | (TAB >> 4), (TAB & 0x0F) << 4, 3 << 6 | 2];
        assert_eq!(dlac(&packed), "A   B");
    }

    #[test]
    fn an_apdu_reads_its_product_and_time() {
        let frame = hex("06741b40aabbcc");
        let apdu = apdu(&frame).expect("apdu");
        assert_eq!(apdu.product_id, GENERIC_TEXT);
        assert!(!apdu.segmented);
        assert_eq!(apdu.time.label(), "06:52");
        assert_eq!(apdu.payload, &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn a_dated_apdu_reads_month_and_day() {
        let frame = hex("0021c3d7486700");
        let apdu = apdu(&frame).expect("apdu");
        assert_eq!(apdu.product_id, 8);
        assert_eq!(apdu.time.label(), "08/15 11:41");
    }

    #[test]
    fn a_generic_text_report_splits_type_place_and_time() {
        let mut out = Vec::new();
        let text = crate::synth::uat::dlac("METAR KSPI 070652Z AUTO 36014KT 10SM=");
        generic_text(
            &Apdu {
                product_id: GENERIC_TEXT,
                segmented: false,
                time: ProductTime::default(),
                payload: &text,
            },
            &mut out,
        );
        assert_eq!(
            out,
            [FisbReport {
                product_id: GENERIC_TEXT,
                product: "METAR".to_owned(),
                location: Some("KSPI".to_owned()),
                issued: Some("070652Z".to_owned()),
                text: "AUTO 36014KT 10SM=".to_owned(),
            }]
        );
    }
}
