use sdrmm_wire::{RadioUse, ServerEvent, StateScope};

use crate::{Engine, EngineError};

/// Why a transmitter will not arm on a radio set to receive.
pub const SET_TO_RECEIVE: &str = "Set the radio to send";

impl Engine {
    /// Uses an open radio for `radio_use`, narrowed to what it can do. A radio set to receive
    /// drops its transmitter; one whose receiving starts or stops restarts its lanes.
    pub fn set_radio_use(&self, ds: u32, radio_use: RadioUse) -> Result<(), EngineError> {
        let (was, now) = {
            let mut inner = self.lock();
            let state = inner
                .device_sets
                .get_mut(&ds)
                .ok_or(EngineError::DeviceSetNotFound(ds))?;
            let wanted = radio_use.within(state.capabilities.duplex);
            if wanted == state.radio_use {
                return Ok(());
            }
            let was = std::mem::replace(&mut state.radio_use, wanted);
            inner.revision += 1;
            (was, wanted)
        };
        if !now.sends() {
            self.halt_transmitter(ds);
        }
        let restarted = if was.receives() == now.receives() {
            Ok(())
        } else {
            self.restart_lanes(ds)
        };
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::DeviceSet(ds),
        });
        restarted
    }
}
