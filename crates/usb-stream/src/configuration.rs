use nusb::{Device, Error, ErrorKind, MaybeFuture};

/// Puts `device` in `configuration`, and asks the operating system for nothing when the device is
/// already there. Linux usbfs refuses SET_CONFIGURATION while any interface is claimed, so a
/// redundant request on a radio that a kernel module has bound comes back as EBUSY: the radio then
/// reads as held by another program instead of being claimed away from that module a moment later.
/// Windows supports no configuration switch at all, and reports one as unsupported.
pub fn select_configuration(device: &Device, configuration: u8) -> Result<(), Error> {
    if !needs_switch(active_configuration(device), configuration) {
        return Ok(());
    }
    match device.set_configuration(configuration).wait() {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::Unsupported => Ok(()),
        Err(error) => Err(error),
    }
}

fn active_configuration(device: &Device) -> Option<u8> {
    device
        .active_configuration()
        .ok()
        .map(|configuration| configuration.configuration_value())
}

fn needs_switch(active: Option<u8>, wanted: u8) -> bool {
    active != Some(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_radio_already_in_the_configuration_is_left_alone() {
        assert!(!needs_switch(Some(1), 1));
    }

    #[test]
    fn an_unconfigured_radio_is_switched() {
        assert!(needs_switch(None, 1));
    }

    #[test]
    fn another_configuration_is_switched() {
        assert!(needs_switch(Some(2), 1));
    }
}
