use axum::{
    Extension,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{AppendHeaders, IntoResponse, Response},
};
use sdrmm_wire::{ErrorCode, Me, SignedOut};

use super::{AppError, AppState, Json, Query};
use crate::auth::{
    Callback, Identity, LOGIN_COOKIE, PENDING_LIFETIME, SESSION_COOKIE, clear_cookie, cookie,
    session_max_age, set_cookie,
};

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub(super) struct SignInQuery {
    /// The path to return to once signed in.
    next: Option<String>,
}

#[utoipa::path(
    get, path = "/api/auth/login",
    params(SignInQuery),
    responses(
        (status = 303, description = "To the identity provider"),
        (status = 404, description = "Sign-in is off", body = sdrmm_wire::ApiError),
    ),
)]
pub(super) async fn sign_in(
    State(state): State<AppState>,
    Query(query): Query<SignInQuery>,
) -> Result<Response, AppError> {
    let oidc = state.auth.oidc().ok_or_else(sign_in_off)?;
    let secure = oidc.secure();
    Ok(match oidc.start(query.next.as_deref()).await {
        Ok(started) => (
            StatusCode::SEE_OTHER,
            AppendHeaders([
                (header::LOCATION, started.url.to_string()),
                (
                    header::SET_COOKIE,
                    set_cookie(LOGIN_COOKIE, &started.state, PENDING_LIFETIME, secure),
                ),
            ]),
        )
            .into_response(),
        Err(error) => {
            tracing::warn!(%error, "sign-in could not start");
            back_home(error.reason(), secure)
        }
    })
}

#[utoipa::path(
    get, path = "/api/auth/callback",
    params(Callback),
    responses(
        (status = 303, description = "Signed in, back to the page that asked"),
        (status = 404, description = "Sign-in is off", body = sdrmm_wire::ApiError),
    ),
)]
pub(super) async fn sign_in_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(callback): Query<Callback>,
) -> Result<Response, AppError> {
    let oidc = state.auth.oidc().ok_or_else(sign_in_off)?;
    let secure = oidc.secure();
    let finished = match oidc.finish(callback, cookie(&headers, LOGIN_COOKIE)).await {
        Ok(finished) => finished,
        Err(error) => {
            tracing::warn!(%error, "sign-in failed");
            return Ok(back_home(error.reason(), secure));
        }
    };
    let next = finished.next;
    let sessions = state.auth.sessions.clone();
    let opened = tokio::task::spawn_blocking(move || {
        tracing::info!(user = %finished.user.name, role = finished.user.role.as_str(), "signed in");
        sessions.open(finished.user, Some(finished.id_token))
    })
    .await
    .map_err(|error| AppError::internal(error.to_string()))?
    .map_err(|error| AppError::internal(format!("session not kept: {error}")))?;
    Ok((
        StatusCode::SEE_OTHER,
        AppendHeaders([
            (header::LOCATION, next),
            (
                header::SET_COOKIE,
                set_cookie(SESSION_COOKIE, &opened, session_max_age(), secure),
            ),
            (header::SET_COOKIE, clear_cookie(LOGIN_COOKIE, secure)),
        ]),
    )
        .into_response())
}

#[utoipa::path(
    post, path = "/api/auth/logout",
    responses((status = 200, description = "Signed out", body = SignedOut)),
)]
pub(super) async fn sign_out(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let oidc = state.auth.oidc().cloned();
    let secure = oidc.as_ref().is_some_and(|oidc| oidc.secure());
    let id_token = match cookie(&headers, SESSION_COOKIE).map(str::to_owned) {
        Some(session) => {
            let sessions = state.auth.sessions.clone();
            tokio::task::spawn_blocking(move || sessions.close(&session))
                .await
                .map_err(|error| AppError::internal(error.to_string()))??
        }
        None => None,
    };
    let redirect = match oidc {
        Some(oidc) => oidc.logout_url(id_token.as_deref()).await,
        None => None,
    };
    Ok((
        AppendHeaders([(header::SET_COOKIE, clear_cookie(SESSION_COOKIE, secure))]),
        Json(SignedOut { redirect }),
    )
        .into_response())
}

#[utoipa::path(
    get, path = "/api/me",
    responses(
        (status = 200, description = "Who the caller is and what they may do", body = Me),
        (status = 403, description = "A phone has no role", body = sdrmm_wire::ApiError),
    ),
)]
pub(super) async fn get_me(Extension(identity): Extension<Identity>) -> Result<Json<Me>, AppError> {
    identity
        .me()
        .map(Json)
        .ok_or_else(|| AppError::new(StatusCode::FORBIDDEN, ErrorCode::Auth, "No role".to_owned()))
}

fn sign_in_off() -> AppError {
    AppError::not_found("Sign-in is off".to_owned())
}

fn back_home(reason: &str, secure: bool) -> Response {
    (
        StatusCode::SEE_OTHER,
        AppendHeaders([
            (header::LOCATION, format!("/?sign_in={reason}")),
            (header::SET_COOKIE, clear_cookie(LOGIN_COOKIE, secure)),
        ]),
    )
        .into_response()
}
