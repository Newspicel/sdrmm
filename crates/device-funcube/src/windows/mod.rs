use std::{
    ffi::{OsStr, c_void},
    os::windows::ffi::OsStrExt,
    sync::Arc,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Devices::{
        DeviceAndDriverInstallation::{
            CM_DRP_SERVICE, CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            CM_Get_DevNode_Registry_PropertyW, CM_Get_Device_Interface_List_SizeW,
            CM_Get_Device_Interface_ListW, CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW,
            CR_SUCCESS,
        },
        Usb::{
            GUID_DEVINTERFACE_USB_DEVICE, PIPE_TRANSFER_TIMEOUT, USB_INTERFACE_DESCRIPTOR,
            USBD_ISO_PACKET_DESCRIPTOR, WINUSB_INTERFACE_HANDLE, WINUSB_PIPE_INFORMATION_EX,
            WINUSB_SETUP_PACKET, WinUsb_AbortPipe, WinUsb_ControlTransfer, WinUsb_Free,
            WinUsb_GetAssociatedInterface, WinUsb_GetOverlappedResult, WinUsb_Initialize,
            WinUsb_QueryInterfaceSettings, WinUsb_QueryPipeEx, WinUsb_ReadIsochPipeAsap,
            WinUsb_ReadPipe, WinUsb_RegisterIsochBuffer, WinUsb_SetCurrentAlternateSetting,
            WinUsb_SetPipePolicy, WinUsb_UnregisterIsochBuffer, WinUsb_WritePipe,
        },
    },
    Foundation::{
        CloseHandle, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, ERROR_SEM_TIMEOUT, GENERIC_READ,
        GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    },
    System::{
        IO::OVERLAPPED,
        Threading::{CreateEventW, ResetEvent, WaitForSingleObject},
    },
};

use crate::{
    discovery,
    error::{Error, Result},
    protocol::{self, REPORT_LEN, Report, SAMPLE_RATE_HZ},
    radio::Link,
    raw::{Assembler, PACKETS, Packet, Pump, PumpState, RawStream, SAMPLES_PER_PACKET, TRANSFERS},
};

pub(crate) use crate::raw::{BLOCK_SAMPLES, FORMAT, RawStream as Stream};

const STREAM_INTERFACE: u8 = 1;
const HID_INTERFACE: u8 = 2;
const STREAM_ALT: u8 = 1;
const SAMPLES_IN: u8 = 0x81;
const HID_OUT: u8 = 0x02;
const HID_IN: u8 = 0x82;
const HID_TIMEOUT: Duration = Duration::from_millis(500);
const WAIT_MS: u32 = 100;
const WINUSB: &str = "WinUSB";
const SET_CUR: u8 = 0x01;
const CLASS_ENDPOINT_OUT: u8 = 0x22;
const SAMPLING_FREQ_CONTROL: u16 = 0x0100;

fn packet_offset(reported: usize, base: usize, transfer: usize) -> usize {
    if reported >= transfer {
        reported.saturating_sub(base)
    } else {
        reported
    }
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

fn check(ok: i32, operation: &'static str) -> Result<()> {
    if ok != 0 {
        return Ok(());
    }
    Err(Error::win32(operation, last_error()))
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

fn until_nul(text: &[u16]) -> &[u16] {
    let end = text
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(text.len());
    &text[..end]
}

fn service(instance: &[u16]) -> Result<String> {
    let mut node = 0;
    if unsafe { CM_Locate_DevNodeW(&mut node, instance.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) }
        != CR_SUCCESS
    {
        return Err(Error::DeviceNotFound);
    }
    let mut buffer = [0u16; 256];
    let mut length = size_of_val(&buffer) as u32;
    let mut kind = 0;
    let found = unsafe {
        CM_Get_DevNode_Registry_PropertyW(
            node,
            CM_DRP_SERVICE,
            &mut kind,
            buffer.as_mut_ptr().cast(),
            &mut length,
            0,
        )
    };
    if found != CR_SUCCESS {
        return Ok(String::new());
    }
    Ok(String::from_utf16_lossy(until_nul(&buffer)))
}

fn device_path(instance: &[u16]) -> Result<Vec<u16>> {
    let mut size = 0;
    let sized = unsafe {
        CM_Get_Device_Interface_List_SizeW(
            &mut size,
            &GUID_DEVINTERFACE_USB_DEVICE,
            instance.as_ptr(),
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    if sized != CR_SUCCESS || size == 0 {
        return Err(Error::NeedsWinUsb);
    }
    let mut list = vec![0u16; size as usize];
    let listed = unsafe {
        CM_Get_Device_Interface_ListW(
            &GUID_DEVINTERFACE_USB_DEVICE,
            instance.as_ptr(),
            list.as_mut_ptr(),
            size,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    let path = until_nul(&list);
    if listed != CR_SUCCESS || path.is_empty() {
        return Err(Error::NeedsWinUsb);
    }
    Ok(path.iter().copied().chain(std::iter::once(0)).collect())
}

#[derive(Debug)]
struct Handles {
    file: HANDLE,
    first: WINUSB_INTERFACE_HANDLE,
    stream: WINUSB_INTERFACE_HANDLE,
    hid: WINUSB_INTERFACE_HANDLE,
}

unsafe impl Send for Handles {}
unsafe impl Sync for Handles {}

impl Drop for Handles {
    fn drop(&mut self) {
        for handle in [self.hid, self.stream, self.first] {
            if !handle.is_null() {
                unsafe { WinUsb_Free(handle) };
            }
        }
        if self.file != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.file) };
        }
    }
}

fn associated(first: WINUSB_INTERFACE_HANDLE, interface: u8) -> Result<WINUSB_INTERFACE_HANDLE> {
    let mut handle = std::ptr::null_mut();
    check(
        unsafe { WinUsb_GetAssociatedInterface(first, interface - 1, &mut handle) },
        "open associated interface",
    )?;
    let mut descriptor: USB_INTERFACE_DESCRIPTOR = unsafe { std::mem::zeroed() };
    let queried = unsafe { WinUsb_QueryInterfaceSettings(handle, 0, &mut descriptor) };
    if queried == 0 || descriptor.bInterfaceNumber != interface {
        unsafe { WinUsb_Free(handle) };
        return Err(Error::NeedsWinUsb);
    }
    Ok(handle)
}

fn open_handles(instance: &[u16]) -> Result<Handles> {
    if !service(instance)?.eq_ignore_ascii_case(WINUSB) {
        return Err(Error::NeedsWinUsb);
    }
    let path = device_path(instance)?;
    let file = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
            std::ptr::null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        return Err(Error::win32("open the dongle", last_error()));
    }
    let mut handles = Handles {
        file,
        first: std::ptr::null_mut(),
        stream: std::ptr::null_mut(),
        hid: std::ptr::null_mut(),
    };
    check(
        unsafe { WinUsb_Initialize(file, &mut handles.first) },
        "start WinUSB",
    )?;
    handles.stream = associated(handles.first, STREAM_INTERFACE)?;
    handles.hid = associated(handles.first, HID_INTERFACE)?;
    let timeout = u32::try_from(HID_TIMEOUT.as_millis()).unwrap_or(u32::MAX);
    for pipe in [HID_OUT, HID_IN] {
        check(
            unsafe {
                WinUsb_SetPipePolicy(
                    handles.hid,
                    pipe,
                    PIPE_TRANSFER_TIMEOUT,
                    size_of::<u32>() as u32,
                    (&raw const timeout).cast(),
                )
            },
            "set HID timeout",
        )?;
    }
    Ok(handles)
}

pub(crate) fn open(key: &str) -> Result<(Control, Port)> {
    let info = discovery::find(key)?;
    let handles = Arc::new(open_handles(&wide(info.instance_id()))?);
    Ok((
        Control {
            handles: handles.clone(),
        },
        Port { handles },
    ))
}

pub(crate) struct Control {
    handles: Arc<Handles>,
}

impl Link for Control {
    fn exchange(&mut self, request: &Report) -> Result<Report> {
        let mut written = 0;
        check(
            unsafe {
                WinUsb_WritePipe(
                    self.handles.hid,
                    HID_OUT,
                    request.as_ptr(),
                    REPORT_LEN as u32,
                    &mut written,
                    std::ptr::null(),
                )
            },
            "send HID report",
        )?;
        let deadline = Instant::now() + HID_TIMEOUT;
        while Instant::now() < deadline {
            let mut reply = [0u8; REPORT_LEN];
            let mut read = 0;
            let ok = unsafe {
                WinUsb_ReadPipe(
                    self.handles.hid,
                    HID_IN,
                    reply.as_mut_ptr(),
                    REPORT_LEN as u32,
                    &mut read,
                    std::ptr::null(),
                )
            };
            if ok == 0 {
                let code = last_error();
                if code == ERROR_SEM_TIMEOUT {
                    break;
                }
                return Err(Error::win32("read HID report", code));
            }
            if reply[0] == request[0] {
                return Ok(reply);
            }
        }
        Err(Error::Timeout {
            operation: protocol::name_of(request[0]),
            timeout: HID_TIMEOUT,
        })
    }
}

pub(crate) struct Port {
    handles: Arc<Handles>,
}

impl Port {
    pub(crate) fn start(&self) -> Result<Stream> {
        let stream = self.handles.stream;
        check(
            unsafe { WinUsb_SetCurrentAlternateSetting(stream, STREAM_ALT) },
            "select alternate setting",
        )?;
        self.request_rate();
        let mut pipe: WINUSB_PIPE_INFORMATION_EX = unsafe { std::mem::zeroed() };
        check(
            unsafe { WinUsb_QueryPipeEx(stream, STREAM_ALT, 0, &mut pipe) },
            "query sample pipe",
        )?;
        if pipe.PipeId != SAMPLES_IN || pipe.MaximumBytesPerInterval == 0 {
            return Err(Error::protocol(
                "query sample pipe",
                "the streaming interface has no isochronous input",
            ));
        }
        let mut pump = IsoPump::new(self.handles.clone(), pipe.MaximumBytesPerInterval as usize)?;
        for index in 0..TRANSFERS {
            pump.submit(index, index > 0)?;
        }
        RawStream::spawn(pump)
    }

    fn request_rate(&self) {
        let mut rate = SAMPLE_RATE_HZ.to_le_bytes();
        let setup = WINUSB_SETUP_PACKET {
            RequestType: CLASS_ENDPOINT_OUT,
            Request: SET_CUR,
            Value: SAMPLING_FREQ_CONTROL,
            Index: SAMPLES_IN.into(),
            Length: 3,
        };
        let mut moved = 0;
        let sent = unsafe {
            WinUsb_ControlTransfer(
                self.handles.first,
                setup,
                rate.as_mut_ptr(),
                3,
                &mut moved,
                std::ptr::null(),
            )
        };
        if sent == 0 {
            tracing::debug!(code = last_error(), "funcube sample rate request");
        }
    }
}

struct IsoPump {
    handles: Arc<Handles>,
    stride: usize,
    buffer: Box<[u8]>,
    registered: *mut c_void,
    descriptors: Box<[[USBD_ISO_PACKET_DESCRIPTOR; PACKETS]]>,
    overlapped: Box<[OVERLAPPED]>,
    events: Vec<HANDLE>,
    in_flight: [bool; TRANSFERS],
}

unsafe impl Send for IsoPump {}

fn empty_descriptor() -> USBD_ISO_PACKET_DESCRIPTOR {
    USBD_ISO_PACKET_DESCRIPTOR {
        Offset: 0,
        Length: 0,
        Status: 0,
    }
}

impl IsoPump {
    fn new(handles: Arc<Handles>, stride: usize) -> Result<Self> {
        let mut pump = Self {
            handles,
            stride,
            buffer: vec![0u8; TRANSFERS * PACKETS * stride].into_boxed_slice(),
            registered: std::ptr::null_mut(),
            descriptors: vec![[empty_descriptor(); PACKETS]; TRANSFERS].into_boxed_slice(),
            overlapped: (0..TRANSFERS)
                .map(|_| unsafe { std::mem::zeroed() })
                .collect(),
            events: Vec::with_capacity(TRANSFERS),
            in_flight: [false; TRANSFERS],
        };
        for _ in 0..TRANSFERS {
            let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            if event.is_null() {
                return Err(Error::win32("create transfer event", last_error()));
            }
            pump.events.push(event);
        }
        check(
            unsafe {
                WinUsb_RegisterIsochBuffer(
                    pump.handles.stream,
                    SAMPLES_IN,
                    pump.buffer.as_mut_ptr(),
                    u32::try_from(pump.buffer.len()).unwrap_or(u32::MAX),
                    &mut pump.registered,
                )
            },
            "register sample buffer",
        )?;
        Ok(pump)
    }

    fn transfer_bytes(&self) -> usize {
        PACKETS * self.stride
    }

    fn submit(&mut self, index: usize, continuing: bool) -> Result<()> {
        unsafe { ResetEvent(self.events[index]) };
        self.overlapped[index] = unsafe { std::mem::zeroed() };
        self.overlapped[index].hEvent = self.events[index];
        let ok = unsafe {
            WinUsb_ReadIsochPipeAsap(
                self.registered,
                u32::try_from(index * self.transfer_bytes()).unwrap_or(u32::MAX),
                u32::try_from(self.transfer_bytes()).unwrap_or(u32::MAX),
                i32::from(continuing),
                PACKETS as u32,
                self.descriptors[index].as_mut_ptr(),
                &self.overlapped[index],
            )
        };
        if ok == 0 && last_error() != ERROR_IO_PENDING {
            return Err(Error::win32("submit isochronous transfer", last_error()));
        }
        self.in_flight[index] = true;
        Ok(())
    }

    fn finish(&mut self, index: usize, wait: bool) -> Result<()> {
        let mut moved = 0;
        let ok = unsafe {
            WinUsb_GetOverlappedResult(
                self.handles.stream,
                &self.overlapped[index],
                &mut moved,
                i32::from(wait),
            )
        };
        self.in_flight[index] = false;
        check(ok, "isochronous transfer")
    }

    fn complete(&mut self, index: usize, assembler: &mut Assembler) -> Result<()> {
        match self.finish(index, false) {
            Ok(()) => {}
            Err(error) if error.is_disconnected() => return Err(error),
            Err(error) => {
                tracing::warn!("funcube transfer failed: {error}");
                assembler.lost(PACKETS as u64 * SAMPLES_PER_PACKET);
                return Ok(());
            }
        }
        let base = index * self.transfer_bytes();
        let region = &self.buffer[base..base + self.transfer_bytes()];
        let transfer = self.transfer_bytes();
        let packets = self.descriptors[index].iter().map(|packet| Packet {
            offset: packet_offset(packet.Offset as usize, base, transfer),
            length: packet.Length as usize,
            ok: packet.Status == 0,
        });
        assembler.transfer(region, packets);
        Ok(())
    }

    fn resubmit(&mut self, index: usize, assembler: &mut Assembler) -> Result<()> {
        if self.submit(index, true).is_ok() {
            return Ok(());
        }
        assembler.lost(PACKETS as u64 * SAMPLES_PER_PACKET);
        self.submit(index, false)
    }
}

impl Pump for IsoPump {
    fn run(mut self, assembler: &mut Assembler, state: &PumpState) -> Result<()> {
        let mut index = 0;
        while !state.stopping() && !assembler.is_closed() {
            match unsafe { WaitForSingleObject(self.events[index], WAIT_MS) } {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => continue,
                _ => return Err(Error::win32("wait for transfer", last_error())),
            }
            self.complete(index, assembler)?;
            if !state.stopping() {
                self.resubmit(index, assembler)?;
            }
            index = (index + 1) % TRANSFERS;
        }
        Ok(())
    }
}

impl Drop for IsoPump {
    fn drop(&mut self) {
        unsafe { WinUsb_AbortPipe(self.handles.stream, SAMPLES_IN) };
        for index in 0..TRANSFERS {
            if self.in_flight[index]
                && let Err(error) = self.finish(index, true)
                && !matches!(error, Error::Win32 { code, .. } if code == ERROR_OPERATION_ABORTED)
            {
                tracing::debug!("funcube transfer at stop: {error}");
            }
        }
        if !self.registered.is_null() {
            unsafe { WinUsb_UnregisterIsochBuffer(self.registered) };
        }
        for event in &self.events {
            unsafe { CloseHandle(*event) };
        }
        unsafe { WinUsb_SetCurrentAlternateSetting(self.handles.stream, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_offsets_read_the_same_whether_relative_or_absolute() {
        let transfer = PACKETS * 792;
        assert_eq!(packet_offset(792, 0, transfer), 792);
        assert_eq!(packet_offset(792, 3 * transfer, transfer), 792);
        assert_eq!(
            packet_offset(3 * transfer + 792, 3 * transfer, transfer),
            792
        );
    }

    #[test]
    fn wide_strings_end_in_one_nul() {
        let text = wide(OsStr::new("USB\\VID_04D8&PID_FB31\\5&1"));
        assert_eq!(text.last(), Some(&0));
        assert_eq!(until_nul(&text).len(), text.len() - 1);
    }

    #[test]
    fn a_double_nul_list_yields_its_first_path() {
        let list: Vec<u16> = "\\\\?\\usb#a\0\\\\?\\usb#b\0\0".encode_utf16().collect();
        assert_eq!(String::from_utf16_lossy(until_nul(&list)), "\\\\?\\usb#a");
    }
}
