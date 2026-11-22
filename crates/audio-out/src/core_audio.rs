use std::{ffi::c_void, mem::MaybeUninit, ptr::NonNull, sync::Arc};

use objc2_core_audio::{
    AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart,
    AudioDeviceStop, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertyScope, AudioObjectPropertySelector,
    kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyNominalSampleRate,
    kAudioDevicePropertyStreams, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
    kAudioStreamPropertyVirtualFormat,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp,
    kAudioFormatFlagIsFloat, kAudioFormatLinearPCM,
};
use objc2_core_foundation::{CFRetained, CFString};

use crate::{
    CHANNELS, Error, Result,
    mixer::{Mixer, Shared},
};

const NO_ERROR: i32 = 0;
const CHUNK_FRAMES: usize = 4096;
const FLOAT_BITS: u32 = 32;

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
        return Err(Error::Platform { operation, status });
    }
    if size as usize != size_of::<T>() {
        return Err(Error::Backend(format!(
            "{operation}: Core Audio answered with the wrong size"
        )));
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
        return Err(Error::Platform { operation, status });
    }
    let mut ids = vec![0 as AudioObjectID; size as usize / size_of::<AudioObjectID>()];
    if ids.is_empty() {
        return Ok(ids);
    }
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
        return Err(Error::Platform { operation, status });
    }
    ids.truncate(size as usize / size_of::<AudioObjectID>());
    Ok(ids)
}

fn system() -> AudioObjectID {
    kAudioObjectSystemObject.cast_unsigned()
}

fn output_streams(device: AudioObjectID) -> Result<Vec<AudioObjectID>> {
    objects(
        device,
        kAudioDevicePropertyStreams,
        kAudioObjectPropertyScopeOutput,
        "list audio output streams",
    )
}

fn name(device: AudioObjectID) -> Option<String> {
    let raw: *const CFString = property(
        device,
        kAudioObjectPropertyName,
        kAudioObjectPropertyScopeGlobal,
        "read audio device name",
    )
    .ok()?;
    let name = unsafe { CFRetained::from_raw(NonNull::new(raw.cast_mut())?) };
    Some(name.to_string())
}

fn output_devices() -> Result<Vec<(AudioObjectID, String)>> {
    let devices = objects(
        system(),
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
        "list audio devices",
    )?;
    Ok(devices
        .into_iter()
        .filter(|device| output_streams(*device).is_ok_and(|streams| !streams.is_empty()))
        .filter_map(|device| Some((device, name(device)?)))
        .collect())
}

pub(crate) fn outputs() -> Result<Vec<String>> {
    Ok(output_devices()?
        .into_iter()
        .map(|(_, name)| name)
        .collect())
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

fn is_float(format: &AudioStreamBasicDescription) -> bool {
    format.mFormatID == kAudioFormatLinearPCM
        && format.mFormatFlags & kAudioFormatFlagIsFloat != 0
        && format.mBitsPerChannel == FLOAT_BITS
}

pub(crate) struct Device {
    id: AudioObjectID,
    rate: u32,
}

impl Device {
    pub(crate) fn find(wanted: Option<&str>) -> Result<Self> {
        let id = match wanted {
            None => property(
                system(),
                kAudioHardwarePropertyDefaultOutputDevice,
                kAudioObjectPropertyScopeGlobal,
                "find the default audio output",
            )?,
            Some(wanted) => output_devices()?
                .into_iter()
                .find(|(_, name)| name == wanted)
                .map(|(id, _)| id)
                .ok_or_else(|| Error::NotFound(wanted.to_owned()))?,
        };
        let rate: f64 = property(
            id,
            kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal,
            "read audio sample rate",
        )?;
        let streams = output_streams(id)?;
        let Some(first) = streams.first() else {
            return Err(Error::Backend("The device has no output".to_owned()));
        };
        let format: AudioStreamBasicDescription = property(
            *first,
            kAudioStreamPropertyVirtualFormat,
            kAudioObjectPropertyScopeGlobal,
            "read audio format",
        )?;
        if !is_float(&format) {
            return Err(Error::Backend(
                "The device does not take float audio".to_owned(),
            ));
        }
        Ok(Self {
            id,
            rate: rate.round() as u32,
        })
    }

    pub(crate) const fn rate(&self) -> u32 {
        self.rate
    }

    pub(crate) fn start(self, mixer: Mixer, shared: Arc<Shared>) -> Result<Stream> {
        let render = NonNull::from(Box::leak(Box::new(Render {
            mixer,
            scratch: vec![0.0; CHUNK_FRAMES * CHANNELS],
        })));
        let mut proc_id: AudioDeviceIOProcID = None;
        let created = unsafe {
            AudioDeviceCreateIOProcID(
                self.id,
                Some(on_output),
                render.as_ptr().cast(),
                NonNull::from(&mut proc_id),
            )
        };
        if created != NO_ERROR {
            unsafe { drop(Box::from_raw(render.as_ptr())) };
            return Err(Error::Platform {
                operation: "create audio callback",
                status: created,
            });
        }
        let stream = Stream {
            device: self.id,
            proc_id,
            render,
            shared,
        };
        let started = unsafe { AudioDeviceStart(self.id, proc_id) };
        if started != NO_ERROR {
            return Err(Error::Platform {
                operation: "start audio output",
                status: started,
            });
        }
        Ok(stream)
    }
}

struct Render {
    mixer: Mixer,
    scratch: Vec<f32>,
}

impl Render {
    /// Fills every buffer of the list: left and right on the first two device channels, their
    /// mean on a mono device, silence on the rest.
    fn fill(&mut self, buffers: &mut [AudioBuffer]) {
        let lanes: usize = buffers
            .iter()
            .map(|buffer| buffer.mNumberChannels as usize)
            .sum();
        let Some(frames) = buffers.first().map(frames_in) else {
            return;
        };
        let mut done = 0;
        while done < frames {
            let count = (frames - done).min(CHUNK_FRAMES);
            let stereo = &mut self.scratch[..count * CHANNELS];
            self.mixer.fill(stereo);
            let mut lane = 0;
            for buffer in buffers.iter_mut() {
                let width = buffer.mNumberChannels as usize;
                if let Some(samples) = samples_of(buffer, frames) {
                    spread(
                        stereo,
                        &mut samples[done * width..(done + count) * width],
                        width,
                        lane,
                        lanes,
                    );
                }
                lane += width;
            }
            done += count;
        }
    }
}

fn frames_in(buffer: &AudioBuffer) -> usize {
    let width = (buffer.mNumberChannels as usize).max(1);
    buffer.mDataByteSize as usize / (size_of::<f32>() * width)
}

fn samples_of(buffer: &mut AudioBuffer, frames: usize) -> Option<&mut [f32]> {
    let width = buffer.mNumberChannels as usize;
    if buffer.mData.is_null() || frames_in(buffer) < frames || width == 0 {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts_mut(buffer.mData.cast::<f32>(), frames * width) })
}

fn spread(stereo: &[f32], out: &mut [f32], width: usize, first_lane: usize, lanes: usize) {
    let (pairs, _) = stereo.as_chunks::<CHANNELS>();
    for (frame, pair) in out.chunks_exact_mut(width).zip(pairs) {
        for (offset, sample) in frame.iter_mut().enumerate() {
            *sample = match (lanes, first_lane + offset) {
                (1, 0) => f32::midpoint(pair[0], pair[1]),
                (_, 0) => pair[0],
                (_, 1) => pair[1],
                _ => 0.0,
            };
        }
    }
}

unsafe extern "C-unwind" fn on_output(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    _input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    context: *mut c_void,
) -> i32 {
    let Some(mut render) = NonNull::new(context.cast::<Render>()) else {
        return NO_ERROR;
    };
    let render = unsafe { render.as_mut() };
    let list = output.as_ptr();
    let count = unsafe { (&raw const (*list).mNumberBuffers).read() } as usize;
    if count == 0 {
        return NO_ERROR;
    }
    let first = unsafe { (&raw mut (*list).mBuffers).cast::<AudioBuffer>() };
    let buffers = unsafe { std::slice::from_raw_parts_mut(first, count) };
    render.fill(buffers);
    NO_ERROR
}

pub(crate) struct Stream {
    device: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    render: NonNull<Render>,
    shared: Arc<Shared>,
}

unsafe impl Send for Stream {}

impl Stream {
    pub(crate) fn fault(&self) -> Option<String> {
        if !is_alive(self.device) {
            self.shared.fail("The device is gone".to_owned());
        }
        self.shared.fault()
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let stopped = unsafe { AudioDeviceStop(self.device, self.proc_id) };
        let destroyed = unsafe { AudioDeviceDestroyIOProcID(self.device, self.proc_id) };
        if stopped != NO_ERROR || destroyed != NO_ERROR {
            tracing::debug!(stopped, destroyed, "audio output teardown");
        }
        unsafe { drop(Box::from_raw(self.render.as_ptr())) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_lands_on_the_first_two_lanes_of_a_wide_device() {
        let stereo = [0.25, -0.5, 1.0, 0.0];
        let mut out = [9.0; 8];
        spread(&stereo, &mut out, 4, 0, 4);
        assert_eq!(out, [0.25, -0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_mono_device_hears_both_sides() {
        let mut out = [9.0; 1];
        spread(&[0.5, 0.25], &mut out, 1, 0, 1);
        assert_eq!(out, [0.375]);
    }

    #[test]
    fn split_buffers_each_get_their_own_side() {
        let stereo = [0.25, -0.5];
        let (mut left, mut right) = ([9.0; 1], [9.0; 1]);
        spread(&stereo, &mut left, 1, 0, 2);
        spread(&stereo, &mut right, 1, 1, 2);
        assert_eq!((left, right), ([0.25], [-0.5]));
    }
}
