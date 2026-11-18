use rtrb::{Consumer, Producer, PushError, RingBuffer};
use sdrmm_wire::{RadarUpdate, RangeDopplerOwned, SurfaceFrame};

use crate::passive_radar::{PassiveRadarParams, RadarPod, fill_surface, fill_update};
use crate::{ProcessorOutput, Reading, Settings};

const PACKETS: usize = 4;

struct Packet {
    reading: Reading,
    surface: SurfaceFrame,
    with_surface: bool,
}

impl Packet {
    fn new(cells: usize) -> Self {
        Self {
            reading: Reading::new(PassiveRadarParams::TYPE_ID, RadarUpdate::reserved()),
            surface: SurfaceFrame::RangeDoppler(RangeDopplerOwned {
                cells: Vec::with_capacity(cells),
                ..RangeDopplerOwned::default()
            }),
            with_surface: false,
        }
    }

    fn fill(&mut self, pod: &RadarPod, with_surface: bool) {
        if let Some(update) = self.reading.get_mut::<RadarUpdate>() {
            fill_update(pod, update);
        }
        if with_surface {
            fill_surface(pod, &mut self.surface);
        }
        self.with_surface = with_surface;
    }
}

pub(super) struct Publisher {
    free: Consumer<Box<Packet>>,
    ready: Producer<Box<Packet>>,
    spare: Option<Box<Packet>>,
}

impl Publisher {
    pub(super) fn publish(&mut self, pod: &RadarPod, with_surface: bool) -> bool {
        let Some(mut packet) = self.spare.take().or_else(|| self.free.pop().ok()) else {
            return false;
        };
        packet.fill(pod, with_surface);
        match self.ready.push(packet) {
            Ok(()) => true,
            Err(PushError::Full(packet)) => {
                self.spare = Some(packet);
                false
            }
        }
    }
}

pub(super) struct Mailbox {
    ready: Consumer<Box<Packet>>,
    free: Producer<Box<Packet>>,
}

impl Mailbox {
    pub(super) fn deliver(&mut self, out: &mut ProcessorOutput<'_>) -> bool {
        let Ok(mut packet) = self.ready.pop() else {
            return false;
        };
        if let Some(slot) = out.report() {
            std::mem::swap(slot, &mut packet.reading);
            out.publish_report();
        }
        if packet.with_surface
            && let Some(slot) = out.surface()
        {
            std::mem::swap(slot, &mut packet.surface);
            out.publish_surface();
        }
        let returned = self.free.push(packet);
        debug_assert!(returned.is_ok());
        true
    }
}

pub(super) fn channel(cells: usize) -> (Publisher, Mailbox) {
    let (ready_tx, ready_rx) = RingBuffer::new(PACKETS);
    let (mut free_tx, free_rx) = RingBuffer::new(PACKETS);
    for _ in 0..PACKETS {
        let filled = free_tx.push(Box::new(Packet::new(cells)));
        debug_assert!(filled.is_ok());
    }
    (
        Publisher {
            free: free_rx,
            ready: ready_tx,
            spare: None,
        },
        Mailbox {
            ready: ready_rx,
            free: free_tx,
        },
    )
}
