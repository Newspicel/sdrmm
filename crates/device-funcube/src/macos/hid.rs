use std::{
    ffi::{CStr, c_void},
    ptr::NonNull,
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    time::{Duration, Instant},
};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2_core_foundation::{CFDictionary, CFIndex, CFNumber, CFRetained, CFString};
use objc2_io_kit::{
    IOHIDDevice, IOHIDManager, IOHIDReportType, IOReturn, kIOHIDLocationIDKey,
    kIOHIDOptionsTypeNone, kIOHIDProductIDKey, kIOHIDVendorIDKey, kIOReturnSuccess,
};

use crate::{
    discovery::{PRODUCT_ID, VENDOR_ID},
    error::{Error, Result},
    protocol::{self, REPORT_LEN, Report},
    radio::Link,
};

const TIMEOUT: Duration = Duration::from_millis(500);
const CANCEL_TIMEOUT: Duration = Duration::from_secs(1);
const INBOX_DEPTH: usize = 8;

pub(crate) struct HidLink {
    device: CFRetained<IOHIDDevice>,
    _queue: DispatchRetained<DispatchQueue>,
    _on_cancel: RcBlock<dyn Fn()>,
    cancelled: Receiver<()>,
    replies: Receiver<Report>,
    sender: NonNull<SyncSender<Report>>,
    buffer: NonNull<Report>,
}

unsafe impl Send for HidLink {}

unsafe extern "C-unwind" fn on_report(
    context: *mut c_void,
    result: IOReturn,
    _sender: *mut c_void,
    _kind: IOHIDReportType,
    _id: u32,
    report: NonNull<u8>,
    length: CFIndex,
) {
    if result != kIOReturnSuccess || context.is_null() {
        return;
    }
    let sender = unsafe { &*context.cast::<SyncSender<Report>>() };
    let length = usize::try_from(length).unwrap_or(0).min(REPORT_LEN);
    let bytes = unsafe { std::slice::from_raw_parts(report.as_ptr(), length) };
    let mut reply = [0; REPORT_LEN];
    reply[..length].copy_from_slice(bytes);
    if sender.try_send(reply).is_err() {
        tracing::debug!("funcube reply dropped: nothing is waiting for it");
    }
}

fn key(name: &CStr) -> CFRetained<CFString> {
    CFString::from_str(&name.to_string_lossy())
}

fn location_of(device: &IOHIDDevice) -> Option<u32> {
    let value = device.property(&key(kIOHIDLocationIDKey))?;
    let number = value.downcast_ref::<CFNumber>()?;
    u32::try_from(number.as_i64()?).ok()
}

fn find(location: u32) -> Result<CFRetained<IOHIDDevice>> {
    let manager = IOHIDManager::new(None, kIOHIDOptionsTypeNone);
    let keys = [key(kIOHIDVendorIDKey), key(kIOHIDProductIDKey)];
    let values = [
        CFNumber::new_i32(VENDOR_ID.into()),
        CFNumber::new_i32(PRODUCT_ID.into()),
    ];
    let matching = CFDictionary::from_slices(&[&*keys[0], &*keys[1]], &[&*values[0], &*values[1]]);
    unsafe { manager.set_device_matching(Some(matching.as_opaque())) };
    let devices = manager.devices().ok_or(Error::DeviceNotFound)?;
    let mut found = vec![std::ptr::null::<c_void>(); usize::try_from(devices.count()).unwrap_or(0)];
    unsafe { devices.values(found.as_mut_ptr()) };
    found
        .into_iter()
        .filter_map(|device| NonNull::new(device.cast_mut().cast::<IOHIDDevice>()))
        .map(|device| unsafe { CFRetained::retain(device) })
        .find(|device| location_of(device) == Some(location))
        .ok_or(Error::DeviceNotFound)
}

impl HidLink {
    pub(crate) fn open(location: u32) -> Result<Self> {
        let device = find(location)?;
        let opened = device.open(kIOHIDOptionsTypeNone);
        if opened != kIOReturnSuccess {
            return Err(Error::hid("open HID interface", opened));
        }
        let queue = DispatchQueue::new("sdrmm-funcube-hid", None);
        let (sender, replies) = mpsc::sync_channel(INBOX_DEPTH);
        let sender = NonNull::from(Box::leak(Box::new(sender)));
        let buffer = NonNull::from(Box::leak(Box::new([0u8; REPORT_LEN])));
        let (done, cancelled) = mpsc::sync_channel(1);
        let on_cancel = RcBlock::new(move || {
            if done.try_send(()).is_err() {
                tracing::debug!("funcube HID cancelled twice");
            }
        });
        unsafe {
            device.set_dispatch_queue(&queue);
            device.set_cancel_handler(RcBlock::as_ptr(&on_cancel));
            device.register_input_report_callback(
                buffer.cast(),
                REPORT_LEN as CFIndex,
                Some(on_report),
                sender.as_ptr().cast(),
            );
        }
        device.activate();
        Ok(Self {
            device,
            _queue: queue,
            _on_cancel: on_cancel,
            cancelled,
            replies,
            sender,
            buffer,
        })
    }

    fn send(&self, request: &Report) -> Result<()> {
        let mut report = *request;
        let sent = unsafe {
            self.device.set_report(
                IOHIDReportType::Output,
                0,
                NonNull::from(&mut report).cast(),
                REPORT_LEN as CFIndex,
            )
        };
        if sent != kIOReturnSuccess {
            return Err(Error::hid("send HID report", sent));
        }
        Ok(())
    }
}

impl Link for HidLink {
    fn exchange(&mut self, request: &Report) -> Result<Report> {
        while self.replies.try_recv().is_ok() {}
        self.send(request)?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.replies.recv_timeout(left) {
                Ok(reply) if reply[0] == request[0] => return Ok(reply),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    return Err(Error::Timeout {
                        operation: protocol::name_of(request[0]),
                        timeout: TIMEOUT,
                    });
                }
            }
        }
    }
}

impl Drop for HidLink {
    fn drop(&mut self) {
        self.device.cancel();
        let closed = self.device.close(kIOHIDOptionsTypeNone);
        if closed != kIOReturnSuccess {
            tracing::debug!("funcube HID close returned {closed:#x}");
        }
        if self.cancelled.recv_timeout(CANCEL_TIMEOUT).is_err() {
            tracing::warn!("funcube HID never confirmed its cancel; leaking its report buffer");
            return;
        }
        unsafe {
            drop(Box::from_raw(self.sender.as_ptr()));
            drop(Box::from_raw(self.buffer.as_ptr()));
        }
    }
}
