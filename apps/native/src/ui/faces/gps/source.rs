use std::net::Ipv6Addr;

use sdrmm_wire::{
    phone::{PHONE_NOT_PAIRED, PHONE_OFFLINE, Phone},
    position::{
        DEFAULT_NMEA_BAUD, DEFAULT_NMEA_UPDATE_INTERVAL_MS, HeadingSource, MAX_NMEA_BAUD,
        MIN_NMEA_BAUD, NmeaDeviceInfo, PositionFix, PositionSource,
    },
};

pub const BAUDS: [u32; 5] = [4_800, 9_600, 38_400, 57_600, 115_200];

#[must_use]
pub const fn tab_hint(tab: GpsTab) -> &'static str {
    match tab {
        GpsTab::Receiver => "A GPS receiver on a serial or USB port, read as NMEA",
        GpsTab::Network => "A gpsd daemon on this machine or another one",
        GpsTab::Fixed => "A place typed in once, for a station that never moves",
        GpsTab::Phone => "A paired phone's position and heading",
    }
}

const fn heading_source_label(source: HeadingSource) -> &'static str {
    match source {
        HeadingSource::Compass => "compass",
        HeadingSource::Course => "GPS course",
        HeadingSource::Fused => "fused",
        HeadingSource::Gnss => "GNSS",
        HeadingSource::Sensor => "sensor",
    }
}

#[must_use]
pub fn heading_label(fix: &PositionFix) -> String {
    let Some(heading) = fix.attitude.heading_deg else {
        return String::from("-");
    };
    let degrees = (heading.round() as i64).rem_euclid(360);
    let source = fix
        .attitude
        .heading_source
        .map_or_else(String::new, |source| {
            format!(" {}", heading_source_label(source))
        });
    let accuracy = fix
        .attitude
        .heading_accuracy_deg
        .map_or_else(String::new, |accuracy| {
            format!(" \u{b1}{accuracy:.0}\u{b0}")
        });
    format!("{degrees:03}\u{b0}{source}{accuracy}")
}

fn angle(value: Option<f64>) -> String {
    value.map_or_else(|| String::from("-"), |value| format!("{value:.0}\u{b0}"))
}

#[must_use]
pub fn tilt_label(fix: &PositionFix) -> Option<String> {
    let attitude = fix.attitude;
    (attitude.pitch_deg.is_some() || attitude.roll_deg.is_some()).then(|| {
        format!(
            "{} / {}",
            angle(attitude.pitch_deg),
            angle(attitude.roll_deg)
        )
    })
}

#[must_use]
pub fn age_label(ms: i64) -> String {
    let seconds = ms.max(0) / 1_000;
    if seconds < 60 {
        return format!("{seconds} s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes} min");
    }
    let hours = minutes / 60;
    if hours < 48 {
        format!("{hours} h")
    } else {
        format!("{} d", hours / 24)
    }
}

#[must_use]
pub fn fix_age_label(time: &str, now_ms: i64) -> String {
    time.parse::<jiff::Timestamp>().map_or_else(
        |_| String::from("-"),
        |at| age_label(now_ms - at.as_millisecond()),
    )
}

#[must_use]
pub fn shown_as_standing(source: Option<&PositionSource>, error: &str) -> bool {
    matches!(source, Some(PositionSource::Phone { .. }))
        && (error == PHONE_OFFLINE || error == PHONE_NOT_PAIRED)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpsTab {
    Receiver,
    Network,
    Fixed,
    Phone,
}

#[must_use]
pub fn gps_tabs() -> Vec<(GpsTab, String)> {
    vec![
        (GpsTab::Receiver, "Receiver".to_owned()),
        (GpsTab::Network, "Network".to_owned()),
        (GpsTab::Fixed, "Fixed".to_owned()),
        (GpsTab::Phone, "Phone".to_owned()),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhoneStanding {
    Online,
    Offline,
    NotPaired,
}

impl PhoneStanding {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Offline => "Offline",
            Self::NotPaired => "Not paired",
        }
    }
}

#[must_use]
pub fn phone_standing(
    phones: Option<&[Phone]>,
    id: &str,
    error: Option<&str>,
) -> Option<PhoneStanding> {
    if error == Some(PHONE_NOT_PAIRED) {
        return Some(PhoneStanding::NotPaired);
    }
    let Some(phones) = phones else {
        return (error == Some(PHONE_OFFLINE)).then_some(PhoneStanding::Offline);
    };
    let Some(phone) = phones.iter().find(|phone| phone.id == id) else {
        return Some(PhoneStanding::NotPaired);
    };
    Some(if phone.online && error != Some(PHONE_OFFLINE) {
        PhoneStanding::Online
    } else {
        PhoneStanding::Offline
    })
}

fn valid_host(host: &str) -> bool {
    if host.starts_with('[') || host.ends_with(']') {
        return host
            .strip_prefix('[')
            .and_then(|inner| inner.strip_suffix(']'))
            .is_some_and(|inner| inner.parse::<Ipv6Addr>().is_ok());
    }
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[must_use]
pub fn valid_gpsd_address(address: &str) -> bool {
    let Some((host, port)) = address.rsplit_once(':') else {
        return false;
    };
    let port_ok = port
        .parse::<u32>()
        .is_ok_and(|port| (1..=65_535).contains(&port));
    !host.is_empty() && port_ok && valid_host(host)
}

#[must_use]
pub fn valid_baud(text: &str) -> Option<u32> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|baud| (MIN_NMEA_BAUD..=MAX_NMEA_BAUD).contains(baud))
}

#[must_use]
pub fn nmea_detail(device: &NmeaDeviceInfo) -> String {
    [
        device.product.as_ref().or(device.manufacturer.as_ref()),
        device.serial.as_ref(),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .cloned()
    .collect::<Vec<_>>()
    .join(" · ")
}

#[must_use]
pub fn nmea_suggestion(device: &NmeaDeviceInfo) -> (String, Option<String>) {
    let Some(description) = device.product.as_ref().or(device.manufacturer.as_ref()) else {
        return (device.path.clone(), None);
    };
    let serial = device
        .serial
        .as_ref()
        .map_or_else(String::new, |serial| format!(" · {serial}"));
    (device.path.clone(), Some(format!("{description}{serial}")))
}

#[must_use]
pub fn filter_nmea_devices(devices: &[NmeaDeviceInfo], query: &str) -> Vec<NmeaDeviceInfo> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return devices.to_vec();
    }
    devices
        .iter()
        .filter(|device| {
            [
                Some(&device.path),
                device.product.as_ref(),
                device.manufacturer.as_ref(),
                device.serial.as_ref(),
            ]
            .into_iter()
            .flatten()
            .any(|part| part.to_lowercase().contains(&needle))
        })
        .cloned()
        .collect()
}

#[must_use]
pub fn nmea_source(path: &str) -> PositionSource {
    PositionSource::Nmea {
        device: path.to_owned(),
        baud: DEFAULT_NMEA_BAUD,
        update_interval_ms: DEFAULT_NMEA_UPDATE_INTERVAL_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(
        path: &str,
        product: Option<&str>,
        manufacturer: Option<&str>,
        serial: Option<&str>,
    ) -> NmeaDeviceInfo {
        NmeaDeviceInfo {
            path: path.to_owned(),
            product: product.map(str::to_owned),
            manufacturer: manufacturer.map(str::to_owned),
            serial: serial.map(str::to_owned),
            usb_vid: None,
            usb_pid: None,
        }
    }

    #[test]
    fn a_phone_is_offered_beside_the_receivers() {
        let values = gps_tabs()
            .into_iter()
            .map(|(tab, _)| tab)
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            vec![
                GpsTab::Receiver,
                GpsTab::Network,
                GpsTab::Fixed,
                GpsTab::Phone
            ]
        );
    }

    fn phone(id: &str, online: bool) -> Phone {
        Phone {
            id: id.to_owned(),
            name: id.to_owned(),
            platform: sdrmm_wire::phone::PhonePlatform::Android,
            created_at: String::new(),
            last_seen: None,
            online,
            gps_nodes: Vec::new(),
        }
    }

    #[test]
    fn a_phone_stands_online_offline_or_unpaired() {
        let phones = [phone("a", true), phone("b", false)];
        assert_eq!(
            phone_standing(Some(&phones), "a", None),
            Some(PhoneStanding::Online)
        );
        assert_eq!(
            phone_standing(Some(&phones), "a", Some(PHONE_OFFLINE)),
            Some(PhoneStanding::Offline)
        );
        assert_eq!(
            phone_standing(Some(&phones), "b", None),
            Some(PhoneStanding::Offline)
        );
        assert_eq!(
            phone_standing(Some(&phones), "gone", None),
            Some(PhoneStanding::NotPaired)
        );
        assert_eq!(
            phone_standing(None, "a", Some(PHONE_NOT_PAIRED)),
            Some(PhoneStanding::NotPaired)
        );
        assert_eq!(phone_standing(None, "a", None), None);
    }

    #[test]
    fn host_and_bracketed_ipv6_endpoints_are_accepted() {
        assert!(valid_gpsd_address("127.0.0.1:2947"));
        assert!(valid_gpsd_address("gps.local:2947"));
        assert!(valid_gpsd_address("[::1]:2947"));
    }

    #[test]
    fn missing_hosts_ports_and_malformed_endpoints_are_refused() {
        for address in [
            "",
            "localhost",
            "localhost:0",
            "bad host:2947",
            "[::1:2947",
            "[::::]:2947",
            "[1:2:3:4:5:6:7:8:9]:2947",
        ] {
            assert!(!valid_gpsd_address(address), "{address}");
        }
    }

    #[test]
    fn a_suggestion_names_the_receiver_behind_the_path() {
        let ublox = device(
            "/dev/cu.usbmodem11401",
            Some("GNSS receiver"),
            Some("u-blox"),
            Some("GPS-1"),
        );
        assert_eq!(
            nmea_suggestion(&ublox),
            (
                "/dev/cu.usbmodem11401".to_owned(),
                Some("GNSS receiver · GPS-1".to_owned())
            )
        );
        assert_eq!(
            nmea_suggestion(&device("/dev/ttyS0", None, None, None)),
            ("/dev/ttyS0".to_owned(), None)
        );
    }

    #[test]
    fn the_receiver_list_names_filters_and_reads_at_the_shipped_rate() {
        let devices = [
            device(
                "/dev/cu.usbmodem11401",
                Some("GNSS receiver"),
                Some("u-blox"),
                None,
            ),
            device("/dev/ttyS0", None, None, None),
        ];
        assert_eq!(nmea_detail(&devices[0]), "GNSS receiver");
        assert_eq!(
            nmea_detail(&NmeaDeviceInfo {
                serial: Some("GPS-1".to_owned()),
                ..devices[0].clone()
            }),
            "GNSS receiver · GPS-1"
        );
        assert_eq!(nmea_detail(&devices[1]), "");
        let paths =
            |found: Vec<NmeaDeviceInfo>| found.into_iter().map(|d| d.path).collect::<Vec<_>>();
        assert_eq!(
            paths(filter_nmea_devices(&devices, " USBMODEM ")),
            vec!["/dev/cu.usbmodem11401"]
        );
        assert_eq!(
            paths(filter_nmea_devices(&devices, "u-blox")),
            vec!["/dev/cu.usbmodem11401"]
        );
        assert_eq!(filter_nmea_devices(&devices, " ").len(), 2);
        assert!(filter_nmea_devices(&devices, "garmin").is_empty());
        assert_eq!(
            nmea_source("/dev/ttyS0"),
            PositionSource::Nmea {
                device: "/dev/ttyS0".to_owned(),
                baud: 9_600,
                update_interval_ms: 1_000
            }
        );
    }

    #[test]
    fn a_baud_is_an_integer_the_port_can_run() {
        assert_eq!(valid_baud("9600"), Some(9_600));
        assert_eq!(valid_baud("300"), None);
        assert_eq!(valid_baud("9600.5"), None);
        assert_eq!(valid_baud("fast"), None);
    }

    fn fix(attitude: serde_json::Value) -> PositionFix {
        let mut held = serde_json::json!({
            "latitude": 48.0, "longitude": 11.0, "time": "2026-09-23T12:00:00Z"
        });
        if let (Some(held), Some(attitude)) = (held.as_object_mut(), attitude.as_object()) {
            held.extend(attitude.clone());
        }
        serde_json::from_value(held).expect("a fix")
    }

    #[test]
    fn heading_reads_degrees_source_and_accuracy() {
        assert_eq!(heading_label(&fix(serde_json::json!({}))), "-");
        let full = fix(serde_json::json!({
            "heading_deg": 7.4, "heading_source": "compass", "heading_accuracy_deg": 12.2
        }));
        assert_eq!(heading_label(&full), "007\u{b0} compass \u{b1}12\u{b0}");
        assert_eq!(
            heading_label(&fix(serde_json::json!({ "heading_deg": -10.0 }))),
            "350\u{b0}"
        );
    }

    #[test]
    fn tilt_shows_only_when_the_phone_reports_it() {
        assert_eq!(tilt_label(&fix(serde_json::json!({}))), None);
        assert_eq!(
            tilt_label(&fix(serde_json::json!({ "pitch_deg": 4.6 }))).as_deref(),
            Some("5\u{b0} / -")
        );
    }

    #[test]
    fn a_fix_ages_in_seconds_minutes_hours_then_days() {
        let at = "2026-09-23T12:00:00Z"
            .parse::<jiff::Timestamp>()
            .map(|at| at.as_millisecond())
            .unwrap_or_default();
        assert_eq!(fix_age_label("2026-09-23T12:00:00Z", at + 42_000), "42 s");
        assert_eq!(fix_age_label("2026-09-23T12:00:00Z", at + 300_000), "5 min");
        assert_eq!(age_label(3 * 3_600_000), "3 h");
        assert_eq!(age_label(72 * 3_600_000), "3 d");
        assert_eq!(age_label(-5), "0 s");
        assert_eq!(fix_age_label("garbage", at), "-");
    }

    #[test]
    fn offline_and_unpaired_phones_are_said_once_in_the_header() {
        let phone = PositionSource::Phone {
            phone: String::from("p"),
        };
        assert!(shown_as_standing(Some(&phone), PHONE_OFFLINE));
        assert!(shown_as_standing(Some(&phone), PHONE_NOT_PAIRED));
        assert!(!shown_as_standing(Some(&phone), "timeout"));
        assert!(!shown_as_standing(None, PHONE_OFFLINE));
        assert!(GpsTab::Phone != GpsTab::Fixed && !tab_hint(GpsTab::Phone).is_empty());
    }
}
