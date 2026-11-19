use super::{super::syntax::read_codeword, *};

/// Twelve HD1 frames of KUT, two of them in eight short windows.
const FRAMES: &[u8] = include_bytes!("../../../../../fixtures/hd_radio/kut_hd1_hdc.aus");

fn frames(mut bytes: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    while bytes.len() >= 2 {
        let size = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
        frames.push(&bytes[2..2 + size]);
        bytes = &bytes[2 + size..];
    }
    frames
}

/// Reads one standard channel stream of a common-window pair.
fn read_standard_channel(reader: &mut BitReader<'_>, info: IcsInfo) -> Option<()> {
    let offsets = if info.short() {
        BANDS_SHORT
    } else {
        BANDS_LONG
    };
    reader.skip(8)?;
    let sections = read_sections(reader, info, false)?;
    skip_scalefactors(reader, info, &sections)?;
    if reader.flag()? {
        return None;
    }
    if reader.flag()? {
        super::super::syntax::skip_tns(reader, info)?;
    }
    if reader.flag()? {
        return None;
    }
    for (codebook, _, _) in standard_positions(info, &sections, offsets) {
        read_codeword(reader, codebook)?;
    }
    Some(())
}

/// Walks a rewritten frame as ISO/IEC 14496-3 syntax: a channel pair, the SBR fill element,
/// the end element; returns how many bits it read.
fn read_standard(raw: &[u8]) -> Option<usize> {
    let mut reader = BitReader::new(raw);
    if reader.read(3)? != 1 || reader.read(4)? != 0 || !reader.flag()? || reader.flag()? {
        return None;
    }
    let info = IcsInfo::read_header(&mut reader)?;
    if !info.short() && reader.flag()? {
        return None;
    }
    if reader.read(2)? == 1 {
        reader.skip(info.groups().len() * usize::from(info.max_sfb))?;
    }
    read_standard_channel(&mut reader, info)?;
    read_standard_channel(&mut reader, info)?;
    if reader.read(3)? != ID_FIL {
        return None;
    }
    let mut count = reader.read(4)? as usize;
    if count == 15 {
        count += reader.read(8)? as usize - 1;
    }
    if reader.read(4)? != SBR_EXTENSION {
        return None;
    }
    reader.skip(count * 8 - 4)?;
    (reader.read(3)? == ID_END).then(|| reader.position())
}

#[test]
fn every_frame_rewrites_to_standard_aac_syntax() {
    let frames = frames(FRAMES);
    assert_eq!(frames.len(), 12);
    let mut short = 0;
    let mut raw = Vec::new();
    for (index, frame) in frames.iter().enumerate() {
        let parsed = parse(frame).expect("an HDC frame");
        assert!(parsed.stereo && parsed.sbr.is_some(), "frame {index}");
        short += usize::from(parsed.info.short());
        raw_frame(frame, &mut raw).expect("a raw data block");
        let read = read_standard(&raw).unwrap_or_else(|| panic!("frame {index} misread"));
        assert_eq!(read.div_ceil(8), raw.len(), "frame {index}");
    }
    assert_eq!(short, 2);
}

#[test]
fn the_config_signals_sbr_over_lc_at_22050_hz() {
    assert_eq!(audio_specific_config(true), [0x2B, 0x92, 0x08, 0x00]);
    assert_eq!(audio_specific_config(false), [0x2B, 0x8A, 0x08, 0x00]);
}

#[test]
fn block_types_name_their_channel_layout() {
    assert_eq!(stereo(&[0b0100_0000]), Ok(true));
    assert_eq!(stereo(&[0b1110_0000]), Ok(true));
    assert_eq!(stereo(&[0b0010_0000]), Ok(false));
    assert!(stereo(&[0b0110_0000]).is_err());
    assert!(stereo(&[]).is_err());
}

#[test]
fn the_extra_sbr_bit_follows_the_header_and_data_flag() {
    let mut writer = BitWriter::with_capacity(8);
    writer.bit(true);
    writer.put(0, 14);
    writer.put(0b11, 2);
    writer.put(0, 5 + 6);
    writer.bit(false);
    writer.put(0, 8);
    let bytes = writer.into_bytes();
    let mut reader = BitReader::new(&bytes);
    assert_eq!(sbr_extra_bit(&mut reader), Some(1 + 14 + 2 + 5 + 6 + 1));
    let mut reader = BitReader::new(&[0b0100_0000]);
    assert_eq!(sbr_extra_bit(&mut reader), Some(6));
}
