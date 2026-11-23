//! Reads a `NeMo` `.nemo` archive's weights: a tar holding a `PyTorch` zip, whose pickle names
//! each tensor's storage. Only what a state dict of float tensors needs is understood.

use std::collections::HashMap;

use anyhow::{Context, Result, bail, ensure};

const BLOCK: usize = 512;
const WEIGHTS: &str = "model_weights.ckpt";

/// A tensor as `(shape, values)`.
pub(super) type Tensor = (Vec<usize>, Vec<f32>);

/// The named float tensors of a `.nemo` file; integer bookkeeping such as batch counts is left out.
pub(super) fn tensors(archive: &[u8]) -> Result<HashMap<String, Tensor>> {
    let checkpoint = tar_member(archive, WEIGHTS)?;
    let zip = Zip::open(checkpoint)?;
    let pickle = zip.entry_ending("/data.pkl")?;
    let prefix = pickle.0.trim_end_matches("data.pkl").to_owned();
    let state = Unpickler::default().load(pickle.1)?;
    let Py::Dict(items) = state else {
        bail!("checkpoint is not a state dict");
    };
    let mut out = HashMap::new();
    for (key, value) in items {
        let Py::Str(name) = key else { continue };
        if let Some(tensor) = rebuilt(&zip, &prefix, &value)? {
            out.insert(name, tensor);
        }
    }
    Ok(out)
}

fn tar_member<'a>(archive: &'a [u8], name: &str) -> Result<&'a [u8]> {
    let mut at = 0;
    while at + BLOCK <= archive.len() {
        let header = &archive[at..at + BLOCK];
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let path = String::from_utf8_lossy(&header[..100]);
        let path = path.trim_end_matches('\0');
        let size_text = String::from_utf8_lossy(&header[124..136]);
        let size =
            usize::from_str_radix(size_text.trim_matches(|c: char| c == '\0' || c == ' '), 8)
                .with_context(|| format!("bad tar size for {path}"))?;
        let start = at + BLOCK;
        if path.trim_start_matches("./") == name {
            return archive.get(start..start + size).context("truncated tar");
        }
        at = start + size.div_ceil(BLOCK) * BLOCK;
    }
    bail!("{name} is not in the archive")
}

struct Zip<'a> {
    bytes: &'a [u8],
    entries: Vec<(String, usize, usize)>,
}

fn le(bytes: &[u8], at: usize, len: usize) -> Result<u64> {
    let slice = bytes.get(at..at + len).context("truncated zip")?;
    Ok(slice
        .iter()
        .rev()
        .fold(0u64, |value, &byte| (value << 8) | u64::from(byte)))
}

impl<'a> Zip<'a> {
    fn open(bytes: &'a [u8]) -> Result<Self> {
        let end = (0..bytes.len().saturating_sub(21))
            .rev()
            .find(|&at| bytes[at..].starts_with(b"PK\x05\x06"))
            .context("no zip directory")?;
        let mut count = le(bytes, end + 10, 2)? as usize;
        let mut at = le(bytes, end + 16, 4)? as usize;
        if count == 0xffff || at == 0xffff_ffff {
            let locator = end.checked_sub(20).context("no zip64 locator")?;
            ensure!(
                bytes[locator..].starts_with(b"PK\x06\x07"),
                "no zip64 locator"
            );
            let record = le(bytes, locator + 8, 8)? as usize;
            count = le(bytes, record + 32, 8)? as usize;
            at = le(bytes, record + 48, 8)? as usize;
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            ensure!(bytes[at..].starts_with(b"PK\x01\x02"), "bad zip entry");
            ensure!(
                le(bytes, at + 10, 2)? == 0,
                "compressed checkpoints are not supported"
            );
            let mut size = le(bytes, at + 24, 4)? as usize;
            let name_len = le(bytes, at + 28, 2)? as usize;
            let extra_len = le(bytes, at + 30, 2)? as usize;
            let comment_len = le(bytes, at + 32, 2)? as usize;
            let mut local = le(bytes, at + 42, 4)? as usize;
            let name = String::from_utf8_lossy(&bytes[at + 46..at + 46 + name_len]).into_owned();
            let extra = &bytes[at + 46 + name_len..at + 46 + name_len + extra_len];
            (size, local) = zip64(extra, size, local)?;
            let local_name = le(bytes, local + 26, 2)? as usize;
            let local_extra = le(bytes, local + 28, 2)? as usize;
            entries.push((name, local + 30 + local_name + local_extra, size));
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Self { bytes, entries })
    }

    fn entry_ending(&self, suffix: &str) -> Result<(&str, &'a [u8])> {
        self.entries
            .iter()
            .find(|(name, ..)| name.ends_with(suffix))
            .map(|(name, start, size)| (name.as_str(), &self.bytes[*start..*start + *size]))
            .with_context(|| format!("no {suffix} in the checkpoint"))
    }

    fn entry(&self, name: &str) -> Result<&'a [u8]> {
        self.entries
            .iter()
            .find(|(entry, ..)| entry == name)
            .map(|(_, start, size)| &self.bytes[*start..*start + *size])
            .with_context(|| format!("no {name} in the checkpoint"))
    }
}

/// Sizes and offsets that overflowed 32 bits, read from the zip64 extra field.
fn zip64(extra: &[u8], mut size: usize, mut local: usize) -> Result<(usize, usize)> {
    let mut at = 0;
    while at + 4 <= extra.len() {
        let id = le(extra, at, 2)?;
        let len = le(extra, at + 2, 2)? as usize;
        if id == 1 {
            let mut field = at + 4;
            if size == 0xffff_ffff {
                field += 8;
                size = le(extra, field, 8)? as usize;
            }
            if local == 0xffff_ffff {
                local = le(extra, field, 8)? as usize;
            }
        }
        at += 4 + len;
    }
    Ok((size, local))
}

/// A pickled value, as far as a state dict uses them.
#[derive(Clone, Debug)]
enum Py {
    None,
    Bool,
    Int(i64),
    Float,
    Str(String),
    Tuple(Vec<Py>),
    List(Vec<Py>),
    Dict(Vec<(Py, Py)>),
    Global(String),
    Call(String, Vec<Py>),
    Persistent(Vec<Py>),
    Mark,
}

#[derive(Default)]
struct Unpickler {
    stack: Vec<Py>,
    memo: HashMap<u64, Py>,
}

impl Unpickler {
    fn load(mut self, data: &[u8]) -> Result<Py> {
        let mut at = 0;
        loop {
            let op = *data.get(at).context("pickle ends early")?;
            at += 1;
            if op == b'.' {
                return self.stack.pop().context("empty pickle");
            }
            at = self.step(op, data, at)?;
        }
    }

    fn pop(&mut self) -> Result<Py> {
        self.stack.pop().context("pickle stack underflow")
    }

    fn since_mark(&mut self) -> Result<Vec<Py>> {
        let mark = self
            .stack
            .iter()
            .rposition(|item| matches!(item, Py::Mark))
            .context("no mark")?;
        let items = self.stack.split_off(mark + 1);
        self.stack.pop();
        Ok(items)
    }

    fn top(&mut self) -> Result<&mut Py> {
        self.stack.last_mut().context("pickle stack underflow")
    }

    fn step(&mut self, op: u8, data: &[u8], at: usize) -> Result<usize> {
        let take = |len: usize| data.get(at..at + len).context("pickle ends early");
        let num = |len: usize| le(data, at, len);
        Ok(match op {
            0x80 => at + 1,
            0x95 => at + 8,
            b'}' => self.push(Py::Dict(Vec::new()), at),
            b']' => self.push(Py::List(Vec::new()), at),
            b')' => self.push(Py::Tuple(Vec::new()), at),
            b'(' => self.push(Py::Mark, at),
            b'N' => self.push(Py::None, at),
            0x88 | 0x89 => self.push(Py::Bool, at),
            b'K' => self.push(Py::Int(num(1)? as i64), at + 1),
            b'M' => self.push(Py::Int(num(2)? as i64), at + 2),
            b'J' => self.push(Py::Int(i64::from(num(4)? as u32 as i32)), at + 4),
            0x8a => {
                let len = num(1)? as usize;
                let bytes = data
                    .get(at + 1..at + 1 + len)
                    .context("pickle ends early")?;
                let value = bytes
                    .iter()
                    .rev()
                    .fold(0i64, |v, &b| (v << 8) | i64::from(b));
                let value = if len > 0 && len < 8 && bytes[len - 1] & 0x80 != 0 {
                    value - (1i64 << (8 * len))
                } else {
                    value
                };
                self.push(Py::Int(value), at + 1 + len)
            }
            b'G' => {
                take(8)?;
                self.push(Py::Float, at + 8)
            }
            b'X' | b'T' => {
                let len = num(4)? as usize;
                let text = data
                    .get(at + 4..at + 4 + len)
                    .context("pickle ends early")?;
                self.push(
                    Py::Str(String::from_utf8_lossy(text).into_owned()),
                    at + 4 + len,
                )
            }
            0x8c | b'U' => {
                let len = num(1)? as usize;
                let text = data
                    .get(at + 1..at + 1 + len)
                    .context("pickle ends early")?;
                self.push(
                    Py::Str(String::from_utf8_lossy(text).into_owned()),
                    at + 1 + len,
                )
            }
            b'c' => {
                let line_end = |from: usize| {
                    data[from..]
                        .iter()
                        .position(|&b| b == b'\n')
                        .map(|len| from + len)
                        .context("bad global")
                };
                let module_end = line_end(at)?;
                let name_end = line_end(module_end + 1)?;
                let module = String::from_utf8_lossy(&data[at..module_end]);
                let name = String::from_utf8_lossy(&data[module_end + 1..name_end]);
                self.push(Py::Global(format!("{module}.{name}")), name_end + 1)
            }
            b'q' => self.memoize(num(1)?, at + 1)?,
            b'r' => self.memoize(num(4)?, at + 4)?,
            0x94 => self.memoize(self.memo.len() as u64, at)?,
            b'h' => self.recall(num(1)?, at + 1)?,
            b'j' => self.recall(num(4)?, at + 4)?,
            b't' => {
                let items = self.since_mark()?;
                self.push(Py::Tuple(items), at)
            }
            0x85..=0x87 => {
                let len = usize::from(op - 0x84);
                let split = self.stack.len().checked_sub(len).context("short tuple")?;
                let items = self.stack.split_off(split);
                self.push(Py::Tuple(items), at)
            }
            b'Q' => {
                let Py::Tuple(id) = self.pop()? else {
                    bail!("persistent id is not a tuple");
                };
                self.push(Py::Persistent(id), at)
            }
            b'R' | 0x81 => {
                let Py::Tuple(args) = self.pop()? else {
                    bail!("call arguments are not a tuple");
                };
                let Py::Global(name) = self.pop()? else {
                    bail!("call target is not a global");
                };
                let value = if name == "collections.OrderedDict" {
                    Py::Dict(Vec::new())
                } else {
                    Py::Call(name, args)
                };
                self.push(value, at)
            }
            b'b' => {
                self.pop()?;
                at
            }
            b's' => {
                let value = self.pop()?;
                let key = self.pop()?;
                if let Py::Dict(items) = self.top()? {
                    items.push((key, value));
                }
                at
            }
            b'u' => {
                let flat = self.since_mark()?;
                if let Py::Dict(items) = self.top()? {
                    let mut pairs = flat.into_iter();
                    while let (Some(key), Some(value)) = (pairs.next(), pairs.next()) {
                        items.push((key, value));
                    }
                }
                at
            }
            b'a' => {
                let value = self.pop()?;
                if let Py::List(items) = self.top()? {
                    items.push(value);
                }
                at
            }
            b'e' => {
                let values = self.since_mark()?;
                if let Py::List(items) = self.top()? {
                    items.extend(values);
                }
                at
            }
            other => bail!("unsupported pickle opcode 0x{other:02x}"),
        })
    }

    fn push(&mut self, value: Py, at: usize) -> usize {
        self.stack.push(value);
        at
    }

    fn memoize(&mut self, key: u64, at: usize) -> Result<usize> {
        let value = self.stack.last().context("nothing to memoize")?.clone();
        self.memo.insert(key, value);
        Ok(at)
    }

    fn recall(&mut self, key: u64, at: usize) -> Result<usize> {
        let value = self.memo.get(&key).context("unknown memo")?.clone();
        Ok(self.push(value, at))
    }
}

fn ints(value: &Py) -> Result<Vec<usize>> {
    let Py::Tuple(items) = value else {
        bail!("expected a tuple of sizes");
    };
    items
        .iter()
        .map(|item| match item {
            Py::Int(n) => usize::try_from(*n).context("negative size"),
            _ => bail!("size is not an integer"),
        })
        .collect()
}

/// A float tensor from `torch._utils._rebuild_tensor_v2`, or none for any other value.
fn rebuilt(zip: &Zip<'_>, prefix: &str, value: &Py) -> Result<Option<Tensor>> {
    let Py::Call(name, args) = value else {
        return Ok(None);
    };
    if name != "torch._utils._rebuild_tensor_v2" {
        return Ok(None);
    }
    let [Py::Persistent(storage), Py::Int(offset), size, stride, ..] = args.as_slice() else {
        bail!("unexpected tensor layout");
    };
    let [_, Py::Global(kind), Py::Str(key), ..] = storage.as_slice() else {
        bail!("unexpected storage");
    };
    if kind != "torch.FloatStorage" {
        return Ok(None);
    }
    let shape = ints(size)?;
    let strides = ints(stride)?;
    let mut expected = 1;
    for (dim, step) in shape.iter().zip(&strides).rev() {
        ensure!(*dim <= 1 || *step == expected, "{key} is not contiguous");
        expected *= dim;
    }
    let count: usize = shape.iter().product();
    let raw = zip.entry(&format!("{prefix}data/{key}"))?;
    let start = usize::try_from(*offset)? * 4;
    let bytes = raw
        .get(start..start + count * 4)
        .context("storage too short")?;
    let values = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|quad| f32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
        .collect();
    Ok(Some((shape, values)))
}
