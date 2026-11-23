use sdrmm_wire::{SpeechModel, SpeechModelsResponse};

use super::{ApiError, AppError, AppState, Json, OpenApiRouter, Path, State, StatusCode, routes};

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_speech_models))
        .routes(routes!(download_speech_model, delete_speech_model))
}

fn parse(name: &str) -> Result<SpeechModel, AppError> {
    SpeechModel::from_name(name).ok_or_else(|| AppError::not_found(format!("no model {name}")))
}

#[utoipa::path(
    get, path = "/api/speech-models",
    responses((status = 200, description = "Every speech model and whether it is here", body = SpeechModelsResponse)),
)]
pub(super) async fn list_speech_models(
    State(state): State<AppState>,
) -> Json<SpeechModelsResponse> {
    Json(SpeechModelsResponse {
        models: state.speech.statuses(),
    })
}

#[utoipa::path(
    post, path = "/api/speech-models/{model}",
    params(("model" = String, Path, description = "Model name")),
    responses(
        (status = 202, description = "Download started"),
        (status = 404, description = "No such model", body = ApiError),
        (status = 409, description = "Already downloading", body = ApiError),
        (status = 503, description = "No data directory to keep models in", body = ApiError),
    ),
)]
pub(super) async fn download_speech_model(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    let model = parse(&name)?;
    if state.speech.path(model).is_none() {
        return Err(AppError::unavailable("no data directory to keep models in"));
    }
    if !state.speech.start(model) {
        return Err(AppError::conflict(format!("{name} is already downloading")));
    }
    Ok(StatusCode::ACCEPTED)
}

#[utoipa::path(
    delete, path = "/api/speech-models/{model}",
    params(("model" = String, Path, description = "Model name")),
    responses(
        (status = 204, description = "Model removed"),
        (status = 404, description = "No such model", body = ApiError),
    ),
)]
pub(super) async fn delete_speech_model(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    let model = parse(&name)?;
    state.speech.remove(model).map_err(AppError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}
