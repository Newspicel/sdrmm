use sdrmm_wire::{ApiError, ServerOutputsResponse};

use super::{AppError, Json};

#[utoipa::path(
    get, path = "/api/audio/outputs",
    responses(
        (status = 200, description = "Sound cards a speaker can play on at the server", body = ServerOutputsResponse),
        (status = 500, description = "The server could not list its sound cards", body = ApiError),
    ),
)]
pub(super) async fn get_server_outputs() -> Result<Json<ServerOutputsResponse>, AppError> {
    let outputs = tokio::task::spawn_blocking(sdrmm_audio_out::outputs)
        .await?
        .map_err(|error| AppError::internal(error.to_string()))?;
    Ok(Json(ServerOutputsResponse { outputs }))
}
