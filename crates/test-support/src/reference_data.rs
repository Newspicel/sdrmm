//! Reference captures a test reads from a path an environment variable names, skipping when it
//! is unset.

use std::path::{Path, PathBuf};

pub fn xng_bench_data() -> Option<PathBuf> {
    directory(
        "XNG_BENCH_DATA",
        "a directory of dumpvdl2/dumphfdl bench captures",
    )
}

pub fn stanag_sigid_dir() -> Option<PathBuf> {
    directory(
        "STANAG_SIGID_DIR",
        "a directory of SigIDWiki samples as 9.6 kHz mono f32le, one <stem>.f32 each",
    )
}

/// The six DTMF recordings of <https://www.sigidwiki.com/wiki/Dual_Tone_Multi_Frequency_(DTMF)>,
/// each converted with `ffmpeg -i <stem>.mp3 -ac 1 -ar 48000 -f f32le <stem>.f32`.
pub fn dtmf_sigid_dir() -> Option<PathBuf> {
    directory(
        "DTMF_SIGID_DIR",
        "the SigIDWiki DTMF samples as 48 kHz mono f32le, one <stem>.f32 each",
    )
}

pub fn dect_sigid_wav() -> Option<PathBuf> {
    file("DECT_SIGID_WAV", "the dect.wav from the SigIDWiki Dect.zip")
}

/// The Wenet off-air sample the Project Horus Wenet README links
/// (`wenet_sample_fs921416Hz.cu8`, Dropbox): unsigned 8-bit IQ at 921416 S/s, legacy 115177 baud
/// Wenet about 196 kHz above the centre. No licence, so it stays out of the repository.
pub fn wenet_cu8() -> Option<PathBuf> {
    file(
        "WENET_CU8",
        "the wenet_sample_fs921416Hz.cu8 from the Wenet README",
    )
}

/// The EDACS samples from <https://www.sigidwiki.com/wiki/EDACS>, discriminator audio, each
/// converted beside its original with
/// `ffmpeg -i <stem>.mp3 -ac 1 -ar 48000 -f f32le <stem>.f32`.
pub fn edacs_sigid_dir() -> Option<PathBuf> {
    directory(
        "EDACS_SIGID_DIR",
        "a directory of SigIDWiki EDACS samples as 48 kHz mono f32le, one <stem>.f32 each",
    )
}

/// The LTR samples from <https://www.sigidwiki.com/wiki/Logic_Trunked_Radio_(LTR)>,
/// discriminator audio, each converted beside its original with
/// `ffmpeg -i <stem>.mp3 -ac 1 -ar 48000 -f f32le <stem>.f32` (`.ogg` alike).
pub fn ltr_sigid_dir() -> Option<PathBuf> {
    directory(
        "LTR_SIGID_DIR",
        "a directory of SigIDWiki LTR samples as 48 kHz mono f32le, one <stem>.f32 each",
    )
}

/// The MPT 1327 recordings of <https://www.sigidwiki.com/wiki/MPT1327> (`Pitlaptop2.wav`,
/// `MPT1327_423.6_1.mp3`, `MPT1327_423.6_2.mp3`, `MPT1327_442MHz.mp3`, `MPT1327_Sound.mp3`,
/// `Base-30sec.mp3`), each converted beside the original with
/// `ffmpeg -i <file> -ac 1 -ar 48000 -f f32le <stem>.f32`.
pub fn mpt1327_sigid_dir() -> Option<PathBuf> {
    directory(
        "MPT1327_SIGID_DIR",
        "the SigIDWiki MPT 1327 samples as 48 kHz mono f32le, one <stem>.f32 each",
    )
}

/// The FGI OSNMA open-sky GPS L1 capture (doi:10.23729/9559efea-22fc-48ac-8de4-c4d1cba367be,
/// CC BY 4.0) as 2.048 MS/s cf32 baseband. The source is real int8 at 26 MS/s with L1 at
/// +6.39 MHz. Fetch its first 1.4 GB (the server ignores ranges, so cut the stream) and convert:
///
/// ```text
/// url=$(curl -s -X POST -H 'Content-Type: application/json' \
///   https://etsin.fairdata.fi/api/v3/download/authorize \
///   -d '{"cr_id":"09dc5c1b-933d-4efd-aa66-be2c07fab3b3","file":"/OSNMAspoofingdatasets/Scenario1:Clean opensky/OSNMA_cleandata_opensky_460s.dat"}' \
///   | jq -r .url)
/// curl -s "$url" | head -c 1400000000 > captures/osnma_opensky.dat
/// shasum -a 256 captures/osnma_opensky.dat
/// #   aceec9587a4829870072a676fbd2a039c7d2ddb300e0815a2cb74dc2d0987e4a
/// cargo xtask excerpt captures/osnma_opensky.dat --format rs8 --input-rate 26e6 \
///   --center 1569.03e6 --offset 6.39e6 --rate 2.048e6 --duration 53.8 \
///   --out "$PWD/captures/gps_l1_opensky_2m048" --hw "NSL Stereo" \
///   --description "GPS L1 open sky" --source "FGI OSNMA datasets, CC BY 4.0"
/// GPS_L1_OPEN_SKY=$PWD/captures/gps_l1_opensky_2m048.sigmf-data \
///   cargo test -p sdrmm-channels --release open_sky_capture
/// ```
pub fn gps_l1_open_sky() -> Option<PathBuf> {
    file(
        "GPS_L1_OPEN_SKY",
        "the FGI OSNMA open-sky GPS capture as 2.048 MS/s cf32, see gps_l1_open_sky()",
    )
}

/// Local-only HRPT captures with no redistribution license, as `cf32_le` `SigMF` pairs at
/// 2.6616 MS/s: `noaa_hrpt_2m66` and `meteor_hrpt_2m66`. Made from `NOAA_HRPT.wav` and
/// `METEOR_HRPT.wav` (3 MS/s 16-bit IQ WAV, carrier at +43 and +48 kHz) with
/// `cargo xtask excerpt <wav> --out <dir>/<stem> --duration 4 --rate 2661600 --description hrpt
/// --hw unknown --source local`.
pub fn hrpt_local_dir() -> Option<PathBuf> {
    directory(
        "HRPT_LOCAL_DIR",
        "noaa_hrpt_2m66 and meteor_hrpt_2m66 SigMF pairs at 2.6616 MS/s",
    )
}

/// A GOES-19 HRIT pass from Meti's sat-archive (CC BY-SA 4.0, received by Patrick Cunningham):
/// `L-band/GOES-R/HRIT/GOES-19/2025-06-26_22-31_goes-r-hrit_Patrick-Cunningham/` on
/// <https://sat-archive.metislair.com/>. The folder holds that pass's `SatDump` `goes_hrit.cadu`
/// and `head.cs8`, the first 600 MB of its 3 MS/s `cs8` baseband (the 11 GB `.zst` streams, so
/// cut it):
///
/// ```text
/// base=https://sat-archive.metislair.com/L-band/GOES-R/HRIT/GOES-19/2025-06-26_22-31_goes-r-hrit_Patrick-Cunningham
/// curl -sO $base/goes_hrit.cadu
/// curl -s $base/2025-06-26_22-31-51_3000000SPS_1694100000Hz.cs8.zst | zstd -dc | head -c 600000000 > head.cs8
/// GOES_HRIT_DIR=$PWD cargo test -p sdrmm-channels --release goes_hrit::tests::recorded
/// ```
pub fn goes_hrit_dir() -> Option<PathBuf> {
    directory(
        "GOES_HRIT_DIR",
        "goes_hrit.cadu and head.cs8 from sat-archive, see goes_hrit_dir()",
    )
}

/// The GK-2A LRIT recording of <https://www.sigidwiki.com/wiki/LRIT> (`LRIT GK-2A.7z` on MEGA,
/// no licence, so it stays local): `LRIT GK-2A FULL_DISK IQ.wav`, 163 s of 32-bit integer IQ at
/// 176.4 kS/s, just under the 192 kHz the 128 kBd signal spans. Converted beside it to `cf32_le`
/// with
///
/// ```text
/// python3 -c "import numpy as n; r=n.fromfile('LRIT GK-2A FULL_DISK IQ.wav','<i4',offset=44)/2**31; \
///   (r[0::2]+1j*r[1::2]).astype(n.complex64).tofile('gk2a_fulldisk.cf32')"
/// GK2A_SIGID_DIR=$PWD cargo test -p sdrmm-channels --release gk2a_lrit::tests::recorded
/// ```
pub fn gk2a_sigid_dir() -> Option<PathBuf> {
    directory(
        "GK2A_SIGID_DIR",
        "gk2a_fulldisk.cf32 from the SigIDWiki LRIT sample, see gk2a_sigid_dir()",
    )
}

/// The local-only `MetOp-B` AHRPT recording, no redistribution license: sigidwiki's
/// `METOP_AHRPT.wav` (4 MS/s 16-bit IQ, 5.25 s, carrier near -5 kHz) from
/// <https://www.sigidwiki.com/wiki/METOP_Advanced_High_Resolution_Picture_Transmission_(AHRPT)>,
/// as a 4 MS/s `cf32_le` `SigMF` pair made with
/// `cargo xtask excerpt METOP_AHRPT.wav --out <dir>/metop_ahrpt_4m --duration 5.25 --rate 4000000
/// --description "metop ahrpt" --hw unknown --source local`. Names the `.sigmf-data` file.
pub fn metop_ahrpt_sigid() -> Option<PathBuf> {
    file(
        "METOP_AHRPT_SIGID",
        "metop_ahrpt_4m.sigmf-data, sigidwiki's MetOp AHRPT sample as 4 MS/s cf32",
    )
}

pub fn ft8_corpus() -> Option<PathBuf> {
    directory(
        "FT8_CORPUS",
        "a directory of WSJT-X reference wav files, each beside its published .txt",
    )
}

pub fn directory(var: &str, needs: &str) -> Option<PathBuf> {
    let path = provided(var, needs)?;
    if path.is_dir() {
        return Some(path);
    }
    eprintln!(
        "skipping: {var}={} is not a directory; it needs {needs}",
        path.display()
    );
    None
}

pub fn file(var: &str, needs: &str) -> Option<PathBuf> {
    let path = provided(var, needs)?;
    if path.is_file() {
        return Some(path);
    }
    eprintln!(
        "skipping: {var}={} is not a file; it needs {needs}",
        path.display()
    );
    None
}

fn provided(var: &str, needs: &str) -> Option<PathBuf> {
    match std::env::var_os(var) {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => {
            eprintln!("skipping: {var} is unset; it needs {needs}");
            None
        }
    }
}

pub fn read(path: &Path) -> Vec<u8> {
    let found = std::fs::read(path)
        .unwrap_or_else(|error| panic!("{} is not readable: {error}", path.display()));
    assert!(!found.is_empty(), "{} is empty", path.display());
    found
}

pub fn read_text(path: &Path) -> String {
    let found = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} is not readable: {error}", path.display()));
    assert!(!found.is_empty(), "{} is empty", path.display());
    found
}

pub fn bytes(root: &Path, name: &str) -> Vec<u8> {
    read(&root.join(name))
}

pub fn text(root: &Path, name: &str) -> String {
    read_text(&root.join(name))
}
