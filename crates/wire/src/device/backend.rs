use serde::{Deserialize, Serialize};
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

macro_rules! backends {
    ($callback:ident) => {
        $callback! {
            (Virtual, "virtual", Synthetic, "Virtual", None, None),
            (Recording, "recording", Playback, "Recording", None, None),
            (Rtlsdr, "rtlsdr", Local, "RTL-SDR", Some("rtlsdr"), None),
            (Hackrf, "hackrf", Local, "HackRF", Some("hackrf"), None),
            (Airspy, "airspy", Local, "Airspy", Some("airspy"), None),
            (Airspyhf, "airspyhf", Local, "Airspy HF+", Some("airspyhf"), None),
            (Funcube, "funcube", Local, "FUNcube Dongle", Some("fcdpp"), None),
            (Cr8, "cr8", Local, "CR-8", None, None),
            (Sdrplay, "sdrplay", Local, "SDRplay", None, None),
            (Soapy, "soapy", Local, "SoapySDR", None, None),
            (Rtltcp, "rtltcp", Network, "rtl_tcp", None, Some("192.168.1.5:1234")),
            (Spyserver, "spyserver", Network, "SpyServer", None, Some("192.168.1.5:5555")),
            (Sdrconnect, "sdrconnect", Network, "SDRconnect", None, Some("192.168.1.5:5454")),
            (Kiwisdr, "kiwisdr", Network, "KiwiSDR", None, Some("kiwi.example.org:8073")),
            (Ad936x, "ad936x", Network, "AntSDR / Pluto", Some("plutosdr"), Some("192.168.1.10:30431")),
            (Espsdr, "espsdr", Network, "ESP-SDR", None, Some("192.168.1.20:80")),
        }
    };
}

macro_rules! define_backends {
    ($((
        $variant:ident,
        $id:literal,
        $reach:ident,
        $label:literal,
        $soapy:expr,
        $address:expr
    )),* $(,)?) => {
        /// Which driver opens a radio. The one name a backend has, shared by the device crates,
        /// the stored patch, the doctor and the browser.
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
            ToSchema,
        )]
        pub enum DeviceBackend {
            $(
                #[serde(rename = $id)]
                #[schema(rename = $id)]
                $variant,
            )*
        }

        impl DeviceBackend {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $id,)*
                }
            }

            #[must_use]
            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $($id => Some(Self::$variant),)*
                    _ => None,
                }
            }

            #[must_use]
            pub const fn reach(self) -> BackendReach {
                match self {
                    $(Self::$variant => BackendReach::$reach,)*
                }
            }

            #[must_use]
            pub const fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label,)*
                }
            }

            /// The SoapySDR module this backend speaks to the same hardware as, so the Soapy
            /// driver can hide what a native one already lists.
            #[must_use]
            pub const fn soapy_module(self) -> Option<&'static str> {
                match self {
                    $(Self::$variant => $soapy,)*
                }
            }

            /// What an address field shows before the operator types one.
            #[must_use]
            pub const fn address_hint(self) -> Option<&'static str> {
                match self {
                    $(Self::$variant => $address,)*
                }
            }
        }
    };
}

backends!(define_backends);

impl DeviceBackend {
    #[must_use]
    pub const fn is_radio(self) -> bool {
        matches!(self.reach(), BackendReach::Local | BackendReach::Network)
    }

    #[must_use]
    pub const fn is_synthetic(self) -> bool {
        matches!(self.reach(), BackendReach::Synthetic)
    }

    /// Whether a node opens this backend for itself, so the browser never offers it in the radio
    /// list.
    #[must_use]
    pub const fn node_owned(self) -> bool {
        matches!(self.reach(), BackendReach::Playback)
    }

    #[must_use]
    pub const fn addressable(self) -> bool {
        self.address_hint().is_some()
    }

    #[must_use]
    pub fn radios() -> Vec<Self> {
        Self::ALL
            .iter()
            .copied()
            .filter(|backend| backend.is_radio())
            .collect()
    }
}

impl std::fmt::Display for DeviceBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BackendEntry {
    pub backend: DeviceBackend,
    pub label: String,
    pub reach: BackendReach,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address_hint: Option<String>,
}

#[must_use]
pub fn backend_table() -> Vec<BackendEntry> {
    DeviceBackend::ALL
        .iter()
        .map(|&backend| BackendEntry {
            backend,
            label: backend.label().to_owned(),
            reach: backend.reach(),
            address_hint: backend.address_hint().map(str::to_owned),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_backend_is_named_once() {
        let mut ids: Vec<&str> = DeviceBackend::ALL
            .iter()
            .map(|backend| backend.as_str())
            .collect();
        let listed = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), listed);
    }

    #[test]
    fn an_id_round_trips_through_parse() {
        for &backend in DeviceBackend::ALL {
            assert_eq!(DeviceBackend::parse(backend.as_str()), Some(backend));
        }
        assert_eq!(DeviceBackend::parse("kraken"), None);
    }

    #[test]
    fn an_id_serialises_as_itself() {
        for &backend in DeviceBackend::ALL {
            let json = serde_json::to_value(backend).expect("backend serialises");
            assert_eq!(json, serde_json::Value::String(backend.as_str().to_owned()));
        }
    }

    #[test]
    fn only_a_radio_backend_counts_as_hardware() {
        assert!(!DeviceBackend::Virtual.is_radio());
        assert!(!DeviceBackend::Recording.is_radio());
        assert!(DeviceBackend::Rtlsdr.is_radio());
        assert!(DeviceBackend::Rtltcp.is_radio());
        assert!(DeviceBackend::Recording.node_owned());
        assert!(!DeviceBackend::Virtual.node_owned());
    }

    #[test]
    fn a_network_backend_hints_an_address_and_a_local_one_does_not() {
        for &backend in DeviceBackend::ALL {
            assert_eq!(
                backend.addressable(),
                backend.reach() == BackendReach::Network,
                "{backend}"
            );
        }
    }
}
