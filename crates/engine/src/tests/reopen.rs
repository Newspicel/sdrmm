use std::sync::{Arc, atomic::AtomicU64};

use sdrmm_channels::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelLayout, ChannelOutputs, ChannelRx, Decoder,
    install_plugins,
};
use sdrmm_wire::ChannelDescriptor;

use super::*;

const PROBE: &str = "reload_probe";
const PROBE_RATE_HZ: f64 = 48_000.0;

/// Blocks each probe version has processed, so a test sees which one runs.
static PROCESSED: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

fn processed(version: usize) -> u64 {
    PROCESSED[version].load(Ordering::Relaxed)
}

struct ProbeRx(usize);

impl ChannelRx for ProbeRx {
    fn apply(&mut self, _settings: ChannelSettings) -> Result<(), ChannelError> {
        Ok(())
    }

    fn process(&mut self, _iq: &[Complex<f32>], _out: &mut ChannelOutputs) {
        PROCESSED[self.0].fetch_add(1, Ordering::Relaxed);
    }
}

/// A decoder standing in for one build of a plugin; `broken` builds refuse to open.
struct Probe {
    version: usize,
    broken: bool,
    descriptor: ChannelDescriptor,
}

fn probe(version: usize, broken: bool) -> Arc<dyn Decoder> {
    Arc::new(Probe {
        version,
        broken,
        descriptor: ChannelDescriptor {
            type_id: PROBE.to_owned(),
            name: "Probe".to_owned(),
            bandwidth_hz: 10_000.0,
            input_rate_hz: PROBE_RATE_HZ,
            has_audio: false,
            ..ChannelDescriptor::default()
        },
    })
}

impl Decoder for Probe {
    fn descriptor(&self) -> &ChannelDescriptor {
        &self.descriptor
    }

    fn create_rx(
        &self,
        _ctx: ChannelCtx,
        _settings: &ChannelSettings,
    ) -> Result<Box<dyn ChannelRx>, ChannelError> {
        if self.broken {
            return Err(ChannelError::InvalidSettings("broken build".to_owned()));
        }
        Ok(Box::new(ProbeRx(self.version)))
    }

    fn layout(&self, _params: &ChannelParams) -> Result<ChannelLayout, ChannelError> {
        Ok(ChannelLayout {
            input_rate_hz: PROBE_RATE_HZ,
            band: (-5_000.0, 5_000.0),
            audio_channels: 1,
        })
    }

    fn filter(&self, _params: &ChannelParams) -> Result<ChannelFilter, ChannelError> {
        Ok(ChannelFilter::Passthrough)
    }

    fn normalize(&self, params: &ChannelParams) -> Result<ChannelParams, ChannelError> {
        Ok(params.clone())
    }
}

fn probe_settings() -> ChannelSettings {
    ChannelSettings {
        frequency_hz: TEST_CENTER_HZ,
        squelch: sdrmm_wire::Squelch::Off,
        params: ChannelParams::bare(PROBE),
        blanker: Default::default(),
        if_filter: Default::default(),
    }
}

async fn runs(version: usize) {
    let since = processed(version);
    let deadline = Instant::now() + Duration::from_secs(5);
    while processed(version) == since {
        assert!(Instant::now() < deadline, "probe {version} never ran");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn a_replaced_decoder_reopens_its_running_channels_in_place() {
    install_plugins(vec![probe(1, false)]).expect("probe installs");
    let engine = virtual_engine();
    let ds = engine.create_device_set("virtual:band").unwrap();
    let ch = engine.add_channel(ds, 0, probe_settings()).unwrap();
    runs(1).await;

    let replaced = install_plugins(vec![probe(2, false)]).expect("probe installs");
    assert_eq!(replaced, [PROBE]);
    assert!(engine.reopen_decoders(&replaced).is_empty());
    runs(2).await;
    let channels = &engine.snapshot().device_sets[0].channels;
    assert_eq!(channels.len(), 1);
    assert_eq!(
        (channels[0].id, &channels[0].settings),
        (ch, &probe_settings())
    );

    let mut events = engine.subscribe_events();
    let replaced = install_plugins(vec![probe(3, true)]).expect("probe installs");
    let errors = engine.reopen_decoders(&replaced);
    assert_eq!(errors.len(), 1, "{errors:?}");
    runs(2).await;
    assert_eq!(engine.snapshot().device_sets[0].channels.len(), 1);
    let refused = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Ok(ServerEvent::Error { message }) = events.recv().await {
                return message;
            }
        }
    })
    .await
    .expect("the failed reopen is reported");
    assert!(refused.contains("broken build"), "{refused}");

    install_plugins(Vec::new()).expect("probe leaves");
    engine.remove_device_set(ds).unwrap();
}
