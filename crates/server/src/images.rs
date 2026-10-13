use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::body::Bytes;
use sdrmm_engine::{ImageCapture, VideoPicture};
use sdrmm_wire::{
    CapturedImage, ChannelAddr, EventImage, Keeps, PortDirection, ResolvedPatch, StateScope,
    VIDEO_PORT,
};
use tokio::{
    sync::broadcast::error::RecvError,
    time::{MissedTickBehavior, interval},
};

const RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
const RECONCILE_INTERVAL: Duration = Duration::from_secs(60);
const MAX_STORED_IMAGES: usize = 512;
const MAX_STORED_BYTES: usize = 256 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct Images {
    inner: Mutex<StoredImages>,
}

#[derive(Default)]
struct StoredImages {
    next_id: u64,
    images: VecDeque<StoredImage>,
    bytes: usize,
}

struct StoredImage {
    image: CapturedImage,
    png: Option<Bytes>,
    expires: Instant,
}

impl Images {
    pub(crate) fn list(&self, sink: &str) -> Vec<CapturedImage> {
        let mut inner = self.lock();
        prune(&mut inner);
        inner
            .images
            .iter()
            .rev()
            .filter(|item| item.image.sinks.iter().any(|held| held == sink))
            .map(|item| item.image.clone())
            .collect()
    }

    pub(crate) fn png(&self, id: u64) -> Option<Bytes> {
        let mut inner = self.lock();
        prune(&mut inner);
        inner
            .images
            .iter()
            .find(|item| item.image.id == id)
            .and_then(|item| item.png.clone())
    }

    fn expire(&self) -> bool {
        let mut inner = self.lock();
        let before = inner.images.len();
        prune(&mut inner);
        inner.images.len() != before
    }

    fn push(
        &self,
        (capture, route): (&ImageCapture, Route),
        png: Result<Bytes, String>,
    ) -> (CapturedImage, bool) {
        let mut inner = self.lock();
        prune(&mut inner);
        inner.next_id += 1;
        let id = inner.next_id;
        let (png, error) = match png {
            Ok(bytes) => (Some(bytes), None),
            Err(message) => (None, Some(message)),
        };
        let image = CapturedImage {
            id,
            node: route.node,
            sinks: route.sinks,
            device_set: capture.device_set,
            channel: capture.channel,
            at: capture.at.clone(),
            freq_hz: capture.freq_hz,
            source: capture.source.to_owned(),
            mode: capture.mode.clone(),
            width: capture.picture.width,
            height: capture.picture.height,
            lines: capture.lines,
            complete: capture.complete,
            image: png.as_ref().map(|_| EventImage {
                url: crate::rest::captured_image_path(id),
                media_type: "image/png".to_owned(),
            }),
            image_error: error,
        };
        inner.bytes += png.as_ref().map_or(0, Bytes::len);
        inner.images.push_back(StoredImage {
            image: image.clone(),
            png,
            expires: Instant::now() + RETENTION,
        });
        while inner.images.len() > MAX_STORED_IMAGES {
            let dropped = inner.images.pop_front();
            inner.bytes -= dropped
                .and_then(|item| item.png)
                .as_ref()
                .map_or(0, Bytes::len);
        }
        let evicted = evict(&mut inner);
        (image, evicted)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StoredImages> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn prune(inner: &mut StoredImages) {
    let now = Instant::now();
    let mut freed = 0;
    inner.images.retain(|item| {
        let keep = item.expires > now;
        if !keep {
            freed += item.png.as_ref().map_or(0, Bytes::len);
        }
        keep
    });
    inner.bytes -= freed;
}

fn evict(inner: &mut StoredImages) -> bool {
    let mut evicted = false;
    for item in &mut inner.images {
        if inner.bytes <= MAX_STORED_BYTES {
            break;
        }
        let Some(png) = item.png.take() else {
            continue;
        };
        inner.bytes -= png.len();
        item.image.image = None;
        item.image
            .image_error
            .get_or_insert_with(|| "picture evicted by the temporary buffer limit".to_owned());
        evicted = true;
    }
    evicted
}

pub(crate) fn encode_png(picture: &VideoPicture) -> Result<Bytes, String> {
    let width = u32::from(picture.width);
    let height = u32::from(picture.height);
    if width == 0 || height == 0 {
        return Err("picture has no pixels".to_owned());
    }
    let pixels = (width as usize) * (height as usize);
    let (color, data) = if picture.rgb.len() == pixels * 3 {
        (png::ColorType::Rgb, picture.rgb.as_slice())
    } else if picture.luma.len() == pixels {
        (png::ColorType::Grayscale, picture.luma.as_slice())
    } else {
        return Err(format!(
            "picture claims {width}×{height} but carries {} rgb and {} luma bytes",
            picture.rgb.len(),
            picture.luma.len()
        ));
    };
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|err| format!("png header: {err}"))?;
    writer
        .write_image_data(data)
        .map_err(|err| format!("png data: {err}"))?;
    writer
        .finish()
        .map_err(|err| format!("png finish: {err}"))?;
    Ok(Bytes::from(out))
}

pub(crate) struct Route {
    pub(crate) node: String,
    pub(crate) sinks: Vec<String>,
}

pub(crate) fn route(patch: &ResolvedPatch, capture: &ImageCapture) -> Option<Route> {
    let node = patch.owner_of(ChannelAddr {
        device_set: capture.device_set,
        channel: capture.channel,
    })?;
    let sinks = patch
        .reaches(node, VIDEO_PORT)
        .iter()
        .filter(|reach| {
            patch
                .node(&reach.sink)
                .and_then(|sink| sink.input(&reach.port))
                .is_some_and(|port| {
                    port.spec.direction == PortDirection::In
                        && port.spec.keeps == Some(Keeps::Pictures)
                })
        })
        .map(|reach| reach.sink.clone())
        .collect();
    Some(Route {
        node: node.to_owned(),
        sinks,
    })
}

pub(crate) async fn run(state: crate::AppState) {
    let mut captures = state.engine.subscribe_images();
    let mut ticker = interval(RECONCILE_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            received = captures.recv() => match received {
                Ok(capture) => store(&capture, &state),
                Err(RecvError::Lagged(count)) => {
                    tracing::warn!(count, "captured pictures dropped: the store fell behind");
                }
                Err(RecvError::Closed) => break,
            },
            _ = ticker.tick() => {
                if state.images.expire() {
                    reload(&state);
                }
            }
        }
    }
}

fn store(capture: &ImageCapture, state: &crate::AppState) {
    let resolved = state.resolved.current();
    let Some(route) = route(&resolved.patch, capture) else {
        tracing::error!(
            source = capture.source,
            "a picture came from a channel no node owns"
        );
        return;
    };
    if route.sinks.is_empty() {
        state.activity.produced(&route.node, VIDEO_PORT, false);
        return;
    }
    state.activity.produced(&route.node, VIDEO_PORT, true);
    let png = encode_png(&capture.picture);
    if let Err(error) = &png {
        tracing::error!(
            error,
            source = capture.source,
            "a captured picture was lost"
        );
    }
    let (image, evicted) = state.images.push((capture, route), png);
    state.sinks.picture(&image);
    if evicted {
        reload(state);
    }
}

fn reload(state: &crate::AppState) {
    state.sinks.reload_pictures(|sink| state.images.list(sink));
    state.engine.emit_scope(StateScope::Images);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sdrmm_wire::{
        ChannelNode, NodeBody, NodeKind, PatchEdge, PatchGraph, PatchNode, PortRef, Position, Runs,
        patch::resolve::{ResolveCtx, compile},
    };

    use super::*;

    fn route_to(sinks: &[&str]) -> Route {
        Route {
            node: "sstv".to_owned(),
            sinks: sinks.iter().map(|sink| (*sink).to_owned()).collect(),
        }
    }

    fn node(id: &str, body: NodeBody) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body,
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn patch(wired: bool) -> ResolvedPatch {
        let edges = if wired {
            vec![PatchEdge {
                from: PortRef {
                    node: "sstv".to_owned(),
                    port: VIDEO_PORT.to_owned(),
                },
                to: PortRef {
                    node: "screen".to_owned(),
                    port: VIDEO_PORT.to_owned(),
                },
            }]
        } else {
            Vec::new()
        };
        let graph = PatchGraph {
            nodes: vec![
                node(
                    "sstv",
                    NodeBody::Channel(ChannelNode {
                        channel_type: "sstv".to_owned(),
                        tuning_locked: false,
                    }),
                ),
                node("screen", NodeBody::default_for(NodeKind::Video)),
            ],
            edges,
        };
        let mut patch = compile(&graph, &ResolveCtx::default()).expect("compiles");
        for node in &mut patch.nodes {
            if node.id == "sstv" {
                node.runs = Runs::Channel {
                    device_set: 1,
                    channel: 2,
                    stream: 0,
                };
            }
        }
        patch
    }

    #[test]
    fn an_unwired_channel_keeps_no_picture_and_a_wired_one_names_its_video_nodes() {
        let unwired = route(&patch(false), &capture(8, 8)).expect("the channel owns it");
        assert_eq!(unwired.node, "sstv");
        assert!(
            unwired.sinks.is_empty(),
            "no video wire, nowhere to keep it"
        );
        let wired = route(&patch(true), &capture(8, 8)).expect("the channel owns it");
        assert_eq!(wired.sinks, ["screen"]);
        let stranger = ImageCapture {
            channel: 9,
            ..capture(8, 8)
        };
        assert!(route(&patch(true), &stranger).is_none());
    }

    #[test]
    fn a_picture_is_listed_for_each_video_node_it_reached() {
        let images = Images::default();
        images.push(
            (&capture(8, 8), route_to(&["screen"])),
            encode_png(&picture(8, 8)),
        );
        images.push(
            (&capture(8, 8), route_to(&["other"])),
            encode_png(&picture(8, 8)),
        );
        let listed = images.list("screen");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].node, "sstv");
        assert_eq!(listed[0].sinks, ["screen"]);
        assert!(images.list("nobody").is_empty());
    }

    fn picture(width: u16, height: u16) -> VideoPicture {
        let pixels = usize::from(width) * usize::from(height);
        VideoPicture {
            width,
            height,
            luma: vec![128; pixels],
            rgb: (0..pixels * 3).map(|i| (i % 251) as u8).collect(),
        }
    }

    fn capture(width: u16, height: u16) -> ImageCapture {
        ImageCapture {
            device_set: 1,
            channel: 2,
            at: "2026-08-16T10:00:00Z".to_owned(),
            freq_hz: 14_230_000.0,
            source: "sstv",
            mode: "Martin M1".to_owned(),
            complete: true,
            lines: height,
            picture: Arc::new(picture(width, height)),
        }
    }

    #[test]
    fn a_png_carries_the_signature_and_the_stated_size() {
        let bytes = encode_png(&picture(320, 256)).expect("encodes");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&bytes[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 320);
        assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 256);
        assert_eq!(bytes[25], png::ColorType::Rgb as u8);
    }

    #[test]
    fn a_luma_only_picture_encodes_as_greyscale() {
        let mut only_luma = picture(64, 32);
        only_luma.rgb.clear();
        let bytes = encode_png(&only_luma).expect("encodes");
        assert_eq!(bytes[25], png::ColorType::Grayscale as u8);
    }

    #[test]
    fn a_picture_whose_pixels_do_not_match_its_size_is_refused() {
        let mut broken = picture(64, 32);
        broken.rgb.truncate(7);
        broken.luma.truncate(3);
        let err = encode_png(&broken).expect_err("refuses");
        assert!(err.contains("64×32"), "{err}");
        assert!(encode_png(&picture(0, 0)).is_err());
    }

    #[test]
    fn a_stored_picture_is_listed_with_a_url_to_its_png() {
        let images = Images::default();
        let (stored, _) = images.push(
            (&capture(320, 256), route_to(&["screen"])),
            encode_png(&picture(320, 256)),
        );
        assert_eq!(stored.id, 1);
        assert_eq!(stored.source, "sstv");
        assert_eq!(stored.mode, "Martin M1");
        assert_eq!((stored.width, stored.height), (320, 256));
        assert_eq!(
            stored.image.as_ref().map(|image| image.url.as_str()),
            Some("/api/images/1/png")
        );
        assert!(stored.image_error.is_none());
        assert_eq!(images.list("screen").len(), 1);
        let png = images.png(1).expect("png");
        assert_eq!(&png[..4], b"\x89PNG");
        assert!(images.png(2).is_none());
    }

    #[test]
    fn a_picture_that_failed_to_encode_is_kept_and_says_why() {
        let images = Images::default();
        let (stored, _) = images.push(
            (&capture(320, 256), route_to(&["screen"])),
            Err("no pixels".to_owned()),
        );
        assert!(stored.image.is_none());
        assert_eq!(stored.image_error.as_deref(), Some("no pixels"));
        assert_eq!(images.list("screen").len(), 1);
    }

    #[test]
    fn evicting_pixels_keeps_the_record_and_says_why() {
        let images = Images::default();
        let big = Bytes::from(vec![0u8; MAX_STORED_BYTES / 2 + 1]);
        let mut evictions = 0;
        for _ in 0..3 {
            let (_, evicted) =
                images.push((&capture(320, 256), route_to(&["screen"])), Ok(big.clone()));
            evictions += usize::from(evicted);
        }
        assert!(evictions >= 1, "eviction was never reported");
        let listed = images.list("screen");
        assert_eq!(listed.len(), 3);
        assert!(
            listed
                .iter()
                .filter(|image| image.image.is_none())
                .all(|image| image.image_error.is_some())
        );
        assert!(images.lock().bytes <= MAX_STORED_BYTES);
    }

    #[test]
    fn the_store_keeps_only_the_newest_pictures() {
        let images = Images::default();
        for _ in 0..MAX_STORED_IMAGES + 4 {
            images.push(
                (&capture(8, 8), route_to(&["screen"])),
                encode_png(&picture(8, 8)),
            );
        }
        let listed = images.list("screen");
        assert_eq!(listed.len(), MAX_STORED_IMAGES);
        assert_eq!(listed[0].id, (MAX_STORED_IMAGES + 4) as u64);
    }
}
