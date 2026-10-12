use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Once, OnceLock},
    time::{Duration, Instant},
};

use sdrmm_engine::Engine;
use zgui::prelude::*;
use zgui::{
    geom::{Device, DevicePx, Size},
    platform::SurfaceEvent,
    runtime::Runtime,
};
use zgui_platform_headless::Harness;

use super::{
    dom::{self, CssRect},
    gpu::{self, Offscreen},
    picture,
};
use crate::{
    WINDOW, api, boot,
    host::Host,
    shell::prefs::{PrefsFile, fresh_author},
    store::{Phase, Store},
    theme, ui,
};

const FRAME: Duration = Duration::from_micros(16_667);
const PUMPS: u32 = 32;
const NAP: Duration = Duration::from_millis(5);
const WAIT: Duration = Duration::from_secs(20);
const SPOTS: u16 = 24;

const LOG: &str = "error,zgui_render_wgpu=warn,sdrmm_native=warn";

static TOKIO: OnceLock<zgui_tokio::Installed> = OnceLock::new();
static LOGGING: Once = Once::new();

pub struct Screen {
    harness: Option<Harness<Runtime>>,
    store: Store,
    host: Host,
    pub(super) clock: Duration,
    _data: tempfile::TempDir,
}

fn report_warnings() {
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| LOG.into());
    if tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_test_writer()
        .with_ansi(false)
        .try_init()
        .is_err()
    {
        println!("warnings go to the logger installed earlier");
    }
}

fn tokio() -> &'static zgui_tokio::Installed {
    TOKIO.get_or_init(|| zgui_tokio::install().expect("tokio installs as the background executor"))
}

fn engine(data: &Path) -> Arc<Engine> {
    let recordings = data.join("recordings");
    let mut registry = sdrmm_device::DeviceRegistry::new();
    registry.register(0, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
    registry.register(
        0,
        Box::new(sdrmm_device_recording::RecordingDriver::new(Some(
            recordings.clone(),
        ))),
    );
    registry.register(0, Box::new(sdrmm_device_siggen::SigGenDriver::new()));
    Engine::with_registry(registry, Some(recordings))
}

fn viewport() -> Size<DevicePx, Device> {
    Size::new(DevicePx(WINDOW.0), DevicePx(WINDOW.1))
}

impl Screen {
    pub fn open() -> Option<Self> {
        let Some(renderer) = gpu::factory() else {
            println!("skipped: no graphics device");
            return None;
        };
        LOGGING.call_once(report_warnings);
        let handle = tokio().handle();
        let data = tempfile::tempdir().expect("a data dir");
        let host = handle
            .block_on(Host::serve(engine(data.path()), data.path()))
            .expect("the embedded server starts");
        zgui_tokio::enter_here(handle.clone());
        let token = api::Token::default();
        let api = api::Api::new(host.base.clone())
            .expect("an api client")
            .with_token(token.clone())
            .with_author(fresh_author());
        let prefs = PrefsFile::new(data.path().join("native-prefs.json"));
        let websocket = host.websocket_url();
        let mounted = Rc::new(Cell::new(None));
        let kept = Rc::clone(&mounted);
        let runtime = zgui::app()
            .with_size(WINDOW.0, WINDOW.1)
            .with_stylesheet(theme::SHEET)
            .with_renderer(renderer)
            .into_handler(move || {
                let store = boot(&api, &handle, &websocket, &token);
                kept.set(Some(store));
                ui::app(store, prefs.clone())
            })
            .expect("the reactive runtime installs")
            .into_runtime();
        let mut harness = Harness::new(runtime);
        harness.deliver_to_first(SurfaceEvent::Resized(viewport()));
        let store = mounted.get().expect("the app mounted");
        let mut screen = Self {
            harness: Some(harness),
            store,
            host,
            clock: Duration::ZERO,
            _data: data,
        };
        screen.frame();
        Some(screen)
    }

    pub fn ready() -> Option<Self> {
        let mut screen = Self::open()?;
        screen.settle_for("the workspace start", WAIT, |screen| {
            matches!(
                screen.store.phase.get_untracked(),
                Phase::NoWorkspace | Phase::Ready
            )
        });
        Some(screen)
    }

    pub fn empty() -> Option<Self> {
        let mut screen = Self::ready()?;
        let api = screen.store.api();
        let id = screen
            .block_on(api.create_workspace("Screens"))
            .expect("a workspace");
        let store = screen.store;
        zgui::task::spawn_local(async move { store.load(id, false).await });
        screen.settle_for("an empty workspace", WAIT, |screen| {
            screen.store.phase.get_untracked() == Phase::Ready
                && screen.store.workspace.get_untracked() == Some(id)
                && screen.store.graph.get_untracked().nodes.is_empty()
        });
        screen.settle_for("the catalog", WAIT, |screen| {
            !screen.store.catalog.get_untracked().nodes.is_empty()
                && !screen.store.channel_types.get_untracked().is_empty()
                && !screen.store.devices.get_untracked().is_empty()
        });
        Some(screen)
    }

    pub fn store(&self) -> Store {
        self.store
    }

    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        tokio().handle().block_on(future)
    }

    pub(super) fn harness(&mut self) -> &mut Harness<Runtime> {
        self.harness.as_mut().expect("the harness runs until drop")
    }

    fn window(&self) -> &zgui::runtime::window::Window {
        self.harness
            .as_ref()
            .and_then(|harness| harness.app().windows().first())
            .expect("a window")
    }

    pub fn pump(&mut self) {
        for _ in 0..PUMPS {
            if self.harness().pump() == 0 {
                break;
            }
        }
    }

    pub fn frame(&mut self) {
        self.harness().advance(FRAME);
        self.pump();
    }

    pub fn run_for(&mut self, total: Duration) {
        let until = Instant::now() + total;
        while Instant::now() < until {
            self.frame();
            std::thread::sleep(NAP);
        }
    }

    pub fn settle_until(
        &mut self,
        what: &str,
        timeout: Duration,
        done: impl Fn(&Self) -> bool,
    ) -> bool {
        let until = Instant::now() + timeout;
        loop {
            self.frame();
            if done(self) {
                self.run_for(Duration::from_millis(150));
                return true;
            }
            if Instant::now() > until {
                println!("timed out waiting for {what}");
                return false;
            }
            std::thread::sleep(NAP);
        }
    }

    pub fn settle_for(&mut self, what: &str, timeout: Duration, done: impl Fn(&Self) -> bool) {
        assert!(self.settle_until(what, timeout, done), "never saw {what}");
    }

    pub fn find(&self, selector: &str) -> Vec<CssRect> {
        dom::boxes(self.window(), selector)
            .into_iter()
            .filter(|rect| rect.size.width.0 > 0.0 && rect.size.height.0 > 0.0)
            .collect()
    }

    pub fn texts(&self, selector: &str) -> Vec<String> {
        dom::texts(self.window(), selector)
    }

    pub fn nth(&self, selector: &str, index: usize) -> (f32, f32) {
        let found = self.find(selector);
        let rect = found
            .get(index)
            .unwrap_or_else(|| panic!("only {} match {selector}", found.len()));
        dom::center(rect)
    }

    pub fn blank_spot(&self) -> (f32, f32) {
        let pane = self.css_box(".zf-flow").expect("the canvas");
        let taken: Vec<CssRect> = [".zf-node", ".zf-panel"]
            .iter()
            .flat_map(|selector| self.find(selector))
            .collect();
        let margin = 40.0;
        let free = |at: (f32, f32)| {
            !taken.iter().any(|rect| {
                at.0 > rect.origin.x.0 - margin
                    && at.0 < rect.origin.x.0 + rect.size.width.0 + margin
                    && at.1 > rect.origin.y.0 - margin
                    && at.1 < rect.origin.y.0 + rect.size.height.0 + margin
            })
        };
        let (left, top) = (pane.origin.x.0, pane.origin.y.0);
        (1..SPOTS)
            .flat_map(|row| (1..SPOTS).map(move |column| (column, row)))
            .map(|(column, row)| {
                (
                    left + pane.size.width.0 * f32::from(column) / f32::from(SPOTS),
                    top + pane.size.height.0 * f32::from(row) / f32::from(SPOTS),
                )
            })
            .find(|at| free(*at))
            .expect("an empty spot on the canvas")
    }

    pub fn css_box(&self, selector: &str) -> Option<CssRect> {
        self.find(selector).into_iter().next()
    }

    pub fn center_of(&self, selector: &str) -> (f32, f32) {
        let found = self
            .css_box(selector)
            .unwrap_or_else(|| panic!("nothing matches {selector}"));
        dom::center(&found)
    }

    pub fn repaint(&mut self) {
        self.harness()
            .deliver_to_first(SurfaceEvent::Occluded(true));
        self.frame();
        self.harness()
            .deliver_to_first(SurfaceEvent::Occluded(false));
        self.frame();
    }

    pub fn shot(&mut self, name: &str) -> PathBuf {
        self.repaint();
        let pixels = self
            .window()
            .renderer()
            .as_any()
            .and_then(|any| any.downcast_ref::<Offscreen>())
            .expect("the offscreen renderer")
            .read();
        let rgb = picture::rgb(&pixels);
        let path = picture::folder().join(format!("{name}.png"));
        picture::write(&pixels, &rgb, &path).expect("the screenshot saves");
        println!("screen {}", path.display());
        assert!(picture::varied(&rgb), "{name} came out blank");
        path
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        self.host.shutdown();
        if let Some(mut harness) = self.harness.take() {
            harness.shut_down();
            drop(harness);
        }
        zgui::reactive::shutdown();
    }
}
