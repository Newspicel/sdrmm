use std::sync::Arc;

use sdrmm_wire::{AccountKind, AccountService, EventOutputTarget, MatrixLogin, Secret};

use crate::Store;

/// What an output signs in with: its account's settings with the credential read back from this
/// server's store. Read again whenever the patch, the accounts or the secrets change.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum Login {
    None,
    Webhook {
        url: String,
    },
    Matrix {
        homeserver_url: String,
        access_token: String,
    },
    Mqtt {
        broker_url: String,
        username: String,
        password: String,
    },
    Postgres {
        url: String,
        username: String,
        password: String,
    },
    Influx {
        url: String,
        org: String,
        token: String,
    },
}

/// The login `target` signs in with, or why it cannot.
pub(super) async fn resolve(
    store: &Arc<Store>,
    target: &EventOutputTarget,
) -> Result<Login, String> {
    let Some((id, kind)) = target.account() else {
        return Ok(Login::None);
    };
    let (store, id) = (store.clone(), id.to_owned());
    tokio::task::spawn_blocking(move || read(&store, &id, kind))
        .await
        .map_err(|error| format!("Account: {error}"))?
}

fn read(store: &Store, id: &str, kind: AccountKind) -> Result<Login, String> {
    let account = store
        .account(id)
        .map_err(|error| format!("Account: {error}"))?
        .ok_or_else(|| format!("{} account is gone", kind.label()))?;
    let service = &account.service;
    if service.kind() != kind {
        return Err(format!(
            "{} is a {} account, not {}",
            account.name,
            service.kind().label(),
            kind.label()
        ));
    }
    let reveal = |secret: &Secret, what: &str| reveal(store, service, secret, what);
    let required = |secret: &Secret, what: &str| {
        reveal(secret, what)?
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{what} is not set"))
    };
    let optional =
        |secret: &Secret, what: &str| Ok::<_, String>(reveal(secret, what)?.unwrap_or_default());
    Ok(match service {
        AccountService::Webhook { url } => Login::Webhook {
            url: required(url, "Webhook URL")?,
        },
        AccountService::Matrix {
            homeserver_url,
            login,
            access_token,
            session,
            ..
        } => Login::Matrix {
            homeserver_url: homeserver_url.clone(),
            access_token: if *login == MatrixLogin::Token {
                required(access_token, "Matrix token")?
            } else {
                reveal(session, "Matrix session")?
                    .as_deref()
                    .and_then(sdrmm_voicechat::session_access_token)
                    .ok_or_else(|| "Matrix is not signed in".to_owned())?
            },
        },
        AccountService::Mqtt {
            broker_url,
            username,
            password,
        } => Login::Mqtt {
            broker_url: broker_url.clone(),
            username: username.clone(),
            password: optional(password, "MQTT password")?,
        },
        AccountService::Postgres {
            url,
            username,
            password,
        } => Login::Postgres {
            url: url.clone(),
            username: username.clone(),
            password: optional(password, "PostgreSQL password")?,
        },
        AccountService::Influx { url, org, token } => Login::Influx {
            url: url.clone(),
            org: org.clone(),
            token: optional(token, "InfluxDB token")?,
        },
        AccountService::Discord { .. } | AccountService::Teamspeak { .. } => {
            return Err(format!("{} cannot send events", kind.label()));
        }
    })
}

/// The credential's value, none while it is unset. One the account names that this server
/// lacks, or that no longer passes, is a fault.
fn reveal(
    store: &Store,
    service: &AccountService,
    secret: &Secret,
    what: &str,
) -> Result<Option<String>, String> {
    let Secret::Stored { secret: id } = secret else {
        return Ok(secret.inline().map(str::to_owned));
    };
    let value = store
        .secret(id)
        .map_err(|error| format!("{what}: {error}"))?
        .ok_or_else(|| format!("{what} is not set on this server"))?;
    if !service.accepts_secret(&value) {
        return Err(format!("{what} is not valid"));
    }
    Ok(Some(value))
}
