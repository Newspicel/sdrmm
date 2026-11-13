use sdrmm_wire::event_output::{MAX_OUTPUT_SECRET_LEN, Secret, SecretValue, valid_secret_id};

use super::*;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_secret))
        .routes(routes!(put_secret))
}

fn checked(value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::bad_request("the value is empty".to_owned()));
    }
    if value.len() > MAX_OUTPUT_SECRET_LEN {
        return Err(AppError::bad_request(format!(
            "the value is longer than {MAX_OUTPUT_SECRET_LEN} bytes"
        )));
    }
    Ok(())
}

#[utoipa::path(
    post, path = "/api/secrets",
    request_body = SecretValue,
    responses(
        (
            status = 200,
            description = "Kept on this server under a fresh id. A workspace names the id, never \
                           the value",
            body = Secret,
        ),
        (status = 400, description = "Empty or too long", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn create_secret(
    State(state): State<AppState>,
    Json(req): Json<SecretValue>,
) -> Result<Json<Secret>, AppError> {
    checked(&req.value)?;
    let store = state.store.clone();
    let id = tokio::task::spawn_blocking(move || store.create_secret(&req.value)).await??;
    Ok(Json(Secret::stored(id)))
}

#[utoipa::path(
    put, path = "/api/secrets/{id}",
    params(("id" = String, Path, description = "Secret id a workspace names")),
    request_body = SecretValue,
    responses(
        (status = 204, description = "Kept on this server under that id"),
        (status = 400, description = "Unusable id, or the value is empty or too long", body = ApiError),
        (status = 422, description = "Malformed request body", body = ApiError),
    ),
)]
pub(super) async fn put_secret(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SecretValue>,
) -> Result<StatusCode, AppError> {
    if !valid_secret_id(&id) {
        return Err(AppError::bad_request(format!("{id:?} is not a secret id")));
    }
    checked(&req.value)?;
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.put_secret(&id, &req.value)).await??;
    Ok(StatusCode::NO_CONTENT)
}
