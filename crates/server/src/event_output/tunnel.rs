use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};

use sdrmm_wire::{BroadcastData, DecodedRecord, EventOutputTarget};
use tokio::{sync::mpsc, task::JoinHandle};

use super::{Binding, status::Statuses};

struct Entry {
    target: EventOutputTarget,
    scope: Scope,
    sender: mpsc::Sender<Vec<u8>>,
    worker: JoinHandle<()>,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

pub(super) struct Outputs {
    entries: HashMap<String, Entry>,
    statuses: Statuses,
}

impl Outputs {
    pub fn new(statuses: Statuses) -> Self {
        Self {
            entries: HashMap::new(),
            statuses,
        }
    }

    pub fn configure(&mut self, bindings: &[Binding]) {
        self.entries.retain(|node, entry| {
            bindings
                .iter()
                .any(|binding| binding.node == *node && binding.target == entry.target)
        });
        for binding in bindings {
            let EventOutputTarget::Tunnel {
                interface,
                address,
                prefix,
            } = &binding.target
            else {
                continue;
            };
            if self.entries.contains_key(&binding.node) {
                continue;
            }
            let (sender, mut packets) = mpsc::channel::<Vec<u8>>(256);
            let node = binding.node.clone();
            let statuses = self.statuses.clone();
            let builder = tun_rs::DeviceBuilder::new()
                .name(interface)
                .ipv4(*address, *prefix, None)
                .mtu(65535)
                .enable(true);
            let worker = tokio::spawn(async move {
                let device = match tokio::task::spawn_blocking(move || builder.build_async()).await
                {
                    Ok(Ok(device)) => device,
                    Ok(Err(error)) => {
                        return fail(
                            &statuses,
                            &node,
                            format!("Cannot create interface: {error}"),
                        );
                    }
                    Err(error) => {
                        return fail(&statuses, &node, format!("Interface setup failed: {error}"));
                    }
                };
                while let Some(bytes) = packets.recv().await {
                    match tokio::time::timeout(Duration::from_secs(2), device.send(&bytes)).await {
                        Ok(Ok(written)) if written == bytes.len() => statuses.delivered(&node, 1),
                        Ok(Ok(written)) => fail(
                            &statuses,
                            &node,
                            format!("Packet truncated: {written} of {} bytes", bytes.len()),
                        ),
                        Ok(Err(error)) => fail(&statuses, &node, format!("Write failed: {error}")),
                        Err(_) => fail(&statuses, &node, "Write timed out".to_owned()),
                    }
                }
            });
            self.entries.insert(
                binding.node.clone(),
                Entry {
                    target: binding.target.clone(),
                    scope: Scope::new(*address, *prefix),
                    sender,
                    worker,
                },
            );
        }
    }

    pub fn push(&self, bindings: &[Binding], record: &DecodedRecord) {
        let Some(data) = record.event.read::<BroadcastData>() else {
            return;
        };
        let data = &data;
        for binding in bindings {
            let Some(entry) = self.entries.get(&binding.node) else {
                continue;
            };
            if !record.sinks.contains(&binding.node) {
                continue;
            }
            match ip_packet(data, entry.scope) {
                Ok(bytes) => {
                    if let Err(error) = entry.sender.try_send(bytes.to_vec()) {
                        fail(
                            &self.statuses,
                            &binding.node,
                            format!("Queue unavailable: {error}"),
                        );
                    }
                }
                Err(error) => fail(&self.statuses, &binding.node, error.to_owned()),
            }
        }
    }
}

fn fail(statuses: &Statuses, node: &str, error: String) {
    tracing::error!(output = %node, %error, "broadcast TUN output failed");
    statuses.failed(node, 1, error);
}

/// Where a packet off the air may go. Anyone can transmit one, so it reaches multicast listeners
/// or the tunnel's own subnet, never another host interface, and never claims the loopback.
#[derive(Clone, Copy, Debug)]
struct Scope {
    network: u32,
    mask: u32,
}

impl Scope {
    fn new(address: Ipv4Addr, prefix: u8) -> Self {
        let mask = u32::MAX
            .checked_shl(32 - u32::from(prefix.min(32)))
            .unwrap_or(0);
        Self {
            network: address.to_bits() & mask,
            mask,
        }
    }

    fn admits_v4(self, source: Ipv4Addr, destination: Ipv4Addr) -> Result<(), &'static str> {
        if source.is_loopback() || destination.is_loopback() {
            return Err("Loopback address refused");
        }
        if destination.is_multicast() || destination.to_bits() & self.mask == self.network {
            return Ok(());
        }
        Err("Destination outside the tunnel subnet")
    }

    fn admits_v6(source: Ipv6Addr, destination: Ipv6Addr) -> Result<(), &'static str> {
        if source.is_loopback() || destination.is_loopback() {
            return Err("Loopback address refused");
        }
        if destination.is_multicast() {
            return Ok(());
        }
        Err("IPv6 reaches only multicast groups")
    }
}

fn v4_at(bytes: &[u8], at: usize) -> Ipv4Addr {
    let mut octets = [0; 4];
    octets.copy_from_slice(&bytes[at..at + 4]);
    Ipv4Addr::from(octets)
}

fn v6_at(bytes: &[u8], at: usize) -> Ipv6Addr {
    let mut octets = [0; 16];
    octets.copy_from_slice(&bytes[at..at + 16]);
    Ipv6Addr::from(octets)
}

fn ip_packet(data: &BroadcastData, scope: Scope) -> Result<&[u8], &'static str> {
    let bytes = data.bytes.as_slice();
    match data.protocol {
        Some(0x0800) if bytes.len() >= 20 && bytes[0] >> 4 == 4 => {
            let header = usize::from(bytes[0] & 15) * 4;
            if header < 20
                || header > bytes.len()
                || usize::from(u16::from_be_bytes([bytes[2], bytes[3]])) != bytes.len()
            {
                return Err("Invalid IPv4 packet length");
            }
            let mut checksum = bytes[..header]
                .as_chunks::<2>()
                .0
                .iter()
                .fold(0u32, |sum, b| sum + u32::from(u16::from_be_bytes(*b)));
            while checksum >> 16 != 0 {
                checksum = (checksum & 65535) + (checksum >> 16);
            }
            if checksum != 65535 {
                return Err("Invalid IPv4 header checksum");
            }
            scope.admits_v4(v4_at(bytes, 12), v4_at(bytes, 16))?;
        }
        Some(0x86dd) if bytes.len() >= 40 && bytes[0] >> 4 == 6 => {
            if 40 + usize::from(u16::from_be_bytes([bytes[4], bytes[5]])) != bytes.len() {
                return Err("Invalid IPv6 packet length");
            }
            Scope::admits_v6(v6_at(bytes, 8), v6_at(bytes, 24))?;
        }
        _ => return Err("TUN accepts IPv4 and IPv6 datagrams"),
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::DecoderEvent;

    use super::*;
    fn data(protocol: u16, bytes: Vec<u8>) -> BroadcastData {
        BroadcastData {
            protocol: Some(protocol),
            label: Vec::new(),
            service_id: None,
            name: String::new(),
            media_type: String::new(),
            bytes,
        }
    }
    #[tokio::test]
    async fn routed_datagrams_reach_a_bounded_writer_without_opening_a_device() {
        let target = EventOutputTarget::Tunnel {
            interface: "test0".to_owned(),
            address: std::net::Ipv4Addr::new(10, 23, 0, 1),
            prefix: 24,
        };
        let binding = Binding {
            node: "out".to_owned(),
            target: target.clone(),
        };
        let (sender, mut receiver) = mpsc::channel(1);
        let statuses = Statuses::default();
        statuses.configure(std::slice::from_ref(&binding));
        let mut outputs = Outputs::new(statuses.clone());
        outputs.entries.insert(
            "out".to_owned(),
            Entry {
                scope: Scope::new(Ipv4Addr::new(10, 23, 0, 1), 24),
                target,
                sender,
                worker: tokio::spawn(std::future::pending()),
            },
        );
        let mut bytes = vec![0; 44];
        bytes[0] = 0x60;
        bytes[5] = 4;
        bytes[24] = 0xff;
        let elsewhere = DecodedRecord {
            origin: None,
            sinks: vec!["other".to_owned()],
            callsign: None,
            device_set: 1,
            channel: 2,
            at: "2026-09-16T00:00:00Z".to_owned(),
            freq_hz: 1e9,
            event: DecoderEvent::of(&data(0x86dd, bytes.clone())),
            view: None,
        };
        outputs.push(std::slice::from_ref(&binding), &elsewhere);
        assert!(receiver.try_recv().is_err());
        let reached = DecodedRecord {
            origin: None,
            sinks: vec!["out".to_owned()],
            callsign: None,
            ..elsewhere
        };
        outputs.push(std::slice::from_ref(&binding), &reached);
        outputs.push(std::slice::from_ref(&binding), &reached);
        assert_eq!(receiver.recv().await.unwrap(), bytes);
        let status = statuses.get("out").expect("tunnel status");
        assert_eq!(status.failed, 1);
        assert!(
            status
                .error
                .expect("queue error")
                .starts_with("Queue unavailable")
        );
        outputs.configure(&[]);
        assert!(outputs.entries.is_empty());
        assert!(receiver.recv().await.is_none());
    }

    fn scope() -> Scope {
        Scope::new(Ipv4Addr::new(192, 0, 2, 1), 24)
    }

    fn ipv4(source: [u8; 4], destination: [u8; 4]) -> Vec<u8> {
        let mut packet = vec![0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0];
        packet.extend(source);
        packet.extend(destination);
        let mut sum = packet
            .as_chunks::<2>()
            .0
            .iter()
            .fold(0u32, |sum, b| sum + u32::from(u16::from_be_bytes(*b)));
        while sum >> 16 != 0 {
            sum = (sum & 65535) + (sum >> 16);
        }
        packet[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes());
        packet
    }

    fn ipv6(destination: Ipv6Addr) -> Vec<u8> {
        let mut packet = vec![0u8; 44];
        packet[0] = 0x60;
        packet[5] = 4;
        packet[8..24].copy_from_slice(&Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1).octets());
        packet[24..40].copy_from_slice(&destination.octets());
        packet
    }

    #[test]
    fn only_complete_ip_packets_reach_the_interface() {
        let mut ipv4 = ipv4([192, 0, 2, 1], [192, 0, 2, 2]);
        assert_eq!(
            ip_packet(&data(0x800, ipv4.clone()), scope()).unwrap(),
            ipv4
        );
        ipv4[12] ^= 1;
        assert!(ip_packet(&data(0x800, ipv4), scope()).is_err());
        let mut ipv6 = ipv6(Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1));
        assert_eq!(
            ip_packet(&data(0x86dd, ipv6.clone()), scope()).unwrap(),
            ipv6
        );
        ipv6.pop();
        assert!(ip_packet(&data(0x86dd, ipv6), scope()).is_err());
        assert!(ip_packet(&data(0x806, vec![0; 28]), scope()).is_err());
    }

    #[test]
    fn packets_off_the_air_reach_only_multicast_or_the_tunnel_subnet() {
        let admitted =
            |packet: Vec<u8>, protocol| ip_packet(&data(protocol, packet), scope()).is_ok();
        assert!(admitted(ipv4([203, 0, 113, 9], [192, 0, 2, 255]), 0x800));
        assert!(admitted(ipv4([203, 0, 113, 9], [239, 1, 2, 3]), 0x800));
        assert!(!admitted(ipv4([203, 0, 113, 9], [192, 168, 1, 20]), 0x800));
        assert!(!admitted(ipv4([203, 0, 113, 9], [127, 0, 0, 1]), 0x800));
        assert!(!admitted(ipv4([127, 0, 0, 1], [192, 0, 2, 2]), 0x800));
        assert!(!admitted(ipv6(Ipv6Addr::LOCALHOST), 0x86dd));
        assert!(!admitted(
            ipv6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
            0x86dd
        ));
    }

    #[test]
    fn a_scope_masks_its_prefix() {
        let open = Scope::new(Ipv4Addr::new(10, 0, 0, 1), 0);
        assert!(
            open.admits_v4(Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8))
                .is_ok()
        );
        let host = Scope::new(Ipv4Addr::new(10, 0, 0, 1), 32);
        assert!(
            host.admits_v4(Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(10, 0, 0, 1))
                .is_ok()
        );
        assert!(
            host.admits_v4(Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(10, 0, 0, 2))
                .is_err()
        );
    }
}
