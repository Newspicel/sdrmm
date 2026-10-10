use std::{io, net::UdpSocket};

use sdrmm_iqlink::{HEADER_BYTES, Header};

#[cfg(target_os = "linux")]
const MAX_SEGMENTS: usize = 44;

pub struct Sender {
    socket: UdpSocket,
    #[cfg(target_os = "linux")]
    segment_bytes: usize,
    headers: Vec<[u8; HEADER_BYTES]>,
    #[cfg(target_os = "linux")]
    offload: bool,
    #[cfg(not(target_os = "linux"))]
    scratch: Vec<u8>,
}

impl Sender {
    pub fn new(socket: UdpSocket, payload_bytes: usize) -> Self {
        let segment_bytes = HEADER_BYTES + payload_bytes;
        #[cfg(not(target_os = "linux"))]
        let _ = segment_bytes;
        Self {
            #[cfg(target_os = "linux")]
            offload: gso::enable(&socket, segment_bytes),
            socket,
            #[cfg(target_os = "linux")]
            segment_bytes,
            headers: Vec::with_capacity(64),
            #[cfg(not(target_os = "linux"))]
            scratch: Vec::with_capacity(segment_bytes),
        }
    }

    #[cfg(target_os = "linux")]
    pub const fn offloads(&self) -> bool {
        self.offload
    }

    #[cfg(not(target_os = "linux"))]
    pub const fn offloads(&self) -> bool {
        false
    }

    #[cfg(target_os = "linux")]
    pub fn send<'a>(
        &mut self,
        datagrams: impl Iterator<Item = (Header, &'a [u8])>,
    ) -> io::Result<()> {
        let batch = if self.offload { MAX_SEGMENTS } else { 1 };
        let mut payloads: [&[u8]; MAX_SEGMENTS] = [&[]; MAX_SEGMENTS];
        let mut count = 0;
        self.headers.clear();
        for (header, payload) in datagrams {
            self.headers.push(header.encode());
            payloads[count] = payload;
            count += 1;
            let short = HEADER_BYTES + payload.len() < self.segment_bytes;
            if count == batch || short {
                gso::send(&self.socket, &self.headers, &payloads[..count])?;
                self.headers.clear();
                count = 0;
            }
        }
        if count > 0 {
            gso::send(&self.socket, &self.headers, &payloads[..count])?;
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn send<'a>(
        &mut self,
        datagrams: impl Iterator<Item = (Header, &'a [u8])>,
    ) -> io::Result<()> {
        for (header, payload) in datagrams {
            self.scratch.clear();
            self.scratch.extend_from_slice(&header.encode());
            self.scratch.extend_from_slice(payload);
            self.socket.send(&self.scratch)?;
        }
        self.headers.clear();
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod gso {
    use std::{io, net::UdpSocket, os::fd::AsRawFd};

    use sdrmm_iqlink::HEADER_BYTES;

    use super::MAX_SEGMENTS;

    pub fn enable(socket: &UdpSocket, segment_bytes: usize) -> bool {
        let size = segment_bytes as libc::c_int;
        let set = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_UDP,
                libc::UDP_SEGMENT,
                (&raw const size).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        set == 0
    }

    pub fn send(
        socket: &UdpSocket,
        headers: &[[u8; HEADER_BYTES]],
        payloads: &[&[u8]],
    ) -> io::Result<()> {
        let empty = libc::iovec {
            iov_base: std::ptr::null_mut(),
            iov_len: 0,
        };
        let mut iov = [empty; MAX_SEGMENTS * 2];
        let mut total = 0;
        for (slot, (header, payload)) in headers.iter().zip(payloads).enumerate() {
            iov[slot * 2] = libc::iovec {
                iov_base: header.as_ptr().cast_mut().cast(),
                iov_len: header.len(),
            };
            iov[slot * 2 + 1] = libc::iovec {
                iov_base: payload.as_ptr().cast_mut().cast(),
                iov_len: payload.len(),
            };
            total += header.len() + payload.len();
        }
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = iov.as_mut_ptr();
        message.msg_iovlen = (payloads.len() * 2) as _;
        let sent = unsafe { libc::sendmsg(socket.as_raw_fd(), &raw const message, 0) };
        if sent < 0 {
            return Err(io::Error::last_os_error());
        }
        if sent as usize != total {
            return Err(io::Error::other(format!(
                "sent {sent} of {total} bytes in one datagram batch"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sdrmm_iqlink::Header;

    use super::*;

    #[test]
    fn every_datagram_arrives_with_its_own_header() {
        let receiver = UdpSocket::bind("127.0.0.1:0").expect("bind");
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        let socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
        socket
            .connect(receiver.local_addr().expect("addr"))
            .expect("connect");
        let mut sender = Sender::new(socket, 8);
        let payloads: [&[u8]; 3] = [&[1; 8], &[2; 8], &[3; 4]];
        let datagrams = payloads.iter().enumerate().map(|(i, payload)| {
            let header = Header {
                frame_bytes: 4,
                index: i as u64 * 2,
                board_lost: 0,
            };
            (header, *payload)
        });
        sender.send(datagrams).expect("send");
        let mut buf = [0u8; 64];
        for (i, payload) in payloads.iter().enumerate() {
            let n = receiver.recv(&mut buf).expect("datagram");
            let header = Header::decode(&buf[..n]).expect("header");
            assert_eq!(header.index, i as u64 * 2);
            assert_eq!(&buf[HEADER_BYTES..n], *payload);
        }
    }
}
