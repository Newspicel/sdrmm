use std::{
    ffi::{CStr, CString, c_char, c_int, c_long, c_uint, c_ulong, c_void},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

use libloading::Library;

use crate::{
    CHANNELS, Error, Result,
    mixer::{Mixer, Shared},
};

const LIBRARY: &str = "libasound.so.2";
const DEFAULT: &str = "default";
const PLAYBACK: c_int = 0;
const FORMAT_FLOAT_LE: c_int = 14;
const ACCESS_RW_INTERLEAVED: c_int = 3;
const RATE: u32 = 48_000;
const LATENCY_US: c_uint = 100_000;
const CHUNK_FRAMES: usize = 480;

type Pcm = c_void;

/// The few calls of libasound this needs, resolved at runtime so a server without ALSA still
/// starts and only finds no outputs.
struct Alsa {
    _library: Library,
    open: unsafe extern "C" fn(*mut *mut Pcm, *const c_char, c_int, c_int) -> c_int,
    set_params:
        unsafe extern "C" fn(*mut Pcm, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int,
    writei: unsafe extern "C" fn(*mut Pcm, *const c_void, c_ulong) -> c_long,
    recover: unsafe extern "C" fn(*mut Pcm, c_int, c_int) -> c_int,
    close: unsafe extern "C" fn(*mut Pcm) -> c_int,
    strerror: unsafe extern "C" fn(c_int) -> *const c_char,
    name_hint: unsafe extern "C" fn(c_int, *const c_char, *mut *mut *mut c_void) -> c_int,
    get_hint: unsafe extern "C" fn(*const c_void, *const c_char) -> *mut c_char,
    free_hint: unsafe extern "C" fn(*mut *mut c_void) -> c_int,
}

impl Alsa {
    fn load() -> std::result::Result<Self, String> {
        let library = unsafe { Library::new(LIBRARY) }
            .map_err(|error| format!("ALSA is not installed ({LIBRARY}): {error}"))?;
        macro_rules! symbol {
            ($name:literal) => {
                *unsafe { library.get(concat!($name, "\0").as_bytes()) }
                    .map_err(|error| format!("{LIBRARY} lacks {}: {error}", $name))?
            };
        }
        Ok(Self {
            open: symbol!("snd_pcm_open"),
            set_params: symbol!("snd_pcm_set_params"),
            writei: symbol!("snd_pcm_writei"),
            recover: symbol!("snd_pcm_recover"),
            close: symbol!("snd_pcm_close"),
            strerror: symbol!("snd_strerror"),
            name_hint: symbol!("snd_device_name_hint"),
            get_hint: symbol!("snd_device_name_get_hint"),
            free_hint: symbol!("snd_device_name_free_hint"),
            _library: library,
        })
    }

    fn describe(&self, code: c_int) -> String {
        let text = unsafe { (self.strerror)(code) };
        if text.is_null() {
            return format!("ALSA error {code}");
        }
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    }

    fn hint(&self, hint: *const c_void, field: &CStr) -> Option<String> {
        let raw = unsafe { (self.get_hint)(hint, field.as_ptr()) };
        if raw.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        unsafe { libc::free(raw.cast()) };
        Some(text)
    }
}

fn alsa() -> Result<&'static Alsa> {
    static ALSA: OnceLock<std::result::Result<Alsa, String>> = OnceLock::new();
    ALSA.get_or_init(Alsa::load)
        .as_ref()
        .map_err(|reason| Error::Backend(reason.clone()))
}

pub(crate) fn outputs() -> Result<Vec<String>> {
    let alsa = alsa()?;
    let mut hints: *mut *mut c_void = std::ptr::null_mut();
    let status = unsafe { (alsa.name_hint)(-1, c"pcm".as_ptr(), &raw mut hints) };
    if status < 0 {
        return Err(Error::Backend(alsa.describe(status)));
    }
    let mut names = Vec::new();
    let mut cursor = hints;
    while !cursor.is_null() && !unsafe { *cursor }.is_null() {
        let hint = unsafe { *cursor };
        let plays = alsa.hint(hint, c"IOID").is_none_or(|way| way == "Output");
        if let Some(name) = alsa
            .hint(hint, c"NAME")
            .filter(|name| plays && name != "null")
        {
            names.push(name);
        }
        cursor = unsafe { cursor.add(1) };
    }
    unsafe { (alsa.free_hint)(hints) };
    Ok(names)
}

struct Handle(*mut Pcm);

unsafe impl Send for Handle {}

pub(crate) struct Device {
    pcm: Handle,
    rate: u32,
}

impl Device {
    pub(crate) fn find(name: Option<&str>) -> Result<Self> {
        let alsa = alsa()?;
        let wanted = name.unwrap_or(DEFAULT);
        let c_name = CString::new(wanted).map_err(|_| Error::NotFound(wanted.to_owned()))?;
        let mut pcm: *mut Pcm = std::ptr::null_mut();
        let opened = unsafe { (alsa.open)(&raw mut pcm, c_name.as_ptr(), PLAYBACK, 0) };
        if opened < 0 {
            return Err(Error::Backend(format!(
                "{wanted}: {}",
                alsa.describe(opened)
            )));
        }
        let set = unsafe {
            (alsa.set_params)(
                pcm,
                FORMAT_FLOAT_LE,
                ACCESS_RW_INTERLEAVED,
                CHANNELS as c_uint,
                RATE,
                1,
                LATENCY_US,
            )
        };
        if set < 0 {
            unsafe { (alsa.close)(pcm) };
            return Err(Error::Backend(format!("{wanted}: {}", alsa.describe(set))));
        }
        Ok(Self {
            pcm: Handle(pcm),
            rate: RATE,
        })
    }

    pub(crate) const fn rate(&self) -> u32 {
        self.rate
    }

    pub(crate) fn start(self, mixer: Mixer, shared: Arc<Shared>) -> Result<Stream> {
        let alsa = alsa()?;
        let stop = Arc::new(AtomicBool::new(false));
        let writer = Writer {
            pcm: self.pcm,
            mixer,
            shared,
            stop: stop.clone(),
        };
        let thread = std::thread::Builder::new()
            .name("sdrmm-audio-out".to_owned())
            .spawn(move || writer.run(alsa))
            .map_err(|error| Error::Backend(format!("start audio output: {error}")))?;
        Ok(Stream {
            stop,
            thread: Some(thread),
        })
    }
}

struct Writer {
    pcm: Handle,
    mixer: Mixer,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
}

impl Writer {
    fn run(mut self, alsa: &Alsa) {
        let mut buffer = vec![0.0f32; CHUNK_FRAMES * CHANNELS];
        while !self.stop.load(Ordering::Acquire) {
            self.mixer.fill(&mut buffer);
            if let Err(reason) = self.write(alsa, &buffer) {
                self.shared.fail(reason);
                break;
            }
        }
        unsafe { (alsa.close)(self.pcm.0) };
    }

    fn write(&self, alsa: &Alsa, buffer: &[f32]) -> std::result::Result<(), String> {
        let mut done = 0;
        let frames = buffer.len() / CHANNELS;
        while done < frames {
            let rest = &buffer[done * CHANNELS..];
            let wrote = unsafe {
                (alsa.writei)(self.pcm.0, rest.as_ptr().cast(), (frames - done) as c_ulong)
            };
            if wrote >= 0 {
                done += wrote as usize;
                continue;
            }
            let code = c_int::try_from(wrote).unwrap_or(c_int::MIN);
            if unsafe { (alsa.recover)(self.pcm.0, code, 1) } < 0 {
                return Err(alsa.describe(code));
            }
        }
        Ok(())
    }
}

pub(crate) struct Stream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Stream {
    pub(crate) fn fault(&self) -> Option<String> {
        let ended = self.thread.as_ref().is_some_and(JoinHandle::is_finished);
        (ended && !self.stop.load(Ordering::Acquire)).then(|| "Audio output stopped".to_owned())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("audio output thread panicked");
        }
    }
}
