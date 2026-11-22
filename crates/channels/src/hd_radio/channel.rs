use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_decoder::broadcast::{
    BroadcastService, BroadcastServiceKind, BroadcastStatus, publish_status,
};
use sdrmm_decoder::pacer::Pacer;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};
use sdrmm_media::Media;
use sdrmm_modem::constellation::{Constellation, tables};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, ClickProfile, DecoderFamily, EventContract};
use serde_json::{Map, Value};

use super::{
    HdRadioParams, OCCUPIED_HZ,
    frame::PROGRAMS,
    layout::{BLOCK, BLOCKS, DATA_CARRIERS, INPUT_RATE_HZ, MAX_PARTITIONS, SYMBOL},
    model::program_label,
    playout::{PACKET_FRAMES, Playout},
    receiver::{Block, Receiver},
};

/// Blocks between status reports, about a third of a second.
const REPORT_BLOCKS: u32 = 4;
const SYSTEM: &str = "hd_radio";
const SYSTEM_LABEL: &str = "HD Radio";
const CODE_RATE: &str = "2/5";
const P1_SECONDS: f64 = (SYMBOL * BLOCK * BLOCKS) as f64 / INPUT_RATE_HZ;
/// Symbols of each block the scope sees, a few thousand points a second.
const TAP_SYMBOLS: usize = 4;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "hd_radio".to_owned(),
    name: "HD Radio".to_owned(),
    summary: "NRSC-5 hybrid FM: station information and HDC audio".to_owned(),
    family: DecoderFamily::BROADCAST,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: 2.0 * OCCUPIED_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: true,
    emits: vec![BroadcastStatus::KIND],
    click: ClickProfile::Vocoder,
    ..ChannelDescriptor::default()
});

pub struct HdRadioChannel {
    params: HdRadioParams,
    receiver: Receiver,
    blocks: u32,
    was_locked: bool,
    playout: Playout,
    media: Media,
    pacer: Pacer,
    bitrate_kbps: [Option<u32>; PROGRAMS],
    qpsk: Constellation,
    points: Vec<Complex<f32>>,
}

impl HdRadioChannel {
    fn reset(&mut self) {
        self.receiver.reset();
        self.blocks = 0;
        self.was_locked = false;
        self.playout.reset();
        self.media.reset();
        self.pacer.reset();
        self.bitrate_kbps = [None; PROGRAMS];
    }

    fn selected(&self) -> usize {
        self.params.service.map_or(0, usize::from)
    }

    fn take_block(&mut self, block: Block, out: &mut ChannelOutputs) {
        if block.p1 == Some(true) {
            self.take_frame();
        }
        self.blocks = self.blocks.wrapping_add(1);
        let locked = self.receiver.locked();
        if locked != self.was_locked || self.blocks.is_multiple_of(REPORT_BLOCKS) {
            self.was_locked = locked;
            self.report(out);
        }
    }

    fn tap_block(&mut self, out: &mut ChannelOutputs) {
        if !out.symbols.wanted() {
            return;
        }
        self.points.clear();
        self.points.extend(self.receiver.points(TAP_SYMBOLS));
        out.symbols.carriers(
            &self.points,
            &self.qpsk,
            self.receiver.data_carriers() as f64 * INPUT_RATE_HZ / SYMBOL as f64,
            self.receiver.offset_hz(),
        );
    }

    fn take_frame(&mut self) {
        let frame = &self.receiver.frame;
        let selected = self.selected();
        let lost = frame.lost[selected];
        if lost > 0 {
            self.media
                .audio_gap(PACKET_FRAMES * lost as usize, lost, "HDC packets lost");
        }
        let mut bytes = [0usize; PROGRAMS];
        for (program, packet) in frame.packets() {
            bytes[program] += packet.len();
            if program == selected {
                self.playout.packet(packet, &mut self.media);
            }
        }
        for (rate, bytes) in self.bitrate_kbps.iter_mut().zip(bytes) {
            if bytes > 0 {
                *rate = Some((bytes as f64 * 8.0 / P1_SECONDS / 1000.0).round() as u32);
            }
        }
    }

    fn services(&self) -> Vec<BroadcastService> {
        let station = &self.receiver.station;
        let heard = &self.receiver.frame.programs;
        let selected = self.selected();
        (0..PROGRAMS)
            .filter_map(|program| {
                let kind = station.programs[program]
                    .map(|listed| listed.kind)
                    .or_else(|| heard[program].seen.then_some(heard[program].program_type))?;
                Some(BroadcastService {
                    id: program as u32,
                    label: program_label(program, kind),
                    kind: BroadcastServiceKind::Audio,
                    bitrate_kbps: self.bitrate_kbps[program],
                    language: None,
                    selected: program == selected,
                })
            })
            .collect()
    }

    fn report(&self, out: &mut ChannelOutputs) {
        publish_status(out, &self.status(), "service");
    }

    fn status(&self) -> BroadcastStatus {
        let receiver = &self.receiver;
        let station = &receiver.station;
        let locked = receiver.locked();
        let selected = self.selected();
        let services = self.services();
        let label = services
            .iter()
            .find(|service| service.selected)
            .map(|service| service.label.clone());
        BroadcastStatus {
            system: SYSTEM.to_owned(),
            system_label: SYSTEM_LABEL.to_owned(),
            locked,
            snr_db: if locked { receiver.mer_db() } else { 0.0 },
            frequency_error_hz: receiver.offset_hz() as f32,
            symbol_rate: Some(INPUT_RATE_HZ / SYMBOL as f64),
            transmission_mode: locked.then(|| format!("mp{}", receiver.service_mode())),
            transmission_mode_label: locked.then(|| format!("MP{}", receiver.service_mode())),
            ensemble_id: station.facility_id,
            ensemble_label: station.display_name().map(str::to_owned),
            service_id: locked.then_some(selected as u32),
            label,
            code_rate: locked.then(|| CODE_RATE.to_owned()),
            bitrate_kbps: self.bitrate_kbps[selected],
            bit_error_rate: locked.then(|| receiver.bit_error_rate()),
            text: station.slogan.clone().or_else(|| station.long_name.clone()),
            dynamic_label: receiver.frame.now_playing[selected]
                .clone()
                .or_else(|| station.message.clone()),
            frames_ok: receiver.p1_ok,
            frames_bad: receiver.p1_bad,
            data_groups_ok: station.frames_ok,
            data_groups_bad: station.frames_bad,
            audio_frames_ok: self.media.audio_frames,
            audio_frames_bad: self
                .media
                .audio_errors
                .saturating_add(self.pacer.dropped_frames),
            audio_error: self.media.audio_error.clone().or_else(|| {
                (self.pacer.dropped_frames > 0).then(|| "Audio buffer overflow".to_owned())
            }),
            services,
            extra: station_extra(&receiver.station),
            ..BroadcastStatus::default()
        }
    }
}

/// What only HD Radio says about a station, under its own keys.
fn station_extra(station: &super::sis::Station) -> Map<String, Value> {
    let mut extra = Map::new();
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(value) = value {
            extra.insert(key.to_owned(), value);
        }
    };
    put("call_sign", station.call_sign.clone().map(Value::from));
    put("country", station.country.clone().map(Value::from));
    put("facility_id", station.facility_id.map(Value::from));
    put("long_name", station.long_name.clone().map(Value::from));
    put("slogan", station.slogan.clone().map(Value::from));
    put("message", station.message.clone().map(Value::from));
    put("latitude", station.latitude.map(Value::from));
    put("longitude", station.longitude.map(Value::from));
    extra
}

impl NativeChannel for HdRadioChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        Ok(Self {
            params: settings
                .params
                .parse()
                .map_err(ChannelError::InvalidSettings)?,
            receiver: Receiver::new(),
            blocks: 0,
            was_locked: false,
            playout: Playout::default(),
            media: Media::new()?,
            pacer: Pacer::new(INPUT_RATE_HZ),
            bitrate_kbps: [None; PROGRAMS],
            qpsk: tables::psk_rotated(4, std::f64::consts::FRAC_PI_4)
                .map_err(|error| ChannelError::InvalidSettings(error.to_string()))?,
            points: Vec::with_capacity(TAP_SYMBOLS * 2 * MAX_PARTITIONS * DATA_CARRIERS),
        })
    }
}

impl Channel for HdRadioChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let wanted: HdRadioParams = settings
            .params
            .parse()
            .map_err(ChannelError::InvalidSettings)?;
        if wanted != self.params {
            self.playout.reset();
            self.media.reset();
            self.pacer.reset();
        }
        self.params = wanted;
        Ok(())
    }

    fn retuned(&mut self) {
        self.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let mut rest = iq;
        while !rest.is_empty() {
            let (taken, block) = self.receiver.push(rest);
            rest = &rest[taken..];
            if let Some(block) = block {
                if block.pids.is_some() {
                    self.tap_block(out);
                }
                self.take_block(block, out);
            }
        }
        let from = out.audio_pcm.len();
        self.media.drain(out);
        self.pacer.take(out, from);
        self.pacer.release(iq.len(), out);
    }
}
