use std::sync::{Arc, Mutex};

use sdrmm_device::{DeviceError, lock};

use crate::{
    api::Rsa,
    caps::{LOW_PATH_TOP_HZ, bandwidth_for},
    model::{Fitted, Found, Limits},
};

const RATE_MATCH_HZ: f64 = 1.0;

/// What a change needs stopped before the analyser takes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pause {
    /// Tuning: the acquisition stops, the IQ stream stays up.
    Acquisition,
    /// Everything else: the IQ stream is rebuilt, because it fixes its rate and the scale of its
    /// samples when it starts.
    Stream,
}

#[derive(Default)]
struct State {
    streaming: bool,
    low_path_sized: bool,
}

/// The one connection the RSA API holds, and whether its IQ stream is up.
pub struct Link {
    pub api: Arc<dyn Rsa>,
    pub limits: Limits,
    pub fitted: Fitted,
    state: Mutex<State>,
}

impl Link {
    pub fn connect(api: Arc<dyn Rsa>, found: &Found) -> Result<Self, DeviceError> {
        api.connect(found)?;
        match Self::survey(api.as_ref()) {
            Ok((limits, fitted)) => Ok(Self {
                api,
                limits,
                fitted,
                state: Mutex::new(State::default()),
            }),
            Err(error) => {
                if let Err(disconnect) = api.disconnect() {
                    tracing::warn!(%disconnect, "disconnecting an analyser that could not be read");
                }
                Err(error)
            }
        }
    }

    fn survey(api: &dyn Rsa) -> Result<(Limits, Fitted), DeviceError> {
        let fitted = Fitted {
            preamp: answers(|| api.preamp(), |on| api.set_preamp(on))?,
            attenuator: answers(|| api.auto_attenuation(), |on| api.set_auto_attenuation(on))?,
            gnss: api.gnss_fitted()?,
        };
        Ok((api.limits()?, fitted))
    }

    pub fn start(&self) -> Result<(), DeviceError> {
        let mut state = lock(&self.state);
        if state.streaming {
            self.quiesce();
            state.streaming = false;
        }
        self.bring_up(&mut state)?;
        state.streaming = true;
        Ok(())
    }

    pub fn halt(&self) {
        let mut state = lock(&self.state);
        if state.streaming {
            self.quiesce();
            state.streaming = false;
        }
    }

    fn quiesce(&self) {
        if let Err(error) = self.api.stream_stop() {
            tracing::warn!(%error, "stopping the analyser's IQ stream");
        }
        if let Err(error) = self.api.stop() {
            tracing::warn!(%error, "stopping the analyser");
        }
    }

    fn bring_up(&self, state: &mut State) -> Result<(), DeviceError> {
        self.size_low_path(state)?;
        self.api.run()?;
        if let Err(error) = self.api.stream_start() {
            if let Err(stop) = self.api.stop() {
                tracing::warn!(%stop, "stopping an analyser whose stream would not start");
            }
            return Err(error);
        }
        Ok(())
    }

    /// The API sizes the buffer behind its three slowest rates for the first of them it streams
    /// after connecting or after a faster rate, and never grows it, so stepping up among them
    /// overflows and drops samples. Streaming the fastest of the three for a moment first sizes
    /// the buffer for all of them.
    fn size_low_path(&self, state: &mut State) -> Result<(), DeviceError> {
        let api = self.api.as_ref();
        let rate = api.rate()?;
        if rate > LOW_PATH_TOP_HZ + RATE_MATCH_HZ {
            state.low_path_sized = false;
            return Ok(());
        }
        if !state.low_path_sized && rate < LOW_PATH_TOP_HZ - RATE_MATCH_HZ {
            api.set_bandwidth(bandwidth_for(LOW_PATH_TOP_HZ, &self.limits))?;
            api.run()?;
            let sized = api.stream_start().and_then(|()| api.stream_stop());
            api.stop()?;
            api.set_bandwidth(bandwidth_for(rate, &self.limits))?;
            sized?;
        }
        state.low_path_sized = true;
        Ok(())
    }

    /// Runs `change` with the analyser stopped as far as `pause` asks. The API applies settings
    /// to a running acquisition without recalibrating, so a stream is paused around every one.
    pub fn paused<T>(
        &self,
        pause: Pause,
        change: impl FnOnce(&dyn Rsa) -> Result<T, DeviceError>,
    ) -> Result<T, DeviceError> {
        let mut state = lock(&self.state);
        let api = self.api.as_ref();
        if !state.streaming {
            return change(api);
        }
        if pause == Pause::Stream {
            api.stream_stop()?;
        }
        api.stop()?;
        let changed = change(api);
        match pause {
            Pause::Acquisition => api.run()?,
            Pause::Stream => self.bring_up(&mut state)?,
        }
        changed
    }
}

/// The API accepts the preamplifier and attenuator switches on every model and reports no error,
/// so an analyser has the part when its switch reads back on.
fn answers(
    read: impl Fn() -> Result<bool, DeviceError>,
    write: impl Fn(bool) -> Result<(), DeviceError>,
) -> Result<bool, DeviceError> {
    if read()? {
        return Ok(true);
    }
    write(true)?;
    let fitted = read()?;
    write(false)?;
    Ok(fitted)
}

impl Drop for Link {
    fn drop(&mut self) {
        self.halt();
        if let Err(error) = self.api.disconnect() {
            tracing::warn!(%error, "disconnecting the analyser");
        }
    }
}
