use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use crate::{
    protocol::{Deframer, PacketKind, encode, field},
    transport::{Opener, Transport},
};

/// One decode request as the chip saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Heard {
    pub(crate) channel: Option<u8>,
    pub(crate) bit_count: u8,
    pub(crate) bits: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) unplugged: bool,
    pub(crate) heard: Vec<Heard>,
    pub(crate) configured: Vec<(Option<u8>, Vec<u8>)>,
    pub(crate) restarts: Vec<Option<u8>>,
}

/// An AMBE chip that answers like DVSI's, at one baud rate, with speech made from the bits.
#[derive(Clone)]
pub(crate) struct FakeChip {
    pub(crate) product: &'static str,
    pub(crate) baud: u32,
    pub(crate) state: Arc<Mutex<State>>,
}

impl FakeChip {
    pub(crate) fn new(product: &'static str, baud: u32) -> Self {
        Self {
            product,
            baud,
            state: Arc::default(),
        }
    }

    pub(crate) fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn multi_channel(&self) -> bool {
        self.product.contains("3003")
    }

    /// The sample a chip returns at `index` for frame `bits`.
    pub(crate) fn sample(bits: &[u8], index: usize) -> i16 {
        i16::from(bits.first().copied().unwrap_or(0)) * 64 + i16::try_from(index).unwrap_or(0)
    }
}

impl Opener for FakeChip {
    fn open(&self, _port: &str, baud: u32) -> Result<Box<dyn Transport>, String> {
        if self.state().unplugged {
            return Err("no such device".to_owned());
        }
        Ok(Box::new(Link {
            chip: self.clone(),
            listening: baud == self.baud,
            deframer: Deframer::default(),
            replies: VecDeque::new(),
        }))
    }
}

struct Link {
    chip: FakeChip,
    listening: bool,
    deframer: Deframer,
    replies: VecDeque<u8>,
}

impl Link {
    fn answer(&mut self, kind: PacketKind, fields: &[u8]) {
        self.replies.extend(encode(kind, fields));
    }

    fn split_channel(&self, fields: &[u8]) -> (Option<u8>, Vec<u8>) {
        match fields {
            [id, rest @ ..] if self.chip.multi_channel() && (0x40..0x43).contains(id) => {
                (Some(id - field::CHANNEL_0), rest.to_vec())
            }
            _ => (None, fields.to_vec()),
        }
    }

    fn control(&mut self, fields: &[u8]) {
        let (channel, rest) = self.split_channel(fields);
        match rest.as_slice() {
            [field::RESET] => self.answer(PacketKind::Control, &[field::READY]),
            [field::PRODUCT_ID] => self.answer(
                PacketKind::Control,
                &[&[field::PRODUCT_ID], self.chip.product.as_bytes(), &[0]].concat(),
            ),
            [field::VERSION] => self.answer(PacketKind::Control, b"\x31V120.E100.XXXX\0"),
            [field::INIT, _] => {
                self.chip.state().restarts.push(channel);
                self.answer(
                    PacketKind::Control,
                    &with_channel(channel, &[field::INIT, 0]),
                );
            }
            [rate_id, .., field::INIT, _] => {
                self.chip.state().configured.push((channel, rest.clone()));
                self.answer(
                    PacketKind::Control,
                    &with_channel(channel, &[*rate_id, 0, field::INIT, 0]),
                );
            }
            other => panic!("fake chip got control fields {other:?}"),
        }
    }

    fn decode(&mut self, fields: &[u8]) {
        let (channel, rest) = self.split_channel(fields);
        let [field::CHANNEL_DATA, bit_count, bits @ ..] = rest.as_slice() else {
            panic!("fake chip got channel fields {rest:?}");
        };
        self.chip.state().heard.push(Heard {
            channel,
            bit_count: *bit_count,
            bits: bits.to_vec(),
        });
        let mut speech = channel
            .map(|channel| vec![field::CHANNEL_0 + channel])
            .unwrap_or_default();
        speech.extend([field::SPEECH_DATA, 160]);
        speech.extend((0..160).flat_map(|index| FakeChip::sample(bits, index).to_be_bytes()));
        self.answer(PacketKind::Speech, &speech);
    }
}

fn with_channel(channel: Option<u8>, fields: &[u8]) -> Vec<u8> {
    channel
        .map(|channel| vec![field::CHANNEL_0 + channel, 0])
        .unwrap_or_default()
        .into_iter()
        .chain(fields.iter().copied())
        .collect()
}

impl Transport for Link {
    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.chip.state().unplugged {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "unplugged"));
        }
        if !self.listening {
            return Ok(());
        }
        self.deframer.push(bytes);
        while let Some(packet) = self.deframer.next_packet() {
            let packet = packet.map_err(|error| io::Error::other(error.to_string()))?;
            assert_eq!(
                bytes.get(bytes.len().wrapping_sub(2)),
                Some(&field::PARITY),
                "every packet carries parity"
            );
            match packet.kind {
                PacketKind::Control => self.control(&packet.fields),
                PacketKind::Channel => self.decode(&packet.fields),
                PacketKind::Speech => panic!("the decoder never sends speech"),
            }
        }
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if self.replies.is_empty() {
            std::thread::sleep(timeout.min(Duration::from_millis(1)));
            return Ok(0);
        }
        let count = buffer.len().min(self.replies.len());
        for (slot, byte) in buffer.iter_mut().zip(self.replies.drain(..count)) {
            *slot = byte;
        }
        Ok(count)
    }

    fn discard_input(&mut self) -> io::Result<()> {
        self.replies.clear();
        Ok(())
    }
}
