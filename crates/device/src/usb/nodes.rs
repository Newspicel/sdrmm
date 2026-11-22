use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use nusb::MaybeFuture;
use rustix::fs::{Access, AtFlags, CWD, accessat};

use sdrmm_wire::UsbIdentity;

use super::RadioNode;

fn radio_name(identities: &[UsbIdentity], vendor: u16, product: u16) -> Option<String> {
    identities
        .iter()
        .find(|identity| identity.matches(vendor, product))
        .map(|identity| identity.name.clone())
}

/// The device nodes of the attached USB radios, each with what this process may do with it.
///
/// Listing the bus opens no device, and the kernel answers the access question itself, so an ACL
/// from a `uaccess` udev rule counts as much as the file mode does. `identities` are the USB
/// devices the registered drivers speak to, as [`crate::DeviceRegistry::usb_identities`] lists them.
#[must_use]
pub fn radio_nodes(identities: &[UsbIdentity]) -> Vec<RadioNode> {
    let devices = match nusb::list_devices().wait() {
        Ok(devices) => devices,
        Err(error) => {
            tracing::debug!("usb enumeration unavailable: {error}");
            return Vec::new();
        }
    };
    let mut nodes: Vec<RadioNode> = devices
        .filter_map(|device| {
            let name = radio_name(identities, device.vendor_id(), device.product_id())?;
            let path = PathBuf::from(format!(
                "/dev/bus/usb/{:03}/{:03}",
                device.busnum(),
                device.device_address()
            ));
            let node = std::fs::metadata(&path).ok();
            Some(RadioNode {
                name,
                present: node.is_some(),
                uid: node.as_ref().map_or(0, MetadataExt::uid),
                gid: node.as_ref().map_or(0, MetadataExt::gid),
                mode: node.as_ref().map_or(0, |node| node.mode() & 0o777),
                openable: node.is_some() && openable(&path),
                path,
            })
        })
        .collect();
    nodes.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    nodes
}

fn openable(path: &Path) -> bool {
    accessat(
        CWD,
        path,
        Access::READ_OK | Access::WRITE_OK,
        AtFlags::EACCESS,
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identities() -> Vec<UsbIdentity> {
        vec![
            UsbIdentity::new(0x0bda, Some(0x2838), "RTL-SDR"),
            UsbIdentity::new(0x1df7, None, "SDRplay RSP"),
        ]
    }

    #[test]
    fn a_radio_is_named_by_its_usb_identity() {
        assert_eq!(
            radio_name(&identities(), 0x0bda, 0x2838).as_deref(),
            Some("RTL-SDR")
        );
    }

    #[test]
    fn a_vendor_that_makes_only_radios_claims_every_product() {
        assert_eq!(
            radio_name(&identities(), 0x1df7, 0x3020).as_deref(),
            Some("SDRplay RSP")
        );
        assert_eq!(
            radio_name(&identities(), 0x1df7, 0x2500).as_deref(),
            Some("SDRplay RSP")
        );
    }

    #[test]
    fn anything_else_on_the_bus_is_not_a_radio() {
        assert_eq!(radio_name(&identities(), 0x0bda, 0x2839), None);
        assert_eq!(radio_name(&identities(), 0x1234, 0x5678), None);
    }

    #[test]
    fn a_node_nobody_has_cannot_be_opened() {
        assert!(!openable(Path::new("/dev/bus/usb/255/255")));
    }

    #[test]
    fn a_node_we_own_can_be_opened() {
        let path = std::env::temp_dir().join(format!("sdrmm-openable-{}", std::process::id()));
        std::fs::write(&path, []).expect("temp node");
        let result = openable(&path);
        std::fs::remove_file(&path).expect("remove temp node");
        assert!(result);
    }

    #[test]
    fn listing_the_bus_reports_something_or_nothing_without_panicking() {
        for node in radio_nodes(&identities()) {
            assert!(node.path.starts_with("/dev/bus/usb"));
        }
    }
}
