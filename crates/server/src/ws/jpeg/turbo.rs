use std::{
    ffi::{c_int, c_ulong, c_void},
    ptr,
    sync::OnceLock,
};

use libloading::Library;
use sdrmm_decoder::VideoPicture;

const RGB: c_int = 0;
const GRAY: c_int = 6;
const SUBSAMPLE_420: c_int = 2;
const SUBSAMPLE_GRAY: c_int = 3;
const FAST_DCT: c_int = 2048;

type Handle = *mut c_void;
type Init = unsafe extern "C" fn() -> Handle;
type Destroy = unsafe extern "C" fn(Handle) -> c_int;
type Free = unsafe extern "C" fn(*mut u8);
type Compress = unsafe extern "C" fn(
    Handle,
    *const u8,
    c_int,
    c_int,
    c_int,
    c_int,
    *mut *mut u8,
    *mut c_ulong,
    c_int,
    c_int,
    c_int,
) -> c_int;

struct Encoder {
    _library: Library,
    init: Init,
    destroy: Destroy,
    free: Free,
    compress: Compress,
}

impl Encoder {
    fn load() -> Option<Self> {
        unsafe {
            let lib = Library::new("libturbojpeg.so.0").ok()?;
            let encoder = Self {
                init: *lib.get(b"tjInitCompress").ok()?,
                destroy: *lib.get(b"tjDestroy").ok()?,
                free: *lib.get(b"tjFree").ok()?,
                compress: *lib.get(b"tjCompress2").ok()?,
                _library: lib,
            };
            tracing::info!("TurboJPEG enabled for video output");
            Some(encoder)
        }
    }
}

static ENCODER: OnceLock<Option<Encoder>> = OnceLock::new();

pub(super) fn compress(picture: &VideoPicture, color: bool) -> Option<Vec<u8>> {
    let encoder = ENCODER.get_or_init(Encoder::load).as_ref()?;
    let (pixels, format, subsampling) = if color {
        (&picture.rgb, RGB, SUBSAMPLE_420)
    } else {
        (&picture.luma, GRAY, SUBSAMPLE_GRAY)
    };
    unsafe {
        let handle = (encoder.init)();
        if handle.is_null() {
            return None;
        }
        let mut jpeg = ptr::null_mut();
        let mut size = 0;
        let result = (encoder.compress)(
            handle,
            pixels.as_ptr(),
            picture.width.into(),
            0,
            picture.height.into(),
            format,
            &mut jpeg,
            &mut size,
            subsampling,
            super::QUALITY as c_int,
            FAST_DCT,
        );
        let output = if result == 0 && !jpeg.is_null() && size > 0 && size <= isize::MAX as c_ulong
        {
            Some(std::slice::from_raw_parts(jpeg, size as usize).to_vec())
        } else {
            None
        };
        (encoder.free)(jpeg);
        (encoder.destroy)(handle);
        output
    }
}
