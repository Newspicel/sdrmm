use std::sync::LazyLock;

use axum::Router;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use sdrmm_wire::{
    DecoderLogQuery, DecoderLogResponse, DeviceRef, NanoVnaRequest, PatchCatalog, ToolRequest,
    ToolsResponse,
};

use self::{
    args::{
        Applied, ChannelArgs, ChannelSet, ChannelTypes, DeviceChoice, Devices, NodeArgs,
        PutNodeArgs, ScanArgs, Spectrum, SpectrumArgs, TuneArgs, WireArgs, WorkspaceArgs,
    },
    schema::{Args, input},
};
use crate::{
    AppState,
    rest::{self, AppError},
    store::Store,
};

mod args;
mod canvas;
mod live;
mod schema;

pub(crate) const MCP_AUTHOR: &str = "mcp";
const MCP_NAME: &str = "MCP";
const SPECTRUM_BINS: usize = 128;
const SPECTRUM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const MAX_MCP_SWEEP_POINTS: u32 = 401;

static ROUTER: LazyLock<ToolRouter<SdrMcp>> = LazyLock::new(SdrMcp::tool_router);

pub(crate) fn router(state: &AppState) -> Router<AppState> {
    LazyLock::force(&ROUTER);
    let state = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(SdrMcp::new(state.clone())),
        std::sync::Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .disable_allowed_hosts(),
    );
    Router::new().nest_service("/mcp", service)
}

#[derive(Clone)]
struct SdrMcp {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

impl From<AppError> for ErrorData {
    fn from(err: AppError) -> Self {
        if err.is_client_error() {
            Self::invalid_params(err.message(), None)
        } else {
            Self::internal_error(err.message(), None)
        }
    }
}

impl SdrMcp {
    fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: ROUTER.clone(),
        }
    }

    async fn blocking<T: Send + 'static>(
        &self,
        job: impl FnOnce(&AppState) -> Result<T, AppError> + Send + 'static,
    ) -> Result<T, ErrorData> {
        let state = self.state.clone();
        Ok(tokio::task::spawn_blocking(move || job(&state))
            .await
            .map_err(AppError::from)??)
    }

    async fn edit(
        &self,
        change: impl FnOnce(&mut sdrmm_wire::WorkspaceSnapshot) -> Result<Option<String>, AppError>
        + Send
        + 'static,
    ) -> Result<CallToolResult, ErrorData> {
        let (node, report) = self
            .blocking(move |state| canvas::edit(state, change))
            .await?;
        rest::reconcile_graph(self.state.clone()).await?;
        structured(&Applied { node, report })
    }

    async fn step(&self, step: rest::HistoryStep) -> Result<CallToolResult, ErrorData> {
        let id = self
            .blocking(|state| Ok(canvas::active(state)?.info.id))
            .await?;
        let detail =
            rest::step_history(self.state.clone(), id, Some(MCP_AUTHOR.to_owned()), step).await?;
        structured(&detail.0)
    }
}

fn structured<T: serde::Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    let json = serde_json::to_value(value)
        .map_err(|e| ErrorData::internal_error(format!("serializing result: {e}"), None))?;
    Ok(CallToolResult::structured(json))
}

#[tool_router]
impl SdrMcp {
    #[tool(
        description = "The open canvas: nodes, wires, saved settings, undo state, and which \
                       nodes run live. Start here.",
        annotations(title = "Get workspace", read_only_hint = true)
    )]
    async fn get_workspace(&self) -> Result<CallToolResult, ErrorData> {
        structured(&self.blocking(live::workspace_view).await?)
    }

    #[tool(
        description = "One node and what runs behind it: its radio, decoder, saved settings or \
                       scan.",
        input_schema = input::<NodeArgs>(),
        annotations(title = "Get node", read_only_hint = true)
    )]
    async fn get_node(&self, Args(args): Args<NodeArgs>) -> Result<CallToolResult, ErrorData> {
        structured(
            &self
                .blocking(move |state| live::node_view(state, &args.node))
                .await?,
        )
    }

    #[tool(
        description = "Stored workspaces and which one is open.",
        annotations(title = "List workspaces", read_only_hint = true)
    )]
    async fn list_workspaces(&self) -> Result<CallToolResult, ErrorData> {
        structured(
            &self
                .blocking(|state| Ok(state.store.list_workspaces()?))
                .await?,
        )
    }

    #[tool(
        description = "Open another workspace on every client and bring up its radios.",
        input_schema = input::<WorkspaceArgs>(),
        annotations(title = "Open workspace")
    )]
    async fn open_workspace(
        &self,
        Args(args): Args<WorkspaceArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = self
            .blocking(move |state| {
                let _serialized = rest::lock_gate(&state.apply_gate);
                rest::switch(state, args.workspace, Some(MCP_NAME.to_owned()))
            })
            .await?;
        rest::reconcile_graph(self.state.clone()).await?;
        structured(&Applied { node: None, report })
    }

    #[tool(
        description = "Every node kind: its ports and default body. A body passed to put_node \
                       has this shape.",
        annotations(title = "List node types", read_only_hint = true)
    )]
    async fn list_node_types(&self) -> Result<CallToolResult, ErrorData> {
        structured(&PatchCatalog::build())
    }

    #[tool(
        description = "Attached radios, recordings and test sources. `device` goes into a \
                       device node body as data.device.",
        annotations(title = "List devices", read_only_hint = true)
    )]
    async fn list_devices(&self) -> Result<CallToolResult, ErrorData> {
        let devices = self
            .blocking(|state| Ok(state.engine.probe_devices()))
            .await?
            .into_iter()
            .map(|info| DeviceChoice {
                device: DeviceRef::from_info(&info),
                info,
            })
            .collect();
        structured(&Devices { devices })
    }

    #[tool(
        description = "Decoder types for a channel node's channel_type, with the bandwidth \
                       each needs.",
        annotations(title = "List channel types", read_only_hint = true)
    )]
    async fn list_channel_types(&self) -> Result<CallToolResult, ErrorData> {
        structured(&ChannelTypes {
            types: self.state.engine.channel_types(),
        })
    }

    #[tool(
        description = "Add a node (leave out `node`) or change one: body, label, position. The \
                       kind of a node never changes. Radios open as soon as a device node \
                       names one.",
        input_schema = input::<PutNodeArgs>(),
        annotations(title = "Put node")
    )]
    async fn put_node(&self, Args(args): Args<PutNodeArgs>) -> Result<CallToolResult, ErrorData> {
        self.edit(move |snapshot| canvas::put_node(&mut snapshot.graph, args).map(Some))
            .await
    }

    #[tool(
        description = "Remove a node and its wires, closing what ran behind it.",
        input_schema = input::<NodeArgs>(),
        annotations(title = "Remove node", destructive_hint = true)
    )]
    async fn remove_node(&self, Args(args): Args<NodeArgs>) -> Result<CallToolResult, ErrorData> {
        self.edit(move |snapshot| {
            canvas::remove_node(&mut snapshot.graph, &args.node)?;
            Ok(None)
        })
        .await
    }

    #[tool(
        description = "Draw a wire from an output port to an input port.",
        input_schema = input::<WireArgs>(),
        annotations(title = "Connect", idempotent_hint = true)
    )]
    async fn connect(&self, Args(args): Args<WireArgs>) -> Result<CallToolResult, ErrorData> {
        self.edit(move |snapshot| {
            canvas::connect(&mut snapshot.graph, args);
            Ok(None)
        })
        .await
    }

    #[tool(
        description = "Cut a wire.",
        input_schema = input::<WireArgs>(),
        annotations(title = "Disconnect", destructive_hint = true)
    )]
    async fn disconnect(&self, Args(args): Args<WireArgs>) -> Result<CallToolResult, ErrorData> {
        self.edit(move |snapshot| {
            canvas::disconnect(&mut snapshot.graph, args)?;
            Ok(None)
        })
        .await
    }

    #[tool(
        description = "Change a running radio: centre frequency, sample rate, gain, AGC, \
                       antenna. Only the fields given change.",
        input_schema = input::<TuneArgs>(),
        annotations(title = "Tune radio", idempotent_hint = true)
    )]
    async fn tune_radio(&self, Args(args): Args<TuneArgs>) -> Result<CallToolResult, ErrorData> {
        structured(
            &self
                .blocking(move |state| live::tune(state, &args.node, args.settings))
                .await?,
        )
    }

    #[tool(
        description = "Set a channel node's frequency, squelch and decoder settings. Applied \
                       live when it runs, kept for when it does otherwise.",
        input_schema = input::<ChannelArgs>(),
        annotations(title = "Set channel", idempotent_hint = true)
    )]
    async fn set_channel(
        &self,
        Args(args): Args<ChannelArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let live = self
            .blocking(move |state| live::set_channel(state, &args.node, args.settings))
            .await?;
        structured(&ChannelSet { live })
    }

    #[tool(
        description = "Start, stop or skip the scan of a scanner node, over the ranges in its \
                       body, on the decoder its control output is wired to.",
        input_schema = input::<ScanArgs>(),
        annotations(title = "Scan")
    )]
    async fn scan(&self, Args(args): Args<ScanArgs>) -> Result<CallToolResult, ErrorData> {
        structured(
            &self
                .blocking(move |state| live::scan(state, &args.node, args.action))
                .await?,
        )
    }

    #[tool(
        description = "Step the open workspace back one change.",
        annotations(title = "Undo")
    )]
    async fn undo(&self) -> Result<CallToolResult, ErrorData> {
        self.step(Store::undo_workspace).await
    }

    #[tool(
        description = "Step the open workspace forward one undone change.",
        annotations(title = "Redo")
    )]
    async fn redo(&self) -> Result<CallToolResult, ErrorData> {
        self.step(Store::redo_workspace).await
    }

    #[tool(
        description = "One spectrum frame from a device node: noise floor and 128 bins in dBFS.",
        input_schema = input::<SpectrumArgs>(),
        annotations(title = "Spectrum snapshot", read_only_hint = true)
    )]
    async fn spectrum_snapshot(
        &self,
        Args(args): Args<SpectrumArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let stream = args.stream;
        let mut rx = self
            .blocking(move |state| {
                let device_set = live::radio_of(state, &args.node)?;
                Ok(state.engine.subscribe_spectrum(device_set, stream)?)
            })
            .await?;
        let snapshot = tokio::time::timeout(SPECTRUM_TIMEOUT, rx.recv())
            .await
            .map_err(|_| ErrorData::internal_error("no spectrum within 2 s".to_string(), None))?
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let mut bins_db = vec![0.0f32; SPECTRUM_BINS];
        let floor_db = sdrmm_dsp::SpanFloor::default().read(&snapshot.db);
        let signal_db = floor_db.map_or(f32::NEG_INFINITY, |floor| {
            floor + sdrmm_wire::SPECTRUM_SIGNAL_MARGIN_DB
        });
        sdrmm_dsp::decimate_signal(&snapshot.db, signal_db, &mut bins_db);
        structured(&Spectrum {
            center_hz: snapshot.center_hz,
            span_hz: snapshot.span_hz,
            floor_db,
            bins_db,
        })
    }

    #[tool(
        description = "Stored decodes, newest first: aircraft, ships, pagers, APRS, RDS and \
                       more. `nodes` takes comma separated node ids.",
        input_schema = input::<DecoderLogQuery>(),
        annotations(title = "Query decoder log", read_only_hint = true)
    )]
    async fn query_decoder_log(
        &self,
        Args(filter): Args<DecoderLogQuery>,
    ) -> Result<CallToolResult, ErrorData> {
        let (entries, total) = self
            .blocking(move |state| Ok(state.store.query_decoder_log(&filter)?))
            .await?;
        structured(&DecoderLogResponse {
            entries,
            total,
            dropped: self.state.decoder_log_dropped() + self.state.engine.decoded_dropped(),
        })
    }

    #[tool(
        description = "Bench tools beside the receiver, such as the antenna calculator and \
                       the NanoVNA. Only tools this build carries are listed.",
        annotations(title = "List tools", read_only_hint = true)
    )]
    async fn list_tools(&self) -> Result<CallToolResult, ErrorData> {
        structured(&ToolsResponse {
            tools: self.state.tools.descriptors(),
        })
    }

    #[tool(
        description = "Run a bench tool. A NanoVNA sweep here carries at most 401 points.",
        input_schema = input::<ToolRequest>(),
        annotations(title = "Run tool")
    )]
    async fn run_tool(
        &self,
        Args(request): Args<ToolRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if let ToolRequest::NanoVna(NanoVnaRequest::Sweep(sweep)) = &request
            && sweep.points > MAX_MCP_SWEEP_POINTS
        {
            return Err(ErrorData::invalid_params(
                format!("a sweep over MCP carries at most {MAX_MCP_SWEEP_POINTS} points"),
                None,
            ));
        }
        structured(
            &self
                .blocking(move |state| Ok(state.tools.run(request)?))
                .await?,
        )
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SdrMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("SDR--", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Drive the SDR-- canvas: what you change here is what every user sees. Read \
                 get_workspace first. Build with put_node and connect: a device node opens a \
                 radio, a channel node wired from its iq port decodes on it. Every tool takes \
                 node ids. Frequencies are in Hz.",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITHOUT_ARGUMENTS: &[&str] = &[
        "get_workspace",
        "list_channel_types",
        "list_devices",
        "list_node_types",
        "list_tools",
        "list_workspaces",
        "redo",
        "undo",
    ];

    #[test]
    fn tool_names_are_unique_and_stable() {
        let tools = SdrMcp::tool_router().list_all();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        names.sort_unstable();
        let unique: std::collections::HashSet<&&str> = names.iter().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "duplicate tool name in {names:?}"
        );
        assert_eq!(
            names,
            [
                "connect",
                "disconnect",
                "get_node",
                "get_workspace",
                "list_channel_types",
                "list_devices",
                "list_node_types",
                "list_tools",
                "list_workspaces",
                "open_workspace",
                "put_node",
                "query_decoder_log",
                "redo",
                "remove_node",
                "run_tool",
                "scan",
                "set_channel",
                "spectrum_snapshot",
                "tune_radio",
                "undo",
            ]
        );
    }

    #[test]
    fn every_tool_is_described_and_typed() {
        for tool in SdrMcp::tool_router().list_all() {
            assert!(
                tool.description.as_ref().is_some_and(|d| d.len() > 10),
                "{} has no usable description",
                tool.name
            );
            assert_eq!(tool.input_schema["type"], "object", "{}", tool.name);
            let typed = tool
                .input_schema
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|properties| !properties.is_empty())
                || tool.input_schema.contains_key("oneOf");
            assert_eq!(
                typed,
                !WITHOUT_ARGUMENTS.contains(&tool.name.as_ref()),
                "{} declares the wrong arguments",
                tool.name
            );
        }
    }
}
