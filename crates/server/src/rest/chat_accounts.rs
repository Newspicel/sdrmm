use sdrmm_wire::{ChatAccount, ChatAccountsResponse};

use super::{ApiError, AppError, AppState, Json, OpenApiRouter, Path, State, StatusCode, routes};

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_chat_accounts, create_chat_account))
        .routes(routes!(update_chat_account, delete_chat_account))
}

#[utoipa::path(
    get, path = "/api/chat-accounts",
    responses((
        status = 200,
        description = "Chat accounts. Credentials answer as ids, never as values",
        body = ChatAccountsResponse,
    )),
)]
pub(super) async fn list_chat_accounts(
    State(state): State<AppState>,
) -> Result<Json<ChatAccountsResponse>, AppError> {
    let store = state.store.clone();
    let accounts = tokio::task::spawn_blocking(move || store.chat_accounts()).await??;
    Ok(Json(ChatAccountsResponse { accounts }))
}

#[utoipa::path(
    post, path = "/api/chat-accounts",
    request_body = ChatAccount,
    responses(
        (
            status = 200,
            description = "Kept under a fresh id, the id in the request is ignored",
            body = ChatAccount,
        ),
        (status = 400, description = "Unusable name, server or credential", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn create_chat_account(
    State(state): State<AppState>,
    Json(request): Json<ChatAccount>,
) -> Result<Json<ChatAccount>, AppError> {
    let store = state.store.clone();
    let account = tokio::task::spawn_blocking(move || store.create_chat_account(request)).await??;
    Ok(Json(account))
}

#[utoipa::path(
    put, path = "/api/chat-accounts/{id}",
    params(("id" = String, Path, description = "Chat account id")),
    request_body = ChatAccount,
    responses(
        (
            status = 200,
            description = "Replaced. A credential left stored or empty is kept",
            body = ChatAccount,
        ),
        (status = 400, description = "Unusable name, server or credential", body = ApiError),
        (status = 404, description = "Chat account not found", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn update_chat_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ChatAccount>,
) -> Result<Json<ChatAccount>, AppError> {
    let store = state.store.clone();
    let account =
        tokio::task::spawn_blocking(move || store.update_chat_account(&id, request)).await??;
    Ok(Json(account))
}

#[utoipa::path(
    delete, path = "/api/chat-accounts/{id}",
    params(("id" = String, Path, description = "Chat account id")),
    responses(
        (status = 204, description = "Removed with its credentials"),
        (status = 404, description = "Chat account not found", body = ApiError),
    ),
)]
pub(super) async fn delete_chat_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.delete_chat_account(&id)).await??;
    Ok(StatusCode::NO_CONTENT)
}
