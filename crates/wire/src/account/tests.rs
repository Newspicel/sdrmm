use super::*;

fn account(service: AccountService) -> Account {
    Account {
        id: "a1".to_owned(),
        name: "Club".to_owned(),
        service,
    }
}

fn inline(value: &str) -> Secret {
    Secret::Inline(value.to_owned())
}

fn matrix(login: MatrixLogin) -> AccountService {
    AccountService::Matrix {
        homeserver_url: "https://matrix.example".to_owned(),
        user: "@bot:matrix.example".to_owned(),
        login,
        access_token: Secret::default(),
        password: Secret::default(),
        nickname: String::new(),
        recovery_key: Secret::default(),
        session: Secret::default(),
        store_key: Secret::default(),
    }
}

fn token_matrix(homeserver_url: &str, access_token: Secret) -> AccountService {
    let mut service = matrix(MatrixLogin::Token);
    if let AccountService::Matrix {
        homeserver_url: url,
        access_token: token,
        user,
        ..
    } = &mut service
    {
        homeserver_url.clone_into(url);
        *token = access_token;
        user.clear();
    }
    service
}

fn every_service() -> Vec<AccountService> {
    vec![
        AccountService::Discord {
            token: Secret::stored("tok_1"),
            nickname: "Relay".to_owned(),
        },
        AccountService::Teamspeak {
            address: "ts.example.org".to_owned(),
            password: Secret::default(),
            nickname: "sdr".to_owned(),
            identity: Secret::stored("id_1"),
        },
        AccountService::Webhook {
            url: Secret::stored("hook_1"),
        },
        token_matrix("https://matrix.example", Secret::stored("mx_1")),
        AccountService::Mqtt {
            broker_url: "mqtts://broker.example".to_owned(),
            username: "radio".to_owned(),
            password: Secret::default(),
        },
        AccountService::Postgres {
            url: "postgres://db.example/radio".to_owned(),
            username: "radio".to_owned(),
            password: Secret::stored("pg_1"),
        },
        AccountService::Influx {
            url: "http://127.0.0.1:8086".to_owned(),
            org: "home".to_owned(),
            token: Secret::stored("in_1"),
        },
    ]
}

#[test]
fn every_service_round_trips_and_passes_its_checks() {
    for service in every_service() {
        let account = account(service);
        assert_eq!(account.check(), Ok(()), "{account:?}");
        let json = serde_json::to_value(&account).expect("json");
        assert_eq!(json["id"], "a1");
        let back: Account = serde_json::from_value(json).expect("back");
        assert_eq!(back, account);
    }
    let json = serde_json::to_value(account(AccountService::Webhook {
        url: Secret::default(),
    }))
    .expect("json");
    assert_eq!(json["service"], "webhook");
}

#[test]
fn a_teamspeak_account_without_secrets_loads() {
    let account: Account = serde_json::from_value(serde_json::json!({
        "id": "t", "name": "Net", "service": "teamspeak", "address": "ts.example.org"
    }))
    .expect("parsed");
    let AccountService::Teamspeak {
        password,
        identity,
        nickname,
        ..
    } = account.service
    else {
        panic!("not teamspeak");
    };
    assert!(!password.is_set() && !identity.is_set());
    assert!(nickname.is_empty());
}

#[test]
fn an_account_needs_a_bounded_name() {
    let mut nameless = account(AccountService::Webhook {
        url: Secret::default(),
    });
    nameless.name = "  ".to_owned();
    assert_eq!(nameless.check(), Err("name"));
    nameless.name = "n".repeat(MAX_ACCOUNT_LEN + 1);
    assert_eq!(nameless.check(), Err("name"));
}

#[test]
fn each_service_names_the_field_that_fails() {
    for (service, field) in [
        (
            AccountService::Discord {
                token: Secret::default(),
                nickname: "é".repeat(MAX_DISCORD_NICKNAME_LEN + 1),
            },
            "nickname",
        ),
        (
            AccountService::Teamspeak {
                address: " ".to_owned(),
                password: Secret::default(),
                nickname: String::new(),
                identity: Secret::default(),
            },
            "address",
        ),
        (
            AccountService::Webhook {
                url: inline("http://hooks.example/abc"),
            },
            "url",
        ),
        (
            AccountService::Webhook {
                url: inline(&"x".repeat(MAX_SECRET_LEN + 1)),
            },
            "url",
        ),
        (
            token_matrix("https://user:pass@matrix.example", Secret::default()),
            "homeserver_url",
        ),
        (token_matrix("", Secret::default()), "homeserver_url"),
        (
            token_matrix("https://matrix.example", Secret::default()),
            "access_token",
        ),
        (
            AccountService::Mqtt {
                broker_url: "https://broker.example".to_owned(),
                username: String::new(),
                password: Secret::default(),
            },
            "broker_url",
        ),
        (
            AccountService::Mqtt {
                broker_url: "mqtt://broker.example".to_owned(),
                username: "u".repeat(MAX_MQTT_USERNAME_LEN + 1),
                password: Secret::default(),
            },
            "username",
        ),
        (
            AccountService::Postgres {
                url: "postgres://db.example/radio".to_owned(),
                username: String::new(),
                password: Secret::default(),
            },
            "username",
        ),
        (
            AccountService::Postgres {
                url: "postgres://radio:secret@db.example/radio".to_owned(),
                username: "radio".to_owned(),
                password: Secret::default(),
            },
            "url",
        ),
        (
            AccountService::Influx {
                url: "ftp://influx.example".to_owned(),
                org: String::new(),
                token: Secret::default(),
            },
            "url",
        ),
        (
            AccountService::Influx {
                url: "http://influx.example".to_owned(),
                org: "a\nb".to_owned(),
                token: Secret::default(),
            },
            "org",
        ),
        (
            AccountService::Influx {
                url: "http://influx.example".to_owned(),
                org: String::new(),
                token: Secret::stored("not/an/id"),
            },
            "token",
        ),
    ] {
        assert_eq!(service.check(), Err(field), "{service:?}");
    }
}

#[test]
fn optional_credentials_are_the_ones_an_account_works_without() {
    let optional = |mut service: AccountService| -> Vec<bool> {
        service
            .credentials_mut()
            .into_iter()
            .map(|(_, optional)| optional)
            .collect()
    };
    let services = every_service();
    let found: Vec<Vec<bool>> = services.iter().cloned().map(optional).collect();
    assert_eq!(
        found,
        [
            vec![false],
            vec![true, false],
            vec![false],
            vec![false, true, true],
            vec![true],
            vec![true],
            vec![true],
        ]
    );
    for service in &services {
        assert_eq!(
            service.credentials().len(),
            optional(service.clone()).len(),
            "{service:?}"
        );
    }
}

#[test]
fn only_a_chat_service_joins_a_voice_chat() {
    let chats: Vec<AccountKind> = every_service()
        .iter()
        .map(AccountService::kind)
        .filter(|kind| kind.chat())
        .collect();
    assert_eq!(
        chats,
        [
            AccountKind::Discord,
            AccountKind::Teamspeak,
            AccountKind::Matrix
        ]
    );
}

#[test]
fn a_stored_webhook_url_still_has_to_be_https() {
    let webhook = AccountService::Webhook {
        url: Secret::default(),
    };
    assert!(webhook.accepts_secret("https://hooks.example/abc"));
    assert!(!webhook.accepts_secret("http://hooks.example/abc"));
    let matrix = matrix(MatrixLogin::Token);
    assert!(matrix.accepts_secret("token"));
    assert!(!matrix.accepts_secret(&"t".repeat(MAX_SECRET_LEN + 1)));
}

#[test]
fn a_secret_never_prints_its_value() {
    assert_eq!(format!("{:?}", inline("hunter2")), "Inline([redacted])");
    assert_eq!(format!("{:?}", Secret::default()), "Unset");
}

#[test]
fn old_matrix_account_loads_as_token_login() {
    let account: Account = serde_json::from_value(serde_json::json!({
        "id": "m", "name": "Room", "service": "matrix",
        "homeserver_url": "https://matrix.example", "access_token": {"secret": "mx_1"}
    }))
    .expect("parsed");
    assert_eq!(account.check(), Ok(()));
    let AccountService::Matrix {
        login,
        user,
        session,
        store_key,
        ..
    } = &account.service
    else {
        panic!("not matrix");
    };
    assert_eq!(*login, MatrixLogin::Token);
    assert!(user.is_empty() && !session.is_set() && !store_key.is_set());
}

#[test]
fn matrix_password_login_needs_user_and_password() {
    let with = |user: &str, password: Secret| {
        let mut service = matrix(MatrixLogin::Password);
        if let AccountService::Matrix {
            user: held,
            password: kept,
            ..
        } = &mut service
        {
            user.clone_into(held);
            *kept = password;
        }
        service.check()
    };
    assert_eq!(with("@bot:matrix.example", inline("pw")), Ok(()));
    assert_eq!(
        with("@bot:matrix.example", Secret::default()),
        Err("password")
    );
    for user in ["", "bot", "@bot", "@:x", "@bot:", "@b ot:x"] {
        assert_eq!(with(user, inline("pw")), Err("user"), "{user}");
    }
    let long = format!("@{}:x", "b".repeat(MAX_MATRIX_USER_LEN));
    assert_eq!(with(&long, inline("pw")), Err("user"));
    assert_eq!(matrix(MatrixLogin::Oauth).check(), Ok(()));
    let mut nameless = matrix(MatrixLogin::Oauth);
    if let AccountService::Matrix { user, .. } = &mut nameless {
        user.clear();
    }
    assert_eq!(nameless.check(), Err("user"));
}

#[test]
fn managed_secrets_are_not_credentials() {
    let mut service = matrix(MatrixLogin::Oauth);
    if let AccountService::Matrix {
        session, store_key, ..
    } = &mut service
    {
        *session = Secret::stored("ses_1");
        *store_key = Secret::stored("key_1");
    }
    let managed = [Secret::stored("ses_1"), Secret::stored("key_1")];
    assert_eq!(service.managed(), managed.iter().collect::<Vec<_>>());
    assert!(
        service
            .credentials()
            .iter()
            .all(|secret| !managed.contains(secret))
    );
    let written: Vec<bool> = service
        .credentials_mut()
        .into_iter()
        .map(|(_, optional)| optional)
        .collect();
    assert_eq!(written, [true, true, true]);
    assert!(every_service()[0].managed().is_empty());
}

#[test]
fn a_sign_in_state_is_tagged() {
    let pending = MatrixSignInState::Pending(MatrixSignInPrompt {
        verification_uri: "https://auth.example/link".to_owned(),
        user_code: "ABCD".to_owned(),
        expires_in_s: 600,
    });
    let json = serde_json::to_value(&pending).expect("json");
    assert_eq!(json["state"], "pending");
    assert_eq!(json["user_code"], "ABCD");
    assert_eq!(
        serde_json::to_value(MatrixSignInState::Idle).expect("json")["state"],
        "idle"
    );
}
