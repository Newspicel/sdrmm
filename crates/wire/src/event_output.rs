use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::decode::{Capability, EventKind, KindSet};

pub const MAX_OUTPUT_URL_LEN: usize = 2_048;
pub const MAX_MATRIX_ROOM_ID_LEN: usize = 255;
pub const MAX_OUTPUT_SECRET_LEN: usize = 4_096;
pub const MAX_MQTT_TOPIC_LEN: usize = 512;
pub const MAX_MQTT_USERNAME_LEN: usize = 255;
pub const MAX_SQL_IDENTIFIER_LEN: usize = 63;
pub const MAX_INFLUX_NAME_LEN: usize = 255;
pub const DEFAULT_POSTGRES_TABLE: &str = "sdrmm_events";
pub const MAX_CSV_FILE_LEN: usize = 64;
pub const CSV_SUFFIX: &str = ".csv";
pub const MAX_SECRET_ID_LEN: usize = 64;

/// A credential an output needs. The server keeps the value in its own store and a workspace
/// names it by id, so an export never carries it and another server asks for it again.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum Secret {
    Stored {
        secret: String,
    },
    /// A value written in place, as older workspaces held it. Saving the workspace moves it
    /// into the server's store.
    Inline(String),
}

impl Default for Secret {
    fn default() -> Self {
        Self::Inline(String::new())
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stored { secret } => formatter
                .debug_struct("Stored")
                .field("secret", secret)
                .finish(),
            Self::Inline(value) if value.is_empty() => formatter.write_str("Unset"),
            Self::Inline(_) => formatter.write_str("Inline([redacted])"),
        }
    }
}

impl Secret {
    #[must_use]
    pub fn stored(id: impl Into<String>) -> Self {
        Self::Stored { secret: id.into() }
    }

    #[must_use]
    pub fn is_set(&self) -> bool {
        match self {
            Self::Stored { secret } => !secret.is_empty(),
            Self::Inline(value) => !value.trim().is_empty(),
        }
    }

    /// A value still written in place, waiting to move into the server's store.
    #[must_use]
    pub fn inline(&self) -> Option<&str> {
        match self {
            Self::Inline(value) if !value.is_empty() => Some(value),
            Self::Stored { .. } | Self::Inline(_) => None,
        }
    }

    #[must_use]
    pub fn valid(&self) -> bool {
        match self {
            Self::Stored { secret } => valid_secret_id(secret),
            Self::Inline(value) => value.len() <= MAX_OUTPUT_SECRET_LEN,
        }
    }
}

#[must_use]
pub fn valid_secret_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_SECRET_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// A credential to keep in the server's store.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SecretValue {
    pub value: String,
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretValue([redacted])")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebhookFormat {
    #[default]
    Json,
    Discord,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "service", rename_all = "snake_case")]
pub enum EventOutputTarget {
    Recordings,
    Desktop,
    Csv {
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
    Webhook {
        url: Secret,
        #[serde(default)]
        format: WebhookFormat,
    },
    Matrix {
        homeserver_url: String,
        room_id: String,
        access_token: Secret,
    },
    Mqtt {
        broker_url: String,
        topic: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: Secret,
    },
    Postgres {
        url: String,
        table: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: Secret,
    },
    Influx {
        url: String,
        bucket: String,
        #[serde(default)]
        org: String,
        #[serde(default)]
        token: Secret,
    },
}

impl std::fmt::Debug for EventOutputTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Recordings => formatter.write_str("Recordings"),
            Self::Desktop => formatter.write_str("Desktop"),
            Self::Csv { file } => formatter.debug_struct("Csv").field("file", file).finish(),
            Self::Feed {
                format, address, ..
            } => formatter
                .debug_struct("Feed")
                .field("format", format)
                .field("address", address)
                .finish(),
            Self::Tunnel {
                interface,
                address,
                prefix,
            } => formatter
                .debug_struct("Tunnel")
                .field("interface", interface)
                .field("address", address)
                .field("prefix", prefix)
                .finish(),
            Self::Webhook { format, .. } => formatter
                .debug_struct("Webhook")
                .field("url", &"[redacted]")
                .field("format", format)
                .finish(),
            Self::Matrix {
                homeserver_url,
                room_id,
                ..
            } => formatter
                .debug_struct("Matrix")
                .field("homeserver_url", &redacted_if_credentialed(homeserver_url))
                .field("room_id", room_id)
                .field("access_token", &"[redacted]")
                .finish(),
            Self::Mqtt {
                broker_url,
                topic,
                username,
                ..
            } => formatter
                .debug_struct("Mqtt")
                .field("broker_url", &redacted_if_credentialed(broker_url))
                .field("topic", topic)
                .field("username", username)
                .field("password", &"[redacted]")
                .finish(),
            Self::Postgres {
                url,
                table,
                username,
                ..
            } => formatter
                .debug_struct("Postgres")
                .field("url", &redacted_if_credentialed(url))
                .field("table", table)
                .field("username", username)
                .field("password", &"[redacted]")
                .finish(),
            Self::Influx {
                url, bucket, org, ..
            } => formatter
                .debug_struct("Influx")
                .field("url", &redacted_if_credentialed(url))
                .field("bucket", bucket)
                .field("org", org)
                .field("token", &"[redacted]")
                .finish(),
        }
    }
}

/// Workspaces saved before feeds took a format carried only the first one there was.
fn first_feed_format() -> String {
    crate::decode::catalog()
        .feeds()
        .first()
        .map(|feed| feed.format.clone())
        .unwrap_or_default()
}

impl Default for EventOutputTarget {
    fn default() -> Self {
        Self::Webhook {
            url: Secret::default(),
            format: WebhookFormat::Json,
        }
    }
}

impl EventOutputTarget {
    /// The one credential this service takes, with the name an error gives it.
    #[must_use]
    pub const fn secret(&self) -> Option<(&'static str, &Secret)> {
        match self {
            Self::Webhook { url, .. } => Some(("Webhook URL", url)),
            Self::Matrix { access_token, .. } => Some(("Matrix token", access_token)),
            Self::Mqtt { password, .. } => Some(("MQTT password", password)),
            Self::Postgres { password, .. } => Some(("PostgreSQL password", password)),
            Self::Influx { token, .. } => Some(("InfluxDB token", token)),
            Self::Recordings
            | Self::Desktop
            | Self::Csv { .. }
            | Self::Feed { .. }
            | Self::Tunnel { .. } => None,
        }
    }

    pub const fn secret_mut(&mut self) -> Option<&mut Secret> {
        match self {
            Self::Webhook { url: secret, .. }
            | Self::Matrix {
                access_token: secret,
                ..
            }
            | Self::Mqtt {
                password: secret, ..
            }
            | Self::Postgres {
                password: secret, ..
            }
            | Self::Influx { token: secret, .. } => Some(secret),
            Self::Recordings
            | Self::Desktop
            | Self::Csv { .. }
            | Self::Feed { .. }
            | Self::Tunnel { .. } => None,
        }
    }

    /// Whether `value` would pass as this service's credential if written in place. A stored
    /// value never went through [`Self::valid`], so a delivery checks it here.
    #[must_use]
    pub fn accepts_secret(&self, value: &str) -> bool {
        value.len() <= MAX_OUTPUT_SECRET_LEN
            && (!matches!(self, Self::Webhook { .. }) || valid_https_url(value))
    }

    #[must_use]
    pub fn accepts(&self) -> KindSet {
        match self {
            Self::Feed { format, .. } => crate::decode::catalog().feed_kinds(format),
            Self::Tunnel { .. } => KindSet::EMPTY.with(EventKind::BROADCAST_DATA),
            Self::Recordings => KindSet::with_cap(Capability::Clip),
            Self::Desktop
            | Self::Csv { .. }
            | Self::Webhook { .. }
            | Self::Matrix { .. }
            | Self::Mqtt { .. }
            | Self::Postgres { .. }
            | Self::Influx { .. } => KindSet::with_cap(Capability::Log),
        }
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
        match self {
            Self::Recordings | Self::Desktop => true,
            Self::Csv { file } => !file.trim().is_empty(),
            Self::Feed {
                address, enabled, ..
            } => *enabled && !address.is_empty(),
            Self::Tunnel { interface, .. } => !interface.is_empty(),
            Self::Webhook { url, .. } => url.is_set(),
            Self::Matrix {
                homeserver_url,
                room_id,
                access_token,
            } => {
                access_token.is_set()
                    && [homeserver_url, room_id]
                        .into_iter()
                        .all(|value| !value.trim().is_empty())
            }
            Self::Mqtt {
                broker_url, topic, ..
            } => [broker_url, topic]
                .into_iter()
                .all(|value| !value.trim().is_empty()),
            Self::Postgres {
                url,
                table,
                username,
                ..
            } => [url, table, username]
                .into_iter()
                .all(|value| !value.trim().is_empty()),
            Self::Influx { url, bucket, .. } => [url, bucket]
                .into_iter()
                .all(|value| !value.trim().is_empty()),
        }
    }

    #[must_use]
    pub fn valid(&self) -> bool {
        match self {
            Self::Recordings | Self::Desktop => true,
            Self::Csv { file } => file.is_empty() || valid_csv_file(file),
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
            Self::Webhook { url, .. } => url.valid() && url.inline().is_none_or(valid_https_url),
            Self::Matrix {
                homeserver_url,
                room_id,
                access_token,
            } => {
                valid_https_url(homeserver_url)
                    && room_id.len() <= MAX_MATRIX_ROOM_ID_LEN
                    && access_token.valid()
            }
            Self::Mqtt {
                broker_url,
                topic,
                username,
                password,
            } => {
                valid_broker_url(broker_url)
                    && valid_publish_topic(topic)
                    && username.len() <= MAX_MQTT_USERNAME_LEN
                    && password.valid()
            }
            Self::Postgres {
                url,
                table,
                username,
                password,
            } => {
                valid_postgres_url(url)
                    && (table.is_empty() || valid_sql_identifier(table))
                    && username.len() <= MAX_SQL_IDENTIFIER_LEN
                    && password.valid()
            }
            Self::Influx {
                url,
                bucket,
                org,
                token,
            } => {
                valid_influx_url(url)
                    && valid_influx_name(bucket)
                    && valid_influx_name(org)
                    && token.valid()
            }
        }
    }
}

fn valid_https_url(value: &str) -> bool {
    value.is_empty() || valid_url(value, &["https"])
}

fn valid_broker_url(value: &str) -> bool {
    value.is_empty() || valid_url(value, &["mqtt", "mqtts"])
}

fn valid_postgres_url(value: &str) -> bool {
    value.is_empty() || valid_url(value, &["postgres", "postgresql"])
}

fn valid_influx_url(value: &str) -> bool {
    value.is_empty() || valid_url(value, &["http", "https"])
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
pub fn valid_csv_file(value: &str) -> bool {
    let mut chars = value.chars();
    value.len() <= MAX_CSV_FILE_LEN
        && chars
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[must_use]
pub fn csv_file_name(file: &str) -> String {
    if file.to_ascii_lowercase().ends_with(CSV_SUFFIX) {
        file.to_owned()
    } else {
        format!("{file}{CSV_SUFFIX}")
    }
}

fn valid_influx_name(value: &str) -> bool {
    value.len() <= MAX_INFLUX_NAME_LEN && !value.chars().any(char::is_control)
}

fn valid_url(value: &str, schemes: &[&str]) -> bool {
    value.len() <= MAX_OUTPUT_URL_LEN
        && url::Url::parse(value).is_ok_and(|url| {
            schemes.contains(&url.scheme())
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

fn valid_publish_topic(value: &str) -> bool {
    value.len() <= MAX_MQTT_TOPIC_LEN
        && !value.contains(['+', '#', '\0'])
        && !value.starts_with('/')
}

fn redacted_if_credentialed(value: &str) -> &str {
    if url::Url::parse(value)
        .is_ok_and(|url| !url.username().is_empty() || url.password().is_some())
    {
        "[redacted]"
    } else {
        value
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EventOutputNode {
    pub target: EventOutputTarget,
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

    fn matrix(homeserver_url: &str, room_id: &str, access_token: &str) -> EventOutputTarget {
        EventOutputTarget::Matrix {
            homeserver_url: homeserver_url.to_owned(),
            room_id: room_id.to_owned(),
            access_token: Secret::Inline(access_token.to_owned()),
        }
    }

    fn webhook(url: &str) -> EventOutputTarget {
        EventOutputTarget::Webhook {
            url: Secret::Inline(url.to_owned()),
            format: WebhookFormat::Json,
        }
    }

    fn mqtt(broker_url: &str, topic: &str) -> EventOutputTarget {
        EventOutputTarget::Mqtt {
            broker_url: broker_url.to_owned(),
            topic: topic.to_owned(),
            username: String::new(),
            password: Secret::default(),
        }
    }

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
        assert!(!csv(&"e".repeat(MAX_CSV_FILE_LEN + 1)).valid());
        let encoded = serde_json::to_string(&csv("events")).expect("encode");
        assert_eq!(encoded, r#"{"service":"csv","file":"events"}"#);
        assert_eq!(csv_file_name("events"), "events.csv");
        assert_eq!(csv_file_name("events.CSV"), "events.CSV");
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

    #[test]
    fn matrix_requires_every_credential_before_delivery() {
        for target in [
            matrix("", "!radio:example", "secret"),
            matrix("https://matrix.example", "", "secret"),
            matrix("https://matrix.example", "!radio:example", ""),
        ] {
            assert!(!target.configured());
        }
        for target in [
            matrix(
                "https://matrix.example",
                &"r".repeat(MAX_MATRIX_ROOM_ID_LEN + 1),
                "secret",
            ),
            matrix(
                "https://matrix.example",
                "!radio:example",
                &"t".repeat(MAX_OUTPUT_SECRET_LEN + 1),
            ),
        ] {
            assert!(!target.valid());
        }
        let target = matrix("https://matrix.example", "!radio:example", "secret");
        assert!(target.configured() && target.valid());
        assert!(
            !matrix(
                "https://matrix-user:matrix-password@matrix.example",
                "!radio:example",
                "secret",
            )
            .valid()
        );
    }

    #[test]
    fn a_webhook_takes_any_bounded_https_endpoint() {
        assert!(EventOutputTarget::default().valid());
        assert!(!EventOutputTarget::default().configured());
        for url in [
            "https://discord.com/api/webhooks/1/token",
            "https://discord.com/api/v10/webhooks/1/token",
            "https://hooks.example.org/services/abc",
        ] {
            assert!(webhook(url).valid(), "{url}");
            assert!(webhook(url).configured(), "{url}");
        }
        for url in [
            "ftp://example.org/hook",
            "http://example.org/hook",
            "https://",
            "https://user:password@example.org/hook",
        ] {
            assert!(!webhook(url).valid(), "{url}");
        }
        assert!(
            !webhook(&format!(
                "https://example.org/{}",
                "x".repeat(MAX_OUTPUT_URL_LEN)
            ))
            .valid()
        );
    }

    #[test]
    fn the_webhook_format_survives_a_round_trip_and_defaults_to_json() {
        let discord = EventOutputTarget::Webhook {
            url: Secret::Inline("https://discord.com/api/webhooks/1/token".to_owned()),
            format: WebhookFormat::Discord,
        };
        let encoded = serde_json::to_string(&discord).expect("encode");
        assert!(encoded.contains(r#""service":"webhook""#));
        assert!(encoded.contains(r#""format":"discord""#));
        assert_eq!(
            serde_json::from_str::<EventOutputTarget>(&encoded).expect("decode"),
            discord
        );
        assert_eq!(
            serde_json::from_str::<EventOutputTarget>(r#"{"service":"webhook","url":""}"#)
                .expect("decode"),
            EventOutputTarget::default()
        );
    }

    #[test]
    fn mqtt_needs_a_broker_and_a_publishable_topic() {
        assert!(!mqtt("", "sdrmm/events").configured());
        assert!(!mqtt("mqtts://broker.example", "").configured());
        let target = mqtt("mqtts://broker.example:8883", "sdrmm/events");
        assert!(target.configured() && target.valid());
        assert!(mqtt("mqtt://127.0.0.1:1883", "sdrmm/events").valid());
        for broker_url in [
            "https://broker.example",
            "mqtt://user:password@broker.example",
            "mqtts://",
        ] {
            assert!(!mqtt(broker_url, "sdrmm/events").valid(), "{broker_url}");
        }
        for topic in ["sdrmm/+/events", "sdrmm/#", "/sdrmm/events"] {
            assert!(!mqtt("mqtts://broker.example", topic).valid(), "{topic}");
        }
        assert!(
            !mqtt(
                "mqtts://broker.example",
                &"t".repeat(MAX_MQTT_TOPIC_LEN + 1),
            )
            .valid()
        );
        assert!(
            !EventOutputTarget::Mqtt {
                broker_url: "mqtts://broker.example".to_owned(),
                topic: "sdrmm/events".to_owned(),
                username: "radio".to_owned(),
                password: Secret::Inline("p".repeat(MAX_OUTPUT_SECRET_LEN + 1)),
            }
            .valid()
        );
    }

    fn postgres(url: &str, table: &str, username: &str) -> EventOutputTarget {
        EventOutputTarget::Postgres {
            url: url.to_owned(),
            table: table.to_owned(),
            username: username.to_owned(),
            password: Secret::default(),
        }
    }

    fn influx(url: &str, bucket: &str) -> EventOutputTarget {
        EventOutputTarget::Influx {
            url: url.to_owned(),
            bucket: bucket.to_owned(),
            org: String::new(),
            token: Secret::default(),
        }
    }

    #[test]
    fn postgres_needs_a_server_a_plain_table_and_a_user() {
        let target = postgres("postgres://db.example:5432/radio", "sdrmm_events", "radio");
        assert!(target.valid() && target.configured());
        assert!(postgres("postgresql://127.0.0.1/radio?sslmode=require", "t", "radio").valid());
        assert!(!postgres("", "sdrmm_events", "radio").configured());
        assert!(!postgres("postgres://db.example/radio", "", "radio").configured());
        assert!(!postgres("postgres://db.example/radio", "sdrmm_events", "").configured());
        for url in [
            "https://db.example/radio",
            "postgres://radio:secret@db.example/radio",
            "postgres://",
        ] {
            assert!(!postgres(url, "sdrmm_events", "radio").valid(), "{url}");
        }
        for table in [
            "Events",
            "1events",
            "events;drop",
            "public.events",
            "\"events\"",
        ] {
            assert!(
                !postgres("postgres://db.example/radio", table, "radio").valid(),
                "{table}"
            );
        }
        assert!(
            !postgres(
                "postgres://db.example/radio",
                &"t".repeat(MAX_SQL_IDENTIFIER_LEN + 1),
                "radio"
            )
            .valid()
        );
        assert_eq!(
            serde_json::from_str::<EventOutputTarget>(&serde_json::to_string(&target).unwrap())
                .unwrap(),
            target
        );
    }

    #[test]
    fn influx_needs_a_server_and_a_bucket() {
        let target = influx("http://127.0.0.1:8086", "radio");
        assert!(target.valid() && target.configured());
        assert!(influx("https://influx.example", "radio").valid());
        assert!(!influx("", "radio").configured());
        assert!(!influx("http://127.0.0.1:8086", "").configured());
        for url in [
            "influx://127.0.0.1",
            "https://user:secret@influx.example",
            "https://",
        ] {
            assert!(!influx(url, "radio").valid(), "{url}");
        }
        assert!(!influx("https://influx.example", "ra\ndio").valid());
        assert!(
            !influx(
                "https://influx.example",
                &"b".repeat(MAX_INFLUX_NAME_LEN + 1)
            )
            .valid()
        );
        assert!(
            !EventOutputTarget::Influx {
                url: "https://influx.example".to_owned(),
                bucket: "radio".to_owned(),
                org: String::new(),
                token: Secret::Inline("t".repeat(MAX_OUTPUT_SECRET_LEN + 1)),
            }
            .valid()
        );
    }

    #[test]
    fn debug_output_redacts_credentials() {
        let webhook_secret = "webhook-secret";
        let target = webhook(&format!("https://hooks.example/{webhook_secret}"));
        let debug = format!("{target:?}");
        assert!(debug.contains("Webhook"));
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains(webhook_secret));

        let matrix_secret = "matrix-secret";
        let target = matrix(
            "https://matrix.example",
            "!radio:matrix.example",
            matrix_secret,
        );
        let debug = format!("{target:?}");
        assert!(debug.contains("Matrix"));
        assert!(debug.contains("https://matrix.example"));
        assert!(debug.contains("!radio:matrix.example"));
        assert!(!debug.contains(matrix_secret));

        let user = "matrix-user";
        let password = "matrix-password";
        let target = matrix(
            &format!("https://{user}:{password}@matrix.example"),
            "!radio:matrix.example",
            "matrix-secret",
        );
        let debug = format!("{target:?}");
        assert!(!debug.contains(user));
        assert!(!debug.contains(password));
        assert!(debug.contains("homeserver_url: \"[redacted]\""));

        let broker_secret = "broker-secret";
        let target = EventOutputTarget::Mqtt {
            broker_url: "mqtts://broker.example".to_owned(),
            topic: "sdrmm/events".to_owned(),
            username: "radio".to_owned(),
            password: Secret::Inline(broker_secret.to_owned()),
        };
        let debug = format!("{target:?}");
        assert!(debug.contains("mqtts://broker.example"));
        assert!(debug.contains("sdrmm/events"));
        assert!(!debug.contains(broker_secret));

        let database_secret = "database-secret";
        let target = EventOutputTarget::Postgres {
            url: "postgres://db.example/radio".to_owned(),
            table: "sdrmm_events".to_owned(),
            username: "radio".to_owned(),
            password: Secret::Inline(database_secret.to_owned()),
        };
        let debug = format!("{target:?}");
        assert!(debug.contains("postgres://db.example/radio"));
        assert!(!debug.contains(database_secret));

        let token = "influx-token";
        let target = EventOutputTarget::Influx {
            url: "https://influx.example".to_owned(),
            bucket: "radio".to_owned(),
            org: "home".to_owned(),
            token: Secret::Inline(token.to_owned()),
        };
        let debug = format!("{target:?}");
        assert!(debug.contains("radio"));
        assert!(!debug.contains(token));
    }

    #[test]
    fn a_secret_reads_an_old_inline_value_and_writes_only_its_id() {
        let old: EventOutputTarget = serde_json::from_str(
            r#"{"service":"matrix","homeserver_url":"https://m.example","room_id":"!r:m","access_token":"hunter2"}"#,
        )
        .expect("old target");
        assert_eq!(
            old.secret(),
            Some(("Matrix token", &Secret::Inline("hunter2".to_owned())))
        );

        let mut stored = old.clone();
        if let Some(secret) = stored.secret_mut() {
            *secret = Secret::stored("abc_1");
        }
        let encoded = serde_json::to_string(&stored).expect("encode");
        assert!(
            encoded.contains(r#""access_token":{"secret":"abc_1"}"#),
            "{encoded}"
        );
        assert!(!encoded.contains("hunter2"));
        assert_eq!(
            serde_json::from_str::<EventOutputTarget>(&encoded).expect("decode"),
            stored
        );
        assert!(stored.valid() && stored.configured());
        assert_eq!(
            format!("{:?}", Secret::Inline("tok".to_owned())),
            "Inline([redacted])"
        );
    }

    #[test]
    fn a_stored_secret_needs_a_plain_bounded_id() {
        assert!(Secret::stored("a-B_9").valid());
        for id in ["", "a/b", "a b", &"x".repeat(MAX_SECRET_ID_LEN + 1)] {
            assert!(!Secret::stored(id).valid(), "{id}");
        }
        assert!(!Secret::stored("").is_set());
        assert!(!Secret::default().is_set());
        let webhook = EventOutputTarget::Webhook {
            url: Secret::stored("hook"),
            format: WebhookFormat::Json,
        };
        assert!(webhook.valid() && webhook.configured());
        assert!(!webhook.accepts_secret("http://plain.example/hook"));
        assert!(webhook.accepts_secret("https://hooks.example/abc"));
        assert!(
            !EventOutputTarget::Mqtt {
                broker_url: String::new(),
                topic: String::new(),
                username: String::new(),
                password: Secret::default(),
            }
            .accepts_secret(&"p".repeat(MAX_OUTPUT_SECRET_LEN + 1))
        );
    }
}
