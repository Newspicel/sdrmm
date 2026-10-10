use super::*;

impl Reader<'_> {
    fn floats(&mut self, count: usize, out: &mut Vec<f32>) -> Result<(), FrameError> {
        let len = count.checked_mul(4).ok_or(FrameError::Length)?;
        let bytes = self.take(len).ok_or(FrameError::Short)?;
        out.extend(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk)),
        );
        Ok(())
    }

    fn remaining_floats(&mut self, out: &mut Vec<f32>) -> Result<(), FrameError> {
        let len = self.buf.len().saturating_sub(self.at);
        if !len.is_multiple_of(4) {
            return Err(FrameError::Length);
        }
        self.floats(len / 4, out)
    }
}

impl<'a> IqFrame<'a> {
    pub fn decode(buf: &[u8], floats: &'a mut Vec<f32>) -> Result<Self, FrameError> {
        let mut reader = Reader::accepting(buf, &[FrameKind::IqF32])?;
        let stream_id = reader.u16()?;
        let seq = reader.u32()?;
        let timestamp = reader.u64()?;
        let center_hz = reader.f64()?;
        let sample_rate = reader.f32()?;
        floats.clear();
        reader.remaining_floats(floats)?;
        if !floats.len().is_multiple_of(2) {
            return Err(FrameError::Shape);
        }
        Ok(Self {
            stream_id,
            seq,
            timestamp,
            center_hz,
            sample_rate,
            samples: floats,
        })
    }
}

impl<'a> SymbolFrame<'a> {
    pub fn decode(buf: &[u8], floats: &'a mut Vec<f32>) -> Result<Self, FrameError> {
        let mut reader = Reader::accepting(buf, &[FrameKind::Symbols])?;
        let stream_id = reader.u16()?;
        let seq = reader.u32()?;
        let timestamp = reader.u64()?;
        let plane = SymbolPlane::from_u8(reader.u8()?).ok_or(FrameError::Shape)?;
        let symbol_rate = reader.f32()?;
        let evm = reader.f32()?;
        let mer_db = reader.f32()?;
        let margin = reader.f32()?;
        let freq_error_hz = reader.f32()?;
        let points = usize::from(reader.u16()?);
        floats.clear();
        reader.floats(points, floats)?;
        reader.remaining_floats(floats)?;
        let (reference, symbols) = floats.split_at(points);
        Ok(Self {
            stream_id,
            seq,
            timestamp,
            plane,
            symbol_rate,
            evm,
            mer_db,
            margin,
            freq_error_hz,
            reference,
            symbols,
        })
    }
}
