use num_complex::Complex;
use sdrmm_decoder::signal_catalog;

fn power(iq: &[Complex<f32>]) -> f64 {
    if iq.is_empty() {
        return 0.0;
    }
    iq.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>() / iq.len() as f64
}

#[test]
fn every_signal_renders_something_at_its_own_rate() {
    for signal in signal_catalog().signals.iter() {
        let iq = signal
            .rendered()
            .unwrap_or_else(|error| panic!("{}: {error}", signal.id));
        assert!(
            iq.len() >= 64,
            "{} rendered {} samples",
            signal.id,
            iq.len()
        );
        assert!(power(&iq) > 1e-9, "{} rendered silence", signal.id);
    }
}

#[test]
fn signal_ids_and_labels_are_unique() {
    let signals = &signal_catalog().signals;
    let mut ids: Vec<&str> = signals.iter().map(|signal| signal.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), signals.len(), "signal ids must be unique");

    let mut labels: Vec<&str> = signals.iter().map(|signal| signal.label).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), signals.len(), "signal labels must be unique");
}

#[test]
fn the_generator_starts_on_voice_with_a_ctcss_tone() {
    let catalog = signal_catalog();
    let default = catalog.default_signal().expect("a default signal");
    assert_eq!(default.id, "nfm_ctcss");
    assert!(catalog.rates().contains(&default.rate_hz));
}

#[test]
fn the_generator_offers_every_signal_the_decoders_bring() {
    let mut ids: Vec<&str> = signal_catalog().signals.iter().map(|s| s.id).collect();
    ids.sort_unstable();
    let mut expected = vec![
        "tone",
        "two_tone",
        "noise",
        "sweep",
        "nfm_voice",
        "nfm_ctcss",
        "nfm_dcs",
        "am_voice",
        "ssb_voice",
        "wfm_stereo",
        "wfm_rds",
        "pocsag",
        "flex",
        "ermes",
        "eot",
        "same",
        "sarsat",
        "acars",
        "aprs",
        "aprs_weather",
        "bss",
        "ais",
        "adsb",
        "mode_s",
        "morse",
        "rtty",
        "navtex",
        "psk31",
        "selcall",
        "ft8",
        "ft4",
        "wspr",
        "sstv",
        "apt",
        "lrpt",
        "wefax",
        "radiosonde",
        "vor",
        "dcf77",
        "dmr",
        "p25",
        "nxdn",
        "tetrapol",
        "dstar",
        "m17",
        "ysf",
        "tetra",
        "dect",
        "meshtastic",
        "meshcore",
        "lorawan",
        "remote_id_bluetooth",
        "remote_id_wifi",
        "ble_beacons",
        "ism_24",
        "per_frames",
        "noise_source",
        "vlf_stations",
        "gnss",
        "dab",
        "dvbs",
        "dvbs2",
        "dvbt",
        "atv",
    ];
    expected.sort_unstable();
    assert_eq!(ids, expected);
}
