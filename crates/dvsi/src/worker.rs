use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError, mpsc::Receiver},
    time::{Duration, Instant},
};

use sdrmm_channels::chip::{CHIP_FRAME_SAMPLES, ChipEnd, ChipFrame, ChipHealth};

use crate::{
    ChipError,
    chip::{BAUDS, Chip},
    transport::Opener,
};

const POLL: Duration = Duration::from_millis(2);
/// A chip channel goes back to the pool after a call has been quiet this long.
const LEASE_IDLE: Duration = Duration::from_millis(500);
const RECONNECT: Duration = Duration::from_secs(1);
/// The port closes once no decoder has used it this long.
const IDLE_EXIT: Duration = Duration::from_secs(10);
const SILENCE: [i16; CHIP_FRAME_SAMPLES] = [0; CHIP_FRAME_SAMPLES];

pub(crate) struct Dongle {
    pub(crate) attach: std::sync::mpsc::Sender<ChipEnd>,
    pub(crate) name: Arc<str>,
    pub(crate) id: u64,
}

pub(crate) type Dongles = Arc<Mutex<HashMap<String, Dongle>>>;

struct Linked {
    end: ChipEnd,
    channel: Option<u8>,
    last: Instant,
    undelivered: u32,
}

impl Linked {
    fn new(end: ChipEnd) -> Self {
        Self {
            end,
            channel: None,
            last: Instant::now(),
            undelivered: 0,
        }
    }

    fn deliver(&mut self, speech: [i16; CHIP_FRAME_SAMPLES]) {
        if !self.end.deliver(speech) {
            self.undelivered += 1;
            if self.undelivered.is_power_of_two() {
                tracing::warn!(
                    dropped = self.undelivered,
                    "dvsi speech not drained by its decoder"
                );
            }
        }
    }
}

/// Serves every link to one chip from its own thread.
pub(crate) struct Worker {
    port: String,
    id: u64,
    opener: Arc<dyn Opener>,
    chip: Option<Chip>,
    channels: Vec<bool>,
    links: Vec<Linked>,
    attached: Receiver<ChipEnd>,
    lost_at: Instant,
    used_at: Instant,
}

impl Worker {
    pub(crate) fn new(
        chip: Chip,
        port: &str,
        id: u64,
        opener: Arc<dyn Opener>,
        attached: Receiver<ChipEnd>,
    ) -> Self {
        Self {
            port: port.to_owned(),
            id,
            opener,
            channels: vec![false; usize::from(chip.info().channels)],
            chip: Some(chip),
            links: Vec::new(),
            attached,
            lost_at: Instant::now(),
            used_at: Instant::now(),
        }
    }

    pub(crate) fn run(mut self, dongles: &Dongles) {
        loop {
            self.adopt();
            if self.links.is_empty() && self.used_at.elapsed() >= IDLE_EXIT && self.retire(dongles)
            {
                tracing::info!(port = self.port, "dvsi chip closed");
                return;
            }
            let worked = if self.chip.is_some() {
                self.serve()
            } else {
                self.reconnect();
                self.discard();
                false
            };
            self.release_idle();
            if !worked {
                std::thread::sleep(POLL);
            }
        }
    }

    fn adopt(&mut self) {
        while let Ok(end) = self.attached.try_recv() {
            if self.chip.is_none() {
                end.set_health(ChipHealth::Lost);
            }
            self.links.push(Linked::new(end));
        }
    }

    /// Leaves the hub's table unless a decoder attached meanwhile; `true` when it left.
    fn retire(&mut self, dongles: &Dongles) -> bool {
        let mut dongles = dongles.lock().unwrap_or_else(PoisonError::into_inner);
        self.adopt();
        if !self.links.is_empty() {
            return false;
        }
        if dongles
            .get(&self.port)
            .is_some_and(|dongle| dongle.id == self.id)
        {
            dongles.remove(&self.port);
        }
        true
    }

    fn serve(&mut self) -> bool {
        let Some(mut chip) = self.chip.take() else {
            return false;
        };
        let mut worked = false;
        let mut failure = None;
        for link in &mut self.links {
            match serve_link(&mut chip, &mut self.channels, link) {
                Ok(served) => worked |= served,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if worked {
            self.used_at = Instant::now();
        }
        match failure {
            None => self.chip = Some(chip),
            Some(error) => self.lose(&error),
        }
        self.drop_abandoned();
        worked
    }

    fn lose(&mut self, error: &ChipError) {
        tracing::warn!(port = self.port, %error, "dvsi chip lost");
        self.channels.fill(false);
        for link in &mut self.links {
            link.channel = None;
            link.end.set_health(ChipHealth::Lost);
        }
        self.lost_at = Instant::now();
    }

    fn reconnect(&mut self) {
        if self.lost_at.elapsed() < RECONNECT {
            return;
        }
        match Chip::probe(self.opener.as_ref(), &self.port, &BAUDS) {
            Ok(chip) => {
                tracing::info!(
                    port = self.port,
                    product = chip.info().product,
                    "dvsi chip back"
                );
                self.channels = vec![false; usize::from(chip.info().channels)];
                self.chip = Some(chip);
                for link in &self.links {
                    link.end.set_health(ChipHealth::Ready);
                }
            }
            Err(error) => {
                tracing::debug!(port = self.port, %error, "dvsi chip still gone");
                self.lost_at = Instant::now();
            }
        }
    }

    fn discard(&mut self) {
        for link in &mut self.links {
            while link.end.next_frame().is_some() {}
        }
        self.drop_abandoned();
    }

    fn release_idle(&mut self) {
        for link in &mut self.links {
            if let Some(channel) = link.channel
                && link.last.elapsed() >= LEASE_IDLE
            {
                self.channels[usize::from(channel)] = false;
                link.channel = None;
            }
        }
    }

    fn drop_abandoned(&mut self) {
        let channels = &mut self.channels;
        self.links.retain(|link| {
            if !link.end.abandoned() {
                return true;
            }
            if let Some(channel) = link.channel {
                channels[usize::from(channel)] = false;
            }
            false
        });
    }
}

/// Answers every frame `link` has queued; `true` when there was any.
fn serve_link(
    chip: &mut Chip,
    channels: &mut [bool],
    link: &mut Linked,
) -> Result<bool, ChipError> {
    let mut served = false;
    while let Some(frame) = link.end.next_frame() {
        served = true;
        link.last = Instant::now();
        match frame {
            ChipFrame::Silence => link.deliver(SILENCE),
            ChipFrame::Reset => {
                if let Some(channel) = link.channel {
                    chip.restart_decoder(channel)?;
                }
            }
            ChipFrame::Voice { bits, played } => {
                let Some(channel) = lease(chip, channels, link)? else {
                    link.end.set_health(ChipHealth::Busy);
                    if !played {
                        link.deliver(SILENCE);
                    }
                    continue;
                };
                link.end.set_health(ChipHealth::Ready);
                let speech = chip.decode(channel, link.end.rate(), &bits)?;
                if !played {
                    link.deliver(speech);
                }
            }
        }
    }
    Ok(served)
}

/// The chip channel `link` decodes on, taking a free one for a new call.
fn lease(
    chip: &mut Chip,
    channels: &mut [bool],
    link: &mut Linked,
) -> Result<Option<u8>, ChipError> {
    if link.channel.is_some() {
        return Ok(link.channel);
    }
    let Some(free) = channels.iter().position(|held| !held) else {
        return Ok(None);
    };
    let channel =
        u8::try_from(free).map_err(|_| ChipError::Protocol(format!("chip channel {free}")))?;
    chip.configure(channel, link.end.rate())?;
    channels[free] = true;
    link.channel = Some(channel);
    Ok(Some(channel))
}
