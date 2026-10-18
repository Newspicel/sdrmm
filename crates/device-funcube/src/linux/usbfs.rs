use std::{
    ffi::c_void,
    os::fd::{AsFd, OwnedFd},
    path::Path,
    time::Duration,
};

use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    fs::{Mode, OFlags, open},
    io::Errno,
    ioctl::{Ioctl, IoctlOutput, Opcode, ioctl, opcode},
};

use crate::{
    error::{Error, Result},
    raw::PACKETS,
};

const GROUP: u8 = b'U';
pub(crate) const URB_TYPE_ISO: u8 = 0;
pub(crate) const URB_ISO_ASAP: u32 = 0x02;
const DISCONNECT_ANY_DRIVER: u32 = 0;

pub(crate) const CONTROL: Opcode = opcode::read_write::<ControlTransfer>(GROUP, 0);
pub(crate) const BULK: Opcode = opcode::read_write::<BulkTransfer>(GROUP, 2);
pub(crate) const SET_INTERFACE: Opcode = opcode::read::<SetInterface>(GROUP, 4);
pub(crate) const SUBMIT_URB: Opcode = opcode::read::<UrbHeader>(GROUP, 10);
pub(crate) const DISCARD_URB: Opcode = opcode::none(GROUP, 11);
pub(crate) const REAP_URB_NDELAY: Opcode = opcode::write::<*mut c_void>(GROUP, 13);
pub(crate) const RELEASE_INTERFACE: Opcode = opcode::read::<u32>(GROUP, 16);
pub(crate) const IOCTL: Opcode = opcode::read_write::<Forward>(GROUP, 18);
pub(crate) const CONNECT: Opcode = opcode::none(GROUP, 23);
pub(crate) const DISCONNECT_CLAIM: Opcode = opcode::read::<DisconnectClaim>(GROUP, 27);

#[repr(C)]
pub(crate) struct ControlTransfer {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
    timeout_ms: u32,
    data: *mut c_void,
}

#[repr(C)]
pub(crate) struct BulkTransfer {
    endpoint: u32,
    length: u32,
    timeout_ms: u32,
    data: *mut c_void,
}

#[repr(C)]
pub(crate) struct SetInterface {
    interface: u32,
    alt_setting: u32,
}

#[repr(C)]
pub(crate) struct Forward {
    interface: i32,
    code: i32,
    data: *mut c_void,
}

#[repr(C)]
pub(crate) struct DisconnectClaim {
    interface: u32,
    flags: u32,
    driver: [u8; 256],
}

#[repr(C)]
pub(crate) struct UrbHeader {
    pub(crate) kind: u8,
    pub(crate) endpoint: u8,
    pub(crate) status: i32,
    pub(crate) flags: u32,
    pub(crate) buffer: *mut c_void,
    pub(crate) buffer_length: i32,
    pub(crate) actual_length: i32,
    pub(crate) start_frame: i32,
    pub(crate) number_of_packets: i32,
    pub(crate) error_count: i32,
    pub(crate) signal: u32,
    pub(crate) context: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IsoPacket {
    pub(crate) length: u32,
    pub(crate) actual_length: u32,
    pub(crate) status: u32,
}

#[repr(C)]
pub(crate) struct IsoUrb {
    pub(crate) header: UrbHeader,
    pub(crate) packets: [IsoPacket; PACKETS],
}

struct Raw {
    opcode: Opcode,
    argument: *mut c_void,
}

unsafe impl Ioctl for Raw {
    type Output = IoctlOutput;

    const IS_MUTATING: bool = true;

    fn opcode(&self) -> Opcode {
        self.opcode
    }

    fn as_ptr(&mut self) -> *mut c_void {
        self.argument
    }

    unsafe fn output_from_ptr(out: IoctlOutput, _: *mut c_void) -> rustix::io::Result<IoctlOutput> {
        Ok(out)
    }
}

#[derive(Debug)]
pub(crate) struct UsbFs {
    fd: OwnedFd,
}

impl UsbFs {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let fd = open(path, OFlags::RDWR | OFlags::CLOEXEC, Mode::empty())
            .map_err(Error::usbfs("open USB device node"))?;
        Ok(Self { fd })
    }

    unsafe fn call<T>(&self, opcode: Opcode, argument: *mut T) -> rustix::io::Result<IoctlOutput> {
        unsafe {
            ioctl(
                self.fd.as_fd(),
                Raw {
                    opcode,
                    argument: argument.cast(),
                },
            )
        }
    }

    pub(crate) fn disconnect_claim(&self, interface: u8) -> Result<()> {
        let mut claim = DisconnectClaim {
            interface: interface.into(),
            flags: DISCONNECT_ANY_DRIVER,
            driver: [0; 256],
        };
        unsafe { self.call(DISCONNECT_CLAIM, &raw mut claim) }
            .map(drop)
            .map_err(Error::usbfs("claim interface"))
    }

    pub(crate) fn release_and_reconnect(&self, interface: u8) {
        let mut number = u32::from(interface);
        if let Err(errno) = unsafe { self.call(RELEASE_INTERFACE, &raw mut number) } {
            tracing::debug!(interface, %errno, "funcube release interface");
        }
        let mut forward = Forward {
            interface: interface.into(),
            code: CONNECT as i32,
            data: std::ptr::null_mut(),
        };
        if let Err(errno) = unsafe { self.call(IOCTL, &raw mut forward) } {
            tracing::debug!(interface, %errno, "funcube kernel driver not reattached");
        }
    }

    pub(crate) fn set_interface(&self, interface: u8, alt_setting: u8) -> Result<()> {
        let mut setting = SetInterface {
            interface: interface.into(),
            alt_setting: alt_setting.into(),
        };
        unsafe { self.call(SET_INTERFACE, &raw mut setting) }
            .map(drop)
            .map_err(Error::usbfs("select alternate setting"))
    }

    pub(crate) fn transfer(
        &self,
        endpoint: u8,
        data: &mut [u8],
        timeout: Duration,
    ) -> Result<usize> {
        let mut transfer = BulkTransfer {
            endpoint: endpoint.into(),
            length: u32::try_from(data.len()).unwrap_or(u32::MAX),
            timeout_ms: u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX),
            data: data.as_mut_ptr().cast(),
        };
        let moved = unsafe { self.call(BULK, &raw mut transfer) }
            .map_err(Error::usbfs("interrupt transfer"))?;
        Ok(usize::try_from(moved).unwrap_or(0))
    }

    pub(crate) fn control_out(
        &self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &mut [u8],
    ) -> Result<()> {
        let mut transfer = ControlTransfer {
            request_type,
            request,
            value,
            index,
            length: u16::try_from(data.len()).unwrap_or(u16::MAX),
            timeout_ms: 500,
            data: data.as_mut_ptr().cast(),
        };
        unsafe { self.call(CONTROL, &raw mut transfer) }
            .map(drop)
            .map_err(Error::usbfs("control transfer"))
    }

    pub(crate) unsafe fn submit(&self, urb: *mut IsoUrb) -> Result<()> {
        unsafe { self.call(SUBMIT_URB, urb) }
            .map(drop)
            .map_err(Error::usbfs("submit isochronous transfer"))
    }

    pub(crate) unsafe fn discard(&self, urb: *mut IsoUrb) {
        if let Err(errno) = unsafe { self.call(DISCARD_URB, urb) } {
            tracing::debug!(%errno, "funcube discard transfer");
        }
    }

    pub(crate) fn reap(&self) -> Result<Option<*mut IsoUrb>> {
        let mut reaped: *mut c_void = std::ptr::null_mut();
        match unsafe { self.call(REAP_URB_NDELAY, &raw mut reaped) } {
            Ok(_) => Ok(Some(reaped.cast())),
            Err(Errno::AGAIN) => Ok(None),
            Err(errno) => Err(Error::usbfs("reap isochronous transfer")(errno)),
        }
    }

    pub(crate) fn wait(&self, timeout: Duration) -> Result<()> {
        let timeout = Timespec {
            tv_sec: i64::try_from(timeout.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: timeout.subsec_nanos().into(),
        };
        let mut fds = [PollFd::new(&self.fd, PollFlags::OUT)];
        poll(&mut fds, Some(&timeout)).map_err(Error::usbfs("wait for transfers"))?;
        if fds[0].revents().intersects(PollFlags::HUP | PollFlags::ERR) {
            return Err(Error::usbfs("wait for transfers")(Errno::NODEV));
        }
        Ok(())
    }
}

#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::*;

    #[test]
    fn requests_match_the_kernel_header() {
        assert_eq!(CONTROL as u64, 0xc018_5500);
        assert_eq!(BULK as u64, 0xc018_5502);
        assert_eq!(SET_INTERFACE as u64, 0x8008_5504);
        assert_eq!(SUBMIT_URB as u64, 0x8038_550a);
        assert_eq!(DISCARD_URB as u64, 0x550b);
        assert_eq!(REAP_URB_NDELAY as u64, 0x4008_550d);
        assert_eq!(RELEASE_INTERFACE as u64, 0x8004_5510);
        assert_eq!(IOCTL as u64, 0xc010_5512);
        assert_eq!(CONNECT as u64, 0x5517);
        assert_eq!(DISCONNECT_CLAIM as u64, 0x8108_551b);
    }

    #[test]
    fn packet_descriptors_follow_the_header_directly() {
        assert_eq!(size_of::<UrbHeader>(), 56);
        assert_eq!(std::mem::offset_of!(IsoUrb, packets), 56);
        assert_eq!(size_of::<IsoPacket>(), 12);
    }
}
