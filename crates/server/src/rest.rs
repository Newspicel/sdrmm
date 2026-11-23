use axum::{
    body::Body,
    extract::{
        DefaultBodyLimit, FromRequest, FromRequestParts, Multipart, State,
        multipart::Field,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use sdrmm_engine::EngineError;
use sdrmm_recorder::{
    AUDIO_SUFFIX, Export, ExportKind, SigmfError, Stored, read_audio_info, scan_audio, scan_library,
};
use sdrmm_tools::ToolError;
use sdrmm_wire::{
    AboutResponse, ApiError, ApplyTemplateRequest, AudioRecordingInfo, AudioRecordingsResponse,
    AuthInfo, BandPlan, BandRegionMatch, BandRegionsResponse, Bookmark, CapturedImagesQuery,
    CapturedImagesResponse, ChannelSettings, ChannelTypesResponse, ClientCommand, ClientsResponse,
    CreateBookmarkRequest, CreatePresetRequest, CreateWorkspaceRequest, CreatedRowId,
    DecoderLogEntry, DecoderLogGroupsResponse, DecoderLogQuery, DecoderLogResponse, DeletedCount,
    DeviceInfo, DeviceSettings, DevicesResponse, DfFusionView, DiagnosticsReport, DoctorReport,
    EncoderTypesResponse, ErrorCode, ExamplesResponse, ExportFormat, IonosondeReport,
    LicenseTextResponse, LocateQuery, LogGroupKey, MAX_RECORDING_UPLOAD_BYTES,
    MAX_SATELLITE_QUERY_LEN, NodeBody, PRESET_SNAPSHOT_VERSION, PatchApplyReport, PatchCatalog,
    PatchChannelRequest, PatchGraph, PresetDevice, PresetInfo, PresetSnapshot, RecordingAnnotation,
    RecordingDownloadQuery, RecordingFormat, RecordingInfo, RecordingUpload, RecordingsResponse,
    SatelliteCatalogQuery, SatelliteCatalogResponse, SaveRadioRequest, SavedRadio, ScannerStatus,
    SerialDevicesResponse, ServerEvent, ServerStatus, StateScope, StateSnapshot, TemplateInfo,
    TemplatesResponse, ToolRequest, ToolResponse, ToolsResponse, TransmittersResponse,
    TransportInfo, UpdateWorkspaceRequest, WorkspaceDetail, WorkspaceExport, WorkspaceSnapshot,
    WorkspacesResponse, WriteSerialRequest, WrittenSerial,
};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

mod actions;
mod arrays;
mod audio_recordings;
mod chat_accounts;
mod cps;
mod decoderlog;
mod denoise;
mod devices;
mod fusion;
mod info;
mod media;
mod missions;
mod nanovna;
mod nodes;
mod patch;
mod phones;
mod presets;
mod processors;
mod recordings;
mod satellites;
mod secrets;
mod sign_in;
mod sinks;
mod sky_scan;
mod speakers;
mod speech;
mod survey;
mod workspaces;

pub(crate) use actions::{Tuner, act, controlled, tune_node};
use audio_recordings::{
    __path_delete_audio_recording, __path_download_audio_recording, __path_list_audio_recordings,
    __path_play_audio_recording, __path_reveal_audio_recording, delete_audio_recording,
    download_audio_recording, list_audio_recordings, play_audio_recording, reveal_audio_recording,
};
use cps::{
    __path_cancel_cps_job, __path_convert_cps_codeplug, __path_create_cps_codeplug,
    __path_create_cps_device, __path_create_cps_user, __path_delete_cps_codeplug,
    __path_delete_cps_device, __path_delete_cps_user, __path_get_cps_codeplug, __path_get_cps_job,
    __path_get_cps_library, __path_identify_radio, __path_list_cps_jobs, __path_list_cps_ports,
    __path_list_radio_models, __path_merge_cps_codeplug, __path_read_radio,
    __path_update_cps_codeplug, __path_update_cps_device, __path_update_cps_user,
    __path_write_radio, cancel_cps_job, convert_cps_codeplug, create_cps_codeplug,
    create_cps_device, create_cps_user, delete_cps_codeplug, delete_cps_device, delete_cps_user,
    get_cps_codeplug, get_cps_job, get_cps_library, identify_radio, list_cps_jobs, list_cps_ports,
    list_radio_models, merge_cps_codeplug, read_radio, update_cps_codeplug, update_cps_device,
    update_cps_user, write_radio,
};
use decoderlog::{
    __path_clear_decoder_log, __path_export_decoder_log, __path_group_decoder_log,
    __path_list_decoder_log, clear_decoder_log, export_decoder_log, group_decoder_log,
    list_decoder_log,
};
pub(crate) use devices::patch_device_live;
use devices::{
    __path_delete_saved_radio, __path_get_channel_types, __path_get_devices,
    __path_get_encoder_types, __path_get_nmea_devices, __path_get_serial_ports, __path_get_state,
    __path_list_saved_radios, __path_post_channel_band, __path_save_radio, __path_write_serial,
    delete_saved_radio, get_channel_types, get_devices, get_encoder_types, get_nmea_devices,
    get_serial_ports, get_state, list_saved_radios, patch_channel_locked, post_channel_band,
    save_radio, write_serial,
};
use info::{
    __path_get_about, __path_get_auth, __path_get_band_plan, __path_get_clients,
    __path_get_diagnostics, __path_get_doctor, __path_get_ionosonde, __path_get_license_text,
    __path_get_patch_catalog, __path_get_status, __path_get_transport, __path_list_band_regions,
    __path_list_tools, __path_locate_band_region, __path_run_tool, get_about, get_auth,
    get_band_plan, get_clients, get_diagnostics, get_doctor, get_ionosonde, get_license_text,
    get_patch_catalog, get_status, get_transport, list_band_regions, list_tools,
    locate_band_region, run_tool,
};
use media::{
    __path_call_audio, __path_captured_image, __path_list_images, call_audio, captured_image,
    list_images,
};
pub(crate) use media::{call_audio_path, captured_image_path};
pub(crate) use nodes::{channel_settings, edit_channel, radio_of, set_channel, set_device};
use presets::{
    __path_apply_preset, __path_create_bookmark, __path_create_preset, __path_delete_bookmark,
    __path_delete_preset, __path_list_bookmarks, __path_list_presets, apply_preset,
    create_bookmark, create_preset, delete_bookmark, delete_preset, list_bookmarks, list_presets,
};
use recordings::{
    __path_annotate_recording, __path_delete_recording, __path_download_recording,
    __path_list_recordings, __path_reveal_recording, __path_reveal_recordings_dir,
    __path_upload_recording, annotate_recording, byte_stream, collection_row, delete_recording,
    download_recording, list_recordings, recording_row, reveal_recording, reveal_recordings_dir,
    upload_recording,
};
use satellites::{
    __path_satellite_transmitters, __path_search_satellites, satellite_transmitters,
    search_satellites,
};
use workspaces::{
    __path_activate_workspace, __path_apply_template, __path_apply_workspace,
    __path_create_workspace, __path_delete_workspace, __path_dismiss_workspace_notice,
    __path_export_workspace, __path_get_resolved, __path_get_workspace, __path_import_example,
    __path_import_workspace, __path_list_examples, __path_list_templates, __path_list_workspaces,
    __path_redo_workspace, __path_undo_workspace, __path_update_workspace, Merge,
    activate_workspace, apply_template, apply_workspace, create_workspace, delete_workspace,
    dismiss_workspace_notice, export_workspace, get_resolved, get_workspace, import_example,
    import_workspace, list_examples, list_templates, list_workspaces, place_merged, redo_workspace,
    undo_workspace, update_workspace,
};
pub(crate) use workspaces::{
    HistoryStep, active_workspace, check_channel_node, resolve_now, step_history, switch,
};

use crate::{
    AppState, calibration,
    store::{RecordingRow, SteppedWorkspace, Store, StoreError},
    workspace,
};

#[derive(Debug)]
pub(crate) struct AppError {
    status: StatusCode,
    body: ApiError,
}

impl AppError {
    fn new(status: StatusCode, code: ErrorCode, message: String) -> Self {
        Self {
            status,
            body: ApiError {
                error: message,
                detail: None,
                code: Some(code),
            },
        }
    }

    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, ErrorCode::Request, message.into())
    }

    pub(crate) fn not_found(message: String) -> Self {
        Self::new(StatusCode::NOT_FOUND, ErrorCode::NotFound, message)
    }

    pub(crate) fn internal(message: String) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            message,
        )
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::Unavailable,
            message.into(),
        )
    }

    fn too_large(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::Request,
            message.into(),
        )
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, ErrorCode::Conflict, message.into())
    }

    fn with_detail(mut self, detail: String) -> Self {
        self.body.detail = Some(detail);
        self
    }

    pub(crate) fn is_client_error(&self) -> bool {
        self.status.is_client_error()
    }

    pub(crate) fn message(&self) -> String {
        match &self.body.detail {
            Some(detail) => format!("{}: {detail}", self.body.error),
            None => self.body.error.clone(),
        }
    }
}

fn rejection(status: StatusCode, error: &str, detail: String) -> AppError {
    AppError {
        status,
        body: ApiError {
            error: error.to_string(),
            detail: Some(detail),
            code: Some(ErrorCode::Request),
        },
    }
}

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(AppError))]
pub(crate) struct Json<T>(pub T);

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(AppError))]
pub(crate) struct Path<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(AppError))]
pub(crate) struct Query<T>(pub T);

pub(crate) struct Author {
    key: Option<String>,
    phone: Option<String>,
}

impl Author {
    pub(crate) fn as_deref(&self) -> Option<&str> {
        self.key.as_deref()
    }

    pub(crate) fn into_key(self) -> Option<String> {
        self.key
    }

    pub(crate) fn name(&self, state: &AppState) -> Option<String> {
        match &self.phone {
            Some(phone) => state.phones.one(phone, None).ok().map(|phone| phone.name),
            None => self
                .key
                .as_deref()
                .and_then(|key| state.presence.name_of(key)),
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Author {
    type Rejection = AppError;

    #[expect(clippy::unused_async_trait_impl, reason = "axum extractors are async")]
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let phone = parts
            .extensions
            .get::<crate::auth::Identity>()
            .and_then(|identity| identity.phone())
            .map(str::to_owned);
        let Some(value) = parts.headers.get(sdrmm_wire::AUTHOR_HEADER) else {
            let key = phone.as_ref().map(|phone| format!("phone-{phone}"));
            return Ok(Self {
                key: key.filter(|key| sdrmm_wire::valid_author(key)),
                phone,
            });
        };
        match value.to_str() {
            Ok(author) if sdrmm_wire::valid_author(author) => Ok(Self {
                key: Some(author.to_owned()),
                phone,
            }),
            _ => Err(AppError::bad_request(format!(
                "{} must be a short url-safe key",
                sdrmm_wire::AUTHOR_HEADER
            ))),
        }
    }
}

impl From<JsonRejection> for AppError {
    fn from(rej: JsonRejection) -> Self {
        rejection(rej.status(), "invalid request body", rej.body_text())
    }
}

impl From<PathRejection> for AppError {
    fn from(rej: PathRejection) -> Self {
        rejection(rej.status(), "invalid path parameter", rej.body_text())
    }
}

impl From<QueryRejection> for AppError {
    fn from(rej: QueryRejection) -> Self {
        rejection(rej.status(), "invalid query parameter", rej.body_text())
    }
}

impl From<EngineError> for AppError {
    fn from(err: EngineError) -> Self {
        let (status, code) = if err.is_not_found() {
            (StatusCode::NOT_FOUND, ErrorCode::NotFound)
        } else if err.is_bad_request() {
            (StatusCode::BAD_REQUEST, ErrorCode::Request)
        } else if err.is_conflict() {
            (StatusCode::CONFLICT, ErrorCode::Conflict)
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Engine)
        };
        Self::new(status, code, err.to_string())
    }
}

impl From<StoreError> for AppError {
    fn from(err: StoreError) -> Self {
        let (status, code) = match err {
            StoreError::PresetNotFound(_)
            | StoreError::BookmarkNotFound(_)
            | StoreError::SavedRadioNotFound(_)
            | StoreError::RecordingNotFound(_)
            | StoreError::WorkspaceNotFound(_)
            | StoreError::NoticeNotFound(_)
            | StoreError::PhoneNotFound(_)
            | StoreError::OfferGone
            | StoreError::CpsUserNotFound(_)
            | StoreError::CpsDeviceNotFound(_)
            | StoreError::CpsCodeplugNotFound(_)
            | StoreError::ChatAccountNotFound(_)
            | StoreError::NanoVnaAntennaNotFound(_)
            | StoreError::NanoVnaMeasurementNotFound(_) => {
                (StatusCode::NOT_FOUND, ErrorCode::NotFound)
            }
            StoreError::Timestamp(_)
            | StoreError::Sources(_)
            | StoreError::WorkspaceLayout(_)
            | StoreError::CpsField(_)
            | StoreError::ChatAccountField(_)
            | StoreError::NanoVnaField(_) => (StatusCode::BAD_REQUEST, ErrorCode::Request),
            StoreError::WorkspaceNameTaken(_)
            | StoreError::WorkspaceConflict { .. }
            | StoreError::CpsNameTaken
            | StoreError::NanoVnaNameTaken
            | StoreError::WorkspaceHistoryEnd { .. } => (StatusCode::CONFLICT, ErrorCode::Conflict),
            StoreError::Db(_) | StoreError::Corrupt(_) | StoreError::NewerSchema { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Storage)
            }
        };
        Self::new(status, code, err.to_string())
    }
}

impl From<ToolError> for AppError {
    fn from(err: ToolError) -> Self {
        let (status, code) = if err.is_not_found() {
            (StatusCode::NOT_FOUND, ErrorCode::NotFound)
        } else if err.is_bad_request() {
            (StatusCode::BAD_REQUEST, ErrorCode::Request)
        } else if err.is_unavailable() {
            (StatusCode::SERVICE_UNAVAILABLE, ErrorCode::Unavailable)
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Tool)
        };
        Self::new(status, code, err.to_string())
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::internal("engine task failed".to_string()).with_detail(err.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub(crate) fn lock_gate(gate: &std::sync::Mutex<()>) -> std::sync::MutexGuard<'_, ()> {
    gate.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn reveal_path(
    state: &AppState,
    path: &std::path::Path,
) -> Result<StatusCode, AppError> {
    let shell = state.shell.as_ref().ok_or_else(|| {
        AppError::not_found("this server has no file manager to show a recording in".to_string())
            .with_detail(
                "only the desktop app, which runs beside the recordings, can open one".to_string(),
            )
    })?;
    shell
        .reveal(path)
        .map_err(|err| AppError::internal(format!("show {}: {err}", path.display())))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn reconcile_recordings(dir: &std::path::Path, store: &Store) -> Result<(), AppError> {
    if !dir
        .try_exists()
        .map_err(|err| AppError::internal(format!("reach {}: {err}", dir.display())))?
    {
        return Ok(());
    }
    let library = scan_library(dir)
        .map_err(|err| AppError::internal(format!("scan {}: {err}", dir.display())))?;
    let mut kept = Vec::with_capacity(library.len());
    for stored in &library {
        let Some(name) = stored.stem().file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let row = match stored {
            Stored::Recording(stem) => recording_row(stem, name),
            Stored::Collection(stem) => collection_row(stem, name),
        };
        let Some(row) = row else {
            continue;
        };
        store.upsert_recording(&row)?;
        kept.push(name.to_string());
    }
    store.prune_recordings(&kept)?;
    Ok(())
}

#[derive(OpenApi)]
#[openapi(
    info(title = "SDRmm API", version = env!("CARGO_PKG_VERSION")),
    components(schemas(
        ServerEvent,
        ClientCommand,
        PresetSnapshot,
        ExportFormat,
        LogGroupKey,
        RecordingFormat,
        TemplateInfo,
        ScannerStatus,
    )),
)]
struct ApiDoc;

fn upload_limit() -> usize {
    usize::try_from(MAX_RECORDING_UPLOAD_BYTES).unwrap_or(usize::MAX)
}

pub(crate) fn openapi_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(get_state))
        .routes(routes!(get_devices))
        .routes(routes!(write_serial))
        .routes(routes!(get_nmea_devices))
        .routes(routes!(get_serial_ports))
        .routes(routes!(speakers::get_server_outputs))
        .routes(routes!(get_channel_types))
        .routes(routes!(post_channel_band))
        .routes(routes!(get_encoder_types))
        .routes(routes!(call_audio))
        .routes(routes!(crate::stream_out::serve))
        .routes(routes!(list_images))
        .routes(routes!(captured_image))
        .routes(routes!(list_presets, create_preset))
        .routes(routes!(apply_preset))
        .routes(routes!(delete_preset))
        .routes(routes!(list_bookmarks, create_bookmark))
        .routes(routes!(delete_bookmark))
        .routes(routes!(list_saved_radios, save_radio))
        .routes(routes!(delete_saved_radio))
        .routes(routes!(list_audio_recordings))
        .routes(routes!(download_audio_recording))
        .routes(routes!(play_audio_recording, delete_audio_recording))
        .routes(routes!(reveal_audio_recording))
        .merge(
            OpenApiRouter::new()
                .routes(routes!(list_recordings, upload_recording))
                .layer(DefaultBodyLimit::max(upload_limit())),
        )
        .routes(routes!(delete_recording))
        .routes(routes!(reveal_recordings_dir))
        .routes(routes!(reveal_recording))
        .routes(routes!(annotate_recording))
        .routes(routes!(download_recording))
        .routes(routes!(list_decoder_log, clear_decoder_log))
        .routes(routes!(export_decoder_log))
        .routes(routes!(group_decoder_log))
        .routes(routes!(search_satellites))
        .routes(routes!(satellite_transmitters))
        .routes(routes!(list_templates))
        .routes(routes!(apply_template))
        .routes(routes!(list_examples))
        .routes(routes!(import_example))
        .routes(routes!(list_workspaces, create_workspace))
        .routes(routes!(import_workspace))
        .routes(routes!(dismiss_workspace_notice))
        .routes(routes!(export_workspace))
        .routes(routes!(get_workspace, update_workspace, delete_workspace))
        .routes(routes!(activate_workspace))
        .routes(routes!(apply_workspace))
        .routes(routes!(get_resolved))
        .routes(routes!(actions::node_action))
        .routes(routes!(nodes::patch_node_channel))
        .routes(routes!(nodes::patch_node_device))
        .routes(routes!(
            nodes::add_network_radio,
            nodes::forget_network_radio
        ))
        .routes(routes!(undo_workspace))
        .routes(routes!(redo_workspace))
        .routes(routes!(get_patch_catalog))
        .routes(routes!(patch::post_patch_wiring))
        .routes(routes!(sinks::clear_sink))
        .routes(routes!(list_band_regions))
        .routes(routes!(get_band_plan))
        .routes(routes!(locate_band_region))
        .routes(routes!(get_auth))
        .routes(routes!(sign_in::sign_in))
        .routes(routes!(sign_in::sign_in_callback))
        .routes(routes!(sign_in::sign_out))
        .routes(routes!(sign_in::get_me))
        .routes(routes!(get_clients))
        .routes(routes!(get_ionosonde))
        .routes(routes!(get_status))
        .routes(routes!(get_transport))
        .routes(routes!(get_doctor))
        .routes(routes!(get_diagnostics))
        .routes(routes!(list_radio_models))
        .routes(routes!(list_cps_ports))
        .routes(routes!(get_cps_library))
        .routes(routes!(create_cps_user))
        .routes(routes!(update_cps_user, delete_cps_user))
        .routes(routes!(create_cps_device))
        .routes(routes!(update_cps_device, delete_cps_device))
        .routes(routes!(create_cps_codeplug))
        .routes(routes!(
            get_cps_codeplug,
            update_cps_codeplug,
            delete_cps_codeplug
        ))
        .routes(routes!(convert_cps_codeplug))
        .routes(routes!(merge_cps_codeplug))
        .routes(routes!(identify_radio))
        .routes(routes!(read_radio))
        .routes(routes!(write_radio))
        .routes(routes!(list_cps_jobs))
        .routes(routes!(get_cps_job, cancel_cps_job))
        .routes(routes!(list_tools))
        .routes(routes!(run_tool))
        .routes(routes!(get_about))
        .routes(routes!(get_license_text))
        .merge(arrays::routes())
        .merge(processors::routes())
        .merge(phones::routes())
        .merge(missions::routes())
        .merge(sky_scan::routes())
        .merge(survey::routes())
        .merge(fusion::routes())
        .merge(denoise::routes())
        .merge(speech::routes())
        .merge(nanovna::routes())
        .merge(secrets::routes())
        .merge(chat_accounts::routes())
}
