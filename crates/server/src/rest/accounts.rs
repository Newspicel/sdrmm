use sdrmm_wire::{Account, AccountsResponse};

use super::{ApiError, AppError, AppState, Json, OpenApiRouter, Path, State, StatusCode, routes};

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_accounts, create_account))
        .routes(routes!(update_account, delete_account))
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
    tokio::task::spawn_blocking(move || store.delete_account(&id)).await??;
    Ok(StatusCode::NO_CONTENT)
}
