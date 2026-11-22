use std::sync::{Arc, Mutex};

use axum::{
    Form, Json as AxumJson, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use openidconnect::{
    IdTokenClaims, PrivateSigningKey,
    core::{CoreEdDsaPrivateSigningKey, CoreJsonWebKeySet},
};
use serde_json::json;

use super::*;

const KEY: &str = "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEIMr02QeJVKJ15aT8erNpSUfeZEC0WS5juwzR/MyML0LH
-----END PRIVATE KEY-----";
const CLIENT: &str = "sdrmm";
const ACCESS: &str = "access-token";

#[derive(Clone, Default)]
struct Idp {
    issuer: String,
    nonce: Arc<Mutex<Option<String>>>,
    groups: Vec<String>,
    groups_in_userinfo: bool,
}

fn key() -> CoreEdDsaPrivateSigningKey {
    CoreEdDsaPrivateSigningKey::from_ed25519_pem(
        KEY,
        Some(openidconnect::JsonWebKeyId::new("k1".into())),
    )
    .expect("test key")
}

async fn discovery(State(idp): State<Idp>) -> AxumJson<Value> {
    let base = &idp.issuer;
    AxumJson(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "userinfo_endpoint": format!("{base}/userinfo"),
        "jwks_uri": format!("{base}/jwks"),
        "end_session_endpoint": format!("{base}/logout"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["EdDSA", "HS256"],
        "scopes_supported": ["openid", "profile", "email", "groups"],
    }))
}

async fn jwks() -> AxumJson<CoreJsonWebKeySet> {
    AxumJson(CoreJsonWebKeySet::new(vec![key().as_verification_key()]))
}

async fn token(
    State(idp): State<Idp>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<AxumJson<Value>, StatusCode> {
    if form.get("code").map(String::as_str) != Some("good-code")
        || !form.contains_key("code_verifier")
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let nonce = idp.nonce.lock().expect("nonce").clone();
    let groups = if idp.groups_in_userinfo {
        json!(null)
    } else {
        json!(idp.groups)
    };
    let now = jiff::Timestamp::now().as_second();
    let claims: IdTokenClaims<ExtraClaims, CoreGenderClaim> = serde_json::from_value(json!({
        "iss": idp.issuer,
        "aud": [CLIENT],
        "exp": now + 300,
        "iat": now,
        "sub": "ada-1",
        "name": "Ada Lovelace",
        "email": "ada@example.org",
        "nonce": nonce,
        "groups": groups,
    }))
    .expect("claims");
    let signed = Token::new(
        claims,
        &key(),
        CoreJwsSigningAlgorithm::EdDsa,
        Some(&AccessToken::new(ACCESS.into())),
        None,
    )
    .expect("signed");
    Ok(AxumJson(json!({
        "access_token": ACCESS,
        "token_type": "Bearer",
        "id_token": signed.to_string(),
    })))
}

async fn userinfo(
    State(idp): State<Idp>,
    headers: HeaderMap,
) -> Result<AxumJson<Value>, StatusCode> {
    let bearer = format!("Bearer {ACCESS}");
    if headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        != Some(bearer.as_str())
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(AxumJson(json!({ "sub": "ada-1", "groups": idp.groups })))
}

async fn serve_idp(mut idp: Idp) -> Idp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    idp.issuer = format!("http://{}", listener.local_addr().expect("addr"));
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks", get(jwks))
        .route("/token", post(token))
        .route("/userinfo", get(userinfo))
        .with_state(idp.clone());
    tokio::spawn(async move { axum::serve(listener, app).await });
    idp
}

fn options(issuer: &str) -> OidcOptions {
    OidcOptions {
        issuer: issuer.to_owned(),
        client_id: CLIENT.to_owned(),
        client_secret: Some("secret".to_owned()),
        public_url: "https://sdr.example.org".parse().expect("url"),
        groups_claim: "groups".to_owned(),
        admin_groups: vec!["sdr-admins".to_owned()],
        operator_groups: vec!["radio".to_owned()],
        viewer_groups: Vec::new(),
        default_role: None,
    }
}

async fn signed_in(idp: &Idp) -> Result<Finished, SignInError> {
    let oidc = Oidc::new(options(&idp.issuer)).expect("client");
    let started = oidc.start(Some("/canvas")).await?;
    let query: HashMap<_, _> = started.url.query_pairs().into_owned().collect();
    assert_eq!(query.get("state"), Some(&started.state));
    assert!(
        query
            .get("scope")
            .is_some_and(|scope| scope.contains("groups"))
    );
    assert!(query.contains_key("code_challenge"));
    *idp.nonce.lock().expect("nonce") = query.get("nonce").cloned();
    oidc.finish(callback(&started.state), Some(&started.state))
        .await
}

fn callback(state: &str) -> Callback {
    Callback {
        code: Some("good-code".into()),
        state: Some(state.to_owned()),
        ..Callback::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_of_an_operator_group_signs_in_as_operator() {
    let idp = serve_idp(Idp {
        groups: vec!["radio".into()],
        ..Idp::default()
    })
    .await;
    let finished = signed_in(&idp).await.expect("signed in");
    assert_eq!(finished.user.role, UserRole::Operator);
    assert_eq!(finished.user.name, "Ada Lovelace");
    assert_eq!(finished.user.subject, "ada-1");
    assert_eq!(finished.next, "/canvas");
}

#[tokio::test(flavor = "multi_thread")]
async fn groups_kept_in_userinfo_count_too() {
    let idp = serve_idp(Idp {
        groups: vec!["sdr-admins".into()],
        groups_in_userinfo: true,
        ..Idp::default()
    })
    .await;
    let finished = signed_in(&idp).await.expect("signed in");
    assert_eq!(finished.user.role, UserRole::Admin);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_user_in_no_group_is_turned_away() {
    let idp = serve_idp(Idp {
        groups: vec!["family".into()],
        ..Idp::default()
    })
    .await;
    let refused = signed_in(&idp).await.map(|_| ()).expect_err("no role");
    assert_eq!(refused.reason(), "denied");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_callback_from_another_browser_or_twice_signs_no_one_in() {
    let idp = serve_idp(Idp {
        groups: vec!["radio".into()],
        ..Idp::default()
    })
    .await;
    let oidc = Oidc::new(options(&idp.issuer)).expect("client");
    let started = oidc.start(None).await.expect("started");
    let stranger = oidc.finish(callback(&started.state), Some("other")).await;
    assert!(matches!(stranger, Err(SignInError::Stale)));
    let missing = oidc.finish(callback(&started.state), None).await;
    assert!(matches!(missing, Err(SignInError::Stale)));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_nonce_that_does_not_match_fails() {
    let idp = serve_idp(Idp {
        groups: vec!["radio".into()],
        ..Idp::default()
    })
    .await;
    let oidc = Oidc::new(options(&idp.issuer)).expect("client");
    let started = oidc.start(None).await.expect("started");
    *idp.nonce.lock().expect("nonce") = Some("forged".into());
    let forged = oidc
        .finish(callback(&started.state), Some(&started.state))
        .await
        .map(|_| ())
        .expect_err("forged nonce");
    assert!(matches!(forged, SignInError::Token(_)), "{forged}");
    let again = oidc
        .finish(callback(&started.state), Some(&started.state))
        .await;
    assert!(matches!(again, Err(SignInError::Stale)));
}

#[tokio::test(flavor = "multi_thread")]
async fn logout_goes_through_the_providers_page() {
    let idp = serve_idp(Idp::default()).await;
    let oidc = Oidc::new(options(&idp.issuer)).expect("client");
    let url = oidc.logout_url(None).await.expect("logout page");
    assert!(url.starts_with(&format!("{}/logout?", idp.issuer)), "{url}");
    assert!(
        url.contains("post_logout_redirect_uri=https%3A%2F%2Fsdr.example.org"),
        "{url}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_provider_fails_the_start() {
    let oidc = Oidc::new(options("http://127.0.0.1:1")).expect("client");
    let failed = oidc.start(None).await.map(|_| ()).expect_err("unreachable");
    assert_eq!(failed.reason(), "failed");
}

#[test]
fn the_highest_group_wins_and_the_default_covers_the_rest() {
    let mut options = options("https://idp.example.org");
    let groups = |names: &[&str]| {
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        options.role_for(&groups(&["radio", "sdr-admins"])),
        Some(UserRole::Admin)
    );
    assert_eq!(
        options.role_for(&groups(&["radio"])),
        Some(UserRole::Operator)
    );
    assert_eq!(options.role_for(&groups(&["family"])), None);
    options.default_role = Some(UserRole::Viewer);
    assert_eq!(options.role_for(&[]), Some(UserRole::Viewer));
}

#[test]
fn only_a_path_on_this_server_is_returned_to() {
    assert_eq!(safe_next(Some("/canvas?x=1")), "/canvas?x=1");
    assert_eq!(safe_next(Some("//evil.example")), "/");
    assert_eq!(safe_next(Some("https://evil.example")), "/");
    assert_eq!(safe_next(Some("/\\evil.example")), "/");
    assert_eq!(safe_next(None), "/");
}

#[test]
fn groups_are_read_as_a_list_or_one_name() {
    let claims = |value: Value| {
        json!({ "groups": value })
            .as_object()
            .cloned()
            .expect("object")
    };
    assert_eq!(
        groups_in(&claims(json!(["a", "b", 3])), "groups"),
        Some(vec!["a".to_owned(), "b".to_owned()])
    );
    assert_eq!(
        groups_in(&claims(json!("a")), "groups"),
        Some(vec!["a".to_owned()])
    );
    assert_eq!(groups_in(&claims(json!(null)), "groups"), None);
    assert_eq!(groups_in(&claims(json!(["a"])), "roles"), None);
}
