use sdrmm_wire::{
    device::{DeviceInfo, RECORDING_DRIVER_ID, SIGGEN_DRIVER_ID},
    patch::{DeviceRef, NodeBody, PatchGraph},
};

const VIRTUAL_DRIVER: &str = "virtual";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceTab {
    Radios,
    Network,
    Virtual,
}

pub struct NetworkBackend {
    pub driver: &'static str,
    pub label: &'static str,
    pub placeholder: &'static str,
}

pub const NETWORK_BACKENDS: [NetworkBackend; 5] = [
    NetworkBackend {
        driver: "rtltcp",
        label: "rtl_tcp",
        placeholder: "192.168.1.5:1234",
    },
    NetworkBackend {
        driver: "spyserver",
        label: "SpyServer",
        placeholder: "192.168.1.5:5555",
    },
    NetworkBackend {
        driver: "sdrconnect",
        label: "SDRconnect",
        placeholder: "192.168.1.5:5454",
    },
    NetworkBackend {
        driver: "kiwisdr",
        label: "KiwiSDR",
        placeholder: "kiwi.example.org:8073",
    },
    NetworkBackend {
        driver: "ad936x",
        label: "AntSDR / Pluto",
        placeholder: "192.168.1.10:30431",
    },
];

#[must_use]
pub fn device_id(info: &DeviceInfo) -> String {
    format!("{}:{}", info.driver, info.key)
}

#[must_use]
pub fn is_virtual(info: &DeviceInfo) -> bool {
    info.driver == VIRTUAL_DRIVER
}

#[must_use]
pub fn rank_devices(devices: &[DeviceInfo]) -> Vec<DeviceInfo> {
    let mut ranked = devices.to_vec();
    ranked.sort_by(|a, b| {
        is_virtual(a)
            .cmp(&is_virtual(b))
            .then_with(|| a.label.cmp(&b.label))
    });
    ranked
}

#[must_use]
pub fn show_synthetic() -> bool {
    cfg!(debug_assertions)
        || std::env::var("SDRMM_ENABLE_SYNTHETIC_DEVICES").is_ok_and(|value| value == "true")
}

#[must_use]
pub fn visible_devices(devices: &[DeviceInfo], synthetic: bool) -> Vec<DeviceInfo> {
    let node_owned = [RECORDING_DRIVER_ID, SIGGEN_DRIVER_ID];
    let pickable: Vec<DeviceInfo> = devices
        .iter()
        .filter(|info| !node_owned.contains(&info.driver.as_str()))
        .filter(|info| synthetic || !is_virtual(info))
        .cloned()
        .collect();
    rank_devices(&pickable)
}

#[must_use]
pub fn unclaimed_devices(devices: &[DeviceInfo], claimed: &[DeviceRef]) -> Vec<DeviceInfo> {
    devices
        .iter()
        .filter(|info| !claimed.iter().any(|reference| reference.matches(info)))
        .cloned()
        .collect()
}

#[must_use]
pub fn claimed_devices(graph: &PatchGraph, except: &str) -> Vec<DeviceRef> {
    graph
        .nodes
        .iter()
        .filter(|node| node.id != except)
        .filter_map(|node| match &node.body {
            NodeBody::Device(device) => device.device.clone(),
            _ => None,
        })
        .collect()
}

#[must_use]
pub fn group_devices(devices: &[DeviceInfo]) -> (Vec<DeviceInfo>, Vec<DeviceInfo>) {
    devices.iter().cloned().partition(|info| !is_virtual(info))
}

#[must_use]
pub fn source_tabs(virtual_count: usize) -> Vec<(SourceTab, String, String)> {
    let mut tabs = vec![
        (
            SourceTab::Radios,
            "Radios".to_owned(),
            "Radios attached to this machine".to_owned(),
        ),
        (
            SourceTab::Network,
            "Network".to_owned(),
            "rtl_tcp, SpyServer, SDRconnect, KiwiSDR, AntSDR or Pluto".to_owned(),
        ),
    ];
    if virtual_count > 0 {
        tabs.push((
            SourceTab::Virtual,
            format!("Virtual ({virtual_count})"),
            "Synthetic radios for development and tests".to_owned(),
        ));
    }
    tabs
}

fn split_scheme(address: &str) -> (Option<String>, &str) {
    let Some((scheme, rest)) = address.split_once("://") else {
        return (None, address);
    };
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+._-".contains(c));
    if valid {
        (Some(scheme.to_ascii_lowercase()), rest)
    } else {
        (None, address)
    }
}

fn has_port(host: &str) -> bool {
    host.rsplit_once(':')
        .is_some_and(|(_, port)| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
}

#[must_use]
pub fn network_device_id(driver: &str, address: &str) -> Option<String> {
    let (scheme, rest) = split_scheme(address.trim());
    let trimmed = rest.trim_end_matches('/');
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    match scheme.as_deref() {
        Some("https" | "wss") => Some(format!("{driver}:https://{trimmed}")),
        Some("http" | "ws") => {
            let host = trimmed.rsplit_once('@').map_or(trimmed, |(_, host)| host);
            let port = if has_port(host) { "" } else { ":80" };
            Some(format!("{driver}:{trimmed}{port}"))
        }
        _ => Some(format!("{driver}:{trimmed}")),
    }
}

#[must_use]
pub fn network_radio_label(id: &str) -> String {
    let (driver, address) = id.split_once(':').unwrap_or(("", id));
    let address = address.strip_prefix("https://").unwrap_or(address);
    let host = address.rsplit_once('@').map_or(address, |(_, host)| host);
    NETWORK_BACKENDS
        .iter()
        .find(|backend| backend.driver == driver)
        .map_or_else(
            || host.to_owned(),
            |backend| format!("{} {host}", backend.label),
        )
}

#[must_use]
pub fn default_port(placeholder: &str) -> &str {
    placeholder.rsplit(':').next().unwrap_or(placeholder)
}

#[must_use]
pub fn address_title(backend: &NetworkBackend) -> String {
    format!(
        "Host or address of the {} server; without a port it uses {}",
        backend.label,
        default_port(backend.placeholder)
    )
}

#[must_use]
pub fn device_hint(info: &DeviceInfo) -> Option<&'static str> {
    (info.driver == "rtlsdr" && info.serial.is_none())
        .then_some("Known by USB port, not serial: give it its own serial with rtl_eeprom -s")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(driver: &str, key: &str, label: &str) -> DeviceInfo {
        DeviceInfo {
            driver: driver.to_owned(),
            key: key.to_owned(),
            label: label.to_owned(),
            serial: None,
            profile: None,
        }
    }

    fn reference(backend: &str, key: Option<&str>) -> DeviceRef {
        DeviceRef {
            backend: backend.to_owned(),
            serial: None,
            key: key.map(str::to_owned),
        }
    }

    fn ids(devices: &[DeviceInfo]) -> Vec<String> {
        devices.iter().map(device_id).collect()
    }

    #[test]
    fn every_real_radio_ranks_above_the_virtual_ones() {
        let ranked = rank_devices(&[
            device("virtual", "siggen", "Signal Generator"),
            device("rtlsdr", "00000001", "RTL-SDR 00000001"),
            device("rtltcp", "10.0.0.5:1234", "rtl_tcp 10.0.0.5:1234"),
        ]);
        assert_eq!(ranked.last().map(|d| d.driver.as_str()), Some("virtual"));
        assert!(ranked[..2].iter().all(|d| !is_virtual(d)));
    }

    #[test]
    fn radios_a_node_of_their_own_opens_stay_out_of_the_picker() {
        let devices = [
            device("virtual", "siggen", "Signal Generator"),
            device("virtual", "array4", "Coherent Array"),
            device("recording", "airband", "airband"),
            device("siggen", "signal_gen-a1b2", "Signal generator"),
            device("rtlsdr", "00000001", "RTL-SDR 00000001"),
        ];
        assert_eq!(
            ids(&visible_devices(&devices, true)),
            vec!["rtlsdr:00000001", "virtual:array4", "virtual:siggen"]
        );
        assert_eq!(
            ids(&visible_devices(&devices, false)),
            vec!["rtlsdr:00000001"]
        );
    }

    #[test]
    fn a_radio_another_node_names_is_not_offered_again() {
        let devices = [
            device("rtlsdr", "00000001", "RTL-SDR 00000001"),
            device("rtlsdr", "00000002", "RTL-SDR 00000002"),
            device("virtual", "siggen", "Signal Generator"),
        ];
        assert_eq!(
            ids(&unclaimed_devices(
                &devices,
                &[reference("rtlsdr", Some("00000001"))]
            )),
            vec!["rtlsdr:00000002", "virtual:siggen"]
        );
        assert_eq!(
            ids(&unclaimed_devices(&devices, &[reference("virtual", None)])),
            vec!["rtlsdr:00000001", "rtlsdr:00000002"]
        );
        assert_eq!(unclaimed_devices(&devices, &[]), devices.to_vec());
        let serialed = DeviceInfo {
            serial: Some("0".to_owned()),
            ..device("rtlsdr", "0@rx", "RTL-SDR")
        };
        let held = DeviceRef {
            backend: "rtlsdr".to_owned(),
            serial: Some("0".to_owned()),
            key: Some("0@rx".to_owned()),
        };
        assert!(unclaimed_devices(&[serialed], &[held]).is_empty());
    }

    #[test]
    fn virtual_radios_keep_out_of_the_top_list_and_get_their_own_tab() {
        let (radios, synthetic) = group_devices(&[
            device("rtlsdr", "00000001", "RTL-SDR 00000001"),
            device("virtual", "siggen", "Signal Generator"),
            device("virtual", "array4", "Coherent Array"),
        ]);
        assert_eq!(ids(&radios), vec!["rtlsdr:00000001"]);
        assert_eq!(ids(&synthetic), vec!["virtual:siggen", "virtual:array4"]);
        let tabs: Vec<(SourceTab, String)> = source_tabs(1)
            .into_iter()
            .map(|(tab, label, _)| (tab, label))
            .collect();
        assert_eq!(
            tabs,
            vec![
                (SourceTab::Radios, "Radios".to_owned()),
                (SourceTab::Network, "Network".to_owned()),
                (SourceTab::Virtual, "Virtual (1)".to_owned()),
            ]
        );
        assert_eq!(source_tabs(0).len(), 2);
    }

    #[test]
    fn a_network_address_becomes_the_id_the_open_endpoint_takes() {
        assert_eq!(
            network_device_id("rtltcp", "10.0.0.5:1234").as_deref(),
            Some("rtltcp:10.0.0.5:1234")
        );
        assert_eq!(
            network_device_id("spyserver", "spy.local").as_deref(),
            Some("spyserver:spy.local")
        );
        assert_eq!(
            network_device_id("rtltcp", "  Radio.Local  ").as_deref(),
            Some("rtltcp:Radio.Local")
        );
        assert_eq!(
            network_device_id("rtltcp", "[2001:db8::1]:1234").as_deref(),
            Some("rtltcp:[2001:db8::1]:1234")
        );
        assert_eq!(
            network_device_id("rtltcp", "rtl_tcp://10.0.0.5:1234").as_deref(),
            Some("rtltcp:10.0.0.5:1234")
        );
        assert_eq!(
            network_device_id("spyserver", "sdr://spy.local:5555").as_deref(),
            Some("spyserver:spy.local:5555")
        );
        assert_eq!(
            network_device_id("sdrconnect", "ws://rsp.local:5454").as_deref(),
            Some("sdrconnect:rsp.local:5454")
        );
        assert_eq!(
            network_device_id("kiwisdr", "http://kiwi.local/").as_deref(),
            Some("kiwisdr:kiwi.local:80")
        );
        assert_eq!(
            network_device_id("kiwisdr", "ws://user@kiwi.local").as_deref(),
            Some("kiwisdr:user@kiwi.local:80")
        );
        assert_eq!(
            network_device_id("kiwisdr", "HTTPS://kiwi.example.org:8073//").as_deref(),
            Some("kiwisdr:https://kiwi.example.org:8073")
        );
        assert_eq!(
            network_device_id("kiwisdr", "wss://kiwi.example.org").as_deref(),
            Some("kiwisdr:https://kiwi.example.org")
        );
        assert_eq!(
            network_device_id("rtltcp", "10.0.0.5:1234/").as_deref(),
            Some("rtltcp:10.0.0.5:1234")
        );
        assert_eq!(
            network_device_id("rtltcp", "::1").as_deref(),
            Some("rtltcp:::1")
        );
        for address in ["", "   ", "10.0.0.5 1234", "rtl_tcp://"] {
            assert_eq!(network_device_id("rtltcp", address), None, "{address}");
        }
        assert_eq!(
            network_device_id("rtltcp", "[::1]:1234"),
            Some(device_id(&device("rtltcp", "[::1]:1234", "x")))
        );
    }

    #[test]
    fn every_network_protocol_shows_its_default_port() {
        let ports: Vec<&str> = NETWORK_BACKENDS
            .iter()
            .filter_map(|backend| backend.placeholder.rsplit(':').next())
            .collect();
        assert_eq!(ports, vec!["1234", "5555", "5454", "8073", "30431"]);
        assert_eq!(
            address_title(&NETWORK_BACKENDS[3]),
            "Host or address of the KiwiSDR server; without a port it uses 8073"
        );
    }

    #[test]
    fn a_saved_radio_is_named_by_its_protocol_and_host() {
        assert_eq!(
            network_radio_label("rtltcp:10.0.0.5:1234"),
            "rtl_tcp 10.0.0.5:1234"
        );
        assert_eq!(
            network_radio_label("kiwisdr:https://kiwi.example.org"),
            "KiwiSDR kiwi.example.org"
        );
        assert_eq!(
            network_radio_label("kiwisdr:user@kiwi.local:80"),
            "KiwiSDR kiwi.local:80"
        );
        assert_eq!(network_radio_label("mystery:host:1"), "host:1");
    }

    #[test]
    fn a_dongle_without_a_serial_is_told_how_to_get_one() {
        assert!(device_hint(&device("rtlsdr", "0", "RTL-SDR")).is_some());
        let serialed = DeviceInfo {
            serial: Some("00000001".to_owned()),
            ..device("rtlsdr", "0", "RTL-SDR")
        };
        assert!(device_hint(&serialed).is_none());
        assert!(device_hint(&device("hackrf", "0", "HackRF")).is_none());
    }
}
