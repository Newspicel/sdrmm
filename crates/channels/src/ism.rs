mod bits;
mod device;
mod devices;
mod fsk;
mod pulse;
mod slicer;
mod unknown;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily,
    ISM_BANDWIDTH_HZ, ISM_SAMPLE_RATE_HZ, IsmIntegrity, IsmMessage, IsmParams,
};

use self::{
    bits::Bits,
    device::{DEVICES, Device, Outcome, Tier},
    pulse::{Detector, Package},
    slicer::slice,
    unknown::Unknown,
};
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate, datalink,
};

const RATE: f64 = ISM_SAMPLE_RATE_HZ;
const DEDUP_SAMPLES: u64 = 2 * RATE as u64;
const RECENT: usize = 16;
const PENDING: usize = 16;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "ism".to_owned(),
    name: "ISM sensors".to_owned(),
    summary: "OOK and FSK remotes, weather and tyre sensors".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: ISM_BANDWIDTH_HZ,
    input_rate_hz: RATE,
    has_audio: false,
    decoder_kind: Some("ism".to_owned()),
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<IsmParams, ChannelError> {
    match &settings.params {
        ChannelParams::Ism(params) => Ok(*params),
        other => Err(ChannelError::InvalidSettings(format!(
            "ism channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn occupied_band(_: &IsmParams) -> (f64, f64) {
    let half = ISM_BANDWIDTH_HZ / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(_: &IsmParams) -> ChannelFilter {
    datalink::channel_filter(RATE, ISM_BANDWIDTH_HZ / 2.0)
}

#[derive(Default)]
struct Tally {
    decoded: bool,
    mic: bool,
}

struct Decoders {
    bits: Box<Bits>,
    unknown: Unknown,
    show_unknown: bool,
    pending: Vec<IsmMessage>,
    rejected: u32,
}

impl Decoders {
    fn new(params: IsmParams) -> Self {
        Self {
            bits: Bits::new(),
            unknown: Unknown::new(),
            show_unknown: params.unknown,
            pending: Vec::with_capacity(PENDING),
            rejected: 0,
        }
    }

    fn package(&mut self, package: &Package<'_>) {
        let mut tally = Tally::default();
        for tier in [Tier::First, Tier::Fallback] {
            for device in DEVICES
                .iter()
                .filter(|device| device.tier == tier && device.modulation == package.modulation)
            {
                self.device(device, package, &mut tally);
            }
            if tally.decoded {
                return;
            }
        }
        if self.show_unknown
            && let Some(message) = self.unknown.analyse(package, &mut self.bits)
        {
            self.pending.push(message);
            return;
        }
        if tally.mic {
            self.rejected = self.rejected.saturating_add(1);
        }
    }

    fn device(&mut self, device: &Device, package: &Package<'_>, tally: &mut Tally) {
        let pending = &mut self.pending;
        slice(
            device.coding,
            device.timing,
            package,
            &mut self.bits,
            &mut |bits| {
                let start = pending.len();
                match (device.decode)(bits, pending) {
                    Outcome::Decoded => tally.decoded = true,
                    Outcome::Mic => tally.mic = true,
                    Outcome::Skip => {}
                }
                for message in &mut pending[start..] {
                    message.modulation = package.modulation;
                    message.coding = device.coding;
                    if message.integrity == IsmIntegrity::Repeats {
                        message.integrity = device.integrity;
                    }
                    message.snr_db = package.snr_db;
                }
            },
        );
    }
}

pub struct IsmChannel {
    detector: Detector,
    decoders: Decoders,
    clock: u64,
    recent: [(u64, u64); RECENT],
    next: usize,
}

fn fingerprint(message: &IsmMessage) -> u64 {
    message
        .model
        .bytes()
        .chain([0])
        .chain(message.raw.bytes())
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

impl IsmChannel {
    fn repeat(&mut self, message: &IsmMessage) -> bool {
        let print = fingerprint(message);
        let clock = self.clock;
        let seen = self
            .recent
            .iter()
            .any(|&(at, hash)| hash == print && at > 0 && clock.saturating_sub(at) < DEDUP_SAMPLES);
        self.recent[self.next] = (clock.max(1), print);
        self.next = (self.next + 1) % RECENT;
        seen
    }

    fn flush(&mut self, out: &mut ChannelOutputs) {
        let mut pending = std::mem::take(&mut self.decoders.pending);
        for message in pending.drain(..) {
            if !self.repeat(&message) {
                out.events.push(DecoderEvent::Ism(message));
            }
        }
        self.decoders.pending = pending;
    }
}

impl ChannelRx for IsmChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        Ok(Self {
            detector: Detector::new(),
            decoders: Decoders::new(params(&settings)?),
            clock: 0,
            recent: [(0, 0); RECENT],
            next: 0,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.decoders.show_unknown = params(&settings)?.unknown;
        Ok(())
    }

    fn retuned(&mut self) {
        self.detector.reset();
        self.recent = [(0, 0); RECENT];
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let decoders = &mut self.decoders;
        self.detector
            .process(iq, &mut |package| decoders.package(package));
        self.clock += iq.len() as u64;
        self.flush(out);
    }

    fn rejected(&self) -> u32 {
        self.decoders.rejected
    }
}

#[cfg(test)]
mod tests;
