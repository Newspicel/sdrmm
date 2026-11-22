use std::borrow::Cow;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::ToSchema;

/// How a backend gets at its samples, and so whether the browser offers it as a radio, keeps it
/// for a node that owns one, or hides it behind the synthetic tab. The one policy every surface
/// reads.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BackendReach {
    Local,
    Network,
    Synthetic,
    Playback,
}

const MAX_ID: usize = 31;

/// Which driver opens a radio. The one name a backend has, shared by the device crates, plugins,
/// the stored patch, the doctor and the browser. Any id a driver declares is valid, so a patch
/// naming a backend this build lacks still reads back.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceBackend {
    len: u8,
    bytes: [u8; MAX_ID],
}

impl DeviceBackend {
    pub const VIRTUAL: Self = Self::new("virtual");
    pub const RECORDING: Self = Self::new("recording");
    pub const RTLSDR: Self = Self::new("rtlsdr");
    pub const HACKRF: Self = Self::new("hackrf");
    pub const AIRSPY: Self = Self::new("airspy");
    pub const AIRSPYHF: Self = Self::new("airspyhf");
    pub const FUNCUBE: Self = Self::new("funcube");
    pub const CR8: Self = Self::new("cr8");
    pub const SDRPLAY: Self = Self::new("sdrplay");
    pub const AARONIA: Self = Self::new("aaronia");
    pub const SOAPY: Self = Self::new("soapy");
    pub const RTLTCP: Self = Self::new("rtltcp");
    pub const SPYSERVER: Self = Self::new("spyserver");
    pub const SDRCONNECT: Self = Self::new("sdrconnect");
    pub const KIWISDR: Self = Self::new("kiwisdr");
    pub const AD936X: Self = Self::new("ad936x");
    pub const ESPSDR: Self = Self::new("espsdr");

    /// A backend id known when compiling. Fails the build on an id [`Self::parse`] refuses.
    #[must_use]
    pub const fn new(id: &str) -> Self {
        match Self::checked(id) {
            Some(backend) => backend,
            None => panic!("a backend id is 1 to 31 of a-z, 0-9, _ and -"),
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::checked(text)
    }

    const fn checked(id: &str) -> Option<Self> {
        let text = id.as_bytes();
        if text.is_empty() || text.len() > MAX_ID {
            return None;
        }
        let mut bytes = [0u8; MAX_ID];
        let mut at = 0;
        while at < text.len() {
            let byte = text[at];
            if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-')
            {
                return None;
            }
            bytes[at] = byte;
            at += 1;
        }
        Some(Self {
            len: text.len() as u8,
            bytes,
        })
    }

    #[must_use]
    pub const fn as_str(&self) -> &str {
        match std::str::from_utf8(self.bytes.split_at(self.len as usize).0) {
            Ok(text) => text,
            Err(_) => "",
        }
    }
}

impl PartialOrd for DeviceBackend {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DeviceBackend {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl std::fmt::Display for DeviceBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Debug for DeviceBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl Serialize for DeviceBackend {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DeviceBackend {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Cow::<'de, str>::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "backend {text:?} is not 1 to 31 of a-z, 0-9, _ and -"
            ))
        })
    }
}

impl utoipa::PartialSchema for DeviceBackend {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::String)
            .description(Some(
                "Which driver opens a radio: a built-in or a device plugin's id.",
            ))
            .pattern(Some("^[a-z0-9_-]{1,31}$"))
            .into()
    }
}

impl ToSchema for DeviceBackend {}

/// A USB device a driver speaks to, by the identity it announces on the bus. A product of
/// `None` claims the whole vendor, which is only right for a vendor that makes nothing else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UsbIdentity {
    pub vendor: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<u16>,
    pub name: String,
}

impl UsbIdentity {
    #[must_use]
    pub fn new(vendor: u16, product: Option<u16>, name: &str) -> Self {
        Self {
            vendor,
            product,
            name: name.to_owned(),
        }
    }

    #[must_use]
    pub fn matches(&self, vendor: u16, product: u16) -> bool {
        self.vendor == vendor && self.product.is_none_or(|wanted| wanted == product)
    }
}

/// What a driver tells every surface about its backend: the browser's label and reach, the
/// address field's hint, the Soapy module it drives itself and the USB devices it speaks to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BackendSpec {
    pub backend: DeviceBackend,
    pub label: String,
    pub reach: BackendReach,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soapy_module: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub usb: Vec<UsbIdentity>,
}

impl BackendSpec {
    #[must_use]
    pub fn new(backend: DeviceBackend, label: &str, reach: BackendReach) -> Self {
        Self {
            backend,
            label: label.to_owned(),
            reach,
            address_hint: None,
            soapy_module: None,
            usb: Vec::new(),
        }
    }

    /// The spec of a driver that declares nothing beyond its id: a local radio named by it.
    #[must_use]
    pub fn bare(backend: DeviceBackend) -> Self {
        Self::new(backend, backend.as_str(), BackendReach::Local)
    }

    #[must_use]
    pub fn address_hint(mut self, hint: &str) -> Self {
        self.address_hint = Some(hint.to_owned());
        self
    }

    #[must_use]
    pub fn soapy_module(mut self, module: &str) -> Self {
        self.soapy_module = Some(module.to_owned());
        self
    }

    #[must_use]
    pub fn usb(mut self, identities: impl IntoIterator<Item = UsbIdentity>) -> Self {
        self.usb.extend(identities);
        self
    }

    #[must_use]
    pub const fn is_radio(&self) -> bool {
        matches!(self.reach, BackendReach::Local | BackendReach::Network)
    }

    #[must_use]
    pub const fn is_synthetic(&self) -> bool {
        matches!(self.reach, BackendReach::Synthetic)
    }

    /// Whether a node opens this backend for itself, so the browser never offers it in the radio
    /// list.
    #[must_use]
    pub const fn node_owned(&self) -> bool {
        matches!(self.reach, BackendReach::Playback)
    }

    #[must_use]
    pub const fn addressable(&self) -> bool {
        self.address_hint.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_round_trips_as_a_plain_string() {
        let json = serde_json::to_string(&DeviceBackend::RTLSDR).expect("serializes");
        assert_eq!(json, "\"rtlsdr\"");
        let back: DeviceBackend = serde_json::from_str(&json).expect("parses");
        assert_eq!(back, DeviceBackend::RTLSDR);
    }

    #[test]
    fn an_unknown_id_still_parses() {
        let backend = DeviceBackend::parse("kraken_2").expect("valid id");
        assert_eq!(backend.as_str(), "kraken_2");
        assert_eq!(backend.to_string(), "kraken_2");
    }

    #[test]
    fn a_bad_id_is_refused() {
        for bad in ["", "Rtl", "a b", "a:b", &"x".repeat(32)] {
            assert_eq!(DeviceBackend::parse(bad), None, "{bad:?}");
            assert!(serde_json::from_value::<DeviceBackend>(serde_json::json!(bad)).is_err());
        }
        assert!(DeviceBackend::parse(&"x".repeat(31)).is_some());
    }

    #[test]
    fn ids_sort_by_name() {
        let mut ids = [
            DeviceBackend::SOAPY,
            DeviceBackend::AIRSPYHF,
            DeviceBackend::CR8,
        ];
        ids.sort();
        assert_eq!(
            ids,
            [
                DeviceBackend::AIRSPYHF,
                DeviceBackend::CR8,
                DeviceBackend::SOAPY
            ]
        );
    }

    #[test]
    fn a_vendor_wide_identity_matches_every_product() {
        let whole = UsbIdentity::new(0x1df7, None, "SDRplay RSP");
        assert!(whole.matches(0x1df7, 0x3020));
        assert!(!whole.matches(0x1df8, 0x3020));
        let one = UsbIdentity::new(0x0bda, Some(0x2838), "RTL-SDR");
        assert!(one.matches(0x0bda, 0x2838));
        assert!(!one.matches(0x0bda, 0x2839));
    }
}
