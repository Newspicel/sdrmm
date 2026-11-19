use super::open_sky::off_survey_m;
use super::*;
use sdrmm_decoder_test_support::{complex_noise, realtime_budget};
use sdrmm_dsp::math::geodesy::Geodetic;
use std::io::Read;

fn events<T: EventContract + serde::de::DeserializeOwned>(out: &ChannelOutputs) -> Vec<T> {
    out.events
        .iter()
        .filter_map(DecoderEvent::read::<T>)
        .collect()
}

fn sky(satellites: &[(u8, f64, f64)], noise: f32, ms: usize) -> Vec<Complex<f32>> {
    let mut iq = complex_noise(11, noise, ms * SAMPLES_PER_MS);
    for &(prn, doppler, start) in satellites {
        let signal = synth::satellite(prn, doppler, start, ms);
        for (sample, add) in iq.iter_mut().zip(signal) {
            *sample += add * 0.05;
        }
    }
    iq
}

#[test]
fn a_clean_satellite_is_acquired_with_its_doppler_and_code_phase() {
    let iq = synth::acquisition(7, 1_000.0, 317, 400);
    let mut channel = GnssChannel::build(GnssParams::default());
    let mut out = ChannelOutputs::default();
    channel.process(&iq, &mut out);
    let frames: Vec<GnssFrame> = events(&out);
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0].prn, 7);
    assert_eq!(frames[0].doppler_hz, 1_000.0);
    assert!(
        (frames[0].code_phase_chips - 158.34).abs() < 0.05,
        "{frames:?}"
    );
    assert!(frames[0].cn0_db_hz > 45.0);
}

#[test]
fn several_satellites_are_tracked_at_once() {
    let satellites = [
        (3, -3_000.0, 100.0),
        (11, 1_500.0, 1_900.5),
        (22, 4_000.0, 777.2),
        (31, -500.0, 1_234.7),
    ];
    let iq = sky(&satellites, 0.5, 1_500);
    let mut channel = GnssChannel::build(GnssParams::default());
    let mut out = ChannelOutputs::default();
    channel.process(&iq, &mut out);
    let mut prns: Vec<u8> = channel.trackers.iter().map(|t| t.prn).collect();
    prns.sort_unstable();
    assert_eq!(prns, [3, 11, 22, 31]);
    for (tracker, &(_, doppler, _)) in sorted(&channel.trackers).iter().zip(&satellites) {
        assert!(
            (tracker.doppler_hz() - doppler).abs() < 5.0,
            "PRN {} at {} Hz",
            tracker.prn,
            tracker.doppler_hz()
        );
        assert!(
            tracker.cn0_db_hz > 40.0,
            "PRN {} {}",
            tracker.prn,
            tracker.cn0_db_hz
        );
    }
}

fn sorted(trackers: &[Tracker]) -> Vec<&Tracker> {
    let mut sorted: Vec<&Tracker> = trackers.iter().collect();
    sorted.sort_by_key(|t| t.prn);
    sorted
}

#[test]
fn a_clean_signal_from_another_satellite_acquires_nothing_else() {
    let iq = synth::acquisition(7, 1_000.0, 317, 400);
    let mut channel = GnssChannel::build(GnssParams::default());
    let mut out = ChannelOutputs::default();
    channel.process(&iq, &mut out);
    let prns: Vec<u8> = channel.trackers.iter().map(|t| t.prn).collect();
    assert_eq!(prns, [7]);
}

#[test]
fn noise_acquires_no_satellite() {
    let noise = complex_noise(5, 0.5, 600 * SAMPLES_PER_MS);
    let mut channel = GnssChannel::build(GnssParams::default());
    let mut out = ChannelOutputs::default();
    channel.process(&noise, &mut out);
    assert!(out.events.is_empty(), "{:?}", out.events.first());
}

#[test]
fn search_and_twelve_trackers_stay_ahead_of_realtime() {
    let iq = vec![Complex::default(); RATE as usize];
    let mut channel = GnssChannel::build(GnssParams::default());
    for prn in 1..=MAX_SATELLITES as u8 {
        channel.trackers.push(Tracker::new(
            Found {
                prn,
                doppler_hz: 1_000.0,
                code_start: 0.0,
                cn0_db_hz: 45.0,
            },
            0,
        ));
    }
    let mut output = ChannelOutputs::default();
    let started = std::time::Instant::now();
    channel.process(&iq, &mut output);
    let tracking = started.elapsed().as_secs_f64();
    assert!(
        tracking < realtime_budget(1.0),
        "one second of twelve trackers took {tracking:.3} s"
    );
    let mut channel = GnssChannel::build(GnssParams::default());
    let started = std::time::Instant::now();
    channel.process(&iq, &mut output);
    let searching = started.elapsed().as_secs_f64();
    assert!(
        searching < realtime_budget(1.0),
        "one second of sky search took {searching:.3} s"
    );
}

fn stream(path: &std::path::Path, mut each: impl FnMut(&[Complex<f32>])) {
    let mut file = std::io::BufReader::new(std::fs::File::open(path).expect("readable capture"));
    let mut bytes = vec![0u8; 8 * 65_536];
    loop {
        let mut filled = 0;
        while filled < bytes.len() {
            match file.read(&mut bytes[filled..]).expect("readable capture") {
                0 => break,
                got => filled += got,
            }
        }
        if filled < 8 {
            return;
        }
        each(&sdrmm_decoder_test_support::cf32_le(
            &bytes[..filled - filled % 8],
        ));
    }
}

#[test]
fn an_open_sky_excerpt_acquires_and_tracks_five_satellites() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/gps_l1_opensky_2m048.sigmf-data"
    );
    let mut channel = GnssChannel::build(GnssParams::default());
    let mut out = ChannelOutputs::default();
    stream(std::path::Path::new(path), |iq| {
        channel.process(iq, &mut out);
    });
    let acquired: std::collections::BTreeSet<u8> =
        events::<GnssFrame>(&out).iter().map(|f| f.prn).collect();
    for prn in [5, 7, 13, 20, 30] {
        assert!(
            acquired.contains(&prn),
            "PRN {prn} missing from {acquired:?}"
        );
        let tracker = channel
            .trackers
            .iter()
            .find(|t| t.prn == prn)
            .unwrap_or_else(|| panic!("PRN {prn} lost"));
        assert!(
            tracker.cn0_db_hz > 40.0,
            "PRN {prn} at {} dB-Hz",
            tracker.cn0_db_hz
        );
    }
}

/// Runs the whole open-sky capture, about 54 s, and checks every fix against the survey.
#[test]
fn an_open_sky_capture_fixes_within_thirty_metres_of_the_survey() {
    let Some(path) = sdrmm_test_support::reference_data::gps_l1_open_sky() else {
        return;
    };
    let mut channel = GnssChannel::build(GnssParams::default());
    let mut out = ChannelOutputs::default();
    stream(&path, |iq| channel.process(iq, &mut out));
    let fixes: Vec<GnssFix> = events(&out);
    assert!(fixes.len() >= 10, "{} fixes", fixes.len());
    for fix in &fixes {
        let (horizontal, up) = off_survey_m(Geodetic {
            lat_deg: fix.lat,
            lon_deg: fix.lon,
            alt_m: fix.altitude_m,
        });
        assert!(horizontal < 30.0, "{horizontal:.1} m off: {fix:?}");
        assert!(up.abs() < 30.0, "{up:.1} m high: {fix:?}");
        assert!(fix.satellites.len() >= 8 && fix.pdop < 3.0, "{fix:?}");
    }
}
