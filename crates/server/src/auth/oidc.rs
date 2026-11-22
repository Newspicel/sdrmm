use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use openidconnect::{
    AccessToken, AccessTokenHash, AdditionalClaims, AuthorizationCode, ClientId, ClientSecret,
    CsrfToken, EndSessionUrl, EndpointMaybeSet, EndpointNotSet, EndpointSet, HttpRequest,
    HttpResponse, IdToken, IssuerUrl, LogoutRequest, Nonce, OAuth2TokenResponse, PkceCodeChallenge,
    PkceCodeVerifier, PostLogoutRedirectUrl, ProviderMetadataWithLogout, RedirectUrl, Scope,
    TokenResponse, UserInfoUrl,
    core::{
        CoreAuthenticationFlow, CoreClient, CoreGenderClaim, CoreJweContentEncryptionAlgorithm,
        CoreJwsSigningAlgorithm,
    },
};
use sdrmm_wire::UserRole;
use serde_json::{Map, Value};

use super::sessions::User;

pub(crate) const LOGIN_COOKIE: &str = "sdrmm_login";
pub(crate) const CALLBACK_PATH: &str = "/api/auth/callback";
pub(crate) const PENDING_LIFETIME: Duration = Duration::from_mins(10);
const PENDING_CAP: usize = 256;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const WANTED_SCOPES: [&str; 3] = ["profile", "email", "groups"];

/// How the server signs people in with an OIDC identity provider.
#[derive(Clone, Debug)]
pub struct OidcOptions {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    /// The address browsers open this server at, where the provider sends them back.
    pub public_url: url::Url,
    /// The claim that lists a user's groups.
    pub groups_claim: String,
    pub admin_groups: Vec<String>,
    pub operator_groups: Vec<String>,
    pub viewer_groups: Vec<String>,
    /// The role of a user in none of the groups. `None` turns them away.
    pub default_role: Option<UserRole>,
}

impl OidcOptions {
    /// The highest role any of `groups` grants, else the default.
    #[must_use]
    pub fn role_for(&self, groups: &[String]) -> Option<UserRole> {
        let any = |wanted: &[String]| wanted.iter().any(|group| groups.contains(group));
        if any(&self.admin_groups) {
            Some(UserRole::Admin)
        } else if any(&self.operator_groups) {
            Some(UserRole::Operator)
        } else if any(&self.viewer_groups) {
            Some(UserRole::Viewer)
        } else {
            self.default_role
        }
    }

    fn secure(&self) -> bool {
        self.public_url.scheme() == "https"
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SignInError {
    #[error("the identity provider cannot be reached: {0}")]
    Discovery(String),
    #[error("the identity provider answered something unusable: {0}")]
    Provider(String),
    #[error("the sign-in expired or was started in another browser")]
    Stale,
    #[error("the identity provider refused: {0}")]
    Refused(String),
    #[error("the ID token is not valid: {0}")]
    Token(String),
    #[error("{0} is in no group that grants a role")]
    NoRole(String),
}

impl SignInError {
    /// The word the sign-in page shows for this failure.
    pub(crate) const fn reason(&self) -> &'static str {
        match self {
            Self::NoRole(_) => "denied",
            Self::Discovery(_)
            | Self::Provider(_)
            | Self::Stale
            | Self::Refused(_)
            | Self::Token(_) => "failed",
        }
    }
}

/// What the provider adds to the standard claims, kept whole because the groups claim is
/// named by the operator.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct ExtraClaims {
    #[serde(flatten)]
    claims: Map<String, Value>,
}

impl AdditionalClaims for ExtraClaims {}

type Token = IdToken<
    ExtraClaims,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJwsSigningAlgorithm,
>;

type Client = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

struct Provider {
    client: Client,
    algs: Vec<CoreJwsSigningAlgorithm>,
    scopes: Vec<String>,
    userinfo: Option<UserInfoUrl>,
    end_session: Option<EndSessionUrl>,
}

struct Pending {
    nonce: Nonce,
    verifier: PkceCodeVerifier,
    next: String,
    at: Instant,
}

/// What the provider sends the browser back with.
#[derive(Debug, Default, serde::Deserialize, utoipa::IntoParams)]
pub(crate) struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub(crate) struct Started {
    pub(crate) url: url::Url,
    pub(crate) state: String,
}

pub(crate) struct Finished {
    pub(crate) user: User,
    pub(crate) id_token: String,
    pub(crate) next: String,
}

/// Signs people in with one provider. Its metadata is read again for every sign-in, so a
/// rotated signing key or a provider that was down at startup needs no restart.
pub(crate) struct Oidc {
    options: OidcOptions,
    http: reqwest::Client,
    pending: Mutex<HashMap<String, Pending>>,
}

impl Oidc {
    pub(crate) fn new(options: OidcOptions) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(HTTP_TIMEOUT)
            .build()?;
        Ok(Self {
            options,
            http,
            pending: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn secure(&self) -> bool {
        self.options.secure()
    }

    /// Answers where to send the browser, and the state its callback must carry.
    pub(crate) async fn start(&self, next: Option<&str>) -> Result<Started, SignInError> {
        let provider = self.discover().await?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let mut request = provider.client.authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        );
        for scope in &provider.scopes {
            request = request.add_scope(Scope::new(scope.clone()));
        }
        let (url, state, nonce) = request.set_pkce_challenge(challenge).url();
        self.remember(
            state.secret().clone(),
            Pending {
                nonce,
                verifier,
                next: safe_next(next),
                at: Instant::now(),
            },
        )?;
        Ok(Started {
            url,
            state: state.secret().clone(),
        })
    }

    /// Checks the provider's answer and names the user it vouches for. `browser` is the
    /// state the login cookie holds, so a callback link another person started signs no one in.
    pub(crate) async fn finish(
        &self,
        callback: Callback,
        browser: Option<&str>,
    ) -> Result<Finished, SignInError> {
        let state = callback.state.ok_or(SignInError::Stale)?;
        if browser.is_none_or(|held| !super::bytes_eq(held.as_bytes(), state.as_bytes())) {
            return Err(SignInError::Stale);
        }
        let pending = self.take(&state).ok_or(SignInError::Stale)?;
        if let Some(error) = callback.error {
            let detail = callback.error_description.unwrap_or_default();
            return Err(SignInError::Refused(
                format!("{error} {detail}").trim().to_owned(),
            ));
        }
        let code = callback
            .code
            .ok_or_else(|| SignInError::Provider("no code".into()))?;
        let provider = self.discover().await?;
        let http = self.client();
        let response = provider
            .client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(|error| SignInError::Provider(error.to_string()))?
            .set_pkce_verifier(pending.verifier)
            .request_async(&http)
            .await
            .map_err(|error| SignInError::Refused(error.to_string()))?;
        let raw = response
            .id_token()
            .ok_or_else(|| SignInError::Provider("no ID token".into()))?
            .to_string();
        let token: Token = raw
            .parse()
            .map_err(|error: serde_json::Error| SignInError::Token(error.to_string()))?;
        let user = self
            .verify(&provider, &token, &pending.nonce, response.access_token())
            .await?;
        Ok(Finished {
            user,
            id_token: raw,
            next: pending.next,
        })
    }

    /// The provider's logout page for a session opened with `id_token`, if the provider has one.
    pub(crate) async fn logout_url(&self, id_token: Option<&str>) -> Option<String> {
        let provider = self
            .discover()
            .await
            .map_err(|error| tracing::warn!(%error, "cannot reach the provider's logout page"))
            .ok()?;
        let mut request = LogoutRequest::from(provider.end_session?)
            .set_client_id(ClientId::new(self.options.client_id.clone()))
            .set_post_logout_redirect_uri(PostLogoutRedirectUrl::from_url(
                self.options.public_url.clone(),
            ));
        if let Some(token) = id_token.and_then(|raw| raw.parse::<Token>().ok()) {
            request = request.set_id_token_hint(&token);
        }
        Some(request.http_get_url().to_string())
    }

    async fn verify(
        &self,
        provider: &Provider,
        token: &Token,
        nonce: &Nonce,
        access: &AccessToken,
    ) -> Result<User, SignInError> {
        let verifier = provider
            .client
            .id_token_verifier()
            .set_allowed_algs(provider.algs.iter().cloned());
        let claims = token
            .claims(&verifier, nonce)
            .map_err(|error| SignInError::Token(error.to_string()))?;
        if let Some(expected) = claims.access_token_hash() {
            let alg = token
                .signing_alg()
                .map_err(|error| SignInError::Token(error.to_string()))?;
            let key = token
                .signing_key(&verifier)
                .map_err(|error| SignInError::Token(error.to_string()))?;
            let actual = AccessTokenHash::from_token(access, alg, key)
                .map_err(|error| SignInError::Token(error.to_string()))?;
            if actual != *expected {
                return Err(SignInError::Token("the access token was swapped".into()));
            }
        }
        let subject = claims.subject().to_string();
        let groups = match groups_in(
            &claims.additional_claims().claims,
            &self.options.groups_claim,
        ) {
            Some(groups) => groups,
            None => self.userinfo_groups(provider, access, &subject).await?,
        };
        let name = claims
            .name()
            .and_then(|name| name.get(None))
            .map(|name| name.to_string())
            .or_else(|| claims.preferred_username().map(|name| name.to_string()))
            .or_else(|| claims.email().map(|email| email.to_string()))
            .unwrap_or_else(|| subject.clone());
        let role = self
            .options
            .role_for(&groups)
            .ok_or_else(|| SignInError::NoRole(name.clone()))?;
        Ok(User {
            subject,
            name,
            email: claims.email().map(|email| email.to_string()),
            role,
        })
    }

    /// The groups from the userinfo endpoint, where some providers keep them out of the ID
    /// token.
    async fn userinfo_groups(
        &self,
        provider: &Provider,
        access: &AccessToken,
        subject: &str,
    ) -> Result<Vec<String>, SignInError> {
        let Some(url) = &provider.userinfo else {
            return Ok(Vec::new());
        };
        let unusable = |error: reqwest::Error| SignInError::Provider(error.to_string());
        let body: Map<String, Value> = self
            .http
            .get(url.url().clone())
            .bearer_auth(access.secret())
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(unusable)?
            .json()
            .await
            .map_err(unusable)?;
        if body.get("sub").and_then(Value::as_str) != Some(subject) {
            return Err(SignInError::Provider("userinfo names someone else".into()));
        }
        Ok(groups_in(&body, &self.options.groups_claim).unwrap_or_default())
    }

    async fn discover(&self) -> Result<Provider, SignInError> {
        let issuer = IssuerUrl::new(self.options.issuer.clone())
            .map_err(|error| SignInError::Discovery(error.to_string()))?;
        let http = self.client();
        let metadata = ProviderMetadataWithLogout::discover_async(issuer, &http)
            .await
            .map_err(|error| SignInError::Discovery(error.to_string()))?;
        let redirect = self
            .options
            .public_url
            .join(CALLBACK_PATH)
            .map_err(|error| SignInError::Discovery(error.to_string()))?;
        let supported = metadata.scopes_supported().cloned().unwrap_or_default();
        let scopes = WANTED_SCOPES
            .into_iter()
            .filter(|wanted| {
                supported.is_empty() && *wanted != "groups"
                    || supported.iter().any(|scope| scope.as_str() == *wanted)
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let algs = metadata
            .id_token_signing_alg_values_supported()
            .iter()
            .filter(|alg| asymmetric(alg))
            .cloned()
            .collect();
        let userinfo = metadata.userinfo_endpoint().cloned();
        let end_session = metadata.additional_metadata().end_session_endpoint.clone();
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.options.client_id.clone()),
            self.options.client_secret.clone().map(ClientSecret::new),
        )
        .set_redirect_uri(RedirectUrl::from_url(redirect));
        Ok(Provider {
            client,
            algs,
            scopes,
            userinfo,
            end_session,
        })
    }

    /// The HTTP client the library calls, owning its handle so its futures borrow nothing.
    fn client(&self) -> impl Fn(HttpRequest) -> Sent + Send + Sync + use<> {
        let http = self.http.clone();
        move |request| {
            let http = http.clone();
            Box::pin(async move { send(&http, request).await })
        }
    }

    fn remember(&self, state: String, pending: Pending) -> Result<(), SignInError> {
        let mut held = self.lock();
        held.retain(|_, waiting| waiting.at.elapsed() < PENDING_LIFETIME);
        if held.len() >= PENDING_CAP {
            return Err(SignInError::Provider("too many sign-ins at once".into()));
        }
        held.insert(state, pending);
        Ok(())
    }

    fn take(&self, state: &str) -> Option<Pending> {
        self.lock()
            .remove(state)
            .filter(|pending| pending.at.elapsed() < PENDING_LIFETIME)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Pending>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Keeps a return path on this server: a relative path, never another origin.
fn safe_next(next: Option<&str>) -> String {
    next.filter(|path| path.starts_with('/') && !path.starts_with("//") && !path.contains('\\'))
        .unwrap_or("/")
        .to_owned()
}

const fn asymmetric(alg: &CoreJwsSigningAlgorithm) -> bool {
    !matches!(
        alg,
        CoreJwsSigningAlgorithm::HmacSha256
            | CoreJwsSigningAlgorithm::HmacSha384
            | CoreJwsSigningAlgorithm::HmacSha512
            | CoreJwsSigningAlgorithm::None
    )
}

/// The groups a claim lists: an array of names, or one name.
fn groups_in(claims: &Map<String, Value>, claim: &str) -> Option<Vec<String>> {
    match claims.get(claim)? {
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        ),
        Value::String(one) => Some(vec![one.clone()]),
        _ => None,
    }
}

type Sent =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<HttpResponse, HttpError>> + Send>>;

#[derive(Debug, thiserror::Error)]
enum HttpError {
    #[error(transparent)]
    Send(#[from] reqwest::Error),
    #[error(transparent)]
    Build(#[from] axum::http::Error),
}

async fn send(client: &reqwest::Client, request: HttpRequest) -> Result<HttpResponse, HttpError> {
    let (parts, body) = request.into_parts();
    let response = client
        .request(parts.method, parts.uri.to_string())
        .headers(parts.headers)
        .body(body)
        .send()
        .await?;
    let mut answer = axum::http::Response::builder().status(response.status());
    for (name, value) in response.headers() {
        answer = answer.header(name, value);
    }
    Ok(answer.body(response.bytes().await?.to_vec())?)
}

#[cfg(test)]
mod tests;
