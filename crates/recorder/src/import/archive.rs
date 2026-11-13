use std::{
    fs::{self, File},
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{COLLECTION_SUFFIX, DATA_SUFFIX, META_SUFFIX, SigmfError};

pub const ARCHIVE_SUFFIX: &str = ".sigmf";

const BLOCK: usize = 512;
const MAX_META_BYTES: u64 = 8 * 1024 * 1024;
const SPOOL_PREFIX: &str = ".import-";
const SPOOL_SUFFIX: &str = ".sigmf-spool";
const MEMBER_TOO_LARGE: &str = "the archive declares a member longer than the archive itself";
const ONE_RECORDING: &str =
    "a .sigmf archive must hold one .sigmf-meta and one .sigmf-data, or a collection and its lanes";

static SPOOLS: AtomicU64 = AtomicU64::new(0);

/// A data member copied out of the archive onto disk, so an upload of many GiB never sits in
/// memory. The file is removed again unless it is landed in the library.
pub struct Spooled {
    path: PathBuf,
    landed: bool,
}

impl Spooled {
    fn copy(dir: &Path, source: &mut impl Read, size: u64) -> Result<Self, SigmfError> {
        let (path, file) = claim_spool(dir)?;
        let spooled = Self {
            path,
            landed: false,
        };
        let mut out = BufWriter::new(file);
        if io::copy(&mut source.take(size), &mut out)? != size {
            return Err(SigmfError::Malformed(MEMBER_TOO_LARGE.to_owned()));
        }
        out.flush()?;
        out.get_ref().sync_all()?;
        Ok(spooled)
    }

    pub fn open(&self) -> io::Result<File> {
        File::open(&self.path)
    }

    pub fn land(&mut self, to: &Path) -> io::Result<()> {
        fs::rename(&self.path, to)?;
        self.landed = true;
        Ok(())
    }
}

impl Drop for Spooled {
    fn drop(&mut self) {
        if !self.landed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// A fresh spool file in `dir` that [`sweep_spools`] clears away should this process die.
pub fn claim_spool(dir: &Path) -> Result<(PathBuf, File), SigmfError> {
    loop {
        let n = SPOOLS.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!(
            "{SPOOL_PREFIX}{}-{n}{SPOOL_SUFFIX}",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err.into()),
        }
    }
}

/// Removes the spools a crashed import left in `dir` and answers what it removed. A spool this
/// process claimed may belong to an import still running, so it stays.
pub fn sweep_spools(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut removed = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(left_behind)
        {
            fs::remove_file(&path)?;
            removed.push(path);
        }
    }
    Ok(removed)
}

fn left_behind(name: &str) -> bool {
    let Some((pid, n)) = name
        .strip_prefix(SPOOL_PREFIX)
        .and_then(|rest| rest.strip_suffix(SPOOL_SUFFIX))
        .and_then(|rest| rest.split_once('-'))
    else {
        return false;
    };
    let (Ok(pid), Ok(n)) = (pid.parse::<u32>(), n.parse::<u64>()) else {
        return false;
    };
    pid != std::process::id() || n >= SPOOLS.load(Ordering::Relaxed)
}

pub struct Member {
    pub name: String,
    pub meta: Vec<u8>,
    pub data: Spooled,
}

pub struct ArchivedCollection {
    pub name: String,
    pub collection: Vec<u8>,
    pub lanes: Vec<Member>,
}

pub enum Archive {
    Pair(Member),
    Collection(ArchivedCollection),
}

struct Found {
    name: String,
    meta: Option<Vec<u8>>,
    data: Option<Spooled>,
}

#[derive(Default)]
struct Parts {
    collection: Option<(String, Vec<u8>)>,
    pairs: Vec<Found>,
}

impl Parts {
    fn take_collection(&mut self, name: String, body: Vec<u8>) -> Result<(), SigmfError> {
        match self.collection.replace((name, body)) {
            None => Ok(()),
            Some(_) => Err(SigmfError::Malformed(ONE_RECORDING.to_owned())),
        }
    }

    fn take_meta(&mut self, name: String, body: Vec<u8>) -> Result<(), SigmfError> {
        let found = self.found(name);
        if found.meta.replace(body).is_some() {
            return Err(twice(&found.name, META_SUFFIX));
        }
        Ok(())
    }

    fn take_data(&mut self, name: String, body: Spooled) -> Result<(), SigmfError> {
        let found = self.found(name);
        if found.data.replace(body).is_some() {
            return Err(twice(&found.name, DATA_SUFFIX));
        }
        Ok(())
    }

    fn found(&mut self, name: String) -> &mut Found {
        let index = match self.pairs.iter().position(|found| found.name == name) {
            Some(index) => index,
            None => {
                self.pairs.push(Found {
                    name,
                    meta: None,
                    data: None,
                });
                self.pairs.len() - 1
            }
        };
        &mut self.pairs[index]
    }

    fn finish(self) -> Result<Archive, SigmfError> {
        let mut lanes = Vec::with_capacity(self.pairs.len());
        for found in self.pairs {
            match (found.meta, found.data) {
                (Some(meta), Some(data)) => lanes.push(Member {
                    name: found.name,
                    meta,
                    data,
                }),
                _ => return Err(SigmfError::Malformed(ONE_RECORDING.to_owned())),
            }
        }
        match (self.collection, lanes.len()) {
            (Some((name, collection)), _) => Ok(Archive::Collection(ArchivedCollection {
                name,
                collection,
                lanes,
            })),
            (None, 1) => lanes
                .pop()
                .map(Archive::Pair)
                .ok_or_else(|| SigmfError::Malformed(ONE_RECORDING.to_owned())),
            (None, _) => Err(SigmfError::Malformed(ONE_RECORDING.to_owned())),
        }
    }
}

fn twice(name: &str, suffix: &str) -> SigmfError {
    SigmfError::Malformed(format!("the archive holds `{name}{suffix}` twice"))
}

/// Reads a .sigmf tar. Metadata stays in memory; every data member is spooled into `spool`, which
/// must sit on the filesystem the recording lands on so landing it is a rename.
pub fn read_archive(mut archive: impl Read, spool: &Path) -> Result<Archive, SigmfError> {
    let mut parts = Parts::default();
    let mut header = [0u8; BLOCK];
    while fill(&mut archive, &mut header)? && header.iter().any(|byte| *byte != 0) {
        let entry = entry_name(&header)?;
        let size = entry_size(&header)?;
        let padded = size
            .div_ceil(BLOCK as u64)
            .checked_mul(BLOCK as u64)
            .ok_or_else(|| SigmfError::Malformed(MEMBER_TOO_LARGE.to_owned()))?;
        let Some(suffix) = member_suffix(&entry) else {
            skip(&mut archive, padded)?;
            continue;
        };
        let name = stem_of(&entry, suffix);
        if suffix == DATA_SUFFIX {
            let data = Spooled::copy(spool, &mut archive, size)?;
            skip(&mut archive, padded - size)?;
            parts.take_data(name, data)?;
            continue;
        }
        if size > MAX_META_BYTES {
            return Err(SigmfError::Malformed(
                "the archive's metadata is larger than any SigMF metadata".to_owned(),
            ));
        }
        let mut body = Vec::new();
        if archive.by_ref().take(size).read_to_end(&mut body)? as u64 != size {
            return Err(SigmfError::Malformed(MEMBER_TOO_LARGE.to_owned()));
        }
        skip(&mut archive, padded - size)?;
        if suffix == COLLECTION_SUFFIX {
            parts.take_collection(name, body)?;
        } else {
            parts.take_meta(name, body)?;
        }
    }
    parts.finish()
}

fn member_suffix(entry: &str) -> Option<&'static str> {
    if entry.ends_with(COLLECTION_SUFFIX) {
        Some(COLLECTION_SUFFIX)
    } else if entry.ends_with(META_SUFFIX) {
        Some(META_SUFFIX)
    } else if entry.ends_with(DATA_SUFFIX) {
        Some(DATA_SUFFIX)
    } else {
        None
    }
}

fn stem_of(entry: &str, suffix: &str) -> String {
    entry
        .trim_end_matches(suffix)
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(entry)
        .to_owned()
}

fn fill(source: &mut impl Read, block: &mut [u8; BLOCK]) -> Result<bool, SigmfError> {
    let mut filled = 0;
    while filled < BLOCK {
        let read = source.read(&mut block[filled..])?;
        if read == 0 {
            return Ok(false);
        }
        filled += read;
    }
    Ok(true)
}

fn skip(source: &mut impl Read, mut bytes: u64) -> Result<(), SigmfError> {
    let mut sink = [0u8; BLOCK];
    while bytes > 0 {
        let want = bytes.min(BLOCK as u64) as usize;
        source.read_exact(&mut sink[..want])?;
        bytes -= want as u64;
    }
    Ok(())
}

fn entry_name(header: &[u8; BLOCK]) -> Result<String, SigmfError> {
    let prefix = trimmed(&header[345..500]);
    let name = trimmed(&header[..100]);
    if name.is_empty() {
        return Err(SigmfError::Malformed(
            "the archive holds an entry with no name".to_owned(),
        ));
    }
    Ok(if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    })
}

fn entry_size(header: &[u8; BLOCK]) -> Result<u64, SigmfError> {
    let raw = &header[124..136];
    if raw[0] & 0x80 != 0 {
        let start = raw.len() - size_of::<u64>();
        let mut bytes = [0u8; size_of::<u64>()];
        bytes.copy_from_slice(&raw[start..]);
        return Ok(u64::from_be_bytes(bytes));
    }
    u64::from_str_radix(&trimmed(raw), 8).map_err(|_| {
        SigmfError::Malformed("the archive holds an entry with no readable size".to_owned())
    })
}

fn trimmed(field: &[u8]) -> String {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use num_complex::Complex;
    use tempfile::TempDir;

    use super::*;
    use crate::{Export, ExportKind, SigmfWriter, data_path};

    const TRAILER: usize = 2 * BLOCK;

    fn entries(dir: &Path) -> usize {
        std::fs::read_dir(dir).expect("dir").count()
    }

    impl Spooled {
        fn len(&self) -> u64 {
            self.open()
                .expect("spool")
                .metadata()
                .expect("metadata")
                .len()
        }
    }

    fn recorded(dir: &Path, name: &str) -> std::path::PathBuf {
        let stem = dir.join(name);
        let mut writer =
            SigmfWriter::create(&stem, 48_000.0, 100e6, "archive fixture").expect("create");
        writer
            .write_block(&[Complex::new(0.25f32, -0.5); 128])
            .expect("write");
        writer.finalize().expect("finalize");
        stem
    }

    #[test]
    fn an_archive_this_build_wrote_reads_back_whole() {
        let dir = TempDir::new().expect("temp");
        let stem = recorded(dir.path(), "round-trip");
        let mut export = Export::open(&stem, ExportKind::SigmfArchive).expect("export");
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut export, &mut bytes).expect("read");

        let spool = TempDir::new().expect("temp");
        let Archive::Pair(member) =
            read_archive(bytes.as_slice(), spool.path()).expect("reads back")
        else {
            panic!("one recording");
        };
        assert_eq!(member.name, "round-trip");
        assert_eq!(member.data.len(), 128 * 8);
        let mut spooled = Vec::new();
        std::io::Read::read_to_end(&mut member.data.open().expect("spool"), &mut spooled)
            .expect("read spool");
        assert_eq!(std::fs::read(data_path(&stem)).expect("source"), spooled);
        let meta: crate::SigmfMeta = serde_json::from_slice(&member.meta).expect("meta");
        assert_eq!(meta.global.sample_rate, Some(48_000.0));
        drop(member);
        assert_eq!(entries(spool.path()), 0, "an unlanded spool is removed");
    }

    #[test]
    fn a_crashed_import_leaves_no_spool_behind_and_a_running_one_keeps_its_own() {
        let dir = TempDir::new().expect("temp");
        let spool = |name: String| {
            let path = dir.path().join(name);
            std::fs::write(&path, b"iq").expect("spool");
            path
        };
        let other = spool(format!("{SPOOL_PREFIX}0-3{SPOOL_SUFFIX}"));
        let unclaimed = spool(format!(
            "{SPOOL_PREFIX}{}-{}{SPOOL_SUFFIX}",
            std::process::id(),
            u64::MAX
        ));
        let foreign = spool(format!("{SPOOL_PREFIX}notes{SPOOL_SUFFIX}"));
        let (running, _file) = claim_spool(dir.path()).expect("claim");

        let mut removed = sweep_spools(dir.path()).expect("swept");

        removed.sort();
        let mut expected = vec![other, unclaimed];
        expected.sort();
        assert_eq!(removed, expected);
        assert!(running.exists() && foreign.exists());
        assert!(
            sweep_spools(&dir.path().join("absent"))
                .expect("no library yet")
                .is_empty()
        );
    }

    #[test]
    fn a_member_longer_than_the_archive_is_refused_without_allocating_for_it() {
        let mut header = [0u8; BLOCK];
        header[..12].copy_from_slice(b"x.sigmf-data");
        header[124] = 0x80;
        header[128..136].copy_from_slice(&(1u64 << 48).to_be_bytes());
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(b"short");

        let spool = TempDir::new().expect("temp");
        let Err(error) = read_archive(bytes.as_slice(), spool.path()) else {
            panic!("a member longer than the archive must be refused");
        };
        assert!(
            matches!(&error, SigmfError::Malformed(why) if why == MEMBER_TOO_LARGE),
            "{error:?}"
        );
        assert_eq!(entries(spool.path()), 0, "a cut member leaves no spool");
    }

    #[test]
    fn a_collection_archive_reads_back_with_every_lane() {
        let dir = TempDir::new().expect("temp");
        let stem = crate::library::tests::collection(dir.path(), "bank", 3, 40);
        let mut export = Export::open(&stem, ExportKind::SigmfArchive).expect("export");
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut export, &mut bytes).expect("read");

        let spool = TempDir::new().expect("temp");
        let Archive::Collection(collection) =
            read_archive(bytes.as_slice(), spool.path()).expect("reads back")
        else {
            panic!("a collection");
        };
        assert_eq!(collection.name, "bank");
        assert_eq!(
            collection.collection,
            std::fs::read(crate::collection_path(&stem)).expect("collection")
        );
        let lanes: Vec<(&str, u64)> = collection
            .lanes
            .iter()
            .map(|lane| (lane.name.as_str(), lane.data.len()))
            .collect();
        assert_eq!(
            lanes,
            [
                ("bank-lane0", 320),
                ("bank-lane1", 320),
                ("bank-lane2", 320)
            ]
        );
    }

    #[test]
    fn two_recordings_without_a_collection_are_refused() {
        let dir = TempDir::new().expect("temp");
        let stem = crate::library::tests::collection(dir.path(), "bank", 2, 8);
        std::fs::remove_file(crate::collection_path(&stem)).expect("drop the collection");
        let mut bytes = Vec::new();
        for lane in 0..2 {
            let lane = crate::lane_stem(&stem, lane);
            let mut export = Export::open(&lane, ExportKind::SigmfArchive).expect("export");
            std::io::Read::read_to_end(&mut export, &mut bytes).expect("read");
            bytes.truncate(bytes.len() - TRAILER);
        }
        bytes.extend_from_slice(&[0u8; TRAILER]);
        let spool = TempDir::new().expect("temp");
        assert!(matches!(
            read_archive(bytes.as_slice(), spool.path()),
            Err(SigmfError::Malformed(_))
        ));
        assert_eq!(
            entries(spool.path()),
            0,
            "a refused archive leaves no spool"
        );
    }

    #[test]
    fn a_member_too_large_for_the_octal_size_field_is_still_read() {
        let mut header = [0u8; BLOCK];
        header[..12].copy_from_slice(b"huge.sigmf-d");
        header[12..17].copy_from_slice(b"ata\0\0");
        let size = 9_000_000_000u64;
        header[124] = 0x80;
        header[128..136].copy_from_slice(&size.to_be_bytes());
        assert_eq!(entry_size(&header).expect("base 256 size"), size);
    }

    #[test]
    fn an_archive_without_both_members_is_refused() {
        let spool = TempDir::new().expect("temp");
        assert!(matches!(
            read_archive([0u8; BLOCK * 2].as_slice(), spool.path()),
            Err(SigmfError::Malformed(_))
        ));
        assert!(matches!(
            read_archive(b"not a tar at all".as_slice(), spool.path()),
            Err(SigmfError::Malformed(_)) | Err(SigmfError::Io(_))
        ));
    }
}
