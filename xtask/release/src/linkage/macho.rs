//! What a Mach-O image loads, read from its load commands. `otool` is no option: it takes a
//! path ending in `(name)`, like `SDRmm Helper (Alerts)`, for an archive member.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail, ensure};

const FAT: u32 = 0xcafe_babe;
const FAT_64: u32 = 0xcafe_babf;
const IMAGE_32: u32 = 0xfeed_face;
const IMAGE_64: u32 = 0xfeed_facf;
const LOAD_DYLIB: u32 = 0xc;
const LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
const REEXPORT_DYLIB: u32 = 0x8000_001f;
const LOAD_UPWARD_DYLIB: u32 = 0x8000_0023;
const RPATH: u32 = 0x8000_001c;

pub fn is_macho(head: &[u8; 4]) -> bool {
    let fat = [FAT, FAT_64].map(u32::to_be_bytes);
    let thin = [IMAGE_32, IMAGE_64];
    fat.contains(head)
        || thin
            .iter()
            .any(|magic| *head == magic.to_le_bytes() || *head == magic.to_be_bytes())
}

#[derive(Debug, Default, PartialEq)]
pub struct Image {
    pub dependencies: Vec<String>,
    pub rpaths: Vec<String>,
}

/// Every slice of a universal binary counts: each has to find its libraries.
pub fn read(data: &[u8]) -> Result<Image> {
    let mut dependencies = BTreeSet::new();
    let mut image = Image::default();
    for slice in slices(data)? {
        commands(slice, &mut dependencies, &mut image.rpaths)?;
    }
    image.dependencies = dependencies.into_iter().collect();
    Ok(image)
}

fn slices(data: &[u8]) -> Result<Vec<&[u8]>> {
    let fat = Bytes { data, big: true };
    let wide = match fat.u32(0)? {
        FAT => false,
        FAT_64 => true,
        _ => return Ok(vec![data]),
    };
    let stride = if wide { 32 } else { 20 };
    (0..fat.u32(4)? as usize)
        .map(|index| {
            let at = 8 + index * stride;
            let (offset, size) = if wide {
                (fat.u64(at + 8)?, fat.u64(at + 16)?)
            } else {
                (u64::from(fat.u32(at + 8)?), u64::from(fat.u32(at + 12)?))
            };
            let start = usize::try_from(offset)?;
            let end = start.saturating_add(usize::try_from(size)?);
            data.get(start..end)
                .with_context(|| format!("slice {index} runs past the file"))
        })
        .collect()
}

fn commands(
    data: &[u8],
    dependencies: &mut BTreeSet<String>,
    rpaths: &mut Vec<String>,
) -> Result<()> {
    let magic = Bytes { data, big: false }.u32(0)?;
    let (big, wide) = match magic {
        IMAGE_64 => (false, true),
        IMAGE_32 => (false, false),
        _ if magic.swap_bytes() == IMAGE_64 => (true, true),
        _ if magic.swap_bytes() == IMAGE_32 => (true, false),
        _ => bail!("no Mach-O magic: {magic:#x}"),
    };
    let image = Bytes { data, big };
    let mut at = if wide { 32 } else { 28 };
    for _ in 0..image.u32(16)? {
        let size = image.u32(at + 4)? as usize;
        ensure!(size >= 12, "load command at {at:#x} is only {size} bytes");
        let end = at + size;
        match image.u32(at)? {
            LOAD_DYLIB | LOAD_WEAK_DYLIB | REEXPORT_DYLIB | LOAD_UPWARD_DYLIB => {
                dependencies.insert(image.name(at, end)?);
            }
            RPATH => {
                let path = image.name(at, end)?;
                if !rpaths.contains(&path) {
                    rpaths.push(path);
                }
            }
            _ => {}
        }
        at = end;
    }
    Ok(())
}

struct Bytes<'a> {
    data: &'a [u8],
    big: bool,
}

impl Bytes<'_> {
    fn array<const N: usize>(&self, at: usize) -> Result<[u8; N]> {
        self.data
            .get(at..at.saturating_add(N))
            .and_then(|bytes| bytes.try_into().ok())
            .with_context(|| format!("offset {at:#x} lies past the end"))
    }

    fn u32(&self, at: usize) -> Result<u32> {
        let bytes = self.array(at)?;
        Ok(if self.big {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        })
    }

    fn u64(&self, at: usize) -> Result<u64> {
        let bytes = self.array(at)?;
        Ok(if self.big {
            u64::from_be_bytes(bytes)
        } else {
            u64::from_le_bytes(bytes)
        })
    }

    /// The string a dylib or rpath command carries, at the offset its third word names.
    fn name(&self, command: usize, end: usize) -> Result<String> {
        let start = command + self.u32(command + 8)? as usize;
        let bytes = self
            .data
            .get(start..end)
            .with_context(|| format!("load command at {command:#x} names a string outside it"))?;
        let len = bytes
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len());
        Ok(String::from_utf8_lossy(&bytes[..len]).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(kind: u32, name: &str) -> Vec<u8> {
        let mut text = name.as_bytes().to_vec();
        text.push(0);
        text.resize(text.len().next_multiple_of(8), 0);
        let offset = if kind == RPATH { 12u32 } else { 24 };
        let size = offset as usize + text.len();
        let mut bytes = Vec::new();
        for word in [kind, size as u32, offset] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.resize(offset as usize, 0);
        bytes.extend(text);
        bytes
    }

    fn image(commands: &[(u32, &str)]) -> Vec<u8> {
        let body: Vec<u8> = commands
            .iter()
            .flat_map(|(kind, name)| command(*kind, name))
            .collect();
        let mut bytes = Vec::new();
        for word in [
            IMAGE_64,
            0x0100_000c,
            0,
            6,
            commands.len() as u32,
            body.len() as u32,
            0,
            0,
        ] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend(body);
        bytes
    }

    fn universal(slices: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(FAT.to_be_bytes());
        bytes.extend((slices.len() as u32).to_be_bytes());
        let mut offset = 8 + 20 * slices.len();
        for slice in slices {
            for word in [0, 0, offset as u32, slice.len() as u32, 0] {
                bytes.extend(u32::to_be_bytes(word));
            }
            offset += slice.len();
        }
        for slice in slices {
            bytes.extend(slice);
        }
        bytes
    }

    const LOADS: [(u32, &str); 5] = [
        (0xd, "@rpath/libSoapySDR.0.8.dylib"),
        (LOAD_DYLIB, "/usr/lib/libSystem.B.dylib"),
        (LOAD_WEAK_DYLIB, "@rpath/libusb-1.0.0.dylib"),
        (RPATH, "@executable_path/../Frameworks"),
        (RPATH, "@loader_path/.."),
    ];

    #[test]
    fn reads_what_an_image_loads_and_ignores_its_own_name() {
        let read = read(&image(&LOADS)).unwrap();
        assert_eq!(
            read.dependencies,
            ["/usr/lib/libSystem.B.dylib", "@rpath/libusb-1.0.0.dylib"]
        );
        assert_eq!(
            read.rpaths,
            ["@executable_path/../Frameworks", "@loader_path/.."]
        );
    }

    #[test]
    fn dedupes_the_slices_of_a_universal_binary() {
        let bytes = universal(&[image(&LOADS), image(&LOADS)]);
        assert!(is_macho(&bytes[..4].try_into().unwrap()));
        let read = read(&bytes).unwrap();
        assert_eq!(read.dependencies.len(), 2);
        assert_eq!(read.rpaths.len(), 2);
    }

    #[test]
    fn a_truncated_image_is_an_error() {
        let bytes = image(&LOADS);
        assert!(read(&bytes[..bytes.len() - 40]).is_err());
    }

    #[test]
    fn only_mach_o_heads_count() {
        assert!(is_macho(&IMAGE_64.to_le_bytes()));
        assert!(!is_macho(b"MZ\x90\0"));
        assert!(!is_macho(b"\x7fELF"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_a_system_binary() {
        let read = read(&std::fs::read("/bin/ls").unwrap()).unwrap();
        assert!(
            read.dependencies
                .iter()
                .any(|dep| dep == "/usr/lib/libSystem.B.dylib")
        );
    }
}
