//! GOES-R HRIT on 1694.1 MHz: 927 kBd BPSK carrying CCSDS frames of LRIT/HRIT files. ABI images
//! come as Rice-coded lines and are drawn; EMWIN bulletins, DCS data and admin messages are
//! handed on as files.

use sdrmm_decoder::registry::Native;
use sdrmm_wire::{Control, FilterSpec, KindSpec, SettingField};
mod model;
pub(crate) mod product;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use num_complex::Complex;
use product::Product;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, DecodedImage, NativeChannel, VideoPicture,
    check_input_rate,
};
use sdrmm_modem::ccsds::{Basis, Precoding};
use sdrmm_modem::xrit::{
    Canvas, Headers, ImageSegment, LinkConfig, Mosaic, TransportFile, XritLink, header,
};
use sdrmm_wire::{
    BroadcastData, ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind,
    HzRange, Modulation, SignalSignature,
};

pub const SYMBOL_RATE: f64 = 927_000.0;
const SPS: usize = 2;
pub(crate) const INPUT_RATE_HZ: f64 = SPS as f64 * SYMBOL_RATE;
pub(crate) const ROLL_OFF: f64 = 0.3;
/// HRIT NRZ-M codes ahead of the convolutional encoder; LRIT does not.
pub(crate) const PRECODING: Precoding = Precoding::Nrzm;
const FILTER_TAPS: usize = 63;
const CHUNK: usize = 16_384;
/// Files assembled at once: every virtual channel can have one or two going.
const SESSIONS: usize = 32;
const CANVASES: usize = 6;
/// Pictures are scaled down until their longer side fits.
const MAX_SIDE: usize = 1_400;
const SOURCE: &str = "goes_hrit";
const DOWNLINK_HZ: f64 = 1_694_100_000.0;

/// NOAA's compression codes in its LRIT header.
const NOAA_NONE: u8 = 0;
const NOAA_RICE: u8 = 1;
const NOAA_JPEG: u8 = 2;
const NOAA_GIF: u8 = 5;
const NOAA_ZIP: u8 = 10;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "goes_hrit".to_owned(),
    name: "GOES HRIT".to_owned(),
    summary: "GOES-R weather satellite pictures and bulletins".to_owned(),
    family: DecoderFamily::WEATHER,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: SYMBOL_RATE * (1.0 + ROLL_OFF),
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    has_video: true,
    emits: vec![EventKind::named("goes_hrit"), EventKind::BROADCAST_DATA],
    ..ChannelDescriptor::default()
});

fn goes_hrit_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "GOES HRIT",
            &[Modulation::Psk2],
            HzRange::new(1_100_000.0, 1_300_000.0),
            "927 kBd BPSK from a GOES-R satellite",
        )
        .on(&[HzRange::mhz(1_694.0, 1_694.2)]),
    ]
}

pub(crate) fn goes_hrit_decoder() -> Native<GoesHritParams> {
    let decoder = Native::new::<GoesHritChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .home(DOWNLINK_HZ)
        .signatures(goes_hrit_signatures)
        .form(goes_hrit_form)
        .kinds(goes_hrit_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn goes_hrit_form() -> Vec<SettingField> {
    vec![
        SettingField::new("picture", "Picture", Control::choice::<GoesPicture>())
            .title("Which ABI images to draw; every file is still logged"),
    ]
}

fn goes_hrit_kinds() -> Vec<KindSpec> {
    vec![model::goes_hrit_kind()]
}

pub(crate) fn occupied_band() -> (f64, f64) {
    let half = DESCRIPTOR.bandwidth_hz / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter() -> FilterSpec {
    let (_, half) = occupied_band();
    FilterSpec::lowpass(FILTER_TAPS, half)
}

fn params(settings: &ChannelSettings) -> Result<GoesHritParams, ChannelError> {
    sdrmm_decoder::settings_of::<GoesHritParams>(settings)
}

fn link() -> Result<XritLink, ChannelError> {
    XritLink::new(LinkConfig {
        sps: SPS,
        roll_off: ROLL_OFF,
        basis: Basis::Dual,
        precoding: PRECODING,
        sessions: SESSIONS,
    })
    .map_err(|error| ChannelError::InvalidSettings(error.to_string()))
}

fn media_type(file_type: u8, compression: Option<u8>) -> &'static str {
    match compression {
        Some(NOAA_ZIP) => "application/zip",
        Some(NOAA_GIF) => "image/gif",
        Some(NOAA_JPEG) => "image/jpeg",
        Some(NOAA_NONE) | None if file_type == header::TEXT => "text/plain",
        _ => "application/octet-stream",
    }
}

/// What the files of the link are turned into: pictures, and every other file handed on.
struct Pictures {
    mosaic: Mosaic,
    seq: u32,
}

impl Pictures {
    fn new() -> Self {
        Self {
            mosaic: Mosaic::new(CANVASES, MAX_SIDE),
            seq: 0,
        }
    }

    fn take(&mut self, file: &TransportFile<'_>, picture: GoesPicture, out: &mut ChannelOutputs) {
        let Some(headers) = Headers::parse(file.bytes) else {
            return;
        };
        let name = headers.annotation().unwrap_or_default();
        let start = headers.primary.header_len;
        let end = (start + headers.primary.data_len()).min(file.bytes.len());
        let data = &file.bytes[start..end];
        let compression = headers.noaa().map(|noaa| noaa.compression);
        let drawable = headers.primary.file_type == header::IMAGE
            && matches!(compression, Some(NOAA_NONE | NOAA_RICE));
        if drawable {
            self.draw(&headers, name, data, file, picture, out);
            return;
        }
        out.events.push(DecoderEvent::of(&BroadcastData {
            protocol: None,
            label: Vec::new(),
            service_id: Some(u32::from(file.vcid)),
            name: name.to_owned(),
            media_type: media_type(headers.primary.file_type, compression).to_owned(),
            bytes: data.to_vec(),
        }));
    }

    fn draw(
        &mut self,
        headers: &Headers<'_>,
        name: &str,
        data: &[u8],
        file: &TransportFile<'_>,
        picture: GoesPicture,
        out: &mut ChannelOutputs,
    ) {
        if !Product::parse(name).wanted(picture) {
            return;
        }
        let Some(image) = headers.image().filter(|image| image.bits_per_pixel == 8) else {
            return;
        };
        let columns = usize::from(image.columns);
        let segment = headers.segment();
        let key = segment.map_or(
            1 << 40 | u64::from(file.vcid) << 16 | u64::from(file.counter),
            |segment| 1 << 32 | u64::from(segment.image),
        );
        let placed = ImageSegment {
            key,
            width: segment.map_or(columns, |segment| usize::from(segment.max_column)),
            height: segment.map_or(usize::from(image.lines), |segment| {
                usize::from(segment.max_line)
            }),
            start_line: segment.map_or(0, |segment| usize::from(segment.start_line)),
            columns,
            pixels: &data[..data.len() - data.len() % columns.max(1)],
            expected: segment.map_or(1, |segment| segment.max_segment),
            lines_missing: file.lines_missing,
        };
        if let Some(victim) = self.mosaic.victim_for(key) {
            self.finish(victim, out);
        }
        let Some(index) = self.mosaic.place(&placed, name) else {
            return;
        };
        if self.mosaic.canvas(index).complete() {
            self.finish(index, out);
        } else {
            out.video.push(picture_of(self.mosaic.canvas(index)));
        }
    }

    fn finish(&mut self, index: usize, out: &mut ChannelOutputs) {
        let canvas = self.mosaic.canvas(index);
        self.seq = self.seq.wrapping_add(1);
        let product = Product::parse(&canvas.name);
        let picture = picture_of(canvas);
        let complete = canvas.complete();
        out.events.push(DecoderEvent::of(&GoesHritImage {
            seq: self.seq,
            name: canvas.name.clone(),
            satellite: product.satellite.clone(),
            product: product.label(),
            band: product.band,
            width: picture.width,
            lines: picture.height,
            segments: canvas.segments,
            segments_expected: canvas.expected,
            complete,
            lines_missing: canvas.lines_missing,
        }));
        let mode = [product.satellite.clone(), Some(product.label())]
            .into_iter()
            .flatten()
            .chain(product.band.map(|band| format!("band {band}")))
            .collect::<Vec<_>>()
            .join(" ");
        out.video.push(picture.clone());
        out.images.push(DecodedImage {
            source: SOURCE.into(),
            mode,
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

fn picture_of(canvas: &Canvas) -> VideoPicture {
    VideoPicture::grey(canvas.width as u16, canvas.height as u16, &canvas.luma)
}

pub struct GoesHritChannel {
    params: GoesHritParams,
    link: XritLink,
    pictures: Pictures,
    closing: bool,
}

impl NativeChannel for GoesHritChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        Ok(Self {
            params: params(&settings)?,
            link: link()?,
            pictures: Pictures::new(),
            closing: false,
        })
    }
}

impl Channel for GoesHritChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.params = params(&settings)?;
        Ok(())
    }

    fn retuned(&mut self) {
        self.link.reset();
        self.closing = true;
    }

    fn rejected(&self) -> u32 {
        self.link.stats().lost()
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        if self.closing {
            self.closing = false;
            self.pictures.close(out);
        }
        let picture = self.params.picture;
        for chunk in iq.chunks(CHUNK) {
            let pictures = &mut self.pictures;
            self.link
                .process(chunk, &mut |file| pictures.take(&file, picture, out));
        }
    }
}
