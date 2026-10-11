use super::*;

#[utoipa::path(
    get, path = "/api/satellites",
    params(SatelliteCatalogQuery),
    responses(
        (status = 200, description = "Element sets matching a name or catalog number, or the \
                                      amateur and weather groups when `q` is empty. Both groups \
                                      are downloaded twice a day and kept on disk",
                                      body = SatelliteCatalogResponse),
        (status = 400, description = "A search with characters no satellite name has", body = ApiError),
        (status = 503, description = "The element set source could not be reached", body = ApiError),
    ),
)]
pub(super) async fn search_satellites(
    State(state): State<AppState>,
    Query(query): Query<SatelliteCatalogQuery>,
) -> Result<Json<SatelliteCatalogResponse>, AppError> {
    let query = query.q.unwrap_or_default();
    if query.trim().len() > MAX_SATELLITE_QUERY_LEN {
        return Err(AppError::bad_request(format!(
            "a search is at most {MAX_SATELLITE_QUERY_LEN} characters"
        )));
    }
    state
        .satellites
        .catalog
        .search(&state.reference, &query)
        .await
        .map(Json)
        .map_err(AppError::unavailable)
}

#[utoipa::path(
    get, path = "/api/satellites/{catalog}/transmitters",
    params(("catalog" = String, Path, description = "NORAD catalog number")),
    responses(
        (status = 200, description = "Known transmitters of the satellite, live ones first. \
                                      Downloaded daily and kept on disk", body = TransmittersResponse),
        (status = 400, description = "A catalog number that is not digits", body = ApiError),
        (status = 503, description = "The transmitter database could not be reached", body = ApiError),
    ),
)]
pub(super) async fn satellite_transmitters(
    State(state): State<AppState>,
    Path(catalog): Path<String>,
) -> Result<Json<TransmittersResponse>, AppError> {
    if catalog.is_empty() || !catalog.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AppError::bad_request("a catalog number is digits only"));
    }
    state
        .satellites
        .catalog
        .transmitters(&state.reference, &catalog)
        .await
        .map(Json)
        .map_err(AppError::unavailable)
}
