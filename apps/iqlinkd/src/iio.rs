use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

const QUEUE_CHUNKS: usize = 8;
const MAX_BLOCKS: usize = 64;
const CHUNK_BYTES: usize = 512 << 10;
#[cfg(target_os = "linux")]
const POLL: std::time::Duration = std::time::Duration::from_millis(200);

#[derive(Debug, thiserror::Error)]
pub enum IioError {
    #[error("no IIO device is named {0}")]
    NoDevice(String),
    #[error("{0} has no scan element with index {1}")]
    NoElement(String, u32),
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("{0}: unreadable scan element type {1:?}")]
    Type(PathBuf, String),
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub devices: PathBuf,
    pub dev: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        Self {
            devices: PathBuf::from("/sys/bus/iio/devices"),
            dev: PathBuf::from("/dev"),
        }
    }
}

fn io_at(path: &Path) -> impl FnOnce(io::Error) -> IioError + '_ {
    move |source| IioError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn read_text(path: &Path) -> Result<String, IioError> {
    fs::read_to_string(path)
        .map(|text| text.trim().to_string())
        .map_err(io_at(path))
}

fn write_text(path: &Path, text: &str) -> Result<(), IioError> {
    fs::write(path, text).map_err(io_at(path))
}

fn find(paths: &Paths, name: &str) -> Result<String, IioError> {
    let entries = fs::read_dir(&paths.devices).map_err(io_at(&paths.devices))?;
    entries
        .filter_map(Result::ok)
        .find(|entry| read_text(&entry.path().join("name")).is_ok_and(|found| found == name))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .ok_or_else(|| IioError::NoDevice(name.to_string()))
}

struct Element {
    enable: PathBuf,
    index: u32,
    bytes: usize,
}

fn storage_bytes(path: &Path, kind: &str) -> Result<usize, IioError> {
    let bad = || IioError::Type(path.to_path_buf(), kind.to_string());
    let (_, shape) = kind.split_once(':').ok_or_else(bad)?;
    let (_, rest) = shape.split_once('/').ok_or_else(bad)?;
    let bits: usize = rest
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|digits| digits.parse().ok())
        .ok_or_else(bad)?;
    Ok(bits.div_ceil(8))
}

fn elements(dir: &Path) -> Result<Vec<Element>, IioError> {
    let scan = dir.join("scan_elements");
    let mut found = Vec::new();
    for entry in fs::read_dir(&scan)
        .map_err(io_at(&scan))?
        .filter_map(Result::ok)
    {
        let file = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = file.strip_suffix("_index") else {
            continue;
        };
        let index_path = entry.path();
        let index = read_text(&index_path)?
            .parse()
            .map_err(|_| IioError::Type(index_path.clone(), "index".to_string()))?;
        let type_path = scan.join(format!("{stem}_type"));
        found.push(Element {
            enable: scan.join(format!("{stem}_en")),
            index,
            bytes: storage_bytes(&type_path, &read_text(&type_path)?)?,
        });
    }
    found.sort_by_key(|element| element.index);
    Ok(found)
}

pub struct Buffer {
    #[cfg(target_os = "linux")]
    blocks: Option<crate::mapped::Blocks>,
    chunk_frames: usize,
    queued_chunks: usize,
    file: File,
    enable: PathBuf,
    rate: PathBuf,
    frame_bytes: usize,
}

impl Buffer {
    pub fn open(
        paths: &Paths,
        name: &str,
        wanted: &[u32],
        buffer_frames: usize,
    ) -> Result<Self, IioError> {
        let id = find(paths, name)?;
        let dir = paths.devices.join(&id);
        let enable = dir.join("buffer").join("enable");
        write_text(&enable, "0")?;
        let all = elements(&dir)?;
        if let Some(missing) = wanted.iter().find(|i| !all.iter().any(|e| e.index == **i)) {
            return Err(IioError::NoElement(name.to_string(), *missing));
        }
        let mut frame_bytes = 0;
        for element in &all {
            let on = wanted.contains(&element.index);
            write_text(&element.enable, if on { "1" } else { "0" })?;
            frame_bytes += if on { element.bytes } else { 0 };
        }
        let node = paths.dev.join(&id);
        let file = File::open(&node).map_err(io_at(&node))?;
        let chunk_frames = buffer_frames.min(CHUNK_BYTES / frame_bytes.max(1)).max(1);
        let queued_chunks = (buffer_frames * QUEUE_CHUNKS)
            .div_ceil(chunk_frames)
            .clamp(QUEUE_CHUNKS, MAX_BLOCKS);
        #[cfg(target_os = "linux")]
        let blocks = map_blocks(&file, chunk_frames * frame_bytes, queued_chunks);
        #[cfg(target_os = "linux")]
        let mapped = blocks.is_some();
        #[cfg(not(target_os = "linux"))]
        let mapped = false;
        if !mapped {
            let length = chunk_frames * queued_chunks;
            write_text(&dir.join("buffer").join("length"), &length.to_string())?;
        }
        write_text(&enable, "1")?;
        Ok(Self {
            #[cfg(target_os = "linux")]
            blocks,
            chunk_frames,
            queued_chunks,
            file,
            enable,
            rate: dir.join("in_voltage_sampling_frequency"),
            frame_bytes,
        })
    }

    pub const fn frame_bytes(&self) -> usize {
        self.frame_bytes
    }

    pub fn fill(&mut self, chunk: &mut Vec<u8>, stop: &AtomicBool) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        if let Some(blocks) = &mut self.blocks {
            return fill_from_blocks(blocks, chunk, stop);
        }
        if stop.load(Ordering::Acquire) {
            return Ok(false);
        }
        chunk.resize(chunk.capacity(), 0);
        self.file.read_exact(chunk)?;
        Ok(true)
    }

    #[cfg(target_os = "linux")]
    pub const fn mapped(&self) -> bool {
        self.blocks.is_some()
    }

    #[cfg(not(target_os = "linux"))]
    pub const fn mapped(&self) -> bool {
        false
    }

    pub const fn chunk_frames(&self) -> usize {
        self.chunk_frames
    }

    pub const fn queued_chunks(&self) -> usize {
        self.queued_chunks
    }

    pub fn rate(&self) -> f64 {
        read_text(&self.rate)
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(0.0)
    }
}

#[cfg(target_os = "linux")]
fn fill_from_blocks(
    blocks: &mut crate::mapped::Blocks,
    chunk: &mut Vec<u8>,
    stop: &AtomicBool,
) -> io::Result<bool> {
    while !stop.load(Ordering::Acquire) {
        let Some(block) = blocks.dequeue(POLL)? else {
            continue;
        };
        chunk.clear();
        chunk.extend_from_slice(blocks.bytes(block.id, block.len));
        blocks.enqueue(block.id)?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(target_os = "linux")]
fn map_blocks(file: &File, block_bytes: usize, count: usize) -> Option<crate::mapped::Blocks> {
    match crate::mapped::Blocks::new(file, block_bytes, count) {
        Ok(blocks) => Some(blocks),
        Err(e) => {
            eprintln!("iqlinkd: no mapped blocks ({e}), copying with read()");
            None
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        if let Err(e) = write_text(&self.enable, "0") {
            eprintln!("iqlinkd: could not stop the buffer: {e}");
        }
    }
}

#[cfg(test)]
pub mod fake {
    use std::{fs, path::Path};

    use super::Paths;

    pub const ID: &str = "iio:device4";
    pub const NAME: &str = "cf-ad9361-lpc";

    pub fn radio(root: &Path, samples: &[u8]) -> Paths {
        let paths = Paths {
            devices: root.join("devices"),
            dev: root.join("dev"),
        };
        let dir = paths.devices.join(ID);
        let scan = dir.join("scan_elements");
        for path in [&scan, &dir.join("buffer"), &paths.dev] {
            fs::create_dir_all(path).expect("fake sysfs");
        }
        fs::write(dir.join("name"), format!("{NAME}\n")).expect("name");
        fs::write(dir.join("in_voltage_sampling_frequency"), "1000000\n").expect("rate");
        for index in 0..4 {
            let stem = scan.join(format!("in_voltage{index}"));
            fs::write(format!("{}_index", stem.display()), format!("{index}\n")).expect("index");
            fs::write(format!("{}_type", stem.display()), "le:S12/16>>0\n").expect("type");
            fs::write(format!("{}_en", stem.display()), "0\n").expect("en");
        }
        fs::write(dir.join("buffer").join("enable"), "0\n").expect("enable");
        fs::write(dir.join("buffer").join("length"), "0\n").expect("length");
        fs::write(paths.dev.join(ID), samples).expect("samples");
        paths
    }

    pub fn read(paths: &Paths, file: &str) -> String {
        fs::read_to_string(paths.devices.join(ID).join(file))
            .expect("fake file")
            .trim()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{fake::*, *};

    #[test]
    fn opening_enables_only_the_asked_elements_and_sizes_the_frame() {
        let root = tempfile::tempdir().expect("tempdir");
        let paths = radio(root.path(), &[0; 64]);
        let buffer = Buffer::open(&paths, NAME, &[0, 1], 100).expect("open");
        assert_eq!(buffer.frame_bytes(), 4);
        assert_eq!(read(&paths, "scan_elements/in_voltage0_en"), "1");
        assert_eq!(read(&paths, "scan_elements/in_voltage1_en"), "1");
        assert_eq!(read(&paths, "scan_elements/in_voltage2_en"), "0");
        assert_eq!(buffer.chunk_frames(), 100);
        assert_eq!(buffer.queued_chunks(), 8);
        assert_eq!(read(&paths, "buffer/length"), "800");
        assert_eq!(read(&paths, "buffer/enable"), "1");
        assert!((buffer.rate() - 1e6).abs() < f64::EPSILON);
        drop(buffer);
        assert_eq!(read(&paths, "buffer/enable"), "0");
    }

    #[test]
    fn large_buffers_are_cut_into_cache_sized_chunks() {
        let root = tempfile::tempdir().expect("tempdir");
        let paths = radio(root.path(), &[]);
        let buffer = Buffer::open(&paths, NAME, &[0, 1, 2, 3], 524_288).expect("open");
        assert_eq!(buffer.chunk_frames() * buffer.frame_bytes(), CHUNK_BYTES);
        assert_eq!(buffer.queued_chunks(), MAX_BLOCKS);
    }

    #[test]
    fn an_unknown_device_or_element_is_refused() {
        let root = tempfile::tempdir().expect("tempdir");
        let paths = radio(root.path(), &[]);
        assert!(matches!(
            Buffer::open(&paths, "nope", &[0], 10),
            Err(IioError::NoDevice(_))
        ));
        assert!(matches!(
            Buffer::open(&paths, NAME, &[0, 9], 10),
            Err(IioError::NoElement(_, 9))
        ));
    }

    #[test]
    fn storage_width_comes_from_the_scan_type() {
        let path = Path::new("x");
        assert_eq!(storage_bytes(path, "le:S12/16>>0").expect("type"), 2);
        assert_eq!(storage_bytes(path, "be:u24/32>>8").expect("type"), 4);
        assert!(storage_bytes(path, "garbage").is_err());
    }
}
