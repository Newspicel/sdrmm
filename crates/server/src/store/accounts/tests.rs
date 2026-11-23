use sdrmm_wire::MAX_DISCORD_NICKNAME_LEN;

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
            AccountService::Matrix {
                homeserver_url: "https://matrix.example".to_owned(),
                access_token: inline("mx-secret"),
            },
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
        service: AccountService::Matrix {
            homeserver_url: "https://user:pw@matrix.example".to_owned(),
            access_token: inline("mx"),
        },
    };
    assert!(matches!(
        store.create_account(request),
        Err(StoreError::AccountField("homeserver_url"))
    ));
}
