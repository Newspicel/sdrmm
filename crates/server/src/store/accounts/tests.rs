use sdrmm_wire::{MAX_DISCORD_NICKNAME_LEN, MatrixLogin};

use super::*;

fn teamspeak(password: Secret, identity: Secret) -> Account {
    Account {
        id: String::new(),
        name: "Club".to_owned(),
        service: AccountService::Teamspeak {
            address: "ts.example".to_owned(),
            password,
            nickname: "sdr".to_owned(),
            identity,
        },
    }
}

fn inline(value: &str) -> Secret {
    Secret::Inline(value.to_owned())
}

fn held(store: &Store, secret: &Secret) -> Option<String> {
    let Secret::Stored { secret } = secret else {
        panic!("not stored: {secret:?}");
    };
    store.secret(secret).expect("read")
}

fn main_secret_of(account: &Account) -> Secret {
    account.service.credentials()[0].clone()
}

fn identity_of(account: &Account) -> Secret {
    match &account.service {
        AccountService::Teamspeak { identity, .. } => identity.clone(),
        other => panic!("{other:?} has no identity"),
    }
}

#[test]
fn a_stored_id_sent_by_a_client_never_claims_another_accounts_secret() {
    let store = Store::open(None).expect("store");
    let first = store
        .create_account(teamspeak(inline("one"), Secret::default()))
        .expect("first");
    let claim = main_secret_of(&first);

    let second = store
        .create_account(teamspeak(claim.clone(), Secret::default()))
        .expect("second");
    assert!(!main_secret_of(&second).is_set());

    let updated = store
        .update_account(&second.id, teamspeak(claim, Secret::default()))
        .expect("updated");
    assert!(!main_secret_of(&updated).is_set());

    store.delete_account(&second.id).expect("deleted");
    assert_eq!(
        held(&store, &main_secret_of(&first)).as_deref(),
        Some("one")
    );
}

#[test]
fn an_update_keeps_secrets_it_leaves_stored_and_replaces_inline_ones() {
    let store = Store::open(None).expect("store");
    let made = store
        .create_account(teamspeak(inline("one"), Secret::default()))
        .expect("created");
    let identity = held(&store, &identity_of(&made));

    let kept = store
        .update_account(
            &made.id,
            teamspeak(main_secret_of(&made), Secret::default()),
        )
        .expect("kept");
    assert_eq!(held(&store, &main_secret_of(&kept)).as_deref(), Some("one"));
    assert_eq!(held(&store, &identity_of(&kept)), identity);

    let replaced = store
        .update_account(&made.id, teamspeak(inline("two"), Secret::default()))
        .expect("replaced");
    assert_eq!(main_secret_of(&replaced), main_secret_of(&made));
    assert_eq!(
        held(&store, &main_secret_of(&replaced)).as_deref(),
        Some("two")
    );
}

#[test]
fn an_empty_password_clears_the_stored_one_and_keeps_the_identity() {
    let store = Store::open(None).expect("store");
    let made = store
        .create_account(teamspeak(inline("one"), Secret::default()))
        .expect("created");
    let identity = held(&store, &identity_of(&made));

    let cleared = store
        .update_account(&made.id, teamspeak(Secret::default(), Secret::default()))
        .expect("cleared");

    assert!(!main_secret_of(&cleared).is_set());
    assert!(held(&store, &main_secret_of(&made)).is_none());
    assert_eq!(held(&store, &identity_of(&cleared)), identity);
}

#[test]
fn changing_the_service_drops_the_old_secrets() {
    let store = Store::open(None).expect("store");
    let made = store
        .create_account(teamspeak(inline("one"), Secret::default()))
        .expect("created");
    let discord = Account {
        id: String::new(),
        name: "Club".to_owned(),
        service: AccountService::Discord {
            token: inline("tok"),
            nickname: String::new(),
        },
    };

    let changed = store.update_account(&made.id, discord).expect("changed");

    assert_eq!(
        held(&store, &main_secret_of(&changed)).as_deref(),
        Some("tok")
    );
    assert!(held(&store, &main_secret_of(&made)).is_none());
    assert!(held(&store, &identity_of(&made)).is_none());
}

#[test]
fn deleting_takes_the_secrets_with_it() {
    let store = Store::open(None).expect("store");
    let made = store
        .create_account(teamspeak(inline("one"), Secret::default()))
        .expect("created");

    store.delete_account(&made.id).expect("deleted");

    assert!(store.accounts().expect("list").is_empty());
    assert!(held(&store, &main_secret_of(&made)).is_none());
    assert!(held(&store, &identity_of(&made)).is_none());
    assert!(matches!(
        store.delete_account(&made.id),
        Err(StoreError::AccountNotFound(_))
    ));
}

#[test]
fn an_aged_account_secret_survives_collection() {
    let store = Store::open(None).expect("store");
    let made = store
        .create_account(teamspeak(inline("one"), Secret::default()))
        .expect("created");
    store
        .hold()
        .execute(
            "UPDATE secrets SET updated_at = '2000-01-01T00:00:00.000000000Z'",
            [],
        )
        .expect("aged");

    secrets::collect(&store.hold()).expect("collected");

    assert_eq!(held(&store, &main_secret_of(&made)).as_deref(), Some("one"));
}

#[test]
fn an_account_needs_a_name_and_a_server() {
    let store = Store::open(None).expect("store");
    let mut nameless = teamspeak(Secret::default(), Secret::default());
    nameless.name = "  ".to_owned();
    assert!(store.create_account(nameless).is_err());
    let mut homeless = teamspeak(Secret::default(), Secret::default());
    if let AccountService::Teamspeak { address, .. } = &mut homeless.service {
        address.clear();
    }
    assert!(store.create_account(homeless).is_err());
}

#[test]
fn a_discord_nickname_fits_discord() {
    let store = Store::open(None).expect("store");
    let named = |nickname: String| Account {
        id: String::new(),
        name: "Club".to_owned(),
        service: AccountService::Discord {
            token: inline("tok"),
            nickname,
        },
    };
    let longest = "é".repeat(MAX_DISCORD_NICKNAME_LEN);
    assert!(store.create_account(named(longest.clone())).is_ok());
    assert!(store.create_account(named(longest + "x")).is_err());
}

fn mqtt(password: Secret) -> Account {
    Account {
        id: String::new(),
        name: "Broker".to_owned(),
        service: AccountService::Mqtt {
            broker_url: "mqtts://broker.example".to_owned(),
            username: "radio".to_owned(),
            password,
        },
    }
}

#[test]
fn every_output_service_keeps_its_credential_by_id() {
    let store = Store::open(None).expect("store");
    for (service, value) in [
        (
            AccountService::Webhook {
                url: inline("https://hooks.example/abc"),
            },
            "https://hooks.example/abc",
        ),
        (
            token_matrix("https://matrix.example", inline("mx-secret")),
            "mx-secret",
        ),
        (
            AccountService::Postgres {
                url: "postgres://db.example/radio".to_owned(),
                username: "radio".to_owned(),
                password: inline("pg-secret"),
            },
            "pg-secret",
        ),
        (
            AccountService::Influx {
                url: "http://127.0.0.1:8086".to_owned(),
                org: "home".to_owned(),
                token: inline("influx-secret"),
            },
            "influx-secret",
        ),
    ] {
        let made = store
            .create_account(Account {
                id: String::new(),
                name: "Out".to_owned(),
                service,
            })
            .expect("created");
        let raw = serde_json::to_string(&made).expect("json");
        assert!(!raw.contains(value), "{raw}");
        assert_eq!(held(&store, &main_secret_of(&made)).as_deref(), Some(value));
        assert_eq!(store.account(&made.id).expect("read"), Some(made));
    }
    assert_eq!(store.accounts().expect("list").len(), 4);
    assert_eq!(store.account("gone").expect("read"), None);
}

#[test]
fn an_optional_output_credential_clears_when_sent_empty() {
    let store = Store::open(None).expect("store");
    let made = store.create_account(mqtt(inline("pw"))).expect("created");
    let kept = store
        .update_account(&made.id, mqtt(main_secret_of(&made)))
        .expect("kept");
    assert_eq!(held(&store, &main_secret_of(&kept)).as_deref(), Some("pw"));

    let cleared = store
        .update_account(&made.id, mqtt(Secret::default()))
        .expect("cleared");

    assert!(!main_secret_of(&cleared).is_set());
    assert!(held(&store, &main_secret_of(&made)).is_none());
}

#[test]
fn an_output_account_refuses_a_login_in_its_url() {
    let store = Store::open(None).expect("store");
    let request = Account {
        id: String::new(),
        name: "Out".to_owned(),
        service: token_matrix("https://user:pw@matrix.example", inline("mx")),
    };
    assert!(matches!(
        store.create_account(request),
        Err(StoreError::AccountField("homeserver_url"))
    ));
}

fn matrix(login: MatrixLogin, user: &str) -> Account {
    Account {
        id: String::new(),
        name: "Bot".to_owned(),
        service: AccountService::Matrix {
            homeserver_url: "https://matrix.example".to_owned(),
            user: user.to_owned(),
            login,
            access_token: Secret::default(),
            password: Secret::default(),
            nickname: String::new(),
            recovery_key: Secret::default(),
            session: Secret::default(),
            store_key: Secret::default(),
        },
    }
}

fn token_matrix(homeserver_url: &str, access_token: Secret) -> AccountService {
    let mut account = matrix(MatrixLogin::Token, "");
    if let AccountService::Matrix {
        homeserver_url: url,
        access_token: token,
        ..
    } = &mut account.service
    {
        homeserver_url.clone_into(url);
        *token = access_token;
    }
    account.service
}

/// The session and the store key.
fn managed_of(account: &Account) -> (Secret, Secret) {
    let managed = account.service.managed();
    (managed[0].clone(), managed[1].clone())
}

fn with_managed(mut account: Account, session: Secret, store_key: Secret) -> Account {
    if let AccountService::Matrix {
        session: held,
        store_key: key,
        ..
    } = &mut account.service
    {
        *held = session;
        *key = store_key;
    }
    account
}

const BOT: &str = "@bot:matrix.example";

fn session_of(user: &str) -> String {
    format!(
        r#"{{"kind":"oauth","client_id":"c","user_id":"{user}","device_id":"D","access_token":"t"}}"#
    )
}

#[test]
fn a_session_of_another_user_is_refused() {
    let store = Store::open(None).expect("store");
    let made = store
        .create_account(matrix(MatrixLogin::Oauth, BOT))
        .expect("created");
    for session in [session_of("@other:matrix.example"), "{}".to_owned()] {
        assert!(matches!(
            store.set_account_session(&made.id, &session),
            Err(StoreError::AccountField("session"))
        ));
    }
    let kept = store.account(&made.id).expect("read").expect("found");
    assert!(!managed_of(&kept).0.is_set());
}

#[test]
fn a_request_cannot_set_the_session() {
    let store = Store::open(None).expect("store");
    let forged = with_managed(
        matrix(MatrixLogin::Oauth, BOT),
        inline("forged"),
        inline("forged-key"),
    );
    let made = store.create_account(forged.clone()).expect("created");
    let (session, key) = managed_of(&made);
    assert!(!session.is_set());
    assert_ne!(held(&store, &key).as_deref(), Some("forged-key"));

    store
        .set_account_session(&made.id, &session_of(BOT))
        .expect("signed in");
    let updated = store.update_account(&made.id, forged).expect("updated");

    let (session, kept_key) = managed_of(&updated);
    assert_eq!(
        held(&store, &session).as_deref(),
        Some(session_of(BOT).as_str())
    );
    assert_eq!(kept_key, key);
}

#[test]
fn another_user_forgets_the_session() {
    let store = Store::open(None).expect("store");
    let made = store
        .create_account(matrix(MatrixLogin::Oauth, BOT))
        .expect("created");
    store
        .set_account_session(&made.id, &session_of(BOT))
        .expect("signed in");
    let signed_in = store.account(&made.id).expect("read").expect("found");
    let (session, key) = managed_of(&signed_in);

    let mut renamed = signed_in.clone();
    if let AccountService::Matrix { nickname, .. } = &mut renamed.service {
        "Relay".clone_into(nickname);
    }
    let renamed = store.update_account(&made.id, renamed).expect("renamed");
    assert_eq!(managed_of(&renamed).0, session);

    let moved = store
        .update_account(
            &made.id,
            matrix(MatrixLogin::Oauth, "@other:matrix.example"),
        )
        .expect("moved");
    let (gone, kept_key) = managed_of(&moved);
    assert!(!gone.is_set());
    assert!(held(&store, &session).is_none());
    assert_eq!(kept_key, key);
}

#[test]
fn a_store_key_is_made_on_create() {
    let store = Store::open(None).expect("store");
    let mut request = matrix(MatrixLogin::Password, BOT);
    if let AccountService::Matrix { password, .. } = &mut request.service {
        *password = inline("pw");
    }
    let made = store.create_account(request).expect("created");
    let key = held(&store, &managed_of(&made).1).expect("key");
    assert_eq!(key.len(), 64);
    assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));

    let sender = store
        .create_account(Account {
            id: String::new(),
            name: "Out".to_owned(),
            service: token_matrix("https://matrix.example", inline("mx")),
        })
        .expect("created");
    assert!(!managed_of(&sender).1.is_set());
}

#[test]
fn deleting_removes_every_matrix_secret() {
    let store = Store::open(None).expect("store");
    let mut request = matrix(MatrixLogin::Password, BOT);
    if let AccountService::Matrix {
        password,
        recovery_key,
        ..
    } = &mut request.service
    {
        *password = inline("pw");
        *recovery_key = inline("EsTc key");
    }
    let made = store.create_account(request).expect("created");
    store
        .set_account_session(&made.id, &session_of(BOT))
        .expect("signed in");
    let made = store.account(&made.id).expect("read").expect("found");
    let every: Vec<Secret> = made
        .service
        .credentials()
        .into_iter()
        .chain(made.service.managed())
        .filter(|secret| secret.is_set())
        .cloned()
        .collect();
    assert_eq!(every.len(), 4);

    store.delete_account(&made.id).expect("deleted");

    for secret in &every {
        assert!(held(&store, secret).is_none(), "{secret:?}");
    }
}

fn with_password(mut account: Account, value: Secret) -> Account {
    if let AccountService::Matrix { password, .. } = &mut account.service {
        *password = value;
    }
    account
}

#[test]
fn a_new_password_forgets_the_session() {
    let store = Store::open(None).expect("store");
    let made = store
        .create_account(with_password(
            matrix(MatrixLogin::Password, BOT),
            inline("pw"),
        ))
        .expect("created");
    store
        .set_account_session(&made.id, &session_of(BOT))
        .expect("signed in");
    let signed_in = store.account(&made.id).expect("read").expect("found");
    let session = managed_of(&signed_in).0;

    let same = with_password(signed_in.clone(), inline("pw"));
    let kept = store.update_account(&made.id, same).expect("updated");
    assert_eq!(managed_of(&kept).0, session);

    let changed = with_password(signed_in, inline("new pw"));
    let changed = store.update_account(&made.id, changed).expect("updated");
    assert!(!managed_of(&changed).0.is_set());
    assert!(held(&store, &session).is_none());
}

#[test]
fn a_session_hands_its_recovery_key_to_the_account() {
    let store = Store::open(None).expect("store");
    let made = store
        .create_account(matrix(MatrixLogin::Oauth, BOT))
        .expect("created");
    let carrying = session_of(BOT).replacen('{', r#"{"recovery_key":"EsTc made","#, 1);
    store
        .set_account_session(&made.id, &carrying)
        .expect("signed in");
    let signed_in = store.account(&made.id).expect("read").expect("found");
    let AccountService::Matrix {
        recovery_key,
        session,
        ..
    } = &signed_in.service
    else {
        panic!("not matrix");
    };
    assert_eq!(held(&store, recovery_key).as_deref(), Some("EsTc made"));
    let kept = held(&store, session).expect("session");
    assert!(!kept.contains("recovery_key"));
    assert_eq!(sdrmm_voicechat::session_user(&kept).as_deref(), Some(BOT));
}

#[test]
fn a_session_names_its_user_in_any_case() {
    let store = Store::open(None).expect("store");
    let made = store
        .create_account(matrix(MatrixLogin::Oauth, BOT))
        .expect("created");
    store
        .set_account_session(&made.id, &session_of("@bot:Matrix.Example"))
        .expect("signed in");
}
