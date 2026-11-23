use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::event_output::valid_influx_name;

pub const MAX_ACCOUNT_LEN: usize = 64;
/// Discord counts a nickname in UTF-16 units.
pub const MAX_DISCORD_NICKNAME_LEN: usize = 32;
pub const MAX_URL_LEN: usize = 2_048;
pub const MAX_SECRET_LEN: usize = 4_096;
pub const MAX_SECRET_ID_LEN: usize = 64;
pub const MAX_MQTT_USERNAME_LEN: usize = 255;
pub const MAX_SQL_USERNAME_LEN: usize = 63;
pub const MAX_MATRIX_USER_LEN: usize = 255;

/// A credential an account holds. The server keeps the value in its own store and the account
/// names it by id, so a listing never carries it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum Secret {
    Stored {
        secret: String,
    },
    /// A value a client sends to replace the stored one. Saving the account moves it into the
    /// server's store.
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

    /// A value sent in place, waiting to move into the server's store.
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
            Self::Inline(value) => value.len() <= MAX_SECRET_LEN,
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

/// The service an account signs in to, without its settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccountKind {
    Discord,
    Teamspeak,
    Webhook,
    Matrix,
    Mqtt,
    Postgres,
    Influx,
}

impl AccountKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Discord => "Discord",
            Self::Teamspeak => "TeamSpeak",
            Self::Webhook => "Webhook",
            Self::Matrix => "Matrix",
            Self::Mqtt => "MQTT",
            Self::Postgres => "PostgreSQL",
            Self::Influx => "InfluxDB",
        }
    }

    /// Whether a voice chat joins through this kind of account.
    #[must_use]
    pub const fn chat(self) -> bool {
        matches!(self, Self::Discord | Self::Teamspeak | Self::Matrix)
    }
}

/// How a Matrix account signs in. A token only sends events; voice needs a device of its own,
/// made by a password or an OAuth sign-in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatrixLogin {
    #[default]
    Token,
    Password,
    Oauth,
}

/// What the operator opens to approve a Matrix OAuth sign-in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MatrixSignInPrompt {
    pub verification_uri: String,
    pub user_code: String,
    pub expires_in_s: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MatrixSignInState {
    #[default]
    Idle,
    Pending(MatrixSignInPrompt),
    Failed {
        error: String,
    },
}

/// Where an account signs in and with what. Credentials are [`Secret`]s; everything else is a
/// plain setting the account list shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "service", rename_all = "snake_case")]
pub enum AccountService {
    Discord {
        #[serde(default)]
        token: Secret,
        #[serde(default)]
        nickname: String,
    },
    Teamspeak {
        address: String,
        #[serde(default)]
        password: Secret,
        #[serde(default)]
        nickname: String,
        #[serde(default)]
        identity: Secret,
    },
    Webhook {
        #[serde(default)]
        url: Secret,
    },
    /// `session` and `store_key` are kept by the server and never taken from a request.
    Matrix {
        homeserver_url: String,
        #[serde(default)]
        user: String,
        #[serde(default)]
        login: MatrixLogin,
        #[serde(default)]
        access_token: Secret,
        #[serde(default)]
        password: Secret,
        #[serde(default)]
        nickname: String,
        #[serde(default)]
        recovery_key: Secret,
        #[serde(default)]
        session: Secret,
        #[serde(default)]
        store_key: Secret,
    },
    Mqtt {
        broker_url: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: Secret,
    },
    Postgres {
        url: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: Secret,
    },
    Influx {
        url: String,
        #[serde(default)]
        org: String,
        #[serde(default)]
        token: Secret,
    },
}

impl AccountService {
    #[must_use]
    pub const fn kind(&self) -> AccountKind {
        match self {
            Self::Discord { .. } => AccountKind::Discord,
            Self::Teamspeak { .. } => AccountKind::Teamspeak,
            Self::Webhook { .. } => AccountKind::Webhook,
            Self::Matrix { .. } => AccountKind::Matrix,
            Self::Mqtt { .. } => AccountKind::Mqtt,
            Self::Postgres { .. } => AccountKind::Postgres,
            Self::Influx { .. } => AccountKind::Influx,
        }
    }

    #[must_use]
    pub fn credentials(&self) -> Vec<&Secret> {
        match self {
            Self::Discord { token, .. } => vec![token],
            Self::Teamspeak {
                password, identity, ..
            } => vec![password, identity],
            Self::Webhook { url } => vec![url],
            Self::Matrix {
                access_token,
                password,
                recovery_key,
                ..
            } => vec![access_token, password, recovery_key],
            Self::Mqtt { password, .. } | Self::Postgres { password, .. } => vec![password],
            Self::Influx { token, .. } => vec![token],
        }
    }

    /// The credentials this server makes and keeps itself, apart from [`Self::credentials`].
    #[must_use]
    pub fn managed(&self) -> Vec<&Secret> {
        match self {
            Self::Matrix {
                session, store_key, ..
            } => vec![session, store_key],
            _ => Vec::new(),
        }
    }

    /// Every credential in the order of [`Self::credentials`], each with whether the account
    /// works without it, so sending it empty clears it.
    pub fn credentials_mut(&mut self) -> Vec<(&mut Secret, bool)> {
        match self {
            Self::Discord { token, .. } => vec![(token, false)],
            Self::Teamspeak {
                password, identity, ..
            } => vec![(password, true), (identity, false)],
            Self::Webhook { url } => vec![(url, false)],
            Self::Matrix {
                login,
                access_token,
                password,
                recovery_key,
                ..
            } => {
                let login = *login;
                vec![
                    (access_token, login != MatrixLogin::Token),
                    (password, login != MatrixLogin::Password),
                    (recovery_key, true),
                ]
            }
            Self::Mqtt { password, .. } | Self::Postgres { password, .. } => vec![(password, true)],
            Self::Influx { token, .. } => vec![(token, true)],
        }
    }

    /// Whether `value`, read back from the store, still passes as this account's credential.
    #[must_use]
    pub fn accepts_secret(&self, value: &str) -> bool {
        value.len() <= MAX_SECRET_LEN
            && (!matches!(self, Self::Webhook { .. }) || valid_url(value, &["https"]))
    }

    /// The first setting that does not hold, by its field name.
    pub fn check(&self) -> Result<(), &'static str> {
        match self {
            Self::Discord { token, nickname } => {
                holds(
                    nickname.encode_utf16().count() <= MAX_DISCORD_NICKNAME_LEN,
                    "nickname",
                )?;
                holds(token.valid(), "token")
            }
            Self::Teamspeak {
                address,
                password,
                nickname,
                identity,
            } => {
                holds(
                    !address.trim().is_empty() && address.len() <= MAX_URL_LEN,
                    "address",
                )?;
                holds(nickname.chars().count() <= MAX_ACCOUNT_LEN, "nickname")?;
                holds(password.valid(), "password")?;
                holds(identity.valid(), "identity")
            }
            Self::Webhook { url } => holds(
                url.valid() && url.inline().is_none_or(|url| valid_url(url, &["https"])),
                "url",
            ),
            Self::Matrix {
                homeserver_url,
                user,
                login,
                access_token,
                password,
                nickname,
                recovery_key,
                ..
            } => {
                let token = *login == MatrixLogin::Token;
                holds(valid_url(homeserver_url, &["https"]), "homeserver_url")?;
                holds(
                    (token && user.is_empty()) || valid_matrix_user(user),
                    "user",
                )?;
                holds(nickname.chars().count() <= MAX_ACCOUNT_LEN, "nickname")?;
                holds(
                    access_token.valid() && (!token || access_token.is_set()),
                    "access_token",
                )?;
                holds(
                    password.valid() && (*login != MatrixLogin::Password || password.is_set()),
                    "password",
                )?;
                holds(recovery_key.valid(), "recovery_key")
            }
            Self::Mqtt {
                broker_url,
                username,
                password,
            } => {
                holds(valid_url(broker_url, &["mqtt", "mqtts"]), "broker_url")?;
                holds(username.len() <= MAX_MQTT_USERNAME_LEN, "username")?;
                holds(password.valid(), "password")
            }
            Self::Postgres {
                url,
                username,
                password,
            } => {
                holds(valid_url(url, &["postgres", "postgresql"]), "url")?;
                holds(
                    !username.trim().is_empty() && username.len() <= MAX_SQL_USERNAME_LEN,
                    "username",
                )?;
                holds(password.valid(), "password")
            }
            Self::Influx { url, org, token } => {
                holds(valid_url(url, &["http", "https"]), "url")?;
                holds(valid_influx_name(org), "org")?;
                holds(token.valid(), "token")
            }
        }
    }
}

fn holds(ok: bool, field: &'static str) -> Result<(), &'static str> {
    if ok { Ok(()) } else { Err(field) }
}

/// `@local:server`, without spaces. Homeservers lowercase a new localpart, so an uppercase one
/// names no account.
fn valid_matrix_user(user: &str) -> bool {
    user.len() <= MAX_MATRIX_USER_LEN
        && !user.contains(char::is_whitespace)
        && user
            .strip_prefix('@')
            .and_then(|rest| rest.split_once(':'))
            .is_some_and(|(local, server)| {
                !local.is_empty()
                    && !server.is_empty()
                    && !local.bytes().any(|b| b.is_ascii_uppercase())
            })
}

/// An absolute URL in one of `schemes` that carries no login of its own: the login belongs in
/// the account's credentials.
#[must_use]
pub fn valid_url(value: &str, schemes: &[&str]) -> bool {
    value.len() <= MAX_URL_LEN
        && url::Url::parse(value).is_ok_and(|url| {
            schemes.contains(&url.scheme())
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

/// A login on a service this server talks to. Voice chats and event outputs name it by id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Account {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub service: AccountService,
}

impl Account {
    /// The first setting that does not hold, by its field name.
    pub fn check(&self) -> Result<(), &'static str> {
        let name = self.name.trim();
        holds(
            !name.is_empty() && name.chars().count() <= MAX_ACCOUNT_LEN,
            "name",
        )?;
        self.service.check()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AccountsResponse {
    pub accounts: Vec<Account>,
}

#[cfg(test)]
mod tests;
