use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use rustix::io::Errno;

use crate::{
    discovery,
    error::{Error, Result},
    protocol::{self, REPORT_LEN, Report},
    radio::Link,
    raw::{
        Assembler, PACKET_BYTES, PACKETS, Packet, Pump, PumpState, RawStream, SAMPLES_PER_PACKET,
        TRANSFERS,
    },
};

mod usbfs;

use usbfs::{IsoPacket, IsoUrb, URB_ISO_ASAP, URB_TYPE_ISO, UrbHeader, UsbFs};

const INTERFACES: [u8; 3] = [0, 1, 2];
const STREAM_INTERFACE: u8 = 1;
const STREAM_ALT: u8 = 1;
const SAMPLES_IN: u8 = 0x81;
const HID_OUT: u8 = 0x02;
const HID_IN: u8 = 0x82;
const HID_TIMEOUT: Duration = Duration::from_millis(500);
const POLL: Duration = Duration::from_millis(100);
const DRAIN: Duration = Duration::from_secs(1);
const SET_CUR: u8 = 0x01;
const CLASS_ENDPOINT_OUT: u8 = 0x22;
const SAMPLING_FREQ_CONTROL: u16 = 0x0100;

pub(crate) use crate::raw::{BLOCK_SAMPLES, FORMAT, RawStream as Stream};

#[derive(Debug)]
struct Claimed(UsbFs);

impl Drop for Claimed {
    fn drop(&mut self) {
        for interface in INTERFACES {
            self.0.release_and_reconnect(interface);
        }
    }
}

fn node(info: &nusb::DeviceInfo) -> PathBuf {
    PathBuf::from(format!(
        "/dev/bus/usb/{:03}/{:03}",
        info.busnum(),
        info.device_address()
    ))
}

pub(crate) fn open(key: &str) -> Result<(Control, Port)> {
    let usb = Claimed(UsbFs::open(&node(&discovery::find(key)?))?);
    for interface in INTERFACES {
        usb.0.disconnect_claim(interface)?;
    }
    let usb = Arc::new(usb);
    Ok((Control { usb: usb.clone() }, Port { usb }))
}

pub(crate) struct Control {
    usb: Arc<Claimed>,
}

impl Link for Control {
    fn exchange(&mut self, request: &Report) -> Result<Report> {
        let mut out = *request;
        self.usb.0.transfer(HID_OUT, &mut out, HID_TIMEOUT)?;
        let deadline = Instant::now() + HID_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let timeout = Error::Timeout {
                operation: protocol::name_of(request[0]),
                timeout: HID_TIMEOUT,
            };
            if left.is_zero() {
                return Err(timeout);
            }
            let mut reply = [0u8; REPORT_LEN];
            match self.usb.0.transfer(HID_IN, &mut reply, left) {
                Ok(_) if reply[0] == request[0] => return Ok(reply),
                Ok(_) => {}
                Err(Error::Usbfs {
                    errno: Errno::TIMEDOUT,
                    ..
                }) => return Err(timeout),
                Err(error) => return Err(error),
            }
        }
    }
}

pub(crate) struct Port {
    usb: Arc<Claimed>,
}

impl Port {
    pub(crate) fn start(&self) -> Result<Stream> {
        self.usb.0.set_interface(STREAM_INTERFACE, STREAM_ALT)?;
        let mut rate = protocol::SAMPLE_RATE_HZ.to_le_bytes()[..3].to_vec();
        if let Err(error) = self.usb.0.control_out(
            CLASS_ENDPOINT_OUT,
            SET_CUR,
            SAMPLING_FREQ_CONTROL,
            SAMPLES_IN.into(),
            &mut rate,
        ) {
            tracing::debug!("funcube sample rate request: {error}");
        }
        let mut pump = IsoPump::new(self.usb.clone());
        for index in 0..TRANSFERS {
            pump.submit(index)?;
        }
        RawStream::spawn(pump)
    }
}

struct IsoPump {
    usb: Arc<Claimed>,
    urbs: Box<[IsoUrb]>,
    buffers: Vec<Box<[u8]>>,
    in_flight: [bool; TRANSFERS],
}

unsafe impl Send for IsoPump {}

fn empty_urb() -> IsoUrb {
    IsoUrb {
        header: UrbHeader {
            kind: URB_TYPE_ISO,
            endpoint: SAMPLES_IN,
            status: 0,
            flags: URB_ISO_ASAP,
            buffer: std::ptr::null_mut(),
            buffer_length: 0,
            actual_length: 0,
            start_frame: 0,
            number_of_packets: 0,
            error_count: 0,
            signal: 0,
            context: std::ptr::null_mut(),
        },
        packets: [IsoPacket::default(); PACKETS],
    }
}

impl IsoPump {
    fn new(usb: Arc<Claimed>) -> Self {
        Self {
            usb,
            urbs: (0..TRANSFERS).map(|_| empty_urb()).collect(),
            buffers: (0..TRANSFERS)
                .map(|_| vec![0u8; PACKETS * PACKET_BYTES].into_boxed_slice())
                .collect(),
            in_flight: [false; TRANSFERS],
        }
    }

    fn submit(&mut self, index: usize) -> Result<()> {
        let buffer = self.buffers[index].as_mut_ptr();
        let urb = &mut self.urbs[index];
        urb.header.buffer = buffer.cast();
        urb.header.buffer_length = (PACKETS * PACKET_BYTES) as i32;
        urb.header.number_of_packets = PACKETS as i32;
        urb.header.status = 0;
        urb.header.actual_length = 0;
        urb.packets.fill(IsoPacket {
            length: PACKET_BYTES as u32,
            actual_length: 0,
            status: 0,
        });
        unsafe { self.usb.0.submit(&raw mut *urb) }?;
        self.in_flight[index] = true;
        Ok(())
    }

    fn reaped(&mut self, urb: *mut IsoUrb) -> Result<usize> {
        let index = self
            .urbs
            .iter_mut()
            .position(|owned| std::ptr::eq(&raw mut *owned, urb))
            .ok_or(Error::protocol(
                "reap transfer",
                "the kernel returned a transfer it was never given",
            ))?;
        self.in_flight[index] = false;
        Ok(index)
    }

    fn complete(&mut self, index: usize, assembler: &mut Assembler) -> Result<()> {
        let urb = &self.urbs[index];
        let status = -urb.header.status;
        if status == Errno::NODEV.raw_os_error() || status == Errno::SHUTDOWN.raw_os_error() {
            return Err(Error::usbfs("isochronous transfer")(Errno::NODEV));
        }
        let packets = urb.packets.iter().enumerate().map(|(slot, packet)| Packet {
            offset: slot * PACKET_BYTES,
            length: packet.actual_length as usize,
            ok: packet.status == 0,
        });
        assembler.transfer(&self.buffers[index], packets);
        Ok(())
    }

    fn drain(&mut self) {
        for index in 0..TRANSFERS {
            if self.in_flight[index] {
                unsafe { self.usb.0.discard(&raw mut self.urbs[index]) };
            }
        }
        let deadline = Instant::now() + DRAIN;
        while self.in_flight.contains(&true) && Instant::now() < deadline {
            if self.usb.0.wait(POLL).is_err() {
                break;
            }
            while let Ok(Some(urb)) = self.usb.0.reap() {
                if self.reaped(urb).is_err() {
                    break;
                }
            }
        }
    }
}

impl Pump for IsoPump {
    fn run(mut self, assembler: &mut Assembler, state: &PumpState) -> Result<()> {
        while !state.stopping() && !assembler.is_closed() {
            self.usb.0.wait(POLL)?;
            while let Some(urb) = self.usb.0.reap()? {
                let index = self.reaped(urb)?;
                self.complete(index, assembler)?;
                if !self.in_flight.contains(&true) {
                    tracing::warn!("funcube transfers ran dry; samples were lost");
                    assembler.lost(PACKETS as u64 * SAMPLES_PER_PACKET);
                }
                if !state.stopping() {
                    self.submit(index)?;
                }
            }
        }
        Ok(())
    }
}

impl Drop for IsoPump {
    fn drop(&mut self) {
        self.drain();
        if let Err(error) = self.usb.0.set_interface(STREAM_INTERFACE, 0) {
            tracing::debug!("funcube stream interface left streaming: {error}");
        }
    }
}
