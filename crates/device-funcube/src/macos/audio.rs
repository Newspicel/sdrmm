use std::{
    ffi::c_void,
    mem::MaybeUninit,
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use objc2_core_audio::{
    AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart,
    AudioDeviceStop, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertyScope, AudioObjectPropertySelector,
    AudioObjectSetPropertyData, kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyDeviceUID,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertyStreams,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeInput, kAudioObjectSystemObject,
    kAudioStreamPropertyVirtualFormat,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved, kAudioFormatLinearPCM,
};
use objc2_core_foundation::{CFRetained, CFString};
use rtrb::{Consumer, Producer, RingBuffer};
use sdrmm_device::{
    Block, BlockGap, BlockPool, CaptureStream, Next, StopHandle, StreamFailure, lock,
};

use crate::{
    convert::Format,
    discovery,
    error::{Error, Result},
    protocol::SAMPLE_RATE_HZ,
};

pub(crate) const BLOCK_SAMPLES: usize = 8192;
const BYTES_PER_SAMPLE: usize = Format::Float32.width();
const BLOCK_BYTES: usize = BLOCK_SAMPLES * BYTES_PER_SAMPLE;
const RING_BYTES: usize = 1 << 21;
const POLL: Duration = Duration::from_millis(5);
const FIRST_SOUND: Duration = Duration::from_secs(1);
const CHANNELS: u32 = 2;
const BITS: u32 = 32;
const NO_ERROR: i32 = 0;

fn address(
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn property<T: Copy>(
    object: AudioObjectID,
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
    operation: &'static str,
) -> Result<T> {
    let address = address(selector, scope);
    let mut value = MaybeUninit::<T>::zeroed();
    let mut size = size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    if status != NO_ERROR {
        return Err(Error::Audio { operation, status });
    }
    if size as usize != size_of::<T>() {
        return Err(Error::protocol(
            operation,
            "Core Audio answered with the wrong size",
        ));
    }
    Ok(unsafe { value.assume_init() })
}

fn objects(
    object: AudioObjectID,
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
    operation: &'static str,
) -> Result<Vec<AudioObjectID>> {
    let address = address(selector, scope);
    let mut size = 0u32;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(&address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != NO_ERROR {
        return Err(Error::Audio { operation, status });
    }
    let mut ids = vec![0 as AudioObjectID; size as usize / size_of::<AudioObjectID>()];
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(ids.as_mut_slice()).cast(),
        )
    };
    if status != NO_ERROR {
        return Err(Error::Audio { operation, status });
    }
    ids.truncate(size as usize / size_of::<AudioObjectID>());
    Ok(ids)
}

fn device_ids() -> Result<Vec<AudioObjectID>> {
    objects(
        kAudioObjectSystemObject.cast_unsigned(),
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
        "list audio devices",
    )
}

fn input_format(device: AudioObjectID) -> Result<AudioStreamBasicDescription> {
    let streams = objects(
        device,
        kAudioDevicePropertyStreams,
        kAudioObjectPropertyScopeInput,
        "list audio input streams",
    )?;
    let [stream] = streams.as_slice() else {
        return Err(Error::protocol(
            "list audio input streams",
            "the dongle should offer exactly one input stream",
        ));
    };
    property(
        *stream,
        kAudioStreamPropertyVirtualFormat,
        kAudioObjectPropertyScopeGlobal,
        "read audio format",
    )
}

fn uid(device: AudioObjectID) -> Option<String> {
    let raw: *const CFString = property(
        device,
        kAudioDevicePropertyDeviceUID,
        kAudioObjectPropertyScopeGlobal,
        "read audio device UID",
    )
    .ok()?;
    let uid = unsafe { CFRetained::from_raw(NonNull::new(raw.cast_mut())?) };
    Some(uid.to_string())
}

pub(super) fn find_input(location: u32) -> Result<AudioObjectID> {
    device_ids()?
        .into_iter()
        .find(|device| uid(*device).is_some_and(|uid| discovery::audio_belongs_to(&uid, location)))
        .ok_or(Error::AudioNotFound)
}

fn is_alive(device: AudioObjectID) -> bool {
    property::<u32>(
        device,
        kAudioDevicePropertyDeviceIsAlive,
        kAudioObjectPropertyScopeGlobal,
        "read audio device liveness",
    )
    .is_ok_and(|alive| alive != 0)
}

fn nominal_rate(device: AudioObjectID) -> Result<f64> {
    property(
        device,
        kAudioDevicePropertyNominalSampleRate,
        kAudioObjectPropertyScopeGlobal,
        "read audio sample rate",
    )
}

fn set_nominal_rate(device: AudioObjectID) -> Result<()> {
    let address = address(
        kAudioDevicePropertyNominalSampleRate,
        kAudioObjectPropertyScopeGlobal,
    );
    let rate = f64::from(SAMPLE_RATE_HZ);
    let status = unsafe {
        AudioObjectSetPropertyData(
            device,
            NonNull::from(&address),
            0,
            std::ptr::null(),
            size_of::<f64>() as u32,
            NonNull::from(&rate).cast(),
        )
    };
    if status != NO_ERROR {
        return Err(Error::Audio {
            operation: "set audio sample rate",
            status,
        });
    }
    Ok(())
}

pub(crate) fn is_float_stereo(format: &AudioStreamBasicDescription) -> bool {
    format.mFormatID == kAudioFormatLinearPCM
        && format.mFormatFlags & kAudioFormatFlagIsFloat != 0
        && format.mFormatFlags & kAudioFormatFlagIsNonInterleaved == 0
        && format.mBitsPerChannel == BITS
        && format.mChannelsPerFrame == CHANNELS
        && format.mBytesPerFrame as usize == BYTES_PER_SAMPLE
}

fn prepare(device: AudioObjectID) -> Result<()> {
    if (nominal_rate(device)? - f64::from(SAMPLE_RATE_HZ)).abs() >= 1.0 {
        set_nominal_rate(device)?;
    }
    let format = input_format(device)?;
    if !is_float_stereo(&format) || (format.mSampleRate - f64::from(SAMPLE_RATE_HZ)).abs() >= 1.0 {
        return Err(Error::protocol(
            "read audio format",
            "Core Audio does not offer 192 kHz float stereo",
        ));
    }
    Ok(())
}

#[derive(Debug, Default)]
pub(crate) struct Shared {
    lost: AtomicU64,
    rewound: AtomicU64,
    frames: AtomicU64,
    heard: AtomicBool,
    stopped: AtomicBool,
    gone: AtomicBool,
}

pub(crate) struct Feed {
    producer: Producer<u8>,
    next_sample_time: f64,
    shared: Arc<Shared>,
}

impl Feed {
    pub(crate) fn new(producer: Producer<u8>, shared: Arc<Shared>) -> Self {
        Self {
            producer,
            next_sample_time: f64::NAN,
            shared,
        }
    }

    pub(crate) fn accept(&mut self, bytes: &[u8], time: Option<f64>) {
        let whole = bytes.len() - bytes.len() % BYTES_PER_SAMPLE;
        let frames = (whole / BYTES_PER_SAMPLE) as u64;
        if let Some(time) = time {
            self.track(time, frames);
        }
        self.shared.frames.fetch_add(frames, Ordering::Relaxed);
        if !self.shared.heard.load(Ordering::Relaxed) && bytes.iter().any(|byte| *byte != 0) {
            self.shared.heard.store(true, Ordering::Release);
        }
        let room = self.producer.slots() - self.producer.slots() % BYTES_PER_SAMPLE;
        let take = whole.min(room);
        if let Ok(mut chunk) = self.producer.write_chunk(take) {
            let (first, second) = chunk.as_mut_slices();
            first.copy_from_slice(&bytes[..first.len()]);
            second.copy_from_slice(&bytes[first.len()..take]);
            chunk.commit_all();
        }
        let overrun = ((whole - take) / BYTES_PER_SAMPLE) as u64;
        if overrun > 0 {
            self.shared.lost.fetch_add(overrun, Ordering::Relaxed);
        }
    }

    fn track(&mut self, time: f64, frames: u64) {
        if self.next_sample_time.is_finite() {
            let skipped = time - self.next_sample_time;
            if skipped >= 0.5 {
                self.shared
                    .lost
                    .fetch_add(skipped.round() as u64, Ordering::Relaxed);
            } else if skipped <= -0.5 {
                self.shared.rewound.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.next_sample_time = time + frames as f64;
    }
}

unsafe fn filled_buffer(list: *const AudioBufferList) -> Option<AudioBuffer> {
    if unsafe { (&raw const (*list).mNumberBuffers).read() } == 0 {
        return None;
    }
    let buffer = unsafe { (&raw const (*list).mBuffers[0]).read() };
    (!buffer.mData.is_null() && buffer.mDataByteSize > 0).then_some(buffer)
}

unsafe extern "C-unwind" fn on_input(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    input_time: NonNull<AudioTimeStamp>,
    _output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    context: *mut c_void,
) -> i32 {
    let Some(mut feed) = NonNull::new(context.cast::<Feed>()) else {
        return NO_ERROR;
    };
    let feed = unsafe { feed.as_mut() };
    let time = unsafe { input_time.as_ref() };
    let filled = unsafe { filled_buffer(input.as_ptr()) };
    let Some(buffer) = filled else {
        return NO_ERROR;
    };
    let bytes = unsafe {
        std::slice::from_raw_parts(buffer.mData.cast::<u8>(), buffer.mDataByteSize as usize)
    };
    let sample_time = time
        .mFlags
        .contains(AudioTimeStampFlags::SampleTimeValid)
        .then_some(time.mSampleTime);
    feed.accept(bytes, sample_time);
    NO_ERROR
}

#[derive(Clone, Debug)]
pub(crate) struct AudioStop(Arc<Shared>);

impl StopHandle for AudioStop {
    fn stop(&self) {
        self.0.stopped.store(true, Ordering::Release);
    }
}

pub(crate) struct AudioStream {
    device: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    feed: NonNull<Feed>,
    consumer: Mutex<Consumer<u8>>,
    shared: Arc<Shared>,
    pool: BlockPool,
    rewinds_seen: AtomicU64,
}

unsafe impl Send for AudioStream {}

impl AudioStream {
    pub(crate) fn start(device: AudioObjectID) -> Result<Self> {
        prepare(device)?;
        let (producer, consumer) = RingBuffer::new(RING_BYTES);
        let shared = Arc::new(Shared::default());
        let feed = NonNull::from(Box::leak(Box::new(Feed::new(producer, shared.clone()))));
        let mut proc_id: AudioDeviceIOProcID = None;
        let created = unsafe {
            AudioDeviceCreateIOProcID(
                device,
                Some(on_input),
                feed.as_ptr().cast(),
                NonNull::from(&mut proc_id),
            )
        };
        if created != NO_ERROR {
            unsafe { drop(Box::from_raw(feed.as_ptr())) };
            return Err(Error::Audio {
                operation: "create audio callback",
                status: created,
            });
        }
        let stream = Self {
            device,
            proc_id,
            feed,
            consumer: Mutex::new(consumer),
            shared,
            pool: BlockPool::default(),
            rewinds_seen: AtomicU64::new(0),
        };
        let started = unsafe { AudioDeviceStart(device, proc_id) };
        if started != NO_ERROR {
            return Err(Error::Audio {
                operation: "start audio",
                status: started,
            });
        }
        stream.await_first_sound()?;
        Ok(stream)
    }

    fn await_first_sound(&self) -> Result<()> {
        let deadline = Instant::now() + FIRST_SOUND;
        while Instant::now() < deadline {
            if self.shared.heard.load(Ordering::Acquire) {
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
        if self.shared.frames.load(Ordering::Relaxed) == 0 {
            return Err(Error::protocol(
                "start audio",
                "no samples arrived within a second",
            ));
        }
        Err(Error::Silenced)
    }

    fn note_rewinds(&self) {
        let rewound = self.shared.rewound.load(Ordering::Relaxed);
        if self.rewinds_seen.swap(rewound, Ordering::Relaxed) != rewound {
            tracing::warn!(
                rewound,
                "funcube audio clock jumped back; samples may repeat"
            );
        }
    }

    fn take_block(&self, consumer: &mut Consumer<u8>) -> Option<Block> {
        let chunk = consumer.read_chunk(BLOCK_BYTES).ok()?;
        let mut block = self.pool.take(BLOCK_BYTES);
        let (first, second) = chunk.as_slices();
        block.bytes_mut()[..first.len()].copy_from_slice(first);
        block.bytes_mut()[first.len()..].copy_from_slice(second);
        chunk.commit_all();
        Some(block)
    }
}

impl CaptureStream for AudioStream {
    type Block = Block;
    type Stop = AudioStop;

    fn stop_handle(&self) -> AudioStop {
        AudioStop(self.shared.clone())
    }

    fn next_block(&self, timeout: Duration) -> Next<Block> {
        let deadline = Instant::now() + timeout;
        let mut consumer = lock(&self.consumer);
        loop {
            if self.shared.stopped.load(Ordering::Acquire) {
                return Next::Ended;
            }
            if let Some(block) = self.take_block(&mut consumer) {
                return Next::Block(block);
            }
            self.note_rewinds();
            let now = Instant::now();
            if now >= deadline {
                if !is_alive(self.device) {
                    self.shared.gone.store(true, Ordering::Release);
                    return Next::Ended;
                }
                return Next::Idle;
            }
            std::thread::sleep(POLL.min(deadline - now));
        }
    }

    fn dropped(&self) -> u64 {
        self.shared.lost.load(Ordering::Relaxed)
    }

    fn block_gap(&self, _block: &Block, _bytes_per_sample: u64) -> Option<BlockGap> {
        Some(BlockGap {
            exact: self.shared.lost.swap(0, Ordering::Relaxed),
            estimated: 0,
        })
    }

    fn failure(&self) -> StreamFailure {
        let gone = self.shared.gone.load(Ordering::Acquire);
        let reason = if gone {
            "the FUNcube Dongle Pro+ is gone"
        } else if self.shared.stopped.load(Ordering::Acquire) {
            "funcube capture stopped"
        } else {
            "Core Audio stopped delivering"
        };
        StreamFailure {
            reason: reason.to_string(),
            gone,
        }
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        let stopped = unsafe { AudioDeviceStop(self.device, self.proc_id) };
        let destroyed = unsafe { AudioDeviceDestroyIOProcID(self.device, self.proc_id) };
        if stopped != NO_ERROR || destroyed != NO_ERROR {
            tracing::debug!(stopped, destroyed, "funcube audio teardown");
        }
        unsafe { drop(Box::from_raw(self.feed.as_ptr())) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(capacity: usize) -> (Feed, Consumer<u8>, Arc<Shared>) {
        let (producer, consumer) = RingBuffer::new(capacity);
        let shared = Arc::new(Shared::default());
        (Feed::new(producer, shared.clone()), consumer, shared)
    }

    fn frames(count: usize) -> Vec<u8> {
        (0..count * BYTES_PER_SAMPLE)
            .map(|n| (n % 251 + 1) as u8)
            .collect()
    }

    fn buffer_list(buffers: u32, bytes: &mut [u8]) -> AudioBufferList {
        AudioBufferList {
            mNumberBuffers: buffers,
            mBuffers: [AudioBuffer {
                mNumberChannels: 2,
                mDataByteSize: bytes.len() as u32,
                mData: bytes.as_mut_ptr().cast(),
            }],
        }
    }

    #[test]
    fn a_list_that_carries_no_buffer_is_left_untouched() {
        let mut bytes = frames(4);
        let empty = buffer_list(0, &mut bytes);
        assert!(unsafe { filled_buffer(&raw const empty) }.is_none());
        let filled = buffer_list(1, &mut bytes);
        let got = unsafe { filled_buffer(&raw const filled) }.expect("one buffer");
        assert_eq!(got.mDataByteSize as usize, bytes.len());
        let mut nothing: [u8; 0] = [];
        let short = buffer_list(1, &mut nothing);
        assert!(unsafe { filled_buffer(&raw const short) }.is_none());
    }

    #[test]
    fn whole_frames_pass_through_in_order() {
        let (mut feed, mut consumer, shared) = feed(4096);
        let bytes = frames(16);
        feed.accept(&bytes, Some(0.0));
        let got: Vec<u8> = std::iter::from_fn(|| consumer.pop().ok()).collect();
        assert_eq!(got, bytes);
        assert_eq!(shared.frames.load(Ordering::Relaxed), 16);
        assert!(shared.heard.load(Ordering::Relaxed));
        assert_eq!(shared.lost.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn silence_is_noticed_as_silence() {
        let (mut feed, _consumer, shared) = feed(4096);
        feed.accept(&[0; 64], Some(0.0));
        assert!(!shared.heard.load(Ordering::Relaxed));
    }

    #[test]
    fn a_full_ring_counts_every_frame_it_could_not_keep() {
        let (mut feed, _consumer, shared) = feed(10 * BYTES_PER_SAMPLE + 3);
        feed.accept(&frames(16), None);
        assert_eq!(shared.lost.load(Ordering::Relaxed), 6);
    }

    #[test]
    fn a_skip_in_the_audio_clock_counts_as_lost_frames() {
        let (mut feed, _consumer, shared) = feed(1 << 16);
        feed.accept(&frames(512), Some(1000.0));
        feed.accept(&frames(512), Some(1512.0));
        assert_eq!(shared.lost.load(Ordering::Relaxed), 0);
        feed.accept(&frames(512), Some(3048.0));
        assert_eq!(shared.lost.load(Ordering::Relaxed), 1024);
        feed.accept(&frames(512), Some(10.0));
        assert_eq!(shared.rewound.load(Ordering::Relaxed), 1);
        assert_eq!(shared.lost.load(Ordering::Relaxed), 1024);
    }

    #[test]
    fn a_part_frame_is_never_written() {
        let (mut feed, consumer, shared) = feed(4096);
        feed.accept(&frames(3)[..20], None);
        assert_eq!(consumer.slots(), 16);
        assert_eq!(shared.frames.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn only_float_stereo_is_taken() {
        let mut format = AudioStreamBasicDescription {
            mSampleRate: 192_000.0,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat | 8,
            mBytesPerPacket: 8,
            mFramesPerPacket: 1,
            mBytesPerFrame: 8,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        assert!(is_float_stereo(&format));
        format.mFormatFlags |= kAudioFormatFlagIsNonInterleaved;
        assert!(!is_float_stereo(&format));
        format.mFormatFlags = 8;
        assert!(!is_float_stereo(&format));
    }
}
