use std::time::Duration;

use sdrmm_wire::phone::{
    MdnsState, OfferState, PAIR_CODE_DIGITS, PHONE_PORT_IN_USE, PairingOffer, PairingOfferStatus,
    Phone, PhoneAccessStatus, PhoneListenerState, PhonePlatform, PhonesResponse,
};

pub const OFFER_POLL: Duration = Duration::from_secs(2);
pub const OFFER_LINGER_MS: i64 = 10 * 60_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfferView {
    Live {
        code: String,
        uri: Option<String>,
        remaining_ms: i64,
    },
    Burned,
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Danger,
    Dim,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    pub tone: Tone,
    pub title: Option<String>,
}

impl StatusLine {
    fn new(text: &str, tone: Tone, title: Option<String>) -> Self {
        Self {
            text: text.to_owned(),
            tone,
            title,
        }
    }
}

#[must_use]
pub const fn platform_label(platform: PhonePlatform) -> &'static str {
    match platform {
        PhonePlatform::Ios => "iPhone",
        PhonePlatform::Android => "Android",
    }
}

#[must_use]
pub fn format_pairing_code(code: &str) -> String {
    let digits = code.len() == PAIR_CODE_DIGITS && code.bytes().all(|c| c.is_ascii_digit());
    if digits {
        let (head, tail) = code.split_at(PAIR_CODE_DIGITS / 2);
        format!("{head} {tail}")
    } else {
        code.to_owned()
    }
}

#[must_use]
pub fn millis_of(at: &str) -> Option<i64> {
    at.parse::<jiff::Timestamp>()
        .ok()
        .map(jiff::Timestamp::as_millisecond)
}

#[must_use]
pub fn pairing_remaining_ms(expires_at: &str, now_ms: i64) -> i64 {
    millis_of(expires_at).map_or(0, |at| (at - now_ms).max(0))
}

#[must_use]
pub fn countdown_label(ms: i64) -> String {
    let seconds = (ms.max(0) + 999) / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
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
pub fn seen_label(phone: &Phone, now_ms: i64) -> String {
    if phone.online {
        return String::from("Online");
    }
    match phone.last_seen.as_deref().and_then(millis_of) {
        Some(at) => format!("Seen {} ago", age_label(now_ms - at)),
        None => String::from("Never"),
    }
}

#[must_use]
pub fn offer_view(offer: Option<&PairingOfferStatus>, now_ms: i64) -> Option<OfferView> {
    let offer = offer?;
    let recent = millis_of(&offer.expires_at).is_some_and(|at| now_ms - at < OFFER_LINGER_MS);
    let lingering = |view: OfferView| recent.then_some(view);
    match offer.state {
        OfferState::Live => {
            let remaining_ms = pairing_remaining_ms(&offer.expires_at, now_ms);
            match &offer.code {
                Some(code) if remaining_ms > 0 => Some(OfferView::Live {
                    code: code.clone(),
                    uri: offer.uri.clone(),
                    remaining_ms,
                }),
                _ => lingering(OfferView::Expired),
            }
        }
        OfferState::Expired => lingering(OfferView::Expired),
        OfferState::Burned => lingering(OfferView::Burned),
        OfferState::Used { .. } | OfferState::Cancelled | OfferState::Superseded => None,
    }
}

#[must_use]
pub fn offer_is_live(offer: Option<&PairingOfferStatus>) -> bool {
    offer.is_some_and(|offer| offer.state == OfferState::Live)
}

#[must_use]
pub fn paired_phone<'a>(
    offer: Option<&PairingOfferStatus>,
    phones: &'a [Phone],
) -> Option<&'a Phone> {
    let OfferState::Used { phone } = &offer?.state else {
        return None;
    };
    phones.iter().find(|found| found.id == *phone)
}

#[must_use]
pub fn with_offer(data: &PhonesResponse, offer: &PairingOffer) -> PhonesResponse {
    PhonesResponse {
        offer: Some(PairingOfferStatus {
            id: offer.id.clone(),
            state: OfferState::Live,
            expires_at: offer.expires_at.clone(),
            failures: 0,
            code: Some(offer.code.clone()),
            uri: Some(offer.uri.clone()),
        }),
        ..data.clone()
    }
}

#[must_use]
pub fn listener_line(access: &PhoneAccessStatus) -> StatusLine {
    if let Some(endpoint) = &access.endpoint {
        return StatusLine {
            text: format!("Ready on {}", endpoint.port),
            tone: Tone::Ok,
            title: Some(endpoint.hosts.join(", ")),
        };
    }
    match &access.listener {
        PhoneListenerState::Failed { reason, .. } => StatusLine::new(
            if reason == PHONE_PORT_IN_USE {
                "Port in use"
            } else {
                "Failed"
            },
            Tone::Danger,
            Some(reason.clone()),
        ),
        PhoneListenerState::On { .. } => StatusLine::new(
            "No LAN address",
            Tone::Danger,
            Some(String::from("Connect this computer to a network")),
        ),
        PhoneListenerState::Off => StatusLine::new("Off", Tone::Dim, None),
    }
}

#[must_use]
pub fn discovery_line(access: &PhoneAccessStatus) -> StatusLine {
    match &access.mdns {
        MdnsState::On { instance } => {
            StatusLine::new("Discovery on", Tone::Ok, Some(instance.clone()))
        }
        MdnsState::Failed { reason } => {
            StatusLine::new("Discovery failed", Tone::Danger, Some(reason.clone()))
        }
        MdnsState::Off => StatusLine::new("Discovery off", Tone::Dim, None),
    }
}

#[derive(Debug, Default)]
pub struct PairWatch {
    watched: Option<String>,
}

impl PairWatch {
    pub fn observe<'a>(&mut self, data: &'a PhonesResponse) -> Option<&'a Phone> {
        let offer = data.offer.as_ref()?;
        if offer_is_live(Some(offer)) {
            self.watched = Some(offer.id.clone());
            return None;
        }
        if self.watched.as_deref() != Some(offer.id.as_str()) {
            return None;
        }
        self.watched = None;
        paired_phone(Some(offer), &data.phones)
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::phone::{PhoneAccess, PhoneEndpoint};

    use super::*;

    const NOW: &str = "2026-09-29T12:00:00Z";

    fn now() -> i64 {
        millis_of(NOW).unwrap_or_default()
    }

    fn phone(id: &str, online: bool, last_seen: Option<&str>) -> Phone {
        Phone {
            id: id.to_owned(),
            name: String::from("Field phone"),
            platform: PhonePlatform::Ios,
            created_at: String::from("2026-09-01T10:00:00Z"),
            last_seen: last_seen.map(str::to_owned),
            online,
            gps_nodes: Vec::new(),
        }
    }

    fn offer(state: OfferState) -> PairingOfferStatus {
        PairingOfferStatus {
            id: String::from("o1"),
            state,
            expires_at: String::from("2026-09-29T12:04:59Z"),
            failures: 0,
            code: Some(String::from("48210937")),
            uri: Some(String::from("sdrmm://pair?c=48210937")),
        }
    }

    fn access(listener: PhoneListenerState, mdns: MdnsState) -> PhoneAccessStatus {
        PhoneAccessStatus {
            access: PhoneAccess {
                enabled: true,
                port: 8443,
            },
            listener,
            endpoint: None,
            mdns,
            protocol: 1,
        }
    }

    fn data(offer: Option<PairingOfferStatus>, phones: Vec<Phone>) -> PhonesResponse {
        PhonesResponse {
            phones,
            offer,
            access: access(PhoneListenerState::Off, MdnsState::Off),
        }
    }

    #[test]
    fn groups_the_pairing_code() {
        assert_eq!(format_pairing_code("48210937"), "4821 0937");
        assert_eq!(format_pairing_code("123456"), "123456");
        assert_eq!(format_pairing_code("4821x937"), "4821x937");
    }

    #[test]
    fn counts_down_and_never_below_zero() {
        assert_eq!(pairing_remaining_ms("2026-09-29T12:04:59Z", now()), 299_000);
        assert_eq!(pairing_remaining_ms("2026-09-29T11:59:00Z", now()), 0);
        assert_eq!(pairing_remaining_ms("garbage", now()), 0);
        assert_eq!(countdown_label(299_000), "4:59");
        assert_eq!(countdown_label(59_001), "1:00");
        assert_eq!(countdown_label(-5_000), "0:00");
    }

    #[test]
    fn says_online_or_when_last_seen() {
        assert_eq!(seen_label(&phone("a", true, None), now()), "Online");
        assert_eq!(
            seen_label(&phone("a", false, Some("2026-09-29T11:55:00Z")), now()),
            "Seen 5 min ago"
        );
        assert_eq!(seen_label(&phone("a", false, None), now()), "Never");
        assert_eq!(age_label(3_400), "3 s");
        assert_eq!(age_label(2 * 3_600_000), "2 h");
        assert_eq!(age_label(3 * 86_400_000), "3 d");
    }

    #[test]
    fn names_the_platform() {
        assert_eq!(platform_label(PhonePlatform::Ios), "iPhone");
        assert_eq!(platform_label(PhonePlatform::Android), "Android");
    }

    #[test]
    fn finds_the_phone_that_just_paired() {
        let phones = [phone("a", false, None), phone("b", true, None)];
        let used = offer(OfferState::Used {
            phone: String::from("b"),
        });
        assert_eq!(
            paired_phone(Some(&used), &phones).map(|found| found.id.as_str()),
            Some("b")
        );
        assert!(paired_phone(Some(&offer(OfferState::Live)), &phones).is_none());
        assert!(paired_phone(None, &phones).is_none());
    }

    #[test]
    fn shows_a_live_offer_then_says_it_expired_or_burned_for_a_while() {
        assert_eq!(
            offer_view(Some(&offer(OfferState::Live)), now()),
            Some(OfferView::Live {
                code: String::from("48210937"),
                uri: Some(String::from("sdrmm://pair?c=48210937")),
                remaining_ms: 299_000,
            })
        );
        let later = now() + 6 * 60_000;
        assert_eq!(
            offer_view(Some(&offer(OfferState::Live)), later),
            Some(OfferView::Expired)
        );
        assert_eq!(
            offer_view(Some(&offer(OfferState::Burned)), now()),
            Some(OfferView::Burned)
        );
        assert_eq!(
            offer_view(Some(&offer(OfferState::Expired)), now() + 3_600_000),
            None
        );
        let used = OfferState::Used {
            phone: String::from("p1"),
        };
        assert_eq!(offer_view(Some(&offer(used)), now()), None);
        assert_eq!(offer_view(Some(&offer(OfferState::Cancelled)), now()), None);
        assert_eq!(offer_view(None, now()), None);
    }

    #[test]
    fn states_where_phones_connect() {
        let mut ready = access(PhoneListenerState::On { port: 8443 }, MdnsState::Off);
        ready.endpoint = Some(PhoneEndpoint {
            port: 8443,
            hosts: vec![
                String::from("192.168.1.20:8443"),
                String::from("shack.local:8443"),
            ],
            pin: String::from("ab"),
            key_check: String::from("AB"),
            dedicated: true,
        });
        assert_eq!(
            listener_line(&ready),
            StatusLine {
                text: String::from("Ready on 8443"),
                tone: Tone::Ok,
                title: Some(String::from("192.168.1.20:8443, shack.local:8443")),
            }
        );
        let failed = |reason: &str| {
            listener_line(&access(
                PhoneListenerState::Failed {
                    port: 8443,
                    reason: reason.to_owned(),
                },
                MdnsState::Off,
            ))
        };
        assert_eq!(failed(PHONE_PORT_IN_USE).text, "Port in use");
        assert_eq!(failed(PHONE_PORT_IN_USE).tone, Tone::Danger);
        assert_eq!(failed("no key").text, "Failed");
        let on = access(PhoneListenerState::On { port: 8443 }, MdnsState::Off);
        assert_eq!(listener_line(&on).text, "No LAN address");
        let off = access(PhoneListenerState::Off, MdnsState::Off);
        assert_eq!(listener_line(&off).text, "Off");
        assert_eq!(discovery_line(&off).text, "Discovery off");
        let broken = access(
            PhoneListenerState::Off,
            MdnsState::Failed {
                reason: String::from("no socket"),
            },
        );
        assert_eq!(
            discovery_line(&broken),
            StatusLine {
                text: String::from("Discovery failed"),
                tone: Tone::Danger,
                title: Some(String::from("no socket")),
            }
        );
    }

    #[test]
    fn a_new_offer_shows_live_before_the_list_catches_up() {
        let created = PairingOffer {
            id: String::from("o2"),
            code: String::from("11112222"),
            uri: String::from("sdrmm://pair?c=11112222"),
            key_check: String::from("AB"),
            expires_at: String::from("2026-09-29T12:05:00Z"),
            endpoint: PhoneEndpoint {
                port: 8443,
                hosts: Vec::new(),
                pin: String::from("ab"),
                key_check: String::from("AB"),
                dedicated: true,
            },
        };
        let next = with_offer(&data(None, Vec::new()), &created);
        assert!(offer_is_live(next.offer.as_ref()));
        assert_eq!(
            next.offer.and_then(|offer| offer.code).as_deref(),
            Some("11112222")
        );
    }

    #[test]
    fn tells_once_when_the_watched_offer_pairs() {
        let mut watch = PairWatch::default();
        let paired = vec![phone("b", true, None)];
        let used = offer(OfferState::Used {
            phone: String::from("b"),
        });
        let done = data(Some(used), paired);
        let live = data(Some(offer(OfferState::Live)), Vec::new());
        assert!(watch.observe(&done).is_none());
        assert!(watch.observe(&live).is_none());
        let told = watch.observe(&done);
        assert_eq!(told.map(|found| found.id.as_str()), Some("b"));
        assert!(watch.observe(&done).is_none());
    }
}
