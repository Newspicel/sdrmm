use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const BLUETOOTH_RATE_HZ: f64 = 4_000_000.0;
pub const BLUETOOTH_BAND_RATE_HZ: f64 = 20_000_000.0;
pub const BLUETOOTH_CHANNEL_WIDTH_HZ: f64 = 2_000_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BleLink {
    #[default]
    Channel,
    Band,
}

impl BleLink {
    #[must_use]
    pub fn input_rate_hz(self) -> f64 {
        match self {
            Self::Channel => BLUETOOTH_RATE_HZ,
            Self::Band => BLUETOOTH_BAND_RATE_HZ,
        }
    }

    #[must_use]
    pub fn bandwidth_hz(self) -> f64 {
        match self {
            Self::Channel => BLUETOOTH_CHANNEL_WIDTH_HZ,
            Self::Band => BLUETOOTH_BAND_RATE_HZ - BLUETOOTH_CHANNEL_WIDTH_HZ,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BleParams {
    #[serde(default)]
    pub link: BleLink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlePdu {
    AdvInd,
    AdvDirectInd,
    AdvNonconnInd,
    ScanReq,
    ScanRsp,
    ConnectInd,
    AdvScanInd,
    AdvExtInd,
    AuxAdvInd,
    AuxConnectRsp,
}

impl BlePdu {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::AdvInd => "ADV_IND",
            Self::AdvDirectInd => "ADV_DIRECT_IND",
            Self::AdvNonconnInd => "ADV_NONCONN_IND",
            Self::ScanReq => "SCAN_REQ",
            Self::ScanRsp => "SCAN_RSP",
            Self::ConnectInd => "CONNECT_IND",
            Self::AdvScanInd => "ADV_SCAN_IND",
            Self::AdvExtInd => "ADV_EXT_IND",
            Self::AuxAdvInd => "AUX_ADV_IND",
            Self::AuxConnectRsp => "AUX_CONNECT_RSP",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlePhy {
    Le1m,
    Le2m,
    LeCodedS8,
    LeCodedS2,
    LeCoded,
}

impl BlePhy {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Le1m => "LE 1M",
            Self::Le2m => "LE 2M",
            Self::LeCodedS8 => "LE Coded S8",
            Self::LeCodedS2 => "LE Coded S2",
            Self::LeCoded => "LE Coded",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BleAddressKind {
    Public,
    RandomStatic,
    ResolvablePrivate,
    NonResolvablePrivate,
    Reserved,
}

impl BleAddressKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::RandomStatic => "random static",
            Self::ResolvablePrivate => "resolvable private",
            Self::NonResolvablePrivate => "non-resolvable private",
            Self::Reserved => "reserved",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BleAddress {
    pub address: String,
    pub kind: BleAddressKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BleAuxPointer {
    pub channel: u8,
    pub phy: BlePhy,
    pub offset_us: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BleAdi {
    pub set: u8,
    pub data_id: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BleFlags {
    pub limited: bool,
    pub general: bool,
    pub le_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BleService {
    pub uuid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BleManufacturer {
    pub company_id: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BleBeacon {
    Ibeacon {
        uuid: String,
        major: u16,
        minor: u16,
        measured_dbm: i8,
    },
    AltBeacon {
        id: String,
        measured_dbm: i8,
    },
    EddystoneUid {
        namespace: String,
        instance: String,
        tx_power_dbm: i8,
    },
    EddystoneUrl {
        url: String,
        tx_power_dbm: i8,
    },
    EddystoneTlm {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        battery_mv: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temperature_c: Option<f32>,
        adverts: u32,
        uptime_s: f64,
    },
    EddystoneEid {
        eid: String,
        tx_power_dbm: i8,
    },
    FindMy {
        maintained: bool,
    },
    ExposureNotification {
        identifier: String,
    },
    FastPair {
        model: String,
    },
}

impl BleBeacon {
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ibeacon { .. } => "iBeacon",
            Self::AltBeacon { .. } => "AltBeacon",
            Self::EddystoneUid { .. } => "Eddystone UID",
            Self::EddystoneUrl { .. } => "Eddystone URL",
            Self::EddystoneTlm { .. } => "Eddystone TLM",
            Self::EddystoneEid { .. } => "Eddystone EID",
            Self::FindMy { .. } => "Find My",
            Self::ExposureNotification { .. } => "Exposure Notification",
            Self::FastPair { .. } => "Fast Pair",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BleAdvert {
    pub pdu: BlePdu,
    pub phy: BlePhy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<BleAddress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<BleAddress>,
    pub level_dbfs: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub repeats: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<BleFlags>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_power_dbm: Option<i8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<BleService>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manufacturer: Vec<BleManufacturer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beacon: Option<BleBeacon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adi: Option<BleAdi>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aux: Option<BleAuxPointer>,
    pub data: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rejected: u32,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl BleAdvert {
    #[must_use]
    pub fn station(&self) -> Option<String> {
        self.address.as_ref().map(|address| address.address.clone())
    }

    #[must_use]
    pub fn vendor(&self) -> Option<&str> {
        self.manufacturer
            .iter()
            .find_map(|maker| maker.company.as_deref())
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        parts.extend(self.station());
        parts.extend(self.name.clone());
        parts.extend(self.beacon.as_ref().map(|beacon| beacon.label().to_owned()));
        parts.extend(self.vendor().map(str::to_owned));
        if parts.is_empty() {
            parts.push(self.pdu.label().to_owned());
        }
        parts.push(format!("{:.0} dBFS", self.level_dbfs));
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advert() -> BleAdvert {
        BleAdvert {
            pdu: BlePdu::AdvInd,
            phy: BlePhy::Le1m,
            channel: Some(38),
            address: Some(BleAddress {
                address: "C0:11:22:33:44:55".to_owned(),
                kind: BleAddressKind::RandomStatic,
            }),
            target: None,
            level_dbfs: -41.4,
            repeats: 0,
            name: Some("Thermo".to_owned()),
            flags: None,
            tx_power_dbm: None,
            appearance: None,
            services: Vec::new(),
            manufacturer: vec![BleManufacturer {
                company_id: 0x004C,
                company: Some("Apple".to_owned()),
                data: "1005".to_owned(),
            }],
            beacon: None,
            uri: None,
            adi: None,
            aux: None,
            data: "020106".to_owned(),
            rejected: 0,
        }
    }

    #[test]
    fn the_summary_names_the_device() {
        assert_eq!(
            advert().summary(),
            "C0:11:22:33:44:55 · Thermo · Apple · -41 dBFS"
        );
    }

    #[test]
    fn defaults_fill_an_empty_settings_object() {
        let params: BleParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.link, BleLink::Channel);
        let json = serde_json::to_value(advert()).unwrap();
        assert!(json.get("repeats").is_none());
        assert_eq!(json["address"]["kind"], "random_static");
    }
}
