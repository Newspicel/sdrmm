use std::collections::HashMap;

use super::*;
use sdrmm_modem::ccsds::{CADU_BYTES, VCDU_BYTES};
use sdrmm_test_support::reference_data;

const BLOCK: usize = 1 << 16;
const ASM_BYTES: usize = 4;
const RECORDED_RATE: f64 = 3_000_000.0;
const RECORDED_SECONDS: f64 = 70.0;
const ADMIN: &str = "GOES_EAST_Admin_message_updated_2025.04.07.lrit";
const FULL_DISK_13: &str =
    "OR_ABI-L2-CMIPF-M6C13_G19_s20251772230205_e20251772239525_c20251772239580.lrit";

/// The first seconds of the 3 MS/s `cs8` baseband, brought to the channel rate.
fn baseband(dir: &std::path::Path) -> Vec<Complex<f32>> {
    let bytes = reference_data::bytes(dir, "head.cs8");
    let take = (2.0 * RECORDED_RATE * RECORDED_SECONDS) as usize;
    let samples: Vec<Complex<f32>> = bytes[..take.min(bytes.len())]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[i, q]| Complex::new(f32::from(i as i8) / 128.0, f32::from(q as i8) / 128.0))
        .collect();
    let mut ddc = sdrmm_dsp::Ddc::new(RECORDED_RATE, INPUT_RATE_HZ, 0.0).expect("resampler");
    let mut out = Vec::with_capacity(samples.len());
    let mut block = Vec::new();
    for chunk in samples.chunks(BLOCK) {
        ddc.process(chunk, &mut block);
        out.extend_from_slice(&block);
    }
    out
}

/// Every file of `SatDump`'s CADUs, as this decoder's file layer reassembles them.
fn satdump_files(dir: &std::path::Path) -> (Received, sdrmm_modem::xrit::LinkStats) {
    let cadus = reference_data::bytes(dir, "goes_hrit.cadu");
    let mut link = link().expect("link");
    let mut pictures = Pictures::new();
    let mut out = ChannelOutputs::default();
    let mut received = Received::default();
    for cadu in cadus.as_chunks::<CADU_BYTES>().0 {
        out.reset();
        link.push_vcdu(&cadu[ASM_BYTES..ASM_BYTES + VCDU_BYTES], &mut |file| {
            pictures.take(&file, GoesPicture::All, &mut out);
        });
        received.images.append(&mut out.images);
        for event in out.events.drain(..) {
            if let Some(picture) = event.read::<GoesHritImage>() {
                received.pictures.push(picture);
            } else if let Some(file) = event.read::<BroadcastData>() {
                received.files.push(file);
            }
        }
    }
    (received, link.stats())
}

#[test]
fn satdumps_cadus_give_every_file_and_picture() {
    let Some(dir) = reference_data::goes_hrit_dir() else {
        return;
    };
    let (received, stats) = satdump_files(&dir);
    assert_eq!(stats.session.files, 1_535);
    assert_eq!(stats.session.packets_failed, 0);
    assert_eq!(stats.session.lines_failed, 0);
    assert_eq!(received.files.len(), 1_371);
    assert_eq!(received.pictures.len(), 23);
    let admin = received
        .files
        .iter()
        .find(|file| file.name == ADMIN)
        .expect("the admin message");
    assert_eq!(admin.media_type, "text/plain");
    assert_eq!(admin.bytes.len(), 1_817);
    assert!(
        admin
            .bytes
            .starts_with(b"The GOES-19 HRIT/EMWIN East broadcast utilizes virtual channels")
    );
    let index = received
        .pictures
        .iter()
        .position(|picture| picture.name == FULL_DISK_13)
        .expect("band 13 full disk");
    let picture = &received.pictures[index];
    assert_eq!(picture.satellite.as_deref(), Some("GOES-19"));
    assert_eq!((picture.segments, picture.segments_expected), (16, 16));
    assert_eq!((picture.width, picture.lines), (1_356, 1_356));
    assert_eq!(picture.lines_missing, 0);
    assert!(picture.complete);
    let luma = &received.images[index].picture.luma;
    assert_eq!(luma[0], 0, "space in the corner");
    let centre = &luma[678 * 1_356 + 600..678 * 1_356 + 760];
    let mean = centre.iter().map(|&v| f64::from(v)).sum::<f64>() / centre.len() as f64;
    assert!((20.0..230.0).contains(&mean), "disc mean {mean}");
    assert!(centre.iter().all(|&v| v > 0), "the disc has no holes");
}

#[test]
fn the_off_air_baseband_decodes_as_satdump_does() {
    let Some(dir) = reference_data::goes_hrit_dir() else {
        return;
    };
    let iq = baseband(&dir);
    let mut chan = channel(GoesPicture::All);
    let mut received = Received::default();
    run(&mut chan, &iq, &[BLOCK], &mut received);
    let stats = chan.link.stats();
    assert!(stats.frames >= 3_900, "{stats:?}");
    assert!(stats.frames_failed <= 10, "{stats:?}");
    assert_eq!(stats.session.files_lost, 0);
    let (reference, _) = satdump_files(&dir);
    let known: HashMap<&str, &BroadcastData> = reference
        .files
        .iter()
        .map(|file| (file.name.as_str(), file))
        .collect();
    assert!(received.files.len() >= 50, "{} files", received.files.len());
    for file in &received.files {
        let satdump = known.get(file.name.as_str()).expect("SatDump has it too");
        assert_eq!(satdump.bytes, file.bytes, "{}", file.name);
    }
    let bulletin = received
        .files
        .iter()
        .find(|file| file.name == "A_WWUS51KPHI262231_C_KWIN_20250626223153_428265-1-SVSPHIPA.lrit")
        .expect("the Philadelphia statement");
    assert_eq!(bulletin.media_type, "text/plain");
    assert_eq!(bulletin.bytes.len(), 944);
}
