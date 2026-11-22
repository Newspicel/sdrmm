//! GK-2A LRIT on 1692.14 MHz: 128 kBd BPSK carrying CCSDS frames of LRIT files. AMI full disk
//! images come as ten JPEG segments; KMA encrypts most with DES, so they draw only with its
//! keys. Text and additional data files are handed on.

use sdrmm_decoder::registry::Native;
use sdrmm_wire::{Control, FilterSpec, KindSpec, NumberSpec, SettingField};
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_crypto::Des;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, DecodedImage, NativeChannel, VideoPicture,
    check_input_rate,
};
use sdrmm_modem::ccsds::{Basis, Precoding};
use sdrmm_modem::jpeg::JpegGrey;
use sdrmm_modem::xrit::{
    Headers, ImageSegment, LinkConfig, Mosaic, TransportFile, XritLink, header,
};
use sdrmm_wire::{
    BroadcastData, ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind,
    HzRange, Modulation, SignalSignature,
};

pub const SYMBOL_RATE: f64 = 128_000.0;
const SPS: usize = 2;
pub(crate) const INPUT_RATE_HZ: f64 = SPS as f64 * SYMBOL_RATE;
pub(crate) const ROLL_OFF: f64 = 0.5;
const FILTER_TAPS: usize = 63;
const CHUNK: usize = 8_192;
/// One decision point in this many reaches the scope, some sixteen thousand a second.
const TAP_EVERY: usize = 8;
const SESSIONS: usize = 8;
const CANVASES: usize = 2;
const MAX_SIDE: usize = 1_100;
const DES_BLOCK: usize = 8;
const KEY_DIGITS: usize = 16;
const SOURCE: &str = "gk2a_lrit";
const DOWNLINK_HZ: f64 = 1_692_140_000.0;
/// KMA's LRIT full disk comes in ten segments.
const FULL_DISK_SEGMENTS: u16 = 10;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "gk2a_lrit".to_owned(),
    name: "GK-2A LRIT".to_owned(),
    summary: "GK-2A weather satellite pictures and bulletins".to_owned(),
    family: DecoderFamily::WEATHER,
    verification: sdrmm_wire::Verification::OwnModulator,
    bandwidth_hz: SYMBOL_RATE * (1.0 + ROLL_OFF),
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    has_video: true,
    emits: vec![EventKind::named("gk2a_lrit"), EventKind::BROADCAST_DATA],
    ..ChannelDescriptor::default()
});

fn gk2a_lrit_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "GK-2A LRIT",
            &[Modulation::Psk2],
            HzRange::new(150_000.0, 220_000.0),
            "128 kBd BPSK from GK-2A",
        )
        .on(&[HzRange::mhz(1_692.0, 1_692.3)]),
    ]
}

pub(crate) fn gk2a_lrit_decoder() -> Native<Gk2aLritParams> {
    let decoder = Native::new::<Gk2aLritChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .home(DOWNLINK_HZ)
        .signatures(gk2a_lrit_signatures)
        .form(gk2a_lrit_form)
        .kinds(gk2a_lrit_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn gk2a_lrit_form() -> Vec<SettingField> {
    let entry = vec![
        SettingField::new(
            "index",
            "Index",
            NumberSpec::new(0.0, f64::from(u16::MAX), 1.0),
        ),
        SettingField::new("key", "Key", Control::bounded_text("hex", KEY_DIGITS, true)),
    ];
    vec![
        SettingField::new("keys", "Keys", Control::list(entry, "none"))
            .title("KMA's DES keys, by the index each file names")
            .quiet(Vec::<Gk2aKey>::new()),
    ]
}

fn gk2a_lrit_kinds() -> Vec<KindSpec> {
    vec![model::gk2a_lrit_kind()]
}

pub(crate) fn occupied_band() -> (f64, f64) {
    let half = DESCRIPTOR.bandwidth_hz / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter() -> FilterSpec {
    let (_, half) = occupied_band();
    FilterSpec::lowpass(FILTER_TAPS, half)
}

fn params(settings: &ChannelSettings) -> Result<Gk2aLritParams, ChannelError> {
    sdrmm_decoder::settings_of::<Gk2aLritParams>(settings)
}

fn link() -> Result<XritLink, ChannelError> {
    XritLink::new(LinkConfig {
        sps: SPS,
        roll_off: ROLL_OFF,
        basis: Basis::Dual,
        precoding: Precoding::Nrzl,
        sessions: SESSIONS,
    })
    .map_err(|error| ChannelError::InvalidSettings(error.to_string()))
}

fn ciphers(params: &Gk2aLritParams) -> Result<Vec<(u16, Des)>, ChannelError> {
    params
        .keys
        .iter()
        .map(|key| {
            key.bytes()
                .map(|bytes| (key.index, Des::new(bytes)))
                .ok_or_else(|| {
                    ChannelError::InvalidSettings(format!(
                        "key {} needs {KEY_DIGITS} hex digits",
                        key.index
                    ))
                })
        })
        .collect()
}

/// What a file's data are, by their first bytes.
fn sniff(file_type: u8, data: &[u8]) -> &'static str {
    match data {
        [0xFF, 0xD8, ..] => "image/jpeg",
        [b'G', b'I', b'F', b'8', ..] => "image/gif",
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        _ if file_type == header::TEXT => "text/plain",
        _ => "application/octet-stream",
    }
}

/// The AMI channel and segment number of `IMG_FD_047_IR105_20190713_022006_01.lrit`.
fn image_name(name: &str) -> (Option<&str>, Option<u16>) {
    let stem = name.split('.').next().unwrap_or(name);
    let parts: Vec<&str> = stem.split('_').collect();
    let channel = parts
        .get(3)
        .copied()
        .filter(|_| parts.first() == Some(&"IMG"));
    let segment = parts.last().and_then(|part| part.parse().ok());
    (channel, segment)
}

/// Where a segment goes. KMA's segment record is four bytes: the segment, how many there are,
/// and the first line counted from one. Without it the file name gives the segment.
struct Placement {
    expected: u16,
    start_line: usize,
}

impl Placement {
    fn of(headers: &Headers<'_>, name: &str, lines: usize) -> Self {
        if let Some(&[sequence, total, high, low]) = headers.record(header::SEGMENT) {
            let first = usize::from(u16::from_be_bytes([high, low]));
            let start_line = match first {
                0 => usize::from(sequence.saturating_sub(1)) * lines,
                first => first - 1,
            };
            return Self {
                expected: u16::from(total).max(1),
                start_line,
            };
        }
        let number = image_name(name).1.unwrap_or(1);
        Self {
            expected: FULL_DISK_SEGMENTS,
            start_line: usize::from(number.saturating_sub(1)) * lines,
        }
    }
}

struct Files {
    mosaic: Mosaic,
    jpeg: JpegGrey,
    pixels: Vec<u8>,
    seq: u32,
    undecodable: u32,
}

impl Files {
    fn new() -> Self {
        Self {
            mosaic: Mosaic::new(CANVASES, MAX_SIDE),
            jpeg: JpegGrey::new(),
            pixels: Vec::new(),
            seq: 0,
            undecodable: 0,
        }
    }

    /// Decrypts `file`'s data in place when its key is known; `false` when it stays locked.
    fn unlock(file: &mut TransportFile<'_>, ciphers: &[(u16, Des)]) -> bool {
        let Some(headers) = Headers::parse(file.bytes) else {
            return false;
        };
        let index = headers.key().unwrap_or(0);
        if index == 0 {
            return true;
        }
        let Some((_, des)) = ciphers.iter().find(|(found, _)| *found == index) else {
            return false;
        };
        let start = headers.primary.header_len;
        let end = (start + headers.primary.data_len()).min(file.bytes.len());
        for block in file.bytes[start..end].as_chunks_mut::<DES_BLOCK>().0 {
            des.decrypt(block);
        }
        true
    }

    fn take(
        &mut self,
        mut file: TransportFile<'_>,
        ciphers: &[(u16, Des)],
        out: &mut ChannelOutputs,
    ) {
        let open = Self::unlock(&mut file, ciphers);
        let Some(headers) = Headers::parse(file.bytes) else {
            return;
        };
        let name = headers.annotation().unwrap_or_default();
        let start = headers.primary.header_len;
        let end = (start + headers.primary.data_len()).min(file.bytes.len());
        let data = &file.bytes[start..end];
        if open
            && headers.primary.file_type == header::IMAGE
            && self.draw(&headers, name, data, out)
        {
            return;
        }
        let (media_type, bytes) = if open {
            (sniff(headers.primary.file_type, data), data)
        } else {
            ("application/octet-stream", &file.bytes[..])
        };
        out.events.push(DecoderEvent::of(&BroadcastData {
            protocol: None,
            label: Vec::new(),
            service_id: Some(u32::from(file.vcid)),
            name: name.to_owned(),
            media_type: media_type.to_owned(),
            bytes: bytes.to_vec(),
        }));
    }

    /// Draws one JPEG segment; `false` when it is not one this can draw.
    fn draw(
        &mut self,
        headers: &Headers<'_>,
        name: &str,
        data: &[u8],
        out: &mut ChannelOutputs,
    ) -> bool {
        let Ok((columns, lines)) = self.jpeg.decode(data, &mut self.pixels) else {
            self.undecodable = self.undecodable.saturating_add(1);
            return false;
        };
        let place = Placement::of(headers, name, lines);
        let stem = name.rsplit_once('_').map_or(name, |(stem, _)| stem);
        let key = stem.bytes().fold(0u64, |hash, byte| {
            hash.wrapping_mul(0x100_0000_01B3) ^ u64::from(byte)
        });
        if let Some(victim) = self.mosaic.victim_for(key) {
            self.finish(victim, out);
        }
        let placed = ImageSegment {
            key,
            width: columns,
            height: usize::from(place.expected) * lines,
            start_line: place.start_line,
            columns,
            pixels: &self.pixels,
            expected: place.expected,
            lines_missing: 0,
        };
        let Some(index) = self.mosaic.place(&placed, name) else {
            return false;
        };
        let canvas = self.mosaic.canvas(index);
        if canvas.complete() {
            self.finish(index, out);
        } else {
            out.video.push(VideoPicture::grey(
                canvas.width as u16,
                canvas.height as u16,
                &canvas.luma,
            ));
        }
        true
    }

    fn finish(&mut self, index: usize, out: &mut ChannelOutputs) {
        let canvas = self.mosaic.canvas(index);
        self.seq = self.seq.wrapping_add(1);
        let picture = VideoPicture::grey(canvas.width as u16, canvas.height as u16, &canvas.luma);
        let channel = image_name(&canvas.name).0.map(str::to_owned);
        let complete = canvas.complete();
        out.events.push(DecoderEvent::of(&Gk2aLritImage {
            seq: self.seq,
            name: canvas.name.clone(),
            channel: channel.clone(),
            width: picture.width,
            lines: picture.height,
            segments: canvas.segments,
            segments_expected: canvas.expected,
            complete,
        }));
        out.video.push(picture.clone());
        out.images.push(DecodedImage {
            source: SOURCE.into(),
            mode: ["GK-2A".to_owned()]
                .into_iter()
                .chain(channel)
                .collect::<Vec<_>>()
                .join(" "),
            complete,
            lines: picture.height,
            picture,
        });
        self.mosaic.release(index);
    }

    fn close(&mut self, out: &mut ChannelOutputs) {
        let open: Vec<usize> = self.mosaic.open().collect();
        for index in open {
            self.finish(index, out);
        }
    }
}

pub struct Gk2aLritChannel {
    ciphers: Vec<(u16, Des)>,
    link: XritLink,
    files: Files,
    points: Vec<Complex<f32>>,
    closing: bool,
}

impl Gk2aLritChannel {
    fn tap_symbols(&mut self, out: &mut ChannelOutputs) {
        if !out.symbols.wanted() {
            return;
        }
        self.points.clear();
        self.points
            .extend(self.link.symbols().iter().step_by(TAP_EVERY).copied());
        out.symbols.linear(
            &self.points,
            self.link.constellation(),
            SYMBOL_RATE,
            self.link.frequency_offset_cycles_per_sample() * INPUT_RATE_HZ,
        );
    }
}

impl NativeChannel for Gk2aLritChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        Ok(Self {
            ciphers: ciphers(&params(&settings)?)?,
            link: link()?,
            files: Files::new(),
            points: Vec::with_capacity(CHUNK),
            closing: false,
        })
    }
}

impl Channel for Gk2aLritChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.ciphers = ciphers(&params(&settings)?)?;
        Ok(())
    }

    fn retuned(&mut self) {
        self.link.reset();
        self.closing = true;
    }

    fn rejected(&self) -> u32 {
        self.link
            .stats()
            .lost()
            .saturating_add(self.files.undecodable)
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        if self.closing {
            self.closing = false;
            self.files.close(out);
        }
        for chunk in iq.chunks(CHUNK) {
            let files = &mut self.files;
            let ciphers = &self.ciphers;
            self.link
                .process(chunk, &mut |file| files.take(file, ciphers, out));
            self.tap_symbols(out);
        }
    }
}
