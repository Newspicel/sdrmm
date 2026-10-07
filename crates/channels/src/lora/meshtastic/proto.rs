const MAX_VARINT_LEN: usize = 10;
const MAX_GROUP_DEPTH: usize = 8;
const VARINT: u8 = 0;
const FIXED64: u8 = 1;
const LENGTH_DELIMITED: u8 = 2;
const START_GROUP: u8 = 3;
const END_GROUP: u8 = 4;
const FIXED32: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Value<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}

impl<'a> Value<'a> {
    pub(super) fn varint(self) -> Option<u64> {
        match self {
            Self::Varint(value) => Some(value),
            _ => None,
        }
    }

    pub(super) fn uint32(self) -> Option<u32> {
        self.varint().map(|value| value as u32)
    }

    pub(super) fn int32(self) -> Option<i32> {
        self.varint().map(|value| value as i32)
    }

    pub(super) fn boolean(self) -> Option<bool> {
        self.varint().map(|value| value != 0)
    }

    pub(super) fn fixed32(self) -> Option<u32> {
        match self {
            Self::Fixed32(value) => Some(value),
            _ => None,
        }
    }

    pub(super) fn sfixed32(self) -> Option<i32> {
        self.fixed32().map(u32::cast_signed)
    }

    pub(super) fn float(self) -> Option<f32> {
        self.fixed32().map(f32::from_bits)
    }

    pub(super) fn bytes(self) -> Option<&'a [u8]> {
        match self {
            Self::Bytes(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub(super) fn text(self) -> Option<String> {
        self.bytes()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }
}

pub(super) fn each_field<'a>(
    bytes: &'a [u8],
    mut visit: impl FnMut(u32, Value<'a>) -> Option<()>,
) -> Option<()> {
    let mut rest = bytes;
    while !rest.is_empty() {
        let (field, wire_type, after) = key(rest)?;
        rest = if wire_type == START_GROUP {
            skip_group(field, after, 0)?
        } else {
            let (value, after) = value(wire_type, after)?;
            visit(field, value)?;
            after
        };
    }
    Some(())
}

pub(super) fn repeated_fixed32(value: Value<'_>, out: &mut Vec<u32>) -> Option<()> {
    match value {
        Value::Fixed32(single) => out.push(single),
        Value::Bytes(packed) => {
            let (words, []) = packed.as_chunks::<4>() else {
                return None;
            };
            out.extend(words.iter().copied().map(u32::from_le_bytes));
        }
        _ => return None,
    }
    Some(())
}

pub(super) fn repeated_varint(value: Value<'_>, out: &mut Vec<u64>) -> Option<()> {
    match value {
        Value::Varint(single) => out.push(single),
        Value::Bytes(mut packed) => {
            while !packed.is_empty() {
                let (single, rest) = varint(packed)?;
                out.push(single);
                packed = rest;
            }
        }
        _ => return None,
    }
    Some(())
}

pub(super) fn varint(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().enumerate().take(MAX_VARINT_LEN) {
        value |= u64::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, bytes.get(index + 1..)?));
        }
    }
    None
}

fn key(bytes: &[u8]) -> Option<(u32, u8, &[u8])> {
    let (key, rest) = varint(bytes)?;
    let field = u32::try_from(key >> 3).ok().filter(|field| *field != 0)?;
    Some((field, (key & 0x07) as u8, rest))
}

fn take(bytes: &[u8], len: usize) -> Option<(&[u8], &[u8])> {
    Some((bytes.get(..len)?, bytes.get(len..)?))
}

fn value(wire_type: u8, bytes: &[u8]) -> Option<(Value<'_>, &[u8])> {
    match wire_type {
        VARINT => varint(bytes).map(|(value, rest)| (Value::Varint(value), rest)),
        FIXED64 => {
            let (raw, rest) = take(bytes, 8)?;
            Some((
                Value::Fixed64(u64::from_le_bytes(raw.try_into().ok()?)),
                rest,
            ))
        }
        LENGTH_DELIMITED => {
            let (len, rest) = varint(bytes)?;
            let (raw, rest) = take(rest, usize::try_from(len).ok()?)?;
            Some((Value::Bytes(raw), rest))
        }
        FIXED32 => {
            let (raw, rest) = take(bytes, 4)?;
            Some((
                Value::Fixed32(u32::from_le_bytes(raw.try_into().ok()?)),
                rest,
            ))
        }
        _ => None,
    }
}

fn skip_group(field: u32, bytes: &[u8], depth: usize) -> Option<&[u8]> {
    if depth >= MAX_GROUP_DEPTH {
        return None;
    }
    let mut rest = bytes;
    loop {
        let (inner, wire_type, after) = key(rest)?;
        rest = match wire_type {
            START_GROUP => skip_group(inner, after, depth + 1)?,
            END_GROUP => return (inner == field).then_some(after),
            _ => value(wire_type, after)?.1,
        };
    }
}

#[cfg(any(test, feature = "synth"))]
#[derive(Default)]
pub(super) struct Writer {
    bytes: Vec<u8>,
}

#[cfg(any(test, feature = "synth"))]
impl Writer {
    fn raw_varint(&mut self, mut value: u64) {
        while value >= 0x80 {
            self.bytes.push((value as u8) | 0x80);
            value >>= 7;
        }
        self.bytes.push(value as u8);
    }

    fn key(&mut self, field: u32, wire_type: u8) {
        self.raw_varint((u64::from(field) << 3) | u64::from(wire_type));
    }

    pub(super) fn varint(&mut self, field: u32, value: u64) -> &mut Self {
        self.key(field, VARINT);
        self.raw_varint(value);
        self
    }

    pub(super) fn int32(&mut self, field: u32, value: i32) -> &mut Self {
        self.varint(field, i64::from(value).cast_unsigned())
    }

    pub(super) fn fixed32(&mut self, field: u32, value: u32) -> &mut Self {
        self.key(field, FIXED32);
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub(super) fn sfixed32(&mut self, field: u32, value: i32) -> &mut Self {
        self.fixed32(field, value.cast_unsigned())
    }

    pub(super) fn bytes(&mut self, field: u32, value: &[u8]) -> &mut Self {
        self.key(field, LENGTH_DELIMITED);
        self.raw_varint(value.len() as u64);
        self.bytes.extend_from_slice(value);
        self
    }

    pub(super) fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
}
