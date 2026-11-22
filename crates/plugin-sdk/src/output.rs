//! A channel's outputs laid out as the host reads them: an [`OutputHeader`] pointing into
//! [`ChannelOutputs`] and into buffers kept here, all reused block after block.

use sdrmm_decoder::{ChannelOutputs, VideoPicture};
use sdrmm_wire::plugin::{OutputHeader, Picture, PictureImage, plane_code};

/// The guest address of `slice`.
pub(crate) fn address<T>(slice: &[T]) -> u32 {
    slice.as_ptr() as usize as u32
}

pub(crate) fn len_u32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The serialized parts of one block's outputs.
#[derive(Default)]
pub struct OutputWire {
    pub(crate) events: Vec<u8>,
    pub(crate) status: Vec<u8>,
    pub(crate) pictures: Vec<u8>,
    /// Blocks whose results could not be serialized, counted up over the channel's life.
    lost: u32,
}

impl OutputWire {
    /// Serializes `out` and returns the header pointing at it and at its buffers. `rejected` and
    /// `skipped` are the channel's own counts.
    pub fn encode(&mut self, out: &ChannelOutputs, rejected: u32, skipped: u32) -> OutputHeader {
        if self.write(out).is_err() {
            self.events.clear();
            self.status.clear();
            self.pictures.clear();
            self.lost = self.lost.saturating_add(1);
        }
        let symbols = &out.symbols;
        OutputHeader {
            audio_ptr: address(&out.audio_pcm),
            audio_len: len_u32(out.audio_pcm.len()),
            audio_rate: out.audio_rate,
            events_ptr: address(&self.events),
            events_len: len_u32(self.events.len()),
            status_ptr: address(&self.status),
            status_len: len_u32(self.status.len()),
            rejected,
            skipped: skipped.saturating_add(self.lost),
            pictures_ptr: address(&self.pictures),
            pictures_len: len_u32(self.pictures.len()),
            symbols_ptr: address(&symbols.symbols),
            symbols_len: len_u32(symbols.symbols.len()),
            reference_ptr: address(&symbols.reference),
            reference_len: len_u32(symbols.reference.len()),
            plane: plane_code(symbols.plane),
            symbol_rate: symbols.symbol_rate,
            evm: symbols.evm,
            mer_db: symbols.mer_db,
            margin: symbols.margin,
            freq_error_hz: symbols.freq_error_hz,
        }
    }

    fn write(&mut self, out: &ChannelOutputs) -> Result<(), String> {
        self.events.clear();
        if !out.events.is_empty() {
            serde_json::to_writer(&mut self.events, &out.events)
                .map_err(|error| error.to_string())?;
        }
        self.status.clear();
        if let Some(status) = &out.status {
            serde_json::to_writer(&mut self.status, status).map_err(|error| error.to_string())?;
        }
        self.pictures.clear();
        for frame in &out.video {
            picture(frame, None).write(&mut self.pictures)?;
        }
        for image in &out.images {
            let meta = PictureImage {
                source: image.source.to_string(),
                mode: image.mode.clone(),
                complete: image.complete,
                lines: image.lines,
            };
            picture(&image.picture, Some(meta)).write(&mut self.pictures)?;
        }
        Ok(())
    }
}

fn picture(frame: &VideoPicture, image: Option<PictureImage>) -> Picture<'_> {
    Picture {
        width: frame.width,
        height: frame.height,
        luma: &frame.luma,
        rgb: &frame.rgb,
        image,
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_decoder::DecodedImage;
    use sdrmm_wire::{
        ChannelStatus, DecoderEvent, EventKind, SymbolPlane,
        plugin::{plane_of, read_pictures},
    };

    use super::*;

    #[test]
    fn every_output_lands_in_its_buffer() {
        let mut out = ChannelOutputs {
            audio_pcm: vec![0.5; 4],
            audio_rate: 8_000,
            ..ChannelOutputs::default()
        };
        out.events
            .push(DecoderEvent::tagged(EventKind::named("probe"), &1));
        out.status = Some(ChannelStatus::default());
        out.video.push(VideoPicture {
            width: 2,
            height: 1,
            luma: vec![1, 2],
            rgb: Vec::new(),
        });
        out.images.push(DecodedImage {
            source: "probe".into(),
            mode: "Test".to_owned(),
            complete: true,
            lines: 1,
            picture: VideoPicture {
                width: 1,
                height: 1,
                luma: vec![7],
                rgb: vec![1, 2, 3],
            },
        });
        out.symbols.set_wanted(true);
        out.symbols.levels(
            &[1.0, -1.0],
            &[],
            &sdrmm_modem::cpm::Mapping::natural(2),
            4_800.0,
            3.0,
        );
        let mut wire = OutputWire::default();
        let header = wire.encode(&out, 2, 5);
        assert_eq!((header.audio_len, header.audio_rate), (4, 8_000));
        assert_eq!((header.rejected, header.skipped), (2, 5));
        let events: Vec<DecoderEvent> = serde_json::from_slice(&wire.events).expect("events");
        assert_eq!(events.len(), 1);
        assert!(!wire.status.is_empty());
        let pictures: Vec<_> = read_pictures(&wire.pictures)
            .collect::<Result<_, _>>()
            .expect("pictures");
        assert_eq!(pictures.len(), 2);
        assert_eq!(pictures[0].luma, [1, 2]);
        let image = pictures[1].image.as_ref().expect("an image");
        assert_eq!((image.source.as_str(), image.lines), ("probe", 1));
        assert_eq!(plane_of(header.plane), Ok(out.symbols.plane));
        assert_eq!(header.symbols_len, 2);
        assert_eq!(header.symbol_rate, 4_800.0);
        assert_eq!(header.freq_error_hz, 3.0);
    }

    #[test]
    fn a_carrier_tap_crosses_on_its_own_plane() {
        let table = sdrmm_modem::constellation::tables::psk(4).expect("qpsk");
        let mut out = ChannelOutputs::default();
        out.symbols.set_wanted(true);
        out.symbols
            .carriers(table.points(), &table, 1_536_000.0, 0.0);
        let header = OutputWire::default().encode(&out, 0, 0);
        assert_eq!(header.plane, 3);
        assert_eq!(plane_of(header.plane), Ok(Some(SymbolPlane::Carriers)));
    }

    #[test]
    fn a_quiet_block_leaves_every_buffer_empty() {
        let mut wire = OutputWire::default();
        let header = wire.encode(&ChannelOutputs::default(), 0, 0);
        assert_eq!(
            header.events_len + header.status_len + header.pictures_len,
            0
        );
        assert_eq!(header.plane, 0);
    }
}
