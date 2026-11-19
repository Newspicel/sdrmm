//! HDC, the AAC variant HD Radio carries, rewritten as a standard AAC raw data block with SBR.
//!
//! An HDC frame is one AAC LC channel element at 22.05 kHz with its syntax rearranged: a block
//! type in place of the element ID, window shape before window sequence, no predictor flag,
//! the TNS data of both channels ahead of the side information and a fixed single filter per
//! long window. Its SBR data follows a fill element ID with no extension type or CRC and one
//! extra bit in a single channel element, as DRM's does. The rewrite moves each field back to
//! where ISO/IEC 14496-3 puts it and copies the coded data bit for bit. Parametric stereo
//! keeps its DRM form, which a standard decoder skips, so such programs play mono.

use std::ops::Range;

use sdrmm_dsp::coding::bits::{BitReader, BitWriter};

use super::{
    MAX_FILL_BYTES, SBR_EXTENSION,
    syntax::{
        IcsInfo, Section, read_codeword, read_sections, skip_scalefactors, standard_positions,
    },
};

/// The core sampling rate; SBR doubles it.
pub const CORE_RATE_HZ: u32 = 22_050;
pub const OUTPUT_RATE_HZ: u32 = 2 * CORE_RATE_HZ;
/// Output samples per channel of one frame.
pub const FRAME_SAMPLES: usize = 2048;
/// The scalefactor bands of 1024-sample frames at 22.05 and 24 kHz (ISO/IEC 14496-3 tables
/// 4.131 and 4.135); DRM's 960-sample tables end short of them.
const BANDS_LONG: &[u16] = &[
    0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 52, 60, 68, 76, 84, 92, 100, 108, 116, 124, 136,
    148, 160, 172, 188, 204, 220, 240, 260, 284, 308, 336, 364, 396, 432, 468, 508, 552, 600, 652,
    704, 768, 832, 896, 960, 1024,
];
const BANDS_SHORT: &[u16] = &[
    0, 4, 8, 12, 16, 20, 24, 28, 36, 44, 52, 64, 76, 92, 108, 128,
];
const ID_FIL: u32 = 6;
const ID_END: u32 = 7;
const DAMAGED: &str = "Damaged HDC frame";

/// The block type's channel layout: two of them carry a channel pair.
pub fn stereo(frame: &[u8]) -> Result<bool, &'static str> {
    match frame.first().map(|byte| byte >> 5) {
        Some(0 | 1 | 5 | 6) => Ok(false),
        Some(2 | 7) => Ok(true),
        Some(_) => Err("Unknown HDC block type"),
        None => Err("Empty HDC frame"),
    }
}

/// The `AudioSpecificConfig` the rewritten frames decode under: explicit SBR over AAC LC at
/// 22.05 kHz, 1024-sample frames.
#[must_use]
pub fn audio_specific_config(stereo: bool) -> Vec<u8> {
    const SBR: u32 = 5;
    const LC: u32 = 2;
    const INDEX_22050: u32 = 7;
    const INDEX_44100: u32 = 4;
    let mut writer = BitWriter::with_capacity(4);
    writer.put(SBR, 5);
    writer.put(INDEX_22050, 4);
    writer.put(if stereo { 2 } else { 1 }, 4);
    writer.put(INDEX_44100, 4);
    writer.put(LC, 5);
    writer.put(0, 3);
    writer.into_bytes()
}

struct Channel {
    gain: u32,
    tns: Option<Range<usize>>,
    sections: Range<usize>,
    scalefactors: Range<usize>,
    spectral: Range<usize>,
}

struct Parsed {
    stereo: bool,
    info: IcsInfo,
    ms: Range<usize>,
    channels: [Option<Channel>; 2],
    sbr: Option<Range<usize>>,
}

/// Rewrites one HDC frame as a raw data block into `out`.
pub fn raw_frame(frame: &[u8], out: &mut Vec<u8>) -> Result<(), &'static str> {
    let reader = BitReader::new(frame);
    let parsed = parse(frame)?;
    let mut writer = BitWriter::with_capacity(frame.len() + 16);
    if parsed.stereo {
        writer.put(1, 3);
        writer.put(0, 4);
        writer.bit(true);
        write_info(&mut writer, parsed.info);
        writer.copy(&reader, parsed.ms.start, parsed.ms.end);
    } else {
        writer.put(0, 3);
        writer.put(0, 4);
    }
    for channel in parsed.channels.iter().flatten() {
        write_channel(&mut writer, &reader, channel, parsed.info, parsed.stereo);
    }
    if let Some(sbr) = parsed.sbr {
        write_sbr(&mut writer, &reader, sbr, parsed.stereo)?;
    }
    writer.put(ID_END, 3);
    writer.align();
    out.clear();
    out.extend_from_slice(writer.bytes());
    Ok(())
}

fn parse(frame: &[u8]) -> Result<Parsed, &'static str> {
    let stereo = stereo(frame)?;
    let mut reader = BitReader::new(frame);
    reader.skip(3).ok_or(DAMAGED)?;
    let info = read_info(&mut reader).ok_or(DAMAGED)?;
    let offsets = if info.short() {
        BANDS_SHORT
    } else {
        BANDS_LONG
    };
    if usize::from(info.max_sfb) + 1 > offsets.len() {
        return Err(DAMAGED);
    }
    let ms_start = reader.position();
    if stereo {
        match reader.read(2).ok_or(DAMAGED)? {
            1 => reader
                .skip(info.groups().len() * usize::from(info.max_sfb))
                .ok_or(DAMAGED)?,
            3 => return Err(DAMAGED),
            _ => {}
        }
    }
    let ms = ms_start..reader.position();
    let count = if stereo { 2 } else { 1 };
    let mut tns = [None, None];
    for slot in &mut tns[..count] {
        if reader.flag().ok_or(DAMAGED)? {
            *slot = Some(skip_tns(&mut reader, info).ok_or(DAMAGED)?);
        }
    }
    let mut channels = [None, None];
    for (slot, tns) in channels[..count].iter_mut().zip(tns) {
        *slot = Some(read_channel(&mut reader, info, offsets, tns).ok_or(DAMAGED)?);
    }
    let sbr =
        (reader.remaining() > 4 && reader.read(3) == Some(ID_FIL) && reader.flag() == Some(true))
            .then(|| reader.position()..reader.position() + reader.remaining());
    Ok(Parsed {
        stereo,
        info,
        ms,
        channels,
        sbr,
    })
}

/// The ICS header in HDC's order: window shape, then sequence.
fn read_info(reader: &mut BitReader<'_>) -> Option<IcsInfo> {
    if reader.flag()? {
        return None;
    }
    let shape = reader.flag()?;
    let sequence = reader.read(2)? as u8;
    let short = sequence == super::syntax::EIGHT_SHORT;
    let max_sfb = reader.read(if short { 4 } else { 6 })? as u8;
    let grouping = if short { reader.read(7)? as u8 } else { 0 };
    Some(IcsInfo {
        sequence,
        shape,
        max_sfb,
        grouping,
    })
}

fn read_channel(
    reader: &mut BitReader<'_>,
    info: IcsInfo,
    offsets: &[u16],
    tns: Option<Range<usize>>,
) -> Option<Channel> {
    let gain = reader.read(8)?;
    let start = reader.position();
    let parsed: Vec<Section> = read_sections(reader, info, false)?;
    let sections = start..reader.position();
    let scalefactors = skip_scalefactors(reader, info, &parsed)?;
    let start = reader.position();
    for (codebook, _, _) in standard_positions(info, &parsed, offsets) {
        read_codeword(reader, codebook)?;
    }
    Some(Channel {
        gain,
        tns,
        sections,
        scalefactors,
        spectral: start..reader.position(),
    })
}

/// TNS data with HDC's single implicit filter per long window.
fn skip_tns(reader: &mut BitReader<'_>, info: IcsInfo) -> Option<Range<usize>> {
    let start = reader.position();
    let short = info.short();
    let windows = if short { 8 } else { 1 };
    for _ in 0..windows {
        let filters = if short { reader.read(1)? } else { 1 };
        if filters == 0 {
            continue;
        }
        let resolution = reader.read(1)?;
        for _ in 0..filters {
            reader.skip(if short { 4 } else { 6 })?;
            let order = reader.read(if short { 3 } else { 5 })?;
            if order == 0 {
                continue;
            }
            reader.skip(1)?;
            let compress = reader.read(1)?;
            reader.skip((order * (3 + resolution - compress)) as usize)?;
        }
    }
    Some(start..reader.position())
}

fn write_info(writer: &mut BitWriter, info: IcsInfo) {
    writer.bit(false);
    info.write_header(writer);
    if !info.short() {
        writer.bit(false);
    }
}

fn write_channel(
    writer: &mut BitWriter,
    reader: &BitReader<'_>,
    channel: &Channel,
    info: IcsInfo,
    common: bool,
) {
    writer.put(channel.gain, 8);
    if !common {
        write_info(writer, info);
    }
    writer.copy(reader, channel.sections.start, channel.sections.end);
    writer.copy(reader, channel.scalefactors.start, channel.scalefactors.end);
    writer.bit(false);
    writer.bit(channel.tns.is_some());
    if let Some(tns) = &channel.tns {
        if !info.short() {
            writer.put(1, 2);
        }
        writer.copy(reader, tns.start, tns.end);
    }
    writer.bit(false);
    writer.copy(reader, channel.spectral.start, channel.spectral.end);
}

/// Where the extra bit of a single channel element's SBR data sits: after the optional
/// header and the extra-data flag.
fn sbr_extra_bit(reader: &mut BitReader<'_>) -> Option<usize> {
    if reader.flag()? {
        reader.skip(1 + 4 + 4 + 3 + 2)?;
        let more_frequency = reader.flag()?;
        let more_limiter = reader.flag()?;
        if more_frequency {
            reader.skip(2 + 1 + 2)?;
        }
        if more_limiter {
            reader.skip(2 + 2 + 1 + 1)?;
        }
    }
    if reader.flag()? {
        reader.skip(4)?;
    }
    Some(reader.position())
}

fn write_sbr(
    writer: &mut BitWriter,
    reader: &BitReader<'_>,
    sbr: Range<usize>,
    stereo: bool,
) -> Result<(), &'static str> {
    let mut skipped = None;
    if !stereo {
        let mut probe = reader.clone();
        probe.seek(sbr.start);
        skipped = Some(sbr_extra_bit(&mut probe).ok_or(DAMAGED)?);
    }
    let bits = sbr.len() - usize::from(skipped.is_some());
    let bytes = (bits + 4).div_ceil(8);
    if bytes > MAX_FILL_BYTES {
        return Err("HDC SBR data too long");
    }
    writer.put(ID_FIL, 3);
    if bytes < 15 {
        writer.put(bytes as u32, 4);
    } else {
        writer.put(15, 4);
        writer.put((bytes - 14) as u32, 8);
    }
    writer.put(SBR_EXTENSION, 4);
    match skipped {
        Some(at) => {
            writer.copy(reader, sbr.start, at);
            writer.copy(reader, at + 1, sbr.end);
        }
        None => writer.copy(reader, sbr.start, sbr.end),
    }
    for _ in bits + 4..bytes * 8 {
        writer.bit(false);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
