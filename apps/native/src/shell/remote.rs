use std::time::Duration;

use sdrmm_wire::remote::{RemoteState, RemoteStatus};

pub const REMOTE_POLL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteAction {
    Pair,
    Cancel,
    Disconnect,
    PairAgain,
}

#[must_use]
pub fn remote_polled(status: Option<&RemoteStatus>) -> bool {
    status.is_some_and(|status| {
        matches!(
            status.state,
            RemoteState::Pairing | RemoteState::Connecting | RemoteState::Retrying
        )
    })
}

#[must_use]
pub fn app_host(origin: &str) -> String {
    let Ok(parsed) = url::Url::parse(origin) else {
        return origin.to_owned();
    };
    match (parsed.host_str(), parsed.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_owned(),
        (None, _) => origin.to_owned(),
    }
}

#[must_use]
pub fn remote_action(status: &RemoteStatus) -> Option<RemoteAction> {
    if status.via_relay {
        return None;
    }
    Some(match status.state {
        RemoteState::Unpaired => RemoteAction::Pair,
        RemoteState::Pairing => RemoteAction::Cancel,
        RemoteState::Rejected => RemoteAction::PairAgain,
        RemoteState::Connecting | RemoteState::Online | RemoteState::Retrying => {
            RemoteAction::Disconnect
        }
    })
}

#[must_use]
pub fn remote_line(status: &RemoteStatus) -> String {
    let because = |lead: &str| match status.error.as_deref().filter(|error| !error.is_empty()) {
        Some(error) => format!("{lead}: {error}"),
        None => lead.to_owned(),
    };
    match status.state {
        RemoteState::Online => String::from("Online"),
        RemoteState::Connecting => String::from("Connecting"),
        RemoteState::Retrying => because("Offline"),
        RemoteState::Rejected => because("Disconnected"),
        RemoteState::Unpaired | RemoteState::Pairing => String::new(),
    }
}

#[must_use]
pub fn approval_url(status: &RemoteStatus) -> &str {
    status
        .verification_uri_complete
        .as_deref()
        .or(status.verification_uri.as_deref())
        .unwrap_or(&status.app_origin)
}

#[must_use]
pub fn shows_app_link(status: &RemoteStatus) -> bool {
    !matches!(status.state, RemoteState::Unpaired | RemoteState::Pairing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: RemoteState) -> RemoteStatus {
        RemoteStatus {
            state,
            device_id: None,
            user_code: None,
            verification_uri: None,
            verification_uri_complete: None,
            error: None,
            app_origin: String::from("https://app.sdrmm.com"),
            via_relay: false,
        }
    }

    fn failed(state: RemoteState, error: &str) -> RemoteStatus {
        RemoteStatus {
            error: Some(error.to_owned()),
            ..status(state)
        }
    }

    #[test]
    fn polls_only_while_something_is_about_to_change() {
        assert!(!remote_polled(None));
        assert!(remote_polled(Some(&status(RemoteState::Pairing))));
        assert!(remote_polled(Some(&status(RemoteState::Connecting))));
        assert!(remote_polled(Some(&status(RemoteState::Retrying))));
        assert!(!remote_polled(Some(&status(RemoteState::Online))));
        assert!(!remote_polled(Some(&status(RemoteState::Unpaired))));
    }

    #[test]
    fn offers_the_one_action_that_fits_the_state() {
        let action = |state| remote_action(&status(state));
        assert_eq!(action(RemoteState::Unpaired), Some(RemoteAction::Pair));
        assert_eq!(action(RemoteState::Pairing), Some(RemoteAction::Cancel));
        assert_eq!(action(RemoteState::Online), Some(RemoteAction::Disconnect));
        assert_eq!(
            action(RemoteState::Retrying),
            Some(RemoteAction::Disconnect)
        );
        assert_eq!(action(RemoteState::Rejected), Some(RemoteAction::PairAgain));
    }

    #[test]
    fn offers_nothing_through_the_relay() {
        for state in [RemoteState::Online, RemoteState::Unpaired] {
            let relayed = RemoteStatus {
                via_relay: true,
                ..status(state)
            };
            assert_eq!(remote_action(&relayed), None);
        }
    }

    #[test]
    fn says_why_a_connection_is_down() {
        assert_eq!(remote_line(&status(RemoteState::Online)), "Online");
        assert_eq!(
            remote_line(&failed(RemoteState::Retrying, "timed out")),
            "Offline: timed out"
        );
        assert_eq!(
            remote_line(&failed(RemoteState::Rejected, "device removed")),
            "Disconnected: device removed"
        );
        assert_eq!(remote_line(&status(RemoteState::Rejected)), "Disconnected");
        assert_eq!(remote_line(&status(RemoteState::Unpaired)), "");
    }

    #[test]
    fn names_the_app_by_host() {
        assert_eq!(app_host("https://app.sdrmm.com"), "app.sdrmm.com");
        assert_eq!(app_host("http://localhost:5173"), "localhost:5173");
        assert_eq!(app_host("not a url"), "not a url");
    }

    #[test]
    fn approves_at_the_most_complete_link_there_is() {
        let mut pairing = status(RemoteState::Pairing);
        assert_eq!(approval_url(&pairing), "https://app.sdrmm.com");
        pairing.verification_uri = Some(String::from("https://app.sdrmm.com/pair"));
        assert_eq!(approval_url(&pairing), "https://app.sdrmm.com/pair");
        pairing.verification_uri_complete =
            Some(String::from("https://app.sdrmm.com/pair?code=BCDF-GHJK"));
        assert_eq!(
            approval_url(&pairing),
            "https://app.sdrmm.com/pair?code=BCDF-GHJK"
        );
    }

    #[test]
    fn links_the_app_once_paired() {
        assert!(!shows_app_link(&status(RemoteState::Unpaired)));
        assert!(!shows_app_link(&status(RemoteState::Pairing)));
        assert!(shows_app_link(&status(RemoteState::Online)));
        assert!(shows_app_link(&status(RemoteState::Rejected)));
    }
}
