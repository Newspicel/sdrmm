use std::{
    io,
    net::{IpAddr, SocketAddr, UdpSocket},
    time::Duration,
};

use socket2::{Domain, Protocol, Socket, Type};

use crate::DeviceError;

pub const RECV_SLICE: Duration = Duration::from_millis(20);
const RECEIVE_BUFFERS: [usize; 4] = [8 << 20, 4 << 20, 2 << 20, 1 << 20];

fn io_error(e: io::Error) -> DeviceError {
    DeviceError::Io(e.to_string())
}

pub fn udp_receiver(local: IpAddr) -> Result<UdpSocket, DeviceError> {
    let domain = if local.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP)).map_err(io_error)?;
    if !RECEIVE_BUFFERS
        .iter()
        .any(|size| socket.set_recv_buffer_size(*size).is_ok())
    {
        tracing::warn!("UDP receiver keeps the system's small receive buffer; expect drops");
    }
    socket
        .bind(&SocketAddr::new(local, 0).into())
        .map_err(io_error)?;
    let socket: UdpSocket = socket.into();
    socket
        .set_read_timeout(Some(RECV_SLICE))
        .map_err(io_error)?;
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    #[test]
    fn a_receiver_binds_an_ephemeral_port_and_waits_one_slice() {
        let socket = udp_receiver(IpAddr::V4(Ipv4Addr::LOCALHOST)).expect("binds");
        assert_ne!(socket.local_addr().expect("addr").port(), 0);
        assert_eq!(socket.read_timeout().expect("timeout"), Some(RECV_SLICE));
    }
}
