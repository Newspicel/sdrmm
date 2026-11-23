use sdrmm_wire::{Account, AccountsResponse, MatrixSignInPrompt, MatrixSignInState};

use super::{ApiError, AppError, AppState, Json, OpenApiRouter, Path, State, StatusCode, routes};
use crate::voice_chat::SignInError;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_accounts, create_account))
        .routes(routes!(update_account, delete_account))
        .routes(routes!(start_sign_in, sign_in_state))
}

#[utoipa::path(
    get, path = "/api/accounts",
    responses((
        status = 200,
        description = "Service logins. Credentials answer as ids, never as values",
        body = AccountsResponse,
    )),
)]
pub(super) async fn list_accounts(
    State(state): State<AppState>,
) -> Result<Json<AccountsResponse>, AppError> {
    let store = state.store.clone();
    let accounts = tokio::task::spawn_blocking(move || store.accounts()).await??;
    Ok(Json(AccountsResponse { accounts }))
}

#[utoipa::path(
    post, path = "/api/accounts",
    request_body = Account,
    responses(
        (
            status = 200,
            description = "Kept under a fresh id, the id in the request is ignored",
            body = Account,
        ),
        (status = 400, description = "Unusable name, address or credential", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn create_account(
    State(state): State<AppState>,
    Json(request): Json<Account>,
) -> Result<Json<Account>, AppError> {
    let store = state.store.clone();
    let account = tokio::task::spawn_blocking(move || store.create_account(request)).await??;
    Ok(Json(account))
}

#[utoipa::path(
    put, path = "/api/accounts/{id}",
    params(("id" = String, Path, description = "Account id")),
    request_body = Account,
    responses(
        (
            status = 200,
            description = "Replaced. A credential left stored or empty is kept",
            body = Account,
        ),
        (status = 400, description = "Unusable name, address or credential", body = ApiError),
        (status = 404, description = "Account not found", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn update_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<Account>,
) -> Result<Json<Account>, AppError> {
    let store = state.store.clone();
    let account = tokio::task::spawn_blocking(move || store.update_account(&id, request)).await??;
    Ok(Json(account))
}

#[utoipa::path(
    delete, path = "/api/accounts/{id}",
    params(("id" = String, Path, description = "Account id")),
    responses(
        (status = 204, description = "Removed with its credentials"),
        (status = 404, description = "Account not found", body = ApiError),
    ),
)]
pub(super) async fn delete_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let store = state.store.clone();
    let deleted = id.clone();
    tokio::task::spawn_blocking(move || store.delete_account(&deleted)).await??;
    state.chat_sessions.forget(&id);
    if let Some(dir) = state.chat_sessions.store_dir(&id) {
        remove_store(&dir).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_store(dir: &std::path::Path) -> Result<(), AppError> {
    match tokio::fs::remove_dir_all(dir).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::internal(format!(
            "Matrix store not removed: {error}"
        ))),
    }
}

async fn find(state: &AppState, id: String) -> Result<Account, AppError> {
    let store = state.store.clone();
    let missing = format!("account {id} not found");
    tokio::task::spawn_blocking(move || store.account(&id))
        .await??
        .ok_or_else(|| AppError::not_found(missing))
}

#[utoipa::path(
    post, path = "/api/accounts/{id}/sign-in",
    params(("id" = String, Path, description = "Account id")),
    responses(
        (status = 200, description = "Open the link and enter the code", body = MatrixSignInPrompt),
        (status = 404, description = "Account not found", body = ApiError),
        (status = 409, description = "Not a Matrix OAuth account", body = ApiError),
        (status = 503, description = "The homeserver refused", body = ApiError),
    ),
)]
pub(super) async fn start_sign_in(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MatrixSignInPrompt>, AppError> {
    let account = find(&state, id).await?;
    match state.chat_sessions.clone().sign_in(account).await {
        Ok(prompt) => Ok(Json(prompt)),
        Err(SignInError::NotOAuth) => Err(AppError::conflict("Not a Matrix OAuth account")),
        Err(SignInError::Refused(reason)) => Err(AppError::unavailable(reason)),
    }
}

#[utoipa::path(
    get, path = "/api/accounts/{id}/sign-in",
    params(("id" = String, Path, description = "Account id")),
    responses(
        (status = 200, description = "Where the sign-in stands", body = MatrixSignInState),
        (status = 404, description = "Account not found", body = ApiError),
    ),
)]
pub(super) async fn sign_in_state(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MatrixSignInState>, AppError> {
    let account = find(&state, id).await?;
    Ok(Json(state.chat_sessions.sign_in_state(&account.id)))
}
