use std::{
    fs::File,
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    ptr::NonNull,
    time::Duration,
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AllocRequest {
    kind: u32,
    size: u32,
    count: u32,
    id: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BlockDesc {
    id: u32,
    size: u32,
    bytes_used: u32,
    kind: u32,
    flags: u32,
    offset: u32,
    timestamp: u64,
}

const fn request(dir: u64, nr: u64, size: usize) -> u64 {
    (dir << 30) | ((size as u64) << 16) | ((b'i' as u64) << 8) | nr
}

const READ_WRITE: u64 = 3;
const BUFFER_FD: u64 = request(READ_WRITE, 0x91, size_of::<libc::c_int>());
const ALLOC: u64 = request(READ_WRITE, 0xa0, size_of::<AllocRequest>());
const FREE: u64 = request(0, 0xa1, 0);
const QUERY: u64 = request(READ_WRITE, 0xa2, size_of::<BlockDesc>());
const ENQUEUE: u64 = request(READ_WRITE, 0xa3, size_of::<BlockDesc>());
const DEQUEUE: u64 = request(READ_WRITE, 0xa4, size_of::<BlockDesc>());

const TIMESTAMP_VALID: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dequeued {
    pub id: u32,
    pub len: usize,
    pub timestamp: Option<u64>,
}

struct Mapping {
    desc: BlockDesc,
    base: NonNull<u8>,
}

pub struct Blocks {
    fd: i32,
    maps: Vec<Mapping>,
    _buffer: Option<OwnedFd>,
}

unsafe impl Send for Blocks {}

fn control<T>(fd: i32, op: u64, arg: *mut T) -> io::Result<()> {
    loop {
        let done = unsafe { libc::ioctl(fd, op as _, arg) };
        if done >= 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

impl Blocks {
    pub fn new(file: &File, block_bytes: usize, count: usize) -> io::Result<Self> {
        let buffer = buffer_fd(file);
        let fd = buffer
            .as_ref()
            .map_or_else(|| file.as_raw_fd(), AsRawFd::as_raw_fd);
        let mut ask = AllocRequest {
            size: u32::try_from(block_bytes).map_err(io::Error::other)?,
            count: u32::try_from(count).map_err(io::Error::other)?,
            ..AllocRequest::default()
        };
        control(fd, ALLOC, &raw mut ask)?;
        let mut blocks = Self {
            fd,
            maps: Vec::with_capacity(ask.count as usize),
            _buffer: buffer,
        };
        for id in 0..ask.count {
            let mut desc = BlockDesc {
                id,
                ..BlockDesc::default()
            };
            control(fd, QUERY, &raw mut desc)?;
            let base = map(fd, &desc)?;
            blocks.maps.push(Mapping { desc, base });
            control(fd, ENQUEUE, &raw mut desc)?;
        }
        Ok(blocks)
    }

    pub fn dequeue(&mut self, timeout: Duration) -> io::Result<Option<Dequeued>> {
        let mut ready = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
        let polled = unsafe { libc::poll(&raw mut ready, 1, millis) };
        if polled < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::Interrupted {
                Ok(None)
            } else {
                Err(error)
            };
        }
        if polled == 0 {
            return Ok(None);
        }
        let mut desc = BlockDesc::default();
        control(self.fd, DEQUEUE, &raw mut desc)?;
        let id = desc.id;
        let mapping = self
            .maps
            .get_mut(id as usize)
            .ok_or_else(|| io::Error::other(format!("the radio handed back unknown block {id}")))?;
        let len = (desc.bytes_used as usize).min(mapping.desc.size as usize);
        mapping.desc = desc;
        Ok(Some(Dequeued {
            id,
            len,
            timestamp: (desc.flags & TIMESTAMP_VALID != 0).then_some(desc.timestamp),
        }))
    }

    pub fn bytes(&self, id: u32, len: usize) -> &[u8] {
        self.maps.get(id as usize).map_or(&[], |mapping| {
            let len = len.min(mapping.desc.size as usize);
            unsafe { std::slice::from_raw_parts(mapping.base.as_ptr(), len) }
        })
    }

    pub fn enqueue(&mut self, id: u32) -> io::Result<()> {
        let mapping = self
            .maps
            .get_mut(id as usize)
            .ok_or_else(|| io::Error::other(format!("no block {id} to hand back")))?;
        control(self.fd, ENQUEUE, &raw mut mapping.desc)
    }
}

fn buffer_fd(file: &File) -> Option<OwnedFd> {
    let mut index: libc::c_int = 0;
    control(file.as_raw_fd(), BUFFER_FD, &raw mut index).ok()?;
    (index >= 0).then(|| unsafe { OwnedFd::from_raw_fd(index) })
}

fn map(fd: i32, desc: &BlockDesc) -> io::Result<NonNull<u8>> {
    let base = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            desc.size as usize,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            libc::off_t::from(desc.offset),
        )
    };
    if base == libc::MAP_FAILED {
        return Err(io::Error::last_os_error());
    }
    NonNull::new(base.cast()).ok_or_else(|| io::Error::other("the radio mapped a block at null"))
}

impl Drop for Blocks {
    fn drop(&mut self) {
        for mapping in self.maps.drain(..) {
            unsafe {
                libc::munmap(mapping.base.as_ptr().cast(), mapping.desc.size as usize);
            }
        }
        if let Err(e) = control(self.fd, FREE, std::ptr::null_mut::<u8>()) {
            eprintln!("iqlinkd: could not free the sample blocks: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_match_the_kernel_numbers() {
        assert_eq!(size_of::<AllocRequest>(), 16);
        assert_eq!(size_of::<BlockDesc>(), 32);
        assert_eq!(ALLOC, 0xc010_69a0);
        assert_eq!(FREE, 0x0000_69a1);
        assert_eq!(QUERY, 0xc020_69a2);
        assert_eq!(ENQUEUE, 0xc020_69a3);
        assert_eq!(DEQUEUE, 0xc020_69a4);
        assert_eq!(BUFFER_FD, 0xc004_6991);
    }
}
