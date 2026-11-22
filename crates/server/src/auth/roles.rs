use axum::{extract::MatchedPath, http::Method};
use sdrmm_wire::{ClientCommand, UserRole};

/// Routes that grant access or hold credentials.
const ADMIN_ROUTES: &[(Method, &str)] = &[
    (Method::GET, "/api/phones"),
    (Method::PATCH, "/api/phones/{id}"),
    (Method::DELETE, "/api/phones/{id}"),
    (Method::PUT, "/api/phones/access"),
    (Method::POST, "/api/phones/offers"),
    (Method::DELETE, "/api/phones/offers"),
    (Method::GET, "/api/chat-accounts"),
    (Method::POST, "/api/chat-accounts"),
    (Method::PUT, "/api/chat-accounts/{id}"),
    (Method::DELETE, "/api/chat-accounts/{id}"),
    (Method::POST, "/api/secrets"),
    (Method::PUT, "/api/secrets/{id}"),
    (Method::GET, "/api/diagnostics"),
];

/// The least role a request needs: reading is open to viewers, changing anything takes an
/// operator, and access and credentials take an admin.
pub(crate) fn needed(method: &Method, matched: Option<&MatchedPath>) -> UserRole {
    let admin = matched.is_some_and(|path| {
        ADMIN_ROUTES
            .iter()
            .any(|(allowed, route)| allowed == method && *route == path.as_str())
    });
    if admin {
        UserRole::Admin
    } else if *method == Method::GET || *method == Method::HEAD {
        UserRole::Viewer
    } else {
        UserRole::Operator
    }
}

/// The least role a socket command needs. Speaking keys a transmitter.
pub(crate) const fn command_needs(command: &ClientCommand) -> UserRole {
    match command {
        ClientCommand::Speak { .. } => UserRole::Operator,
        ClientCommand::SubscribeDiagnostics { .. }
        | ClientCommand::SubscribeSurface { .. }
        | ClientCommand::UnsubscribeSurface { .. }
        | ClientCommand::SubscribePort { .. }
        | ClientCommand::UnsubscribePort { .. }
        | ClientCommand::SubscribeSink { .. }
        | ClientCommand::UnsubscribeSink { .. }
        | ClientCommand::PublishPose { .. }
        | ClientCommand::Present { .. }
        | ClientCommand::Point(_) => UserRole::Viewer,
    }
}

pub(crate) const fn refusal_text(needed: UserRole) -> &'static str {
    match needed {
        UserRole::Viewer => "Needs the viewer role",
        UserRole::Operator => "Needs the operator role",
        UserRole::Admin => "Needs the admin role",
    }
}
