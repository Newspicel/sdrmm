use std::net::Ipv4Addr;

use sdrmm_wire::event_output::{EventOutputTarget, WebhookFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Recordings,
    Desktop,
    Csv,
    Beast,
    Webhook,
    Matrix,
    Mqtt,
    Postgres,
    Influx,
    Tunnel,
}

pub const SERVICES: [(Service, &str); 10] = [
    (Service::Recordings, "Recordings"),
    (Service::Desktop, "Notification"),
    (Service::Csv, "CSV file"),
    (Service::Beast, "ADS-B Beast TCP"),
    (Service::Webhook, "Webhook"),
    (Service::Matrix, "Matrix"),
    (Service::Mqtt, "MQTT"),
    (Service::Postgres, "PostgreSQL"),
    (Service::Influx, "InfluxDB"),
    (Service::Tunnel, "Network interface"),
];

#[must_use]
pub fn service_of(target: &EventOutputTarget) -> Service {
    match target {
        EventOutputTarget::Recordings => Service::Recordings,
        EventOutputTarget::Desktop => Service::Desktop,
        EventOutputTarget::Csv { .. } => Service::Csv,
        EventOutputTarget::Beast { .. } => Service::Beast,
        EventOutputTarget::Tunnel { .. } => Service::Tunnel,
        EventOutputTarget::Webhook { .. } => Service::Webhook,
        EventOutputTarget::Matrix { .. } => Service::Matrix,
        EventOutputTarget::Mqtt { .. } => Service::Mqtt,
        EventOutputTarget::Postgres { .. } => Service::Postgres,
        EventOutputTarget::Influx { .. } => Service::Influx,
    }
}

#[must_use]
pub fn new_target(service: Service) -> EventOutputTarget {
    match service {
        Service::Recordings => EventOutputTarget::Recordings,
        Service::Desktop => EventOutputTarget::Desktop,
        Service::Csv => EventOutputTarget::Csv {
            file: "events".to_owned(),
        },
        Service::Beast => EventOutputTarget::Beast {
            address: "127.0.0.1:30005".to_owned(),
            enabled: false,
        },
        Service::Tunnel => EventOutputTarget::Tunnel {
            interface: String::new(),
            address: Ipv4Addr::new(10, 23, 0, 1),
            prefix: 24,
        },
        Service::Webhook => EventOutputTarget::Webhook {
            url: String::new(),
            format: WebhookFormat::Json,
        },
        Service::Matrix => EventOutputTarget::Matrix {
            homeserver_url: String::new(),
            room_id: String::new(),
            access_token: String::new(),
        },
        Service::Mqtt => EventOutputTarget::Mqtt {
            broker_url: String::new(),
            topic: String::new(),
            username: String::new(),
            password: String::new(),
        },
        Service::Postgres => EventOutputTarget::Postgres {
            url: String::new(),
            table: "sdrmm_events".to_owned(),
            username: String::new(),
            password: String::new(),
        },
        Service::Influx => EventOutputTarget::Influx {
            url: String::new(),
            bucket: String::new(),
            org: String::new(),
            token: String::new(),
        },
    }
}

#[must_use]
pub fn empty_hint(inputs: usize, target: &EventOutputTarget) -> &'static str {
    let configured = target.configured();
    if inputs == 0 {
        return "Wire events in";
    }
    match target {
        EventOutputTarget::Recordings => "One WAV per call in Recordings",
        EventOutputTarget::Desktop => "One notification per event",
        EventOutputTarget::Csv { .. } if configured => "One row per event in Recordings",
        EventOutputTarget::Csv { .. } => "Enter a file name",
        EventOutputTarget::Tunnel { .. } if configured => "Received IPv4 and IPv6 datagrams",
        EventOutputTarget::Tunnel { .. } => "Enter the interface name",
        _ if !configured => "Enter the destination credentials",
        EventOutputTarget::Postgres { .. } => "One row per event",
        EventOutputTarget::Influx { .. } => "One point per event",
        EventOutputTarget::Matrix { .. }
        | EventOutputTarget::Webhook {
            format: WebhookFormat::Discord,
            ..
        } => "One send per event, with available audio",
        _ => "One send per event, as one JSON object",
    }
}

#[must_use]
pub fn services_offered(notify: bool, current: Option<Service>) -> Vec<(Service, &'static str)> {
    SERVICES
        .into_iter()
        .filter(|(service, _)| {
            *service != Service::Desktop || notify || current == Some(Service::Desktop)
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BeastState {
    pub listening: bool,
    pub clients: u32,
    pub frames: u64,
    pub error: Option<String>,
}

#[must_use]
pub fn beast_line(connected: bool, enabled: bool, status: Option<&BeastState>) -> String {
    match status {
        Some(status) if status.listening => {
            format!("{} clients · {} frames", status.clients, status.frames)
        }
        Some(status) if status.error.is_some() => "Server failed".to_owned(),
        _ if !connected => "Wire ADS-B events in".to_owned(),
        _ if enabled => "Opening server".to_owned(),
        _ => "Server closed".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_service_makes_a_target_of_its_own_kind() {
        for (service, _) in SERVICES {
            assert_eq!(service_of(&new_target(service)), service);
        }
    }

    #[test]
    fn notifications_are_offered_only_where_the_shell_shows_them() {
        let offered = |notify, current| -> Vec<Service> {
            services_offered(notify, current)
                .into_iter()
                .map(|(service, _)| service)
                .collect()
        };
        assert!(!offered(false, None).contains(&Service::Desktop));
        assert!(offered(true, None).contains(&Service::Desktop));
        assert!(offered(false, Some(Service::Desktop)).contains(&Service::Desktop));
        assert_eq!(offered(true, None).len(), SERVICES.len());
    }

    #[test]
    fn a_csv_output_asks_for_a_file_name() {
        let named = new_target(Service::Csv);
        assert_eq!(empty_hint(1, &named), "One row per event in Recordings");
        let blank = EventOutputTarget::Csv {
            file: " ".to_owned(),
        };
        assert_eq!(empty_hint(1, &blank), "Enter a file name");
        assert_eq!(
            empty_hint(1, &EventOutputTarget::Recordings),
            "One WAV per call in Recordings"
        );
    }

    fn matrix(access_token: &str) -> bool {
        EventOutputTarget::Matrix {
            homeserver_url: "https://matrix.example".to_owned(),
            room_id: "!radio:matrix.example".to_owned(),
            access_token: access_token.to_owned(),
        }
        .configured()
    }

    fn mqtt(broker_url: &str, topic: &str) -> bool {
        EventOutputTarget::Mqtt {
            broker_url: broker_url.to_owned(),
            topic: topic.to_owned(),
            username: String::new(),
            password: String::new(),
        }
        .configured()
    }

    #[test]
    fn only_services_without_settings_start_configured() {
        for (service, _) in SERVICES {
            let target = new_target(service);
            assert_eq!(service_of(&target), service);
            let bare = matches!(
                service,
                Service::Recordings | Service::Desktop | Service::Csv
            );
            assert_eq!(target.configured(), bare, "{service:?}");
        }
    }

    #[test]
    fn beast_opens_only_with_an_address_and_an_explicit_enable() {
        let beast = |address: &str, enabled| EventOutputTarget::Beast {
            address: address.to_owned(),
            enabled,
        };
        assert!(!beast("127.0.0.1:30005", false).configured());
        assert!(!beast("", true).configured());
        assert!(beast("127.0.0.1:30005", true).configured());
    }

    #[test]
    fn matrix_and_mqtt_need_their_credentials() {
        assert!(!matrix(""));
        assert!(!matrix("   "));
        assert!(matrix("secret"));
        assert!(!mqtt("", "sdrmm/events"));
        assert!(!mqtt("mqtts://broker.example", "  "));
        assert!(mqtt("mqtts://broker.example", "sdrmm/events"));
    }

    #[test]
    fn the_hint_follows_wiring_then_configuration() {
        let webhook = new_target(Service::Webhook);
        assert_eq!(empty_hint(0, &webhook), "Wire events in");
        assert_eq!(empty_hint(1, &webhook), "Enter the destination credentials");
        let discord = EventOutputTarget::Webhook {
            url: "https://discord.com/api/webhooks/1/token".to_owned(),
            format: WebhookFormat::Discord,
        };
        assert_eq!(
            empty_hint(1, &discord),
            "One send per event, with available audio"
        );
        assert_eq!(
            empty_hint(1, &new_target(Service::Tunnel)),
            "Enter the interface name"
        );
    }

    #[test]
    fn the_beast_line_reads_the_server_state() {
        let listening = BeastState {
            listening: true,
            clients: 2,
            frames: 1200,
            error: None,
        };
        assert_eq!(
            beast_line(true, true, Some(&listening)),
            "2 clients · 1200 frames"
        );
        let failed = BeastState {
            error: Some("in use".to_owned()),
            ..BeastState::default()
        };
        assert_eq!(beast_line(true, true, Some(&failed)), "Server failed");
        assert_eq!(beast_line(false, false, None), "Wire ADS-B events in");
        assert_eq!(beast_line(true, true, None), "Opening server");
        assert_eq!(beast_line(true, false, None), "Server closed");
    }
}
