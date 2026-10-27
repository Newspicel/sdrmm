pub use serialport::{
    ClearBuffer, Error, ErrorKind, FlowControl, SerialPort, SerialPortBuilder, SerialPortInfo,
    SerialPortType, UsbPortInfo, new,
};

const DIAL_IN: &str = "tty.";
const PSEUDO_PORTS: [&str; 4] = [
    "Bluetooth-Incoming-Port",
    "debug-console",
    "wlan-debug",
    "WirelessiAP",
];
const SILABS_CALLOUT: &str = "/dev/cu.SLAB_USBtoUART";
const BRIDGE_CALLOUT: &str = "/dev/cu.usbserial-";

pub fn ports() -> Result<Vec<SerialPortInfo>, Error> {
    serialport::available_ports().map(usable)
}

#[must_use]
pub fn usable(ports: Vec<SerialPortInfo>) -> Vec<SerialPortInfo> {
    ports
        .iter()
        .filter(|info| !hidden(info, &ports))
        .cloned()
        .collect()
}

fn hidden(info: &SerialPortInfo, ports: &[SerialPortInfo]) -> bool {
    is_dial_in(&info.port_name) || is_pseudo(&info.port_name) || duplicate_bridge_node(info, ports)
}

fn node(port: &str) -> &str {
    port.rsplit('/').next().unwrap_or(port)
}

fn is_dial_in(port: &str) -> bool {
    node(port).starts_with(DIAL_IN)
}

fn is_pseudo(port: &str) -> bool {
    let node = node(port);
    PSEUDO_PORTS.iter().any(|pseudo| node.contains(pseudo))
}

fn duplicate_bridge_node(info: &SerialPortInfo, ports: &[SerialPortInfo]) -> bool {
    let Some(identity) = usb_identity(info) else {
        return false;
    };
    info.port_name.starts_with(SILABS_CALLOUT)
        && ports.iter().any(|other| {
            other.port_name.starts_with(BRIDGE_CALLOUT) && usb_identity(other) == Some(identity)
        })
}

fn usb_identity(info: &SerialPortInfo) -> Option<(u16, u16, Option<&str>)> {
    match &info.port_type {
        SerialPortType::UsbPort(usb) => Some((usb.vid, usb.pid, usb.serial_number.as_deref())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb(port: &str, serial: &str) -> SerialPortInfo {
        SerialPortInfo {
            port_name: port.to_owned(),
            port_type: SerialPortType::UsbPort(UsbPortInfo {
                vid: 0x10c4,
                pid: 0xea60,
                serial_number: Some(serial.to_owned()),
                manufacturer: Some("Silicon Labs".to_owned()),
                product: Some("CP2102 USB to UART Bridge".to_owned()),
            }),
        }
    }

    fn plain(port: &str) -> SerialPortInfo {
        SerialPortInfo {
            port_name: port.to_owned(),
            port_type: SerialPortType::Unknown,
        }
    }

    fn names(ports: Vec<SerialPortInfo>) -> Vec<String> {
        usable(ports)
            .into_iter()
            .map(|info| info.port_name)
            .collect()
    }

    #[test]
    fn a_dial_in_node_is_hidden_whether_or_not_its_call_out_twin_is_listed() {
        assert_eq!(
            names(vec![
                usb("/dev/tty.usbserial-0001", "0001"),
                usb("/dev/cu.usbserial-0001", "0001"),
                usb("/dev/tty.usbserial-lonely", "0002"),
            ]),
            ["/dev/cu.usbserial-0001"]
        );
    }

    #[test]
    fn a_linux_or_windows_port_keeps_its_name() {
        assert_eq!(
            names(vec![
                plain("/dev/ttyUSB0"),
                plain("/dev/ttyACM0"),
                plain("COM4"),
            ]),
            ["/dev/ttyUSB0", "/dev/ttyACM0", "COM4"]
        );
    }

    #[test]
    fn the_pseudo_ports_a_mac_always_shows_are_hidden() {
        assert!(
            names(vec![
                plain("/dev/cu.Bluetooth-Incoming-Port"),
                plain("/dev/cu.debug-console"),
                plain("/dev/cu.wlan-debug"),
                plain("/dev/cu.WirelessiAP"),
            ])
            .is_empty()
        );
    }

    #[test]
    fn the_second_call_out_node_of_one_bridge_is_hidden() {
        assert_eq!(
            names(vec![
                usb("/dev/cu.usbserial-0001", "0001"),
                usb("/dev/cu.SLAB_USBtoUART", "0001"),
            ]),
            ["/dev/cu.usbserial-0001"]
        );
    }

    #[test]
    fn two_bridges_of_the_same_model_both_stay() {
        assert_eq!(
            names(vec![
                usb("/dev/cu.usbserial-0001", "0001"),
                usb("/dev/cu.SLAB_USBtoUART", "0002"),
            ]),
            ["/dev/cu.usbserial-0001", "/dev/cu.SLAB_USBtoUART"]
        );
    }

    #[test]
    fn a_bluetooth_adapter_with_a_name_of_its_own_is_offered() {
        assert_eq!(
            names(vec![SerialPortInfo {
                port_name: "/dev/cu.GlobalSat-SPPslave".to_owned(),
                port_type: SerialPortType::BluetoothPort,
            }]),
            ["/dev/cu.GlobalSat-SPPslave"]
        );
    }
}
