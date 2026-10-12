use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    decode::EventKind,
    present::{
        EventPayload, Field, Presentation, Unit, count, fields, number, only_if, text, text_of,
    },
};

pub const GSM_SYMBOL_RATE_HZ: f64 = 1_625_000.0 / 6.0;
pub const GSM_CHANNEL_SPACING_HZ: f64 = 200_000.0;
const ARFCN_TOLERANCE_HZ: f64 = 30_000.0;

fn yes() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GsmParams {
    #[serde(default = "yes")]
    pub paging: bool,
    #[serde(default = "yes")]
    pub follow: bool,
}

impl Default for GsmParams {
    fn default() -> Self {
        Self {
            paging: true,
            follow: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GsmBand {
    Gsm450,
    Gsm480,
    Gsm850,
    Gsm900,
    Dcs1800,
    Pcs1900,
}

struct BandPlan {
    band: GsmBand,
    first: u16,
    last: u16,
    first_downlink_hz: f64,
}

const BAND_PLANS: [BandPlan; 7] = [
    BandPlan {
        band: GsmBand::Gsm450,
        first: 259,
        last: 293,
        first_downlink_hz: 460_600_000.0,
    },
    BandPlan {
        band: GsmBand::Gsm480,
        first: 306,
        last: 340,
        first_downlink_hz: 489_000_000.0,
    },
    BandPlan {
        band: GsmBand::Gsm850,
        first: 128,
        last: 251,
        first_downlink_hz: 869_200_000.0,
    },
    BandPlan {
        band: GsmBand::Gsm900,
        first: 0,
        last: 124,
        first_downlink_hz: 935_000_000.0,
    },
    BandPlan {
        band: GsmBand::Gsm900,
        first: 955,
        last: 1023,
        first_downlink_hz: 921_200_000.0,
    },
    BandPlan {
        band: GsmBand::Dcs1800,
        first: 512,
        last: 885,
        first_downlink_hz: 1_805_200_000.0,
    },
    BandPlan {
        band: GsmBand::Pcs1900,
        first: 512,
        last: 810,
        first_downlink_hz: 1_930_200_000.0,
    },
];

impl GsmBand {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Gsm450 => "GSM 450",
            Self::Gsm480 => "GSM 480",
            Self::Gsm850 => "GSM 850",
            Self::Gsm900 => "GSM 900",
            Self::Dcs1800 => "DCS 1800",
            Self::Pcs1900 => "PCS 1900",
        }
    }

    #[must_use]
    pub fn at(frequency_hz: f64) -> Option<(Self, u16)> {
        BAND_PLANS.iter().find_map(|plan| {
            let steps = (frequency_hz - plan.first_downlink_hz) / GSM_CHANNEL_SPACING_HZ;
            let nearest = steps.round();
            let offset = (steps - nearest).abs() * GSM_CHANNEL_SPACING_HZ;
            let span = f64::from(plan.last - plan.first);
            ((0.0..=span).contains(&nearest) && offset <= ARFCN_TOLERANCE_HZ)
                .then(|| (plan.band, plan.first + nearest as u16))
        })
    }

    #[must_use]
    pub fn downlink_hz(self, arfcn: u16) -> Option<f64> {
        BAND_PLANS
            .iter()
            .filter(|plan| plan.band == self)
            .find(|plan| (plan.first..=plan.last).contains(&arfcn))
            .map(|plan| {
                plan.first_downlink_hz + f64::from(arfcn - plan.first) * GSM_CHANNEL_SPACING_HZ
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GsmLogicalChannel {
    Bcch,
    Ccch,
    Sdcch,
    Sacch,
}

impl GsmLogicalChannel {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bcch => "BCCH",
            Self::Ccch => "CCCH",
            Self::Sdcch => "SDCCH",
            Self::Sacch => "SACCH",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GsmIdentityKind {
    Imsi,
    Imei,
    Imeisv,
    Tmsi,
}

impl GsmIdentityKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Imsi => "IMSI",
            Self::Imei => "IMEI",
            Self::Imeisv => "IMEISV",
            Self::Tmsi => "TMSI",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GsmIdentity {
    pub kind: GsmIdentityKind,
    pub value: String,
}

impl GsmIdentity {
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} {}", self.kind.label(), self.value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GsmSi {
    Si1,
    Si2,
    Si2bis,
    Si2ter,
    Si2quater,
    Si3,
    Si4,
    Si5,
    Si5bis,
    Si5ter,
    Si6,
    Si7,
    Si8,
    Si9,
    Si13,
}

impl GsmSi {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Si1 => "SI1",
            Self::Si2 => "SI2",
            Self::Si2bis => "SI2bis",
            Self::Si2ter => "SI2ter",
            Self::Si2quater => "SI2quater",
            Self::Si3 => "SI3",
            Self::Si4 => "SI4",
            Self::Si5 => "SI5",
            Self::Si5bis => "SI5bis",
            Self::Si5ter => "SI5ter",
            Self::Si6 => "SI6",
            Self::Si7 => "SI7",
            Self::Si8 => "SI8",
            Self::Si9 => "SI9",
            Self::Si13 => "SI13",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GsmCcch {
    Separate,
    Combined,
    Two,
    Three,
    Four,
}

impl GsmCcch {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Separate => "1 CCCH",
            Self::Combined => "1 CCCH with SDCCH/4",
            Self::Two => "2 CCCH",
            Self::Three => "3 CCCH",
            Self::Four => "4 CCCH",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GsmCell {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mnc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lac: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ccch: Option<GsmCcch>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cell_channels: Vec<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub neighbours: Vec<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rxlev_access_min_dbm: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms_txpwr_max_cch: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t3212_min: Option<u16>,
    #[serde(default)]
    pub barred: bool,
    #[serde(default)]
    pub gprs: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system_information: Vec<GsmSi>,
}

impl GsmCell {
    #[must_use]
    pub fn plmn(&self) -> Option<String> {
        Some(format!("{}-{}", self.mcc.as_ref()?, self.mnc.as_ref()?))
    }

    #[must_use]
    pub fn cgi(&self) -> Option<String> {
        Some(format!("{}-{}-{}", self.plmn()?, self.lac?, self.cell_id?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GsmChannelType {
    TchF,
    TchH,
    Sdcch4,
    Sdcch8,
    Pdch,
}

impl GsmChannelType {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::TchF => "TCH/F",
            Self::TchH => "TCH/H",
            Self::Sdcch4 => "SDCCH/4",
            Self::Sdcch8 => "SDCCH/8",
            Self::Pdch => "PDCH",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GsmHopping {
    pub maio: u8,
    pub hsn: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GsmAssignment {
    pub channel: GsmChannelType,
    pub timeslot: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subchannel: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arfcn: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hopping: Option<GsmHopping>,
    pub tsc: u8,
    pub request: u8,
    pub timing_advance: u8,
}

impl GsmAssignment {
    #[must_use]
    pub fn label(&self) -> String {
        let mut parts = vec![format!("{} TS{}", self.channel.label(), self.timeslot)];
        if let Some(subchannel) = self.subchannel {
            parts.push(format!("SC{subchannel}"));
        }
        match (self.arfcn, self.hopping) {
            (Some(arfcn), _) => parts.push(format!("ARFCN {arfcn}")),
            (None, Some(hopping)) => parts.push(format!("hopping HSN {}", hopping.hsn)),
            (None, None) => {}
        }
        parts.push(format!("TA {}", self.timing_advance));
        parts.join(" ")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GsmProtocol {
    Rr,
    Mm,
    Cc,
    Ss,
    Sms,
    Gmm,
    Sm,
    Other,
}

impl GsmProtocol {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rr => "RR",
            Self::Mm => "MM",
            Self::Cc => "CC",
            Self::Ss => "SS",
            Self::Sms => "SMS",
            Self::Gmm => "GMM",
            Self::Sm => "SM",
            Self::Other => "L3",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GsmLapdmFrame {
    I,
    Rr,
    Rnr,
    Rej,
    Sabm,
    Dm,
    Ui,
    Disc,
    Ua,
    Frmr,
    Unknown,
}

impl GsmLapdmFrame {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::I => "I",
            Self::Rr => "RR",
            Self::Rnr => "RNR",
            Self::Rej => "REJ",
            Self::Sabm => "SABM",
            Self::Dm => "DM",
            Self::Ui => "UI",
            Self::Disc => "DISC",
            Self::Ua => "UA",
            Self::Frmr => "FRMR",
            Self::Unknown => "?",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GsmDedicated {
    pub sapi: u8,
    pub frame: GsmLapdmFrame,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<GsmProtocol>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<GsmIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cipher: Option<u8>,
}

impl GsmDedicated {
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        match (&self.protocol, &self.message) {
            (Some(protocol), Some(message)) => {
                parts.push(format!("{} {message}", protocol.label()))
            }
            _ => parts.push(format!("LAPDm {}", self.frame.label())),
        }
        if let Some(identity) = &self.identity {
            parts.push(identity.label());
        }
        if let Some(cipher) = self.cipher {
            parts.push(cipher_label(cipher));
        }
        parts.join(" · ")
    }
}

#[must_use]
pub fn cipher_label(algorithm: u8) -> String {
    match algorithm {
        0 => "no cipher".to_owned(),
        n => format!("A5/{n}"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GsmMessage {
    SystemInformation { si: GsmSi, cell: GsmCell },
    Paging { identities: Vec<GsmIdentity> },
    ImmediateAssignment { assignments: Vec<GsmAssignment> },
    AssignmentReject { requests: Vec<u8> },
    Dedicated(GsmDedicated),
    Other { message_type: u8 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GsmFrame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arfcn: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<GsmBand>,
    pub bsic: u8,
    pub frame_number: u32,
    pub timeslot: u8,
    pub channel: GsmLogicalChannel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subchannel: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cgi: Option<String>,
    pub message: GsmMessage,
    pub snr_db: f32,
    pub frequency_error_hz: f32,
    pub raw: String,
}

impl GsmFrame {
    #[must_use]
    pub fn bsic_label(&self) -> String {
        format!("{}-{}", self.bsic >> 3, self.bsic & 7)
    }

    #[must_use]
    pub fn summary(&self) -> String {
        match &self.message {
            GsmMessage::SystemInformation { si, cell } => {
                let mut parts = vec![si.label().to_owned()];
                parts.extend(cell.plmn());
                parts.extend(cell.lac.map(|lac| format!("LAC {lac}")));
                parts.extend(cell.cell_id.map(|ci| format!("CI {ci}")));
                if matches!(si, GsmSi::Si2 | GsmSi::Si2bis | GsmSi::Si2ter | GsmSi::Si5) {
                    parts.push(format!("{} neighbours", cell.neighbours.len()));
                }
                parts.join(" · ")
            }
            GsmMessage::Paging { identities } => format!(
                "Paging · {}",
                identities
                    .iter()
                    .map(GsmIdentity::label)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            GsmMessage::ImmediateAssignment { assignments } => format!(
                "Assignment · {}",
                assignments
                    .iter()
                    .map(GsmAssignment::label)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            GsmMessage::AssignmentReject { requests } => {
                format!("Assignment reject · {} requests", requests.len())
            }
            GsmMessage::Dedicated(dedicated) => {
                format!("{} · {}", self.channel.label(), dedicated.summary())
            }
            GsmMessage::Other { message_type: 0x20 } => {
                format!("{} · NCH notification", self.channel.label())
            }
            GsmMessage::Other { message_type } => {
                format!("{} · RR 0x{message_type:02X}", self.channel.label())
            }
        }
    }
}

impl EventPayload for GsmFrame {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: self.cgi.clone(),
            fields: gsm_fields(self),
            ..Presentation::of(self.summary())
        }
    }
}

fn gsm_fields(f: &GsmFrame) -> Vec<Field> {
    let channel = match f.subchannel {
        Some(sub) => format!("{} {sub}", f.channel.label()),
        None => f.channel.label().to_owned(),
    };
    let mut shown = fields(&[
        ("Message", text_of(f.summary())),
        ("Cell", text(f.cgi.as_deref())),
        ("Band", f.band.and_then(|band| text_of(band.label()))),
        ("ARFCN", count(f.arfcn)),
        ("BSIC", text_of(f.bsic_label())),
        ("Frame", count(Some(f.frame_number))),
        ("Timeslot", count(Some(f.timeslot))),
        ("Channel", text_of(channel)),
    ]);
    if let GsmMessage::SystemInformation { cell, .. } = &f.message {
        shown.extend(cell_fields(cell));
    }
    shown.extend(fields(&[
        ("Offset", number(Some(f.frequency_error_hz), Unit::Hz, 0)),
        ("SNR", number(Some(f.snr_db), Unit::Db, 1)),
        ("Raw", text(Some(&f.raw))),
    ]));
    shown
}

fn cell_fields(cell: &GsmCell) -> Vec<Field> {
    let list = |arfcns: &[u16]| {
        arfcns
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let si = cell
        .system_information
        .iter()
        .map(|si| si.label())
        .collect::<Vec<_>>()
        .join(", ");
    fields(&[
        ("CCCH", cell.ccch.and_then(|ccch| text_of(ccch.label()))),
        ("Cell channels", text(Some(&list(&cell.cell_channels)))),
        ("Neighbours", text(Some(&list(&cell.neighbours)))),
        ("RxLev min", number(cell.rxlev_access_min_dbm, Unit::Dbm, 0)),
        ("Max power level", count(cell.ms_txpwr_max_cch)),
        (
            "Periodic update",
            number(
                cell.t3212_min.map(|minutes| f64::from(minutes) * 60.0),
                Unit::Seconds,
                0,
            ),
        ),
        ("Barred", only_if(cell.barred)),
        ("GPRS", only_if(cell.gprs)),
        ("System information", text(Some(&si))),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downlink_frequencies_name_their_band_and_arfcn() {
        assert_eq!(GsmBand::at(1_847_800_000.0), Some((GsmBand::Dcs1800, 725)));
        assert_eq!(GsmBand::at(935_200_000.0), Some((GsmBand::Gsm900, 1)));
        assert_eq!(GsmBand::at(925_200_000.0), Some((GsmBand::Gsm900, 975)));
        assert_eq!(GsmBand::at(869_200_000.0), Some((GsmBand::Gsm850, 128)));
        assert_eq!(GsmBand::at(1_930_200_000.0), Some((GsmBand::Pcs1900, 512)));
        assert_eq!(GsmBand::at(1_847_900_000.0), None);
        assert_eq!(GsmBand::at(100_000_000.0), None);
        assert_eq!(GsmBand::Dcs1800.downlink_hz(725), Some(1_847_800_000.0));
        assert_eq!(GsmBand::Gsm900.downlink_hz(1023), Some(934_800_000.0));
    }

    #[test]
    fn empty_settings_take_the_defaults() {
        let params: GsmParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params, GsmParams::default());
    }

    #[test]
    fn a_cell_names_its_global_identity() {
        let cell = GsmCell {
            mcc: Some("262".to_owned()),
            mnc: Some("02".to_owned()),
            lac: Some(318),
            cell_id: Some(11225),
            ..GsmCell::default()
        };
        assert_eq!(cell.cgi().as_deref(), Some("262-02-318-11225"));
    }

    #[test]
    fn a_system_information_frame_lists_the_cell() {
        let frame: GsmFrame = serde_json::from_value(serde_json::json!({
            "arfcn": 62, "band": "gsm900", "bsic": 45, "frame_number": 1234, "timeslot": 0,
            "channel": "bcch", "cgi": "262-01-4711-3", "snr_db": 18.25,
            "frequency_error_hz": -120.4, "raw": "55",
            "message": {
                "type": "system_information", "si": "si3",
                "cell": { "mcc": "262", "mnc": "01", "lac": 4711, "cell_id": 3,
                    "ccch": "combined", "neighbours": [1, 5], "t3212_min": 6, "gprs": true,
                    "system_information": ["si1", "si3"] },
            },
        }))
        .unwrap();
        let view = frame.present(EventKind::Gsm);
        assert_eq!(view.field("Band"), text_of("GSM 900").as_ref());
        assert_eq!(view.field("BSIC"), text_of("5-5").as_ref());
        assert_eq!(view.field("Channel"), text_of("BCCH").as_ref());
        assert_eq!(view.field("CCCH"), text_of("1 CCCH with SDCCH/4").as_ref());
        assert_eq!(view.field("Neighbours"), text_of("1, 5").as_ref());
        assert_eq!(
            view.field("Periodic update"),
            number(Some(360.0), Unit::Seconds, 0).as_ref()
        );
        assert_eq!(view.field("GPRS"), only_if(true).as_ref());
        assert_eq!(view.field("Barred"), None);
        assert_eq!(
            view.field("System information"),
            text_of("SI1, SI3").as_ref()
        );
    }
}
