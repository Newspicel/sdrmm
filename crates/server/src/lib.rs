use std::{
    collections::HashSet,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::Router;
use sdrmm_engine::Engine;
use tower_http::{
    compression::predicate::{NotForContentType, Predicate},
    cors::CorsLayer,
};

pub const HOTPLUG_INTERVAL: Duration = Duration::from_secs(5);
pub const LEVEL_INTERVAL: Duration = Duration::from_millis(100);

mod activity;
mod aims;
mod apply;
mod array;
mod assets;
mod audio_fx;
mod auth;
mod bandplan;
mod bearing_fix;
mod calibration;
mod calls;
mod control;
pub(crate) mod cps;
mod decoded;
mod decoderlog;
pub mod denoise_models;
pub(crate) mod df_fusion;
pub mod diagnostics;
pub mod doctor;
mod encoders;
mod event_output;
mod events;
mod examples;
mod fusion_routes;
mod gps;
mod images;
mod ionosonde;
mod json;
mod mcp;
mod merge;
mod missions;
mod monitor;
mod net;
pub mod notices;
mod packed;
pub mod phones;
mod placement;
mod presence;
mod problems;
mod rebind;
mod recorders;
mod reference;
mod remember;
mod reports;
mod resolved;
mod rest;
mod rotators;
mod run_gate;
mod satellites;
mod schedules;
mod sinks;
mod sky_scans;
mod speakers;
mod speech_models;
mod store;
mod stream_out;
mod surfaces;
mod survey;
mod sweeps;
mod templates;
pub mod tls;
mod transcriber;
mod transmit;
mod trunking;
mod voice_chat;
mod workspace;
mod ws;

pub use auth::OidcOptions;
pub use store::{Store, StoreError};

pub trait NativeShell: Send + Sync + std::fmt::Debug {
    fn reveal(&self, path: &Path) -> std::io::Result<()>;
    fn notify(&self, title: &str, body: &str) -> std::io::Result<()>;
}

#[derive(Clone, Debug, Default)]
pub struct ServerOptions {
    pub dev_cors: bool,
    pub token: Option<String>,
    pub oidc: Option<OidcOptions>,
    pub shell: Option<Arc<dyn NativeShell>>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub engine: Arc<Engine>,
    pub store: Arc<Store>,
    pub auth: auth::Auth,
    pub db_path: Option<PathBuf>,
    pub recordings_gate: Arc<std::sync::Mutex<()>>,
    pub apply_gate: Arc<std::sync::Mutex<()>>,
    decoder_log_dropped: Arc<AtomicU64>,
    pub(crate) decoded: decoded::Feed,
    pub(crate) activity: Arc<activity::Activity>,
    pub(crate) sinks: Arc<sinks::SinkHub>,
    pub(crate) calls: Arc<calls::Calls>,
    pub(crate) images: Arc<images::Images>,
    pub(crate) ionosonde: Arc<ionosonde::Ionosonde>,
    pub clients: Arc<std::sync::atomic::AtomicU32>,
    pub(crate) tools: Arc<sdrmm_tools::ToolRegistry>,
    pub(crate) unrestored: Arc<std::sync::Mutex<Vec<String>>>,
    pub(crate) restored: Arc<std::sync::Mutex<HashSet<(i64, String, u32)>>>,
    pub(crate) gps: Arc<gps::GpsHub>,
    pub(crate) resolved: Arc<resolved::ResolvedHub>,
    pub(crate) problems: Arc<problems::ProblemBoard>,
    pub(crate) presence: Arc<presence::Presence>,
    pub(crate) webtransport: Arc<ws::webtransport::Advert>,
    pub(crate) pictures: Arc<ws::jpeg::Memo>,
    pub(crate) spectra: Arc<ws::spectrum::SpectrumShare>,
    pub(crate) control: Arc<control::ControlBus>,
    pub(crate) runs: Arc<run_gate::RunGate>,
    pub(crate) satellites: Arc<satellites::SatelliteHub>,
    pub(crate) schedules: Arc<schedules::ScheduleHub>,
    pub(crate) rotators: Arc<rotators::RotatorHub>,
    pub(crate) aims: Arc<aims::AimHub>,
    pub(crate) sky_scans: Arc<sky_scans::SkyScanHub>,
    pub(crate) reference: Arc<reference::Reference>,
    pub(crate) sweeps: Arc<sweeps::SweepGate>,
    pub(crate) cps: Arc<cps::CpsHub>,
    pub(crate) fusion: df_fusion::SharedFusion,
    pub(crate) shell: Option<Arc<dyn NativeShell>>,
    pub(crate) arrays: Arc<array::ArrayHub>,
    pub(crate) reports: Arc<reports::ReportHub>,
    pub(crate) surfaces: Arc<surfaces::SurfaceHub>,
    pub(crate) survey: Arc<survey::SurveyHub>,
    pub(crate) encoders: Arc<encoders::EncoderLinks>,
    pub(crate) phones: Arc<phones::Phones>,
    pub(crate) gate: Arc<phones::gate::PhoneGate>,
    pub(crate) server_id: Arc<str>,
    pub(crate) server_name: Arc<str>,
    pub(crate) dev_cors: bool,
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) denoise: Arc<denoise_models::Downloads>,
    pub(crate) speech: Arc<speech_models::SpeechModels>,
    pub(crate) transcribers: Arc<transcriber::TranscriberHub>,
    pub(crate) streams: Arc<stream_out::StreamHub>,
    started: std::time::Instant,
}

impl AppState {
    fn new(engine: Arc<Engine>, store: Arc<Store>) -> Self {
        let server_id = store.server_id();
        let phones = Arc::new(phones::Phones::new(store.clone()));
        let runs = Arc::new(run_gate::RunGate::default());
        let sessions = Arc::new(auth::Sessions::new(store.clone()));
        Self {
            engine,
            store,
            auth: auth::Auth::open(sessions),
            db_path: None,
            recordings_gate: Arc::new(std::sync::Mutex::new(())),
            apply_gate: Arc::new(std::sync::Mutex::new(())),
            decoder_log_dropped: Arc::new(AtomicU64::new(0)),
            decoded: decoded::feed(),
            activity: Arc::default(),
            sinks: Arc::default(),
            calls: Arc::new(calls::Calls::default()),
            images: Arc::new(images::Images::default()),
            ionosonde: Arc::new(ionosonde::Ionosonde::default()),
            clients: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            tools: Arc::new(sdrmm_tools::ToolRegistry::with_builtins()),
            unrestored: Arc::new(std::sync::Mutex::new(Vec::new())),
            restored: Arc::new(std::sync::Mutex::new(HashSet::new())),
            gps: Arc::new(gps::GpsHub::default()),
            resolved: Arc::new(resolved::ResolvedHub::default().gated_by(runs.clone())),
            problems: Arc::default(),
            presence: Arc::default(),
            webtransport: Arc::default(),
            pictures: Arc::default(),
            spectra: Arc::default(),
            control: Arc::new(control::ControlBus::gated_by(runs.clone())),
            runs,
            satellites: Arc::new(satellites::SatelliteHub::default()),
            schedules: Arc::default(),
            rotators: Arc::default(),
            aims: Arc::default(),
            sky_scans: Arc::default(),
            reference: Arc::default(),
            cps: Arc::new(cps::CpsHub::default()),
            sweeps: Arc::default(),
            fusion: Arc::new(df_fusion::FusionHub::default()),
            shell: None,
            arrays: Arc::default(),
            reports: Arc::default(),
            surfaces: Arc::default(),
            survey: Arc::default(),
            encoders: Arc::default(),
            phones,
            gate: Arc::default(),
            server_id,
            server_name: net::host_label().into(),
            dev_cors: false,
            data_dir: None,
            denoise: Arc::default(),
            speech: Arc::default(),
            transcribers: Arc::default(),
            streams: Arc::default(),
            started: std::time::Instant::now(),
        }
    }

    pub(crate) fn uptime(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    pub fn decoder_log_dropped(&self) -> u64 {
        self.decoder_log_dropped.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub db_path: Option<PathBuf>,
    pub tls: Option<tls::Tls>,
    pub options: ServerOptions,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], 8080)),
            db_path: None,
            tls: None,
            options: ServerOptions::default(),
        }
    }
}

#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    let (_router, api) = rest::openapi_router().split_for_parts();
    api
}

fn openapi_route(api: &utoipa::openapi::OpenApi) -> axum::routing::MethodRouter<AppState> {
    let spec = serde_json::to_vec(api)
        .map(axum::body::Bytes::from)
        .map_err(|error| error.to_string());
    axum::routing::get(move || {
        let spec = spec.clone();
        async move {
            use axum::response::IntoResponse;
            match spec {
                Ok(body) => (
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    body,
                )
                    .into_response(),
                Err(error) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(sdrmm_wire::ApiError {
                        error: "OpenAPI document failed to serialize".to_string(),
                        detail: Some(error),
                        code: None,
                    }),
                )
                    .into_response(),
            }
        }
    })
}

fn configure(state: &mut AppState, options: &ServerOptions) -> std::io::Result<()> {
    let oidc = options
        .oidc
        .clone()
        .map(|oidc| {
            if let Some(secret) = &oidc.client_secret {
                diagnostics::hide_secret(secret);
            }
            auth::Oidc::new(oidc).map_err(std::io::Error::other)
        })
        .transpose()?;
    state.auth = state.auth.configured(options.token.as_deref(), oidc);
    state.shell.clone_from(&options.shell);
    state.dev_cors = options.dev_cors;
    if let Some(token) = &options.token {
        diagnostics::hide_secret(token);
    }
    Ok(())
}

#[cfg(test)]
fn router_with_state(state: AppState, options: &ServerOptions) -> (Router, Background) {
    main_router(state, options, false)
}

#[cfg(test)]
fn main_router(mut state: AppState, options: &ServerOptions, tls: bool) -> (Router, Background) {
    configure(&mut state, options).expect("sign-in client");
    let background = start_runtime(&state);
    let app = app(&state, auth::ListenerRole::Main, tls);
    (app, background)
}

fn app(state: &AppState, role: auth::ListenerRole, tls: bool) -> Router {
    let (api_router, api) = rest::openapi_router().split_for_parts();
    let mut routes = Router::new()
        .merge(api_router)
        .route("/api/ws", axum::routing::get(ws::handler));
    if role == auth::ListenerRole::Main {
        routes = routes
            .merge(mcp::router(state))
            .route("/api/openapi.json", openapi_route(&api))
            .route("/api/docs", axum::routing::get(assets::api_docs))
            .route("/api/docs/", axum::routing::get(assets::api_docs));
    }
    routes = routes.route_layer(axum::middleware::from_fn_with_state(
        auth::AuthGate::new(state, role, tls),
        auth::authenticate,
    ));
    if role == auth::ListenerRole::Main {
        routes = routes.fallback(assets::static_handler);
    }
    let mut app = routes.with_state(state.clone()).layer(
        tower_http::compression::CompressionLayer::new().compress_when(
            tower_http::compression::predicate::DefaultPredicate::new()
                .and(NotForContentType::const_new("application/x-tar"))
                .and(NotForContentType::const_new("audio/wav"))
                .and(NotForContentType::const_new("video/mp2t")),
        ),
    );
    if state.dev_cors && role == auth::ListenerRole::Main {
        app = app.layer(CorsLayer::very_permissive());
    }
    app
}

fn start_runtime(state: &AppState) -> Background {
    let background = start_background(state);
    workspace::spawn_autosave(state);
    placement::spawn_settling(state);
    resolved::follow(state, "positions", |state| state.gps.reconcile(state));
    resolved::follow(state, "satellites", |state| {
        state.satellites.reconcile(state);
    });
    resolved::follow(state, "rotators", |state| state.rotators.reconcile(state));
    resolved::follow(state, "encoders and transmitters", transmit::reconcile);
    resolved::follow(state, "control", |state| {
        state
            .control
            .settle(&state.engine, &state.resolved.current().patch);
    });
    run_gate::follow(state);
    schedules::follow(state);
    audio_fx::follow(state);
    speakers::follow(state);
    stream_out::follow(state);
    voice_chat::follow(state);
    array::start_pump(state);
    reports::start(state);
    aims::start(state);
    sky_scans::follow(state);
    df_fusion::start(state);
    fusion_routes::start(state);
    bearing_fix::start(state);
    transcriber::start(state);
    sinks::start(state);
    survey::follow(state);
    sweeps::follow(state);
    array::follow_recordings(state);
    missions::watch::spawn(state);
    state.gps.spawn_watchdog(state);
    state.gps.spawn_replay(state);
    phones::spawn_flusher(state);
    background
}

struct Background {
    tasks: Vec<BackgroundTask>,
    detached: bool,
}

enum BackgroundTask {
    Task(tokio::task::JoinHandle<()>),
    Owned,
}

#[cfg(test)]
impl Background {
    fn detach(mut self) {
        self.detached = true;
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        if self.detached {
            return;
        }
        for task in &self.tasks {
            if let BackgroundTask::Task(task) = task {
                task.abort();
            }
        }
    }
}

fn start_background(state: &AppState) -> Background {
    let resolved = {
        let state = state.clone();
        spawn_task("sdrmm-resolved", move || resolved::run(state))
    };
    let decoded = {
        let router = decoded::Router {
            engine: Arc::downgrade(&state.engine),
            resolved: state.resolved.subscribe(),
            feed: state.decoded.clone(),
            reference: state.reference.clone(),
            activity: state.activity.clone(),
        };
        spawn_task("sdrmm-decoded", move || decoded::run(router))
    };
    let activity = {
        let engine = Arc::downgrade(&state.engine);
        let activity = state.activity.clone();
        let resolved = state.resolved.subscribe();
        spawn_task("sdrmm-activity", move || {
            activity::run(engine, activity, resolved)
        })
    };
    let rebind = {
        let state = state.clone();
        let saved = rebind::Saved::active(&state);
        spawn_task("sdrmm-rebind", move || rebind::run(state, saved))
    };
    let log = {
        let engine = Arc::downgrade(&state.engine);
        let store = state.store.clone();
        let dropped = state.decoder_log_dropped.clone();
        let records = state.decoded.subscribe();
        spawn_task("sdrmm-decoderlog", move || {
            decoderlog::run(records, engine, store, dropped)
        })
    };
    let patch = {
        let engine = Arc::downgrade(&state.engine);
        let store = state.store.clone();
        let resolved = state.resolved.subscribe();
        spawn_task("sdrmm-trunking", move || {
            trunking::watch_patch(engine, store, resolved)
        })
    };
    let monitor = {
        let watch = monitor::Watch {
            engine: Arc::downgrade(&state.engine),
            resolved: state.resolved.subscribe(),
            problems: state.problems.clone(),
            calls: state.calls.clone(),
        };
        spawn_task("sdrmm-monitors", move || monitor::run(watch))
    };
    let calls = {
        let watch = calls::CallWatch {
            engine: Arc::downgrade(&state.engine),
            calls: state.calls.clone(),
            resolved: state.resolved.subscribe(),
            problems: state.problems.clone(),
        };
        spawn_task("sdrmm-calls", move || calls::run(watch))
    };
    let images = {
        let state = state.clone();
        spawn_task("sdrmm-images", move || images::run(state))
    };
    let event_output = {
        let engine = Arc::downgrade(&state.engine);
        let patch = state.resolved.subscribe();
        let calls = state.calls.clone();
        let records = state.decoded.subscribe();
        let shell = state.shell.clone();
        let store = state.store.clone();
        spawn_task("sdrmm-event-output", move || {
            event_output::run(records, engine, patch, calls, shell, store)
        })
    };
    Background {
        tasks: vec![
            resolved,
            decoded,
            activity,
            rebind,
            log,
            patch,
            calls,
            images,
            event_output,
            monitor,
        ],
        detached: false,
    }
}

fn spawn_task<F, Fut>(name: &'static str, make: F) -> BackgroundTask
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let _guard = handle.enter();
        return BackgroundTask::Task(tokio::spawn(make()));
    }
    let spawned = std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(make()),
                Err(err) => tracing::error!(error = %err, name, "no runtime for a background task"),
            }
        });
    if let Err(err) = spawned {
        tracing::error!(error = %err, name, "failed to start a background task");
    }
    BackgroundTask::Owned
}

pub struct ServerHandle {
    pub local_addr: SocketAddr,
    pub scheme: &'static str,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
    _gate: phones::gate::GateGuard,
    _background: Background,
}

impl ServerHandle {
    pub async fn join(mut self) -> std::io::Result<()> {
        match (&mut self.task).await {
            Ok(res) => res,
            Err(join_err) => Err(std::io::Error::other(join_err)),
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn serve(config: Config, engine: Arc<Engine>) -> std::io::Result<ServerHandle> {
    refuse_phone_token(&config.options)?;
    #[cfg(feature = "dvsi")]
    if !sdrmm_codec::voice::chip::install_vocoder_chips(Arc::new(sdrmm_dvsi::Chips::new())) {
        tracing::debug!("vocoder chips already installed");
    }
    engine.start_hotplug_prober(HOTPLUG_INTERVAL)?;
    engine.start_level_meter(LEVEL_INTERVAL)?;
    log_start(&config, &engine);
    sweep_import_spools(&engine);
    let store = Store::open(config.db_path.as_deref()).map_err(std::io::Error::other)?;
    workspace::adopt_named_devices(&engine, &store);
    let mut state = AppState::new(engine, Arc::new(store));
    configure(&mut state, &config.options)?;
    state.db_path = config.db_path.clone();
    state.data_dir = config
        .db_path
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    if let Some(dir) = &state.data_dir {
        state
            .engine
            .denoise_models()
            .set_dir(dir.join("denoise-models"));
        state.speech.set_dir(dir.join("speech-models"));
        state.reference.attach(dir.join("reference"));
    }
    let served = config
        .tls
        .as_ref()
        .map(tls::load)
        .transpose()
        .map_err(std::io::Error::other)?;
    let listener = std::net::TcpListener::bind(config.bind)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    state.gate.set_main(main_listener(
        local_addr,
        config.tls.as_ref(),
        served.as_ref(),
    ));
    let mut background = start_runtime(&state);
    let reference = state.reference.clone();
    background
        .tasks
        .push(spawn_task("sdrmm-reference", move || reference.run()));
    let app = app(&state, auth::ListenerRole::Main, served.is_some());
    let scheme = if served.is_some() { "https" } else { "http" };
    if served.is_some() {
        match ws::webtransport::start(&state, local_addr) {
            Ok(task) => background.tasks.push(BackgroundTask::Task(task)),
            Err(error) => tracing::warn!(%error, "WebTransport is off"),
        }
    }
    tracing::info!(%local_addr, scheme, "SDRmm server listening");
    let task = if let Some(served) = served {
        let server = axum_server::from_tcp_rustls(
            listener,
            axum_server::tls_rustls::RustlsConfig::from_config(served.config),
        )?
        .map(tls::redirect::RedirectPlain::before);
        tokio::spawn(async move { server.serve(app.into_make_service()).await })
    } else {
        let listener = tokio::net::TcpListener::from_std(listener)?;
        tokio::spawn(async move { axum::serve(listener, app).await })
    };
    let gate = phones::gate::GateGuard::new(state.gate.clone());
    state.gate.restore(&state).await;
    Ok(ServerHandle {
        local_addr,
        scheme,
        task,
        _gate: gate,
        _background: background,
    })
}

fn refuse_phone_token(options: &ServerOptions) -> std::io::Result<()> {
    if options
        .token
        .as_deref()
        .is_some_and(|token| token.starts_with(sdrmm_wire::phone::PHONE_TOKEN_PREFIX))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the token must not start with sdrmm-phone.",
        ));
    }
    Ok(())
}

fn log_start(config: &Config, engine: &Engine) {
    if let Some(path) = &config.db_path {
        tracing::info!(db = %path.display(), "opening database");
    } else {
        tracing::info!("using in-memory database (nothing will persist)");
    }
    if let Some(dir) = engine.recordings_dir() {
        tracing::info!(dir = %dir.display(), "recordings directory");
    } else {
        tracing::info!("recording disabled (engine has no recordings directory)");
    }
    if config.options.token.is_some() {
        tracing::info!("shared-token auth enabled");
    } else {
        tracing::info!("no token: LAN-trusted, unauthenticated");
    }
}

fn sweep_import_spools(engine: &Engine) {
    let Some(dir) = engine.recordings_dir() else {
        return;
    };
    match sdrmm_recorder::sweep_spools(dir) {
        Ok(removed) => {
            for path in removed {
                tracing::info!(path = %path.display(), "removed a spool a crashed import left");
            }
        }
        Err(error) => {
            tracing::warn!(dir = %dir.display(), %error, "could not clear old import spools");
        }
    }
}

fn main_listener(
    local_addr: SocketAddr,
    asked: Option<&tls::Tls>,
    served: Option<&tls::Served>,
) -> phones::gate::MainListener {
    let own_key = matches!(asked, Some(tls::Tls::SelfSigned { .. }));
    let named = matches!(asked, Some(tls::Tls::SelfSigned { names, .. }) if !names.is_empty());
    phones::gate::MainListener {
        record: phones::gate::ListenerRecord {
            role: auth::ListenerRole::Main,
            port: local_addr.port(),
            bound: local_addr.ip(),
            pin: served.map(|served| served.pin.clone()),
            stable_key: own_key && served.is_some(),
            names: served
                .filter(|_| named)
                .map_or_default(|served| served.names.clone()),
        },
        own_key: served
            .filter(|_| own_key)
            .map(|served| served.config.clone()),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
/// Every test in this binary runs against the built-in decoders and processors, installed once
/// before main.
#[ctor::ctor(unsafe)]
fn install_for_tests() {
    sdrmm_channels::builtin::install();
    sdrmm_array::builtin::install();
}
