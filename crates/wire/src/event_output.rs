use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::account::{AccountKind, MAX_ACCOUNT_LEN};
use crate::decode::{Capability, EventKind, KindSet};

pub const MAX_MATRIX_ROOM_ID_LEN: usize = 255;
pub const MAX_MQTT_TOPIC_LEN: usize = 512;
pub const MAX_SQL_IDENTIFIER_LEN: usize = 63;
pub const MAX_INFLUX_NAME_LEN: usize = 255;
pub const DEFAULT_POSTGRES_TABLE: &str = "sdrmm_events";
pub const MAX_EVENT_FILE_LEN: usize = 64;
pub const CSV_SUFFIX: &str = ".csv";
pub const TEXT_SUFFIX: &str = ".txt";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebhookFormat {
    #[default]
    Json,
    Discord,
}

/// Where an output sends. A service that signs in names its account and keeps only the place
/// inside it: the room, topic, table or bucket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "service", rename_all = "snake_case")]
pub enum EventOutputTarget {
    Recordings,
    Desktop,
    Csv {
        file: String,
    },
    /// One readable line per event, appended to a `.txt` file.
    Text {
        file: String,
    },
    /// A TCP listener streaming events encoded in a format a decoder registered.
    #[serde(alias = "beast")]
    Feed {
        #[serde(default = "first_feed_format")]
        format: String,
        address: String,
        #[serde(default)]
        enabled: bool,
    },
    Tunnel {
        interface: String,
        #[schema(value_type = String, format = "ipv4")]
        address: std::net::Ipv4Addr,
        prefix: u8,
    },
    /// The format shapes the message and decides whether the node keeps call audio, so it stays
    /// on the node.
    Webhook {
        #[serde(default)]
        account: String,
        #[serde(default)]
        format: WebhookFormat,
    },
    Matrix {
        #[serde(default)]
        account: String,
        room_id: String,
    },
    Mqtt {
        #[serde(default)]
        account: String,
        topic: String,
    },
    Postgres {
        #[serde(default)]
        account: String,
        table: String,
    },
    Influx {
        #[serde(default)]
        account: String,
        bucket: String,
    },
}

/// Workspaces saved before feeds took a format carried only the first one there was.
fn first_feed_format() -> String {
    crate::decode::catalog()
        .feeds()
        .first()
        .map_or_default(|feed| feed.format.clone())
}

impl Default for EventOutputTarget {
    fn default() -> Self {
        Self::Webhook {
            account: String::new(),
            format: WebhookFormat::Json,
        }
    }
}

impl EventOutputTarget {
    /// The account id this output signs in with and the kind that account must be.
    #[must_use]
    pub fn account(&self) -> Option<(&str, AccountKind)> {
        match self {
            Self::Webhook { account, .. } => Some((account, AccountKind::Webhook)),
            Self::Matrix { account, .. } => Some((account, AccountKind::Matrix)),
            Self::Mqtt { account, .. } => Some((account, AccountKind::Mqtt)),
            Self::Postgres { account, .. } => Some((account, AccountKind::Postgres)),
            Self::Influx { account, .. } => Some((account, AccountKind::Influx)),
            Self::Recordings
            | Self::Desktop
            | Self::Csv { .. }
            | Self::Text { .. }
            | Self::Feed { .. }
            | Self::Tunnel { .. } => None,
        }
    }

    pub const fn account_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Webhook { account, .. }
            | Self::Matrix { account, .. }
            | Self::Mqtt { account, .. }
            | Self::Postgres { account, .. }
            | Self::Influx { account, .. } => Some(account),
            Self::Recordings
            | Self::Desktop
            | Self::Csv { .. }
            | Self::Text { .. }
            | Self::Feed { .. }
            | Self::Tunnel { .. } => None,
        }
    }

    #[must_use]
    pub fn accepts(&self) -> KindSet {
        match self {
            Self::Feed { format, .. } => crate::decode::catalog().feed_kinds(format),
            Self::Tunnel { .. } => KindSet::EMPTY.with(EventKind::BROADCAST_DATA),
            Self::Recordings => KindSet::with_cap(Capability::Clip),
            Self::Desktop
            | Self::Csv { .. }
            | Self::Text { .. }
            | Self::Webhook { .. }
            | Self::Matrix { .. }
            | Self::Mqtt { .. }
            | Self::Postgres { .. }
            | Self::Influx { .. } => KindSet::with_cap(Capability::Log),
        }
    }

    /// Whether this service sends a message a person reads, so a layout can shape it.
    #[must_use]
    pub const fn takes_message(&self) -> bool {
        matches!(
            self,
            Self::Desktop
                | Self::Text { .. }
                | Self::Webhook { .. }
                | Self::Matrix { .. }
                | Self::Mqtt { .. }
        )
    }

    #[must_use]
    pub const fn keeps_clips(&self) -> bool {
        matches!(
            self,
            Self::Recordings
                | Self::Matrix { .. }
                | Self::Webhook {
                    format: WebhookFormat::Discord,
                    ..
                }
        )
    }

    #[must_use]
    pub fn configured(&self) -> bool {
        let filled = |value: &String| !value.trim().is_empty();
        match self {
            Self::Recordings | Self::Desktop => true,
            Self::Csv { file } | Self::Text { file } => filled(file),
            Self::Feed {
                address, enabled, ..
            } => *enabled && !address.is_empty(),
            Self::Tunnel { interface, .. } => !interface.is_empty(),
            Self::Webhook { account, .. } => filled(account),
            Self::Matrix {
                account,
                room_id: place,
            }
            | Self::Mqtt {
                account,
                topic: place,
            }
            | Self::Postgres {
                account,
                table: place,
            }
            | Self::Influx {
                account,
                bucket: place,
            } => filled(account) && filled(place),
        }
    }

    #[must_use]
    pub fn valid(&self) -> bool {
        if self
            .account()
            .is_some_and(|(account, _)| account.chars().count() > MAX_ACCOUNT_LEN)
        {
            return false;
        }
        match self {
            Self::Recordings | Self::Desktop | Self::Webhook { .. } => true,
            Self::Csv { file } | Self::Text { file } => file.is_empty() || valid_event_file(file),
            Self::Feed {
                format, address, ..
            } => {
                !format.is_empty()
                    && (address.is_empty()
                        || (address.len() <= crate::MAX_NETWORK_ADDRESS_LEN
                            && address
                                .parse::<std::net::SocketAddr>()
                                .is_ok_and(|address| address.port() != 0)))
            }
            Self::Tunnel {
                interface,
                address,
                prefix,
            } => {
                interface.len() <= 15
                    && interface
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                    && *prefix <= 32
                    && !address.is_unspecified()
                    && !address.is_multicast()
                    && !address.is_broadcast()
            }
            Self::Matrix { room_id, .. } => room_id.len() <= MAX_MATRIX_ROOM_ID_LEN,
            Self::Mqtt { topic, .. } => valid_publish_topic(topic),
            Self::Postgres { table, .. } => table.is_empty() || valid_sql_identifier(table),
            Self::Influx { bucket, .. } => valid_influx_name(bucket),
        }
    }
}

#[must_use]
pub fn valid_sql_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= MAX_SQL_IDENTIFIER_LEN
        && bytes
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first == b'_')
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[must_use]
pub fn valid_event_file(value: &str) -> bool {
    let mut chars = value.chars();
    value.len() <= MAX_EVENT_FILE_LEN
        && chars
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[must_use]
pub fn event_file_name(file: &str, suffix: &str) -> String {
    if file.to_ascii_lowercase().ends_with(suffix) {
        file.to_owned()
    } else {
        format!("{file}{suffix}")
    }
}

pub(crate) fn valid_influx_name(value: &str) -> bool {
    value.len() <= MAX_INFLUX_NAME_LEN && !value.chars().any(char::is_control)
}

fn valid_publish_topic(value: &str) -> bool {
    value.len() <= MAX_MQTT_TOPIC_LEN
        && !value.contains(['+', '#', '\0'])
        && !value.starts_with('/')
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EventOutputNode {
    pub target: EventOutputTarget,
    /// The operator's own layout for outputs that send text. Empty keeps the usual one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

impl EventOutputNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.target.valid() && crate::message::valid_message(&self.message)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FeedStatus {
    pub node: String,
    pub format: String,
    pub address: String,
    pub listening: bool,
    pub clients: u32,
    pub frames: u64,
    pub error: Option<String>,
    /// What the node waits for, never a fault: a wire, a setting or data still to come.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EventOutputStatus {
    pub node: String,
    pub delivered: u64,
    pub failed: u64,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::test_kinds;

    fn feed(address: &str, enabled: bool) -> EventOutputTarget {
        EventOutputTarget::Feed {
            format: test_kinds::FEED.to_owned(),
            address: address.to_owned(),
            enabled,
        }
    }

    #[test]
    fn a_feed_requires_an_explicit_listener_and_enable() {
        for address in ["127.0.0.1:30005", "0.0.0.0:30005", "[::1]:30005"] {
            let target = feed(address, true);
            assert!(target.valid() && target.configured());
            assert_eq!(
                serde_json::from_str::<EventOutputTarget>(&serde_json::to_string(&target).unwrap())
                    .unwrap(),
                target
            );
        }
        for address in ["127.0.0.1:0", "localhost", "::1:30005", "host:30005"] {
            assert!(!feed(address, true).valid());
        }
        assert!(!feed("127.0.0.1:30005", false).configured());
        assert!(
            !EventOutputTarget::Feed {
                format: String::new(),
                address: "127.0.0.1:30005".to_owned(),
                enabled: true,
            }
            .valid()
        );
    }

    #[test]
    fn an_old_beast_entry_loads_as_the_first_feed() {
        test_kinds::install();
        let old: EventOutputTarget = serde_json::from_str(
            r#"{"service":"beast","address":"127.0.0.1:30005","enabled":true}"#,
        )
        .expect("old beast entry");
        assert_eq!(old, feed("127.0.0.1:30005", true));
        assert_eq!(old.accepts(), KindSet::EMPTY.with(test_kinds::STATION));
    }

    #[test]
    fn recordings_and_desktop_need_no_settings() {
        for (target, encoded) in [
            (EventOutputTarget::Recordings, r#"{"service":"recordings"}"#),
            (EventOutputTarget::Desktop, r#"{"service":"desktop"}"#),
        ] {
            assert!(target.valid() && target.configured());
            assert_eq!(serde_json::to_string(&target).expect("encode"), encoded);
            assert_eq!(
                serde_json::from_str::<EventOutputTarget>(encoded).expect("decode"),
                target
            );
        }
    }

    #[test]
    fn a_csv_file_is_a_plain_bounded_name() {
        let csv = |file: &str| EventOutputTarget::Csv {
            file: file.to_owned(),
        };
        for file in ["events", "adsb-2026.csv", "a_b.c"] {
            assert!(csv(file).valid() && csv(file).configured(), "{file}");
        }
        assert!(!csv(" ").configured());
        for file in ["../events", "a/b", ".hidden", "a\\b", "a b", "ä"] {
            assert!(!csv(file).valid(), "{file}");
        }
        assert!(!csv(&"e".repeat(MAX_EVENT_FILE_LEN + 1)).valid());
        let encoded = serde_json::to_string(&csv("events")).expect("encode");
        assert_eq!(encoded, r#"{"service":"csv","file":"events"}"#);
        assert_eq!(event_file_name("events", CSV_SUFFIX), "events.csv");
        assert_eq!(event_file_name("events.CSV", CSV_SUFFIX), "events.CSV");
    }

    #[test]
    fn a_text_file_shares_the_csv_name_rules() {
        let text = |file: &str| EventOutputTarget::Text {
            file: file.to_owned(),
        };
        assert!(text("pager").valid() && text("pager").configured());
        assert!(!text("../pager").valid());
        assert!(!text("").configured());
        let encoded = serde_json::to_string(&text("pager")).expect("encode");
        assert_eq!(encoded, r#"{"service":"text","file":"pager"}"#);
        assert_eq!(event_file_name("pager", TEXT_SUFFIX), "pager.txt");
    }

    #[test]
    fn tunnel_configuration_is_bounded_and_round_trips() {
        let target = EventOutputTarget::Tunnel {
            interface: "utun7".to_owned(),
            address: "10.23.0.1".parse().unwrap(),
            prefix: 24,
        };
        assert!(target.valid() && target.configured());
        assert_eq!(
            serde_json::from_str::<EventOutputTarget>(&serde_json::to_string(&target).unwrap())
                .unwrap(),
            target
        );
        for interface in ["../tun", "tun 0", "1234567890123456"] {
            let target = EventOutputTarget::Tunnel {
                interface: interface.to_owned(),
                address: "10.23.0.1".parse().unwrap(),
                prefix: 24,
            };
            assert!(!target.valid());
        }
    }

    fn signed(service: &str, account: &str, place: &str) -> EventOutputTarget {
        let (account, place) = (account.to_owned(), place.to_owned());
        match service {
            "matrix" => EventOutputTarget::Matrix {
                account,
                room_id: place,
            },
            "mqtt" => EventOutputTarget::Mqtt {
                account,
                topic: place,
            },
            "postgres" => EventOutputTarget::Postgres {
                account,
                table: place,
            },
            _ => EventOutputTarget::Influx {
                account,
                bucket: place,
            },
        }
    }

    #[test]
    fn an_output_that_signs_in_needs_its_account_and_its_place() {
        for (service, place) in [
            ("matrix", "!radio:example"),
            ("mqtt", "sdrmm/events"),
            ("postgres", "sdrmm_events"),
            ("influx", "radio"),
        ] {
            let target = signed(service, "a1", place);
            assert!(target.valid() && target.configured(), "{service}");
            assert!(!signed(service, "", place).configured(), "{service}");
            assert!(!signed(service, " ", place).configured(), "{service}");
            assert!(!signed(service, "a1", " ").configured(), "{service}");
            assert!(
                !signed(service, &"a".repeat(MAX_ACCOUNT_LEN + 1), place).valid(),
                "{service}"
            );
            assert_eq!(
                serde_json::from_str::<EventOutputTarget>(&serde_json::to_string(&target).unwrap())
                    .unwrap(),
                target
            );
        }
        assert!(!EventOutputTarget::default().configured());
        let webhook = EventOutputTarget::Webhook {
            account: "a1".to_owned(),
            format: WebhookFormat::Json,
        };
        assert!(webhook.valid() && webhook.configured());
        assert_eq!(webhook.account(), Some(("a1", AccountKind::Webhook)));
        assert_eq!(
            signed("postgres", "a2", "t").account(),
            Some(("a2", AccountKind::Postgres))
        );
        assert_eq!(EventOutputTarget::Desktop.account(), None);
    }

    #[test]
    fn each_place_keeps_its_own_rules() {
        assert!(!signed("matrix", "a", &"r".repeat(MAX_MATRIX_ROOM_ID_LEN + 1)).valid());
        for topic in ["sdrmm/+/events", "sdrmm/#", "/sdrmm/events"] {
            assert!(!signed("mqtt", "a", topic).valid(), "{topic}");
        }
        assert!(!signed("mqtt", "a", &"t".repeat(MAX_MQTT_TOPIC_LEN + 1)).valid());
        for table in [
            "Events",
            "1events",
            "events;drop",
            "public.events",
            "\"events\"",
        ] {
            assert!(!signed("postgres", "a", table).valid(), "{table}");
        }
        assert!(!signed("postgres", "a", &"t".repeat(MAX_SQL_IDENTIFIER_LEN + 1)).valid());
        assert!(!signed("influx", "a", "ra\ndio").valid());
        assert!(!signed("influx", "a", &"b".repeat(MAX_INFLUX_NAME_LEN + 1)).valid());
    }

    #[test]
    fn the_webhook_format_survives_a_round_trip_and_defaults_to_json() {
        let discord = EventOutputTarget::Webhook {
            account: "a1".to_owned(),
            format: WebhookFormat::Discord,
        };
        let encoded = serde_json::to_string(&discord).expect("encode");
        assert_eq!(
            encoded,
            r#"{"service":"webhook","account":"a1","format":"discord"}"#
        );
        assert_eq!(
            serde_json::from_str::<EventOutputTarget>(&encoded).expect("decode"),
            discord
        );
        assert_eq!(
            serde_json::from_str::<EventOutputTarget>(r#"{"service":"webhook"}"#).expect("decode"),
            EventOutputTarget::default()
        );
    }

    #[test]
    fn an_old_target_with_its_login_inline_loads_without_an_account() {
        let old: EventOutputTarget = serde_json::from_str(
            r#"{"service":"matrix","homeserver_url":"https://m.example","room_id":"!r:m","access_token":"hunter2"}"#,
        )
        .expect("old target");
        assert_eq!(old, signed("matrix", "", "!r:m"));
        assert!(!old.configured());
        let encoded = serde_json::to_string(&old).expect("encode");
        assert!(!encoded.contains("hunter2"), "{encoded}");
    }
}
