use num_complex::Complex;
use sdrmm_wire::{
    AcarsParams, AdsbParams, AirtimeSpan, AisParams, AmParams, AprsMode, AprsParams, AptParams,
    AtvParams, AvhrrChannel, BleLink, BleParams, BroadcastStatus, ChannelParams, ChannelSettings,
    DabParams, DabTransmissionMode, DatvParams, DatvStandard, DecoderEvent, DectParams, DpmrParams,
    DrmMode, DrmParams, DscParams, DstarParams, DvbtParams, EotArming, EotBattery, EotParams,
    EotStatus, ErmesParams, EventKind, FlexParams, FreeDvParams, GnssParams, GsmParams,
    IdentParams, IlsParams, InmarsatAeroParams, IsmParams, IsmSurveyParams, LoraParams, LrptMode,
    LrptParams, M17Params, MorseParams, NavtexParams, NfmParams, NfmToneMode, NoiseFigureParams,
    NxdnParams, P25Params, PerParams, PocsagParams, PskBaud, PskParams, RadioClockParams,
    RadiosondeParams, RemoteIdLink, RemoteIdParams, RttyParams, SameParams, SarsatParams,
    SelcallParams, SelcallSystem, SidParams, SondeType, SsbParams, SstvMode, SstvParams,
    StanagParams, TetraParams, TetrapolParams, VorParams, WefaxIoc, WefaxLpm, WefaxParams,
    WfmParams, WifiOccupancyParams, WsjtParams, WsprParams, YsfParams,
};

use crate::{
    AprsTx, ChannelCtx, ChannelOutputs, ChannelRx, ChannelTx, Ft4Channel, Ft8Channel, TxPayload,
    WsprChannel,
    airtime::scene::Scene,
    channel_filter, create, descriptors, input_rate, synth,
    synth::{dv, weak_signal},
    testutil::{cf32_le, complex_noise, settings},
};

const CHUNK: usize = 4_096;
const BLE_38_HZ: f64 = 2_426e6;
const BLE_37_HZ: f64 = 2_402e6;
const WIFI_6_HZ: f64 = 2_437e6;
const GSM_ARFCN_725_HZ: f64 = 1_847_800_000.0;
const SID_CENTRE_HZ: f64 = 30_000.0;
const APRS_POSITION_REPORT: &str = "!5230.00N/01324.00E>SDR FIXTURE";
const GNSS_FIXTURE_PRN: u8 = 7;
const DVBT_SYMBOLS: usize = 1_024;
const NXDN_RAN: u8 = 1;
const NXDN_SOURCE: u16 = 1_001;
const NXDN_DESTINATION: u16 = 2_002;

#[derive(Default)]
struct Heard {
    kinds: Vec<EventKind>,
    events: Vec<DecoderEvent>,
    broadcasts: Vec<BroadcastStatus>,
    audio_samples: usize,
    pictures: usize,
}

impl Heard {
    fn from_events(events: &[DecoderEvent]) -> Self {
        Self {
            kinds: events.iter().map(DecoderEvent::kind).collect(),
            events: events.to_vec(),
            ..Self::default()
        }
    }

    fn take(&mut self, out: &mut ChannelOutputs) {
        for event in out.events.drain(..) {
            self.kinds.push(event.kind());
            self.events.push(event);
        }
        if let Some(broadcast) = out.broadcast.take() {
            self.kinds.push(EventKind::Broadcast);
            self.broadcasts.push(broadcast);
        }
        self.audio_samples += out.audio_pcm.len();
        self.pictures += out.video.len() + out.images.len();
    }

    fn rendered(&self) -> String {
        let events = self.events.iter().map(|event| {
            let view = event.present();
            let fields = view
                .fields
                .iter()
                .map(|field| format!("{}={:?}", field.label, field.value))
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "{} | {} | {} | {} | {} | {}",
                serde_json::to_string(event).unwrap_or_default(),
                view.summary,
                view.headline.unwrap_or_default(),
                view.entity.unwrap_or_default(),
                view.text.unwrap_or_default(),
                fields
            )
        });
        let broadcasts = self
            .broadcasts
            .iter()
            .map(|status| serde_json::to_string(status).unwrap_or_default());
        events.chain(broadcasts).collect::<Vec<_>>().join("\n")
    }

    fn places(&self) -> Vec<(f64, f64)> {
        self.events
            .iter()
            .filter_map(|event| event.present().place)
            .map(|place| (place.at.lat, place.at.lon))
            .collect()
    }
}

struct Payload {
    type_id: &'static str,
    carries: &'static [&'static str],
    at: Option<(f64, f64)>,
    audio_samples: usize,
    pictures: usize,
}

const fn reads(type_id: &'static str, carries: &'static [&'static str]) -> Payload {
    Payload {
        type_id,
        carries,
        at: None,
        audio_samples: 0,
        pictures: 0,
    }
}

impl Payload {
    const fn at(self, lat: f64, lon: f64) -> Self {
        Self {
            at: Some((lat, lon)),
            ..self
        }
    }

    const fn audio(self, audio_samples: usize) -> Self {
        Self {
            audio_samples,
            ..self
        }
    }

    const fn pictures(self, pictures: usize) -> Self {
        Self { pictures, ..self }
    }

    fn proves_a_decode(&self) -> bool {
        !self.carries.is_empty() || self.audio_samples > 0 || self.pictures > 0
    }
}

const PLACE_TOLERANCE_DEG: f64 = 0.001;

const PAYLOADS: &[Payload] = &[
    reads("am", &[]).audio(16_000),
    reads("ssb", &[]).audio(16_000),
    reads("atv", &[]).pictures(1),
    reads("nfm", &[r#""ctcss_hz":88.5"#, r#""open":true"#]).audio(16_000),
    reads(
        "pocsag",
        &[
            r#""address":1234567"#,
            r#""function":3"#,
            r#""baud":1200"#,
            r#""text":"SDR FIXTURE""#,
        ],
    ),
    reads(
        "flex",
        &[
            r#""address":1234567"#,
            r#""cycle":4"#,
            r#""frame":72"#,
            r#""text":"SDR FIXTURE""#,
        ],
    ),
    reads(
        "ermes",
        &[
            r#""local_address":234567"#,
            r#""message_number":3"#,
            r#""urgent":true"#,
            r#""alert":5"#,
            r#""text":"ERMES ALPHA PAGE""#,
        ],
    ),
    reads(
        "eot",
        &[
            r#""unit_address":24680"#,
            r#""arming":"normal""#,
            r#""pressure_psig":90"#,
            r#""battery_charge_pct":76"#,
        ],
    ),
    reads(
        "sarsat",
        &[
            r#""identity":"MMSI 211406406""#,
            r#""beacon":"epirb""#,
            r#""country":211"#,
        ],
    )
    .at(54.3233, 10.1233),
    reads(
        "acars",
        &[
            r#""registration":"D-AIBL""#,
            r#""flight":"LH0400""#,
            r#""label":"H1""#,
            r#""block_id":"4""#,
            r#""text":"SDR FIXTURE""#,
        ],
    ),
    reads(
        "aprs",
        &[
            r#""source":"DL0SDR""#,
            r#""destination":"APRS""#,
            r#""info":"!5230.00N/01324.00E>SDR FIXTURE""#,
        ],
    )
    .at(52.5, 13.4),
    reads(
        "ais",
        &[
            r#""mmsi":244010000"#,
            r#""heading_deg":88"#,
            r#""nav_status":0"#,
        ],
    )
    .at(52.3702, 4.8952),
    reads(
        "adsb",
        &[
            r#""icao":"3C65AC""#,
            r#""callsign":"DLH123""#,
            r#""vertical_rate_fpm":-1216"#,
        ],
    ),
    reads(
        "morse",
        &[
            r#""text":"CQ""#,
            r#""text":" CQ""#,
            r#""text":" DE""#,
            r#""text":" SDR""#,
            r#""text":" K""#,
        ],
    ),
    reads("rtty", &[r#""text":"CQ CQ DE DL1ABC K""#]),
    reads(
        "navtex",
        &[
            r#""station":"F""#,
            r#""subject":"A""#,
            r#""serial":1"#,
            r#""text":"SDR FIXTURE""#,
            r#""complete":true"#,
        ],
    ),
    reads("psk", &[r#""baud":"psk31""#, r#""text":"CQ CQ DE SDR \n""#]),
    reads("selcall", &[r#""system":"ccir1""#, r#""code":"12234""#]),
    reads("ft8", &[r#""text":"CQ W1AW FN42""#]),
    reads("ft4", &[r#""text":"CQ JA1ABC PM95""#]),
    reads(
        "wspr",
        &[
            r#""callsign":"K1ABC""#,
            r#""grid":"FN42""#,
            r#""power_dbm":37"#,
        ],
    ),
    reads(
        "sstv",
        &[
            r#""mode":"martin_m1""#,
            r#""width":320"#,
            r#""height":256"#,
            r#""complete":true"#,
        ],
    ),
    reads(
        "apt",
        &[
            r#""lines":240"#,
            r#""channel_a":"ch2""#,
            r#""channel_b":"ch4""#,
            r#""complete":true"#,
        ],
    ),
    reads(
        "lrpt",
        &[
            r#""mode":"oqpsk72""#,
            r#""apids":[64,65,66]"#,
            r#""complete":true"#,
        ],
    ),
    reads(
        "wefax",
        &[
            r#""ioc":"ioc576""#,
            r#""lpm":"lpm120""#,
            r#""lines":120"#,
            r#""complete":true"#,
        ],
    ),
    reads(
        "same",
        &[
            r#""originator":"WXR""#,
            r#""event":"TOR""#,
            r#""station":"KEC55/NW""#,
            r#""valid_minutes":30"#,
            r#""name":"Harris, TX""#,
            r#""copies":2"#,
        ],
    ),
    reads(
        "radiosonde",
        &[
            r#""sonde":"rs41""#,
            r#""serial":"W3420567""#,
            r#""satellites":9"#,
            r#""battery_v":2.9"#,
        ],
    )
    .at(48.1235, 11.5432),
    reads("vor", &["123.1\u{b0} radial"]),
    reads("ils", &[r#""component":"localizer""#, "+0.160 DDM"]),
    reads(
        "radio_clock",
        &[
            r#""standard":"dcf77""#,
            r#""datetime":"2026-10-03T22:04:00+02:00""#,
            r#""dst":true"#,
        ],
    ),
    reads(
        "dmr",
        &[
            r#""color_code":1"#,
            r#""source":2621001"#,
            r#""destination":505"#,
            r#""crc_verified":true"#,
        ],
    ),
    reads(
        "p25",
        &[
            r#""color_code":659"#,
            r#""source":11259375"#,
            r#""destination":4609"#,
            r#""message_indicator":"0123456789abcdef12""#,
        ],
    ),
    reads(
        "nxdn",
        &[
            r#""color_code":1"#,
            r#""source":1001"#,
            r#""destination":2002"#,
            r#""opcode":"voice call""#,
        ],
    ),
    reads(
        "dstar",
        &[
            r#""source_call":"DL1ABC""#,
            r#""destination_call":"CQCQCQ""#,
            r#""via":"DB0ABC B""#,
            r#""text":"hello from SDRmm""#,
        ],
    ),
    reads(
        "m17",
        &[
            r#""source_call":"DL1ABC""#,
            r#""destination_call":"ALL""#,
            r#""opcode":"stream""#,
        ],
    ),
    reads(
        "ysf",
        &[
            r#""source_call":"DL1ABC""#,
            r#""destination_call":"ALL""#,
            "\"via\":\"DB0XYZ \u{2192} DB0ABC\"",
            r#""destination":7"#,
        ],
    ),
    reads(
        "dpmr",
        &[
            r#""color_code":37"#,
            r#""source":1193046"#,
            r#""destination":65535"#,
        ],
    ),
    reads("freedv", &[r#""mode":"freedv""#, r#""opcode":"1600""#]),
    reads(
        "tetrapol",
        &[
            r#""group":393"#,
            r#""channel":2890"#,
            r#""country":3"#,
            r#""bs":7"#,
        ],
    ),
    reads(
        "tetra",
        &[
            r#""color_code":17"#,
            r#""network_id":1010"#,
            r#""system_id":262"#,
            r#""site_id":4242"#,
            r#""text":"Einsatz Hbf""#,
        ],
    ),
    reads(
        "wfm",
        &[
            r#""pi":"D3C2""#,
            r#""ps":"SDR-M4""#,
            r#""radiotext":"fixture""#,
            r#""pty":10"#,
        ],
    )
    .audio(16_000),
    reads(
        "lora",
        &[
            r#""spreading_factor":7"#,
            r#""coding_rate":"4/8""#,
            r#""integrity":"crc_ok""#,
            r#""payload":"deadbeef""#,
        ],
    ),
    reads(
        "remote_id",
        &[
            r#""uas_id":"1581F5FJD239C00DW22E""#,
            r#""ua_type":"rotorcraft""#,
        ],
    )
    .at(52.5163, 13.3777),
    reads(
        "ble",
        &[
            r#""name":"Greenhouse""#,
            r#""company":"Apple""#,
            r#""url":"https://sdrmm.com""#,
        ],
    ),
    reads(
        "ism",
        &[
            r#""model":"Nexus-TH""#,
            r#""id":"181""#,
            r#""temperature_c":19.0"#,
            r#""humidity_pct":71"#,
        ],
    ),
    reads("ism_survey", &[r#""kind":"wifi""#]),
    reads(
        "wifi_occupancy",
        &[r#""number":6"#, r#""band":"ghz2_4""#, "ch 6 30%"],
    ),
    reads(
        "per",
        &[r#""per":0.0"#, r#""missed":0"#, r#""bit_errors":0"#],
    ),
    reads("noise_figure", &["Y 10.00 dB"]),
    reads("sid", &["GQD 42 dB", "DHO38 48 dB", "NAA 38 dB"]),
    reads(
        "dect",
        &[
            r#""rfpi":"0123456780""#,
            r#""carrier":3"#,
            r#""rf_carriers":1023"#,
            r#""pscn":5"#,
            r#""authentication_supported":true"#,
        ],
    ),
    reads(
        "gnss",
        &[
            r#""prn":7"#,
            r#""doppler_hz":1000.0"#,
            r#""code_phase_chips":158.3"#,
        ],
    ),
    reads(
        "gsm",
        &[r#""arfcn":725"#, r#""bsic":48"#, r#""value":"015C5703""#],
    ),
    reads(
        "dab",
        &[
            r#""service_id":49569"#,
            r#""name":"slide.png""#,
            r#""media_type":"image/png""#,
        ],
    ),
    reads("datv", &[r#""protocol":2048"#, r#""name":"GSE-1-0.bin""#]),
    reads(
        "dvbt",
        &[
            r#""locked":true"#,
            r#""label":"Rust TV""#,
            r#""ensemble_label":"SDRmm""#,
        ],
    ),
    reads(
        "drm",
        &[
            r#""locked":true"#,
            r#""system":"drm30""#,
            r#""label":"Rust Wave""#,
        ],
    ),
    reads("cw_skimmer", &["DL1AAA", "20 WPM"]),
    reads("ident", &[r#""type_id":"pocsag""#, r#""confirmed":true"#]),
    reads(
        "dsc",
        &[
            r#""station":"002191000""#,
            r#""to":"231700000""#,
            r#""category":"safety""#,
            r#""crc_ok":true"#,
        ],
    ),
    reads(
        "stanag",
        &[r#""modem":"stanag_4285""#, r#""confidence":1.0"#],
    ),
    reads(
        "inmarsat_aero",
        &[r#""station":"HL8217""#, r#""tail":"HL8217""#],
    ),
    reads(
        "inmarsat_stdc",
        &[r#""region":"AOR-W""#, r#""frame_number":1000"#],
    ),
    reads(
        "vdl2",
        &[
            r#""station":"VT-ANB""#,
            r#""flight":"AI0142""#,
            r#""label":"B6""#,
        ],
    )
    .at(52.0402, 19.8039),
    reads(
        "hfdl",
        &[r#""gs_name":"Shannon, Ireland""#, r#""frame_index":1234"#],
    ),
    reads(
        "iridium",
        &[r#""sat":44"#, r#""beam":25"#, r#""tmsi":"071ca54a""#],
    )
    .at(40.1088, -127.2933),
    reads(
        "wmbus",
        &[
            r#""manufacturer":"SEN""#,
            r#""id":"33225544""#,
            r#""device_type":7"#,
            r#""value":123.529"#,
        ],
    ),
];

pub(crate) struct Fixture {
    settings: ChannelSettings,
    iq: Vec<Complex<f32>>,
}

impl Fixture {
    pub(crate) fn new(params: ChannelParams, iq: Vec<Complex<f32>>) -> Self {
        Self {
            settings: settings(params),
            iq,
        }
    }

    fn tuned(mut self, frequency_hz: f64) -> Self {
        self.settings.frequency_hz = frequency_hz;
        self
    }

    fn rate(params: &ChannelParams) -> f64 {
        input_rate(params)
    }

    fn heard(self) -> Heard {
        let rate = input_rate(&self.settings.params);
        let mut filter = channel_filter(&self.settings.params).expect("filter");
        let mut channel = create(ChannelCtx { input_rate: rate }, &self.settings).expect("builds");
        let mut filtered = Vec::new();
        let mut out = ChannelOutputs::default();
        let mut heard = Heard::default();
        for block in self.iq.chunks(CHUNK) {
            filter.process(block, &mut filtered);
            out.reset();
            channel.process(&filtered, &mut out);
            heard.take(&mut out);
        }
        heard
    }
}

fn with_tail(mut iq: Vec<Complex<f32>>, rate: f64, seconds: f64) -> Vec<Complex<f32>> {
    iq.extend(synth::silence((seconds * rate) as usize));
    iq
}

fn with_lead(iq: Vec<Complex<f32>>, rate: f64, seconds: f64) -> Vec<Complex<f32>> {
    let mut led = synth::silence((seconds * rate) as usize);
    led.extend(iq);
    led
}

fn ci16_le(bytes: &[u8]) -> Vec<Complex<f32>> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|s| {
            Complex::new(
                f32::from(i16::from_le_bytes([s[0], s[1]])) / 32_768.0,
                f32::from(i16::from_le_bytes([s[2], s[3]])) / 32_768.0,
            )
        })
        .collect()
}

fn cu8(bytes: &[u8]) -> Vec<Complex<f32>> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[i, q]| {
            Complex::new(
                (f32::from(i) - 127.5) / 127.5,
                (f32::from(q) - 127.5) / 127.5,
            )
        })
        .collect()
}

fn payload(type_id: &str) -> &'static Payload {
    PAYLOADS
        .iter()
        .find(|payload| payload.type_id == type_id)
        .unwrap_or_else(|| panic!("{type_id} has no expected payload"))
}

fn check(type_id: &str) {
    let descriptor = crate::descriptor(type_id).expect("registered");
    let heard = heard(type_id).unwrap_or_else(|| panic!("{type_id} has no positive fixture"));
    let expected = payload(type_id);
    if !expected.carries.is_empty() {
        assert!(
            !heard.kinds.is_empty(),
            "{type_id} produced nothing from its fixture"
        );
    }
    for kind in &heard.kinds {
        assert!(
            descriptor.emits.contains(kind),
            "{type_id} emitted {kind} but declares {:?}",
            descriptor.emits
        );
    }
    let rendered = heard.rendered();
    for carry in expected.carries {
        assert!(
            rendered.contains(carry),
            "{type_id} fixture read {rendered} and never carried {carry}"
        );
    }
    if let Some((lat, lon)) = expected.at {
        let places = heard.places();
        assert!(
            places.iter().any(|&(heard_lat, heard_lon)| {
                (heard_lat - lat).abs() < PLACE_TOLERANCE_DEG
                    && (heard_lon - lon).abs() < PLACE_TOLERANCE_DEG
            }),
            "{type_id} fixture placed {places:?} and never {lat}, {lon}"
        );
    }
    assert!(
        heard.audio_samples >= expected.audio_samples,
        "{type_id} fixture gave {} audio samples, short of {}",
        heard.audio_samples,
        expected.audio_samples
    );
    assert!(
        heard.pictures >= expected.pictures,
        "{type_id} fixture gave {} pictures, short of {}",
        heard.pictures,
        expected.pictures
    );
}

fn heard(type_id: &str) -> Option<Heard> {
    let fixture = match type_id {
        "ft8" | "ft4" | "wspr" => return Some(weak(type_id)),
        "wifi_occupancy" => return Some(wifi_occupancy()),
        "ism_survey" => return Some(ism_survey()),
        "am" => am(),
        "ssb" => ssb(),
        "atv" => atv(),
        "nfm" => nfm(),
        "pocsag" => pocsag(),
        "flex" => flex(),
        "ermes" => ermes(),
        "eot" => eot(),
        "sarsat" => sarsat(),
        "acars" => acars(),
        "aprs" => aprs()?,
        "ais" => ais(),
        "adsb" => adsb(),
        "morse" => morse(),
        "rtty" => rtty(),
        "navtex" => navtex(),
        "psk" => psk(),
        "selcall" => selcall()?,
        "sstv" => sstv(),
        "apt" => apt(),
        "lrpt" => lrpt(),
        "wefax" => wefax(),
        "same" => same(),
        "radiosonde" => radiosonde(),
        "vor" => vor(),
        "ils" => ils(),
        "radio_clock" => radio_clock(),
        "dmr" | "p25" | "nxdn" | "dstar" | "m17" | "ysf" | "dpmr" => digital_voice(type_id),
        "freedv" => freedv(),
        "tetrapol" => tetrapol(),
        "tetra" => tetra(),
        "wfm" => wfm(),
        "lora" => lora(),
        "remote_id" => remote_id(),
        "ble" => ble(),
        "ism" => ism(),
        "per" => per(),
        "noise_figure" => noise_figure(),
        "sid" => sid(),
        "dect" => dect(),
        "gnss" => gnss(),
        "gsm" => gsm(),
        "dab" => dab(),
        "datv" => datv(),
        "dvbt" => dvbt(),
        "drm" => drm(),
        "cw_skimmer" => cw_skimmer(),
        "ident" => ident(),
        "dsc" => dsc(),
        "stanag" => stanag(),
        "inmarsat_aero" => inmarsat_aero(),
        "inmarsat_stdc" => crate::inmarsat_stdc::fixture(),
        "vdl2" => crate::vdl2::fixture(),
        "hfdl" => crate::hfdl::fixture(),
        "iridium" => crate::iridium::fixture(),
        "wmbus" => crate::wmbus::fixture(),
        _ => return None,
    };
    Some(fixture.heard())
}

fn am() -> Fixture {
    let params = ChannelParams::Am(AmParams::default());
    let rate = Fixture::rate(&params);
    let iq = (0..(rate * 0.5) as usize)
        .map(|index| {
            let time = index as f64 / rate;
            let envelope = 0.5 * (1.0 + 0.6 * (std::f64::consts::TAU * 1_000.0 * time).sin());
            Complex::new(envelope as f32, 0.0)
        })
        .collect();
    Fixture::new(params, iq)
}

fn ssb() -> Fixture {
    let params = ChannelParams::Ssb(SsbParams::default());
    let rate = Fixture::rate(&params);
    let iq = (0..(rate * 0.5) as usize)
        .map(|index| {
            let phase = std::f64::consts::TAU * 1_000.0 * index as f64 / rate;
            Complex::from_polar(0.5, phase as f32)
        })
        .collect();
    Fixture::new(params, iq)
}

fn atv() -> Fixture {
    let params = ChannelParams::Atv(AtvParams::default());
    let rate = Fixture::rate(&params);
    let source = synth::atv::AtvSource::new(&AtvParams::default(), rate);
    Fixture::new(params, synth::atv::bars(&source, 2))
}

fn nfm() -> Fixture {
    let params = ChannelParams::Nfm(NfmParams {
        tone_mode: NfmToneMode::Detect,
        ..NfmParams::default()
    });
    let rate = Fixture::rate(&params);
    let len = (rate * 3.0) as usize;
    let audio = synth::nfm::mix(
        &synth::nfm::ctcss_audio(88.5, 0.15, rate, len),
        &synth::nfm::speech_audio(rate, len),
    );
    Fixture::new(params, synth::fm_modulate(&audio, 2_500.0, rate))
}

fn pocsag() -> Fixture {
    let params = ChannelParams::Pocsag(PocsagParams::default());
    let pages = [synth::pocsag::Page {
        address: 1_234_567,
        function: 3,
        text: "SDR FIXTURE".to_owned(),
        numeric: false,
    }];
    let rate = Fixture::rate(&params);
    let iq = synth::pocsag::transmission(&pages, 1_200, 4_500.0, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

fn flex() -> Fixture {
    let params = ChannelParams::Flex(FlexParams::default());
    let page = synth::flex::Page {
        address: 1_234_567,
        text: "SDR FIXTURE".to_owned(),
    };
    let rate = Fixture::rate(&params);
    let iq = synth::flex::transmission(&page, 4, 72, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

fn ermes() -> Fixture {
    let params = ChannelParams::Ermes(ErmesParams::default());
    let page = synth::ermes::Page {
        local_address: 234_567,
        message_number: 3,
        text: "ERMES ALPHA PAGE".to_owned(),
        urgent: true,
        alert: 5,
    };
    let rate = Fixture::rate(&params);
    let iq = synth::ermes::transmission(&page, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

fn eot() -> Fixture {
    let params = ChannelParams::Eot(EotParams::default());
    let rear = synth::eot::Rear {
        unit_address: 24_680,
        status: EotStatus {
            message_type: 0,
            arming: EotArming::Normal,
            pressure_psig: 90,
            battery: EotBattery::Ok,
            battery_charge_pct: 76,
            valve_ok: true,
            confirmed: false,
            turbine: true,
            motion: true,
            marker_light: true,
            marker_battery_low: false,
            discretionary: false,
            chaining: 3,
        },
    };
    let rate = Fixture::rate(&params);
    let iq = synth::eot::rear_transmission(&rear, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

fn same() -> Fixture {
    let params = ChannelParams::Same(SameParams::default());
    let rate = Fixture::rate(&params);
    let iq = synth::same::transmission("ZCZC-WXR-TOR-048201+0030-1232200-KEC55/NW-", 2, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

fn sarsat() -> Fixture {
    let params = ChannelParams::Sarsat(SarsatParams::default());
    let message = synth::sarsat::epirb_location(synth::sarsat::EpirbFix {
        country: 211,
        mmsi_tail: 406_406,
        lat: 54.3233,
        lon: 10.1228,
    });
    let rate = Fixture::rate(&params);
    let iq = synth::sarsat::transmission(&message, false, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

fn acars() -> Fixture {
    let params = ChannelParams::Acars(AcarsParams::default());
    let block = synth::acars::Block {
        mode: '2',
        registration: ".D-AIBL",
        ack: '\u{15}',
        label: "H1",
        block_id: '4',
        seq_no: Some("M01A"),
        flight: Some("LH0400"),
        text: "SDR FIXTURE",
        more: false,
    };
    let rate = Fixture::rate(&params);
    let iq = synth::acars::transmission(&block, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

fn aprs_frame(text: &str) -> Vec<u8> {
    let mut frame = Vec::new();
    for call in ["APRS  ", "DL0SDR"] {
        frame.extend(call.bytes().map(|byte| byte << 1));
        frame.push(0x60);
    }
    let last = frame.len() - 1;
    frame[last] |= 1;
    frame.extend([0x03, 0xF0]);
    frame.extend(text.bytes());
    frame
}

fn aprs() -> Option<Fixture> {
    let params = ChannelParams::Aprs(AprsParams {
        mode: AprsMode::Afsk1200,
        ..AprsParams::default()
    });
    let rate = Fixture::rate(&params);
    let mut tx = AprsTx::new(ChannelCtx { input_rate: rate }, settings(params.clone())).ok()?;
    tx.submit(TxPayload::Frame(aprs_frame(APRS_POSITION_REPORT)))
        .ok()?;
    let iq = synth::burst(&mut tx);
    Some(Fixture::new(params, with_tail(iq, rate, 0.5)))
}

fn ais() -> Fixture {
    let params = ChannelParams::Ais(AisParams::default());
    let report = synth::ais::PositionReport {
        mmsi: 244_010_000,
        lat: 52.3702,
        lon: 4.8952,
        sog_kt: 12.4,
        cog_deg: 87.5,
        heading_deg: 88,
        nav_status: 0,
    };
    let rate = Fixture::rate(&params);
    let iq = synth::ais::burst(&synth::ais::position_payload(&report), rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

fn adsb() -> Fixture {
    let params = ChannelParams::Adsb(AdsbParams::default());
    let icao = 0x3C_65_AC;
    let frames = vec![
        synth::adsb::squitter(icao, synth::adsb::me_identification("DLH123")),
        synth::adsb::squitter(icao, synth::adsb::me_velocity(451.0, 87.0, -1_216)),
    ];
    let rate = Fixture::rate(&params);
    let iq = synth::adsb::transmission(&frames, 500.0, 0.8, rate);
    Fixture::new(params, with_tail(iq, rate, 0.01))
}

fn morse() -> Fixture {
    let params = ChannelParams::Morse(MorseParams::default());
    let rate = Fixture::rate(&params);
    let iq = synth::morse::transmission("CQ CQ DE SDR K", 20.0, 0.0, rate);
    Fixture::new(params, with_tail(with_lead(iq, rate, 0.5), rate, 3.5))
}

fn rtty() -> Fixture {
    let params = ChannelParams::Rtty(RttyParams::default());
    let rate = Fixture::rate(&params);
    let iq = synth::rtty::transmission("CQ CQ DE DL1ABC K", 45.45, 170.0, 1.5, rate);
    Fixture::new(params, with_tail(iq, rate, 1.0))
}

fn navtex() -> Fixture {
    let params = ChannelParams::Navtex(NavtexParams::default());
    let rate = Fixture::rate(&params);
    let iq = synth::navtex::transmission("ZCZC FA01 SDR FIXTURE NNNN", rate);
    Fixture::new(params, with_tail(iq, rate, 1.0))
}

fn psk() -> Fixture {
    let params = ChannelParams::Psk(PskParams::default());
    let iq = synth::psk::transmission("CQ CQ DE SDR \n", PskBaud::Psk31.rate());
    let rate = Fixture::rate(&params);
    Fixture::new(params, with_tail(with_lead(iq, rate, 0.5), rate, 1.0))
}

fn selcall() -> Option<Fixture> {
    let params = ChannelParams::Selcall(SelcallParams::default());
    let rate = Fixture::rate(&params);
    let iq = synth::selcall::transmission(SelcallSystem::Ccir1, "12234", rate).ok()?;
    Some(Fixture::new(params, with_tail(iq, rate, 0.5)))
}

fn weak(type_id: &str) -> Heard {
    let ctx = ChannelCtx {
        input_rate: 12_000.0,
    };
    let mut out = ChannelOutputs::default();
    match type_id {
        "ft8" => {
            let mut channel =
                Ft8Channel::new(ctx, settings(ChannelParams::Ft8(WsjtParams::default())))
                    .expect("builds");
            channel.process(&weak_signal::ft8_slot("W1AW", "FN42", 1_500.0), &mut out);
            channel.settle(&mut out);
        }
        "ft4" => {
            let mut channel =
                Ft4Channel::new(ctx, settings(ChannelParams::Ft4(WsjtParams::default())))
                    .expect("builds");
            channel.process(&weak_signal::ft4_slot("JA1ABC", "PM95", 1_000.0), &mut out);
            channel.settle(&mut out);
        }
        _ => {
            let mut channel =
                WsprChannel::new(ctx, settings(ChannelParams::Wspr(WsprParams::default())))
                    .expect("builds");
            channel.process(
                &weak_signal::wspr_slot("K1ABC", "FN42", 37, 1_500.0),
                &mut out,
            );
            channel.settle(&mut out);
        }
    }
    Heard::from_events(&out.events)
}

fn sstv() -> Fixture {
    let params = ChannelParams::Sstv(SstvParams::default());
    let mode = SstvMode::MartinM1;
    let rate = Fixture::rate(&params);
    let iq = synth::sstv::transmission(mode, &synth::sstv::bars(mode), rate);
    Fixture::new(params, with_tail(iq, rate, 2.0))
}

fn apt() -> Fixture {
    let params = ChannelParams::Apt(AptParams::default());
    let rate = Fixture::rate(&params);
    let iq = synth::apt::transmission(240, AvhrrChannel::Ch2, AvhrrChannel::Ch4, rate);
    Fixture::new(params, with_tail(iq, rate, 10.0))
}

fn lrpt() -> Fixture {
    let mode = LrptMode::Oqpsk72;
    let params = ChannelParams::Lrpt(LrptParams { mode });
    let rate = Fixture::rate(&params);
    let iq = synth::lrpt::transmission(mode, 2, rate);
    Fixture::new(params, with_tail(iq, rate, 2.5))
}

fn wefax() -> Fixture {
    let params = ChannelParams::Wefax(WefaxParams::default());
    let rate = Fixture::rate(&params);
    let chart = synth::wefax::bars(WefaxIoc::Ioc576, 120);
    let iq = synth::wefax::transmission(WefaxIoc::Ioc576, WefaxLpm::Lpm120, &chart, rate);
    Fixture::new(params, with_tail(iq, rate, 2.0))
}

fn radiosonde() -> Fixture {
    let params = ChannelParams::Radiosonde(RadiosondeParams::default());
    let rate = Fixture::rate(&params);
    let iq = synth::radiosonde::transmission(SondeType::Rs41, 10, rate);
    Fixture::new(params, with_tail(iq, rate, 0.5))
}

fn vor() -> Fixture {
    Fixture::new(
        ChannelParams::Vor(VorParams::default()),
        synth::vor::transmission(123.0, 2),
    )
}

fn ils() -> Fixture {
    let params = ChannelParams::Ils(IlsParams::default());
    let rate = Fixture::rate(&params);
    let iq = (0..(2.0 * rate) as usize)
        .map(|index| {
            let time = index as f64 / rate;
            let envelope = 1.0
                + 0.28 * (std::f64::consts::TAU * 90.0 * time).cos()
                + 0.12 * (std::f64::consts::TAU * 150.0 * time).cos();
            Complex::new(envelope as f32, 0.0)
        })
        .collect();
    Fixture::new(params, iq)
}

fn radio_clock() -> Fixture {
    Fixture::new(
        ChannelParams::RadioClock(RadioClockParams::default()),
        cf32_le(include_bytes!(
            "../../../fixtures/radio_clock_dcf77_offair_2k.sigmf-data"
        )),
    )
}

fn digital_voice(type_id: &str) -> Fixture {
    let (params, iq) = match type_id {
        "dmr" => (
            ChannelParams::Dmr(sdrmm_wire::DmrParams::default()),
            dv::dmr::transmission(&dv::dmr::Call::default(), 48_000.0),
        ),
        "p25" => (
            ChannelParams::P25(P25Params::default()),
            dv::p25::transmission(0x293, 48_000.0),
        ),
        "nxdn" => (
            ChannelParams::Nxdn(NxdnParams::default()),
            dv::nxdn::addressed_transmission(
                &dv::nxdn::Shape::default(),
                NXDN_RAN,
                NXDN_SOURCE,
                NXDN_DESTINATION,
                true,
                48_000.0,
            ),
        ),
        "dstar" => (
            ChannelParams::Dstar(DstarParams::default()),
            dv::dstar::transmission(&dv::dstar::Call::default(), 48_000.0),
        ),
        "m17" => (
            ChannelParams::M17(M17Params::default()),
            dv::m17::transmission("ALL", "DL1ABC", 48_000.0),
        ),
        "dpmr" => (
            ChannelParams::Dpmr(DpmrParams::default()),
            dv::dpmr::transmission(&dv::dpmr::Call::default(), 48_000.0),
        ),
        _ => (
            ChannelParams::Ysf(YsfParams::default()),
            dv::ysf::transmission_with_callsigns(
                &dv::ysf::Fich::default(),
                &dv::ysf::Call::default(),
                48_000.0,
            ),
        ),
    };
    let mut keyed = complex_noise(1, 0.01, 19_200);
    keyed.extend(iq);
    Fixture::new(params, with_tail(keyed, 48_000.0, 0.5))
}

fn freedv() -> Fixture {
    Fixture::new(
        ChannelParams::Freedv(FreeDvParams::default()),
        cf32_le(include_bytes!(
            "../../../fixtures/freedv_1600_8k.sigmf-data"
        )),
    )
}

fn tetrapol() -> Fixture {
    let params = ChannelParams::Tetrapol(TetrapolParams::default());
    let rate = Fixture::rate(&params);
    Fixture::new(params, synth::tetrapol::control_channel(rate, 1))
}

fn tetra() -> Fixture {
    Fixture::new(
        ChannelParams::Tetra(TetraParams::default()),
        synth::tetra::sample(synth::tetra::Cell::default()).modulate(),
    )
}

fn wfm() -> Fixture {
    let params = ChannelParams::Wfm(WfmParams::default());
    let rate = Fixture::rate(&params);
    let station = synth::rds::Station {
        pi: 0xD3C2,
        ps: "SDR-M4  ".to_owned(),
        radiotext: "fixture".to_owned(),
        pty: 10,
        tp: true,
        ta: false,
        music: true,
        alt_freqs_hz: vec![89_800_000.0],
    };
    Fixture::new(
        params,
        synth::rds::transmission(&station, 6.0, Some(1_000.0), rate),
    )
}

fn lora() -> Fixture {
    Fixture::new(
        ChannelParams::Lora(LoraParams::default()),
        cf32_le(include_bytes!(
            "../../../fixtures/lora_rn2483_sf7_250k.sigmf-data"
        )),
    )
}

fn remote_id() -> Fixture {
    let params = ChannelParams::RemoteId(RemoteIdParams {
        link: RemoteIdLink::Bluetooth,
    });
    let rate = Fixture::rate(&params);
    Fixture::new(params, synth::remote_id::bluetooth_scene(rate)).tuned(BLE_37_HZ)
}

fn ble() -> Fixture {
    let params = ChannelParams::Ble(BleParams {
        link: BleLink::Channel,
    });
    let rate = Fixture::rate(&params);
    Fixture::new(params, synth::ble::scene(rate)).tuned(BLE_38_HZ)
}

fn ism() -> Fixture {
    let params = ChannelParams::Ism(IsmParams::default());
    let rate = Fixture::rate(&params);
    let iq = cu8(include_bytes!(
        "../../../fixtures/ism/nexus_th_433m92_250k.sigmf-data"
    ));
    Fixture::new(params, with_tail(iq, rate, 0.25))
}

fn per() -> Fixture {
    let params = ChannelParams::Per(PerParams::default());
    let rate = Fixture::rate(&params);
    Fixture::new(
        params,
        synth::per::frames(&PerParams::default(), 0..100, rate),
    )
}

fn noise_figure() -> Fixture {
    let params = ChannelParams::NoiseFigure(NoiseFigureParams::default());
    let rate = Fixture::rate(&params);
    Fixture::new(params, synth::noise_figure::switched(10.0, 1.0, rate))
}

fn sid() -> Fixture {
    let params = ChannelParams::Sid(SidParams::default());
    let rate = Fixture::rate(&params);
    Fixture::new(
        params,
        synth::sid::carriers(
            &[(19_580.0, 0.05), (23_400.0, 0.1), (24_000.0, 0.03)],
            SID_CENTRE_HZ,
            10.0,
            rate,
            0x51D,
        ),
    )
    .tuned(SID_CENTRE_HZ)
}

fn dect() -> Fixture {
    let station = synth::dect::Station {
        capabilities: synth::dect::capability_bits(&[17, 33, 36, 37]),
        ..synth::dect::Station::default()
    };
    Fixture::new(
        ChannelParams::Dect(DectParams::default()),
        synth::dect::dummy_bearer(&station, 40),
    )
}

fn gnss() -> Fixture {
    Fixture::new(
        ChannelParams::Gnss(GnssParams {
            prn: GNSS_FIXTURE_PRN,
            ..GnssParams::default()
        }),
        synth::gnss::acquisition(GNSS_FIXTURE_PRN, 1_000.0, 317, 1_000),
    )
}

fn gsm() -> Fixture {
    Fixture::new(
        ChannelParams::Gsm(GsmParams::default()),
        cf32_le(include_bytes!(
            "../../../fixtures/gsm_dcs1800_arfcn725_541k.sigmf-data"
        )),
    )
    .tuned(GSM_ARFCN_725_HZ)
}

fn dab() -> Fixture {
    Fixture::new(
        ChannelParams::Dab(DabParams::default()),
        synth::dab::ensemble_for_mode(DabTransmissionMode::I, 16),
    )
}

fn datv() -> Fixture {
    Fixture::new(
        ChannelParams::Datv(DatvParams {
            standard: DatvStandard::DvbS2,
            symbol_rate: synth::datv::SYMBOL_RATE,
            ..DatvParams::default()
        }),
        synth::datv::dvbs2_generic(3, &[4]),
    )
}

fn dvbt() -> Fixture {
    let params = ChannelParams::Dvbt(DvbtParams::default());
    let rate = Fixture::rate(&params);
    let iq = synth::dvbt::waveform(synth::dvbt::defaults(), DVBT_SYMBOLS);
    Fixture::new(params, with_tail(iq, rate, 0.01))
}

fn drm() -> Fixture {
    Fixture::new(
        ChannelParams::Drm(DrmParams {
            mode: DrmMode::Auto,
            bandwidth_hz: 100_000.0,
            service: None,
        }),
        synth::drm::signal(synth::drm::defaults(synth::drm::Robustness::B), 6),
    )
}

fn cw_skimmer() -> Fixture {
    let params = ChannelParams::CwSkimmer(sdrmm_wire::CwSkimmerParams {
        bandwidth_hz: 16_000.0,
        ..sdrmm_wire::CwSkimmerParams::default()
    });
    let rate = Fixture::rate(&params);
    let mut iq = synth::morse::hard_keyed(
        "VVV VVV CQ DE DL1AAA K VVV VVV CQ DE DL1AAA K",
        20.0,
        3_500.0,
        rate,
    );
    synth::scale(&mut iq, 10f32.powf(55.0 / 20.0));
    synth::add_noise(&mut iq, 7, 1.0);
    Fixture::new(params, iq)
}

fn ident() -> Fixture {
    let params = ChannelParams::Ident(IdentParams {
        interval_ms: 500,
        ..IdentParams::default()
    });
    let rate = Fixture::rate(&params);
    let pages = [synth::pocsag::Page {
        address: 1_234_567,
        function: 3,
        text: "IDENT TEST".to_owned(),
        numeric: false,
    }];
    let burst = synth::pocsag::transmission(&pages, 1_200, 4_500.0, rate);
    let mut iq = Vec::new();
    while iq.len() < (2.0 * rate) as usize {
        iq.extend_from_slice(&burst);
    }
    synth::add_noise(&mut iq, 0x5d90, 0.01);
    Fixture::new(params, iq)
}

fn dsc() -> Fixture {
    Fixture::new(
        ChannelParams::Dsc(DscParams::default()),
        cf32_le(include_bytes!("../../../fixtures/dsc_offair_8k.sigmf-data")),
    )
}

fn stanag() -> Fixture {
    Fixture::new(
        ChannelParams::Stanag(StanagParams::default()),
        ci16_le(include_bytes!(
            "../../../fixtures/stanag/stanag_4285_9k6.sigmf-data"
        )),
    )
}

fn inmarsat_aero() -> Fixture {
    Fixture::new(
        ChannelParams::InmarsatAero(InmarsatAeroParams::default()),
        ci16_le(include_bytes!(
            "../../../fixtures/inmarsat_aero_offair_48k.sigmf-data"
        )),
    )
}

fn scene_heard(params: ChannelParams, frequency_hz: f64, scene: &Scene) -> Heard {
    let rate = input_rate(&params);
    let mut tuned = settings(params);
    tuned.frequency_hz = frequency_hz;
    let mut channel: Box<dyn ChannelRx> =
        create(ChannelCtx { input_rate: rate }, &tuned).expect("builds");
    Heard::from_events(&scene.run(1.01, channel.as_mut()).events)
}

fn wifi_occupancy() -> Heard {
    let span = AirtimeSpan::Mhz20;
    let rate = span.sample_rate_hz();
    let frame = synth::ism::wifi_frame(300);
    let on_s = frame.len() as f64 / rate;
    let mut scene = Scene::new(rate, 0.01);
    scene.every(on_s / 0.3, 1.01, 0.0, 0.1, &frame);
    let params = ChannelParams::WifiOccupancy(WifiOccupancyParams {
        span,
        ..WifiOccupancyParams::default()
    });
    scene_heard(params, WIFI_6_HZ, &scene)
}

fn ism_survey() -> Heard {
    let params = ChannelParams::IsmSurvey(IsmSurveyParams::default());
    let rate = input_rate(&params);
    let frame = synth::ism::wifi_frame(300);
    let mut scene = Scene::new(rate, 0.01);
    scene.every(0.01, 1.01, 0.0, 0.1, &frame);
    scene_heard(params, WIFI_6_HZ, &scene)
}

#[test]
fn every_registered_channel_has_a_positive_fixture() {
    let missing: Vec<String> = descriptors()
        .into_iter()
        .map(|descriptor| descriptor.type_id)
        .filter(|type_id| !FIXTURED.contains(&type_id.as_str()))
        .collect();
    assert!(missing.is_empty(), "no positive fixture for {missing:?}");
}

#[test]
fn every_positive_fixture_states_what_it_must_read() {
    for type_id in FIXTURED {
        assert!(
            payload(type_id).proves_a_decode(),
            "{type_id} has no expected payload, so its fixture proves nothing"
        );
    }
    let stale: Vec<&str> = PAYLOADS
        .iter()
        .map(|payload| payload.type_id)
        .filter(|type_id| !FIXTURED.contains(type_id))
        .collect();
    assert!(stale.is_empty(), "expected payload for {stale:?}");
}

macro_rules! emitted_kinds_are_declared {
    ($($test:ident => $type_id:literal),* $(,)?) => {
        const FIXTURED: &[&str] = &[$($type_id),*];
        $(
            #[test]
            fn $test() {
                check($type_id);
            }
        )*
    };
}

emitted_kinds_are_declared! {
    am_emits_what_it_declares => "am",
    ssb_emits_what_it_declares => "ssb",
    atv_emits_what_it_declares => "atv",
    nfm_emits_what_it_declares => "nfm",
    pocsag_emits_what_it_declares => "pocsag",
    flex_emits_what_it_declares => "flex",
    ermes_emits_what_it_declares => "ermes",
    eot_emits_what_it_declares => "eot",
    sarsat_emits_what_it_declares => "sarsat",
    acars_emits_what_it_declares => "acars",
    aprs_emits_what_it_declares => "aprs",
    ais_emits_what_it_declares => "ais",
    adsb_emits_what_it_declares => "adsb",
    morse_emits_what_it_declares => "morse",
    rtty_emits_what_it_declares => "rtty",
    navtex_emits_what_it_declares => "navtex",
    psk_emits_what_it_declares => "psk",
    selcall_emits_what_it_declares => "selcall",
    ft8_emits_what_it_declares => "ft8",
    ft4_emits_what_it_declares => "ft4",
    wspr_emits_what_it_declares => "wspr",
    sstv_emits_what_it_declares => "sstv",
    apt_emits_what_it_declares => "apt",
    lrpt_emits_what_it_declares => "lrpt",
    wefax_emits_what_it_declares => "wefax",
    same_emits_what_it_declares => "same",
    radiosonde_emits_what_it_declares => "radiosonde",
    vor_emits_what_it_declares => "vor",
    ils_emits_what_it_declares => "ils",
    radio_clock_emits_what_it_declares => "radio_clock",
    dmr_emits_what_it_declares => "dmr",
    p25_emits_what_it_declares => "p25",
    nxdn_emits_what_it_declares => "nxdn",
    dstar_emits_what_it_declares => "dstar",
    m17_emits_what_it_declares => "m17",
    ysf_emits_what_it_declares => "ysf",
    dpmr_emits_what_it_declares => "dpmr",
    freedv_emits_what_it_declares => "freedv",
    tetrapol_emits_what_it_declares => "tetrapol",
    tetra_emits_what_it_declares => "tetra",
    wfm_emits_what_it_declares => "wfm",
    lora_emits_what_it_declares => "lora",
    remote_id_emits_what_it_declares => "remote_id",
    ble_emits_what_it_declares => "ble",
    ism_emits_what_it_declares => "ism",
    ism_survey_emits_what_it_declares => "ism_survey",
    wifi_occupancy_emits_what_it_declares => "wifi_occupancy",
    per_emits_what_it_declares => "per",
    noise_figure_emits_what_it_declares => "noise_figure",
    sid_emits_what_it_declares => "sid",
    dect_emits_what_it_declares => "dect",
    gnss_emits_what_it_declares => "gnss",
    gsm_emits_what_it_declares => "gsm",
    dab_emits_what_it_declares => "dab",
    datv_emits_what_it_declares => "datv",
    dvbt_emits_what_it_declares => "dvbt",
    drm_emits_what_it_declares => "drm",
    cw_skimmer_emits_what_it_declares => "cw_skimmer",
    ident_emits_what_it_declares => "ident",
    dsc_emits_what_it_declares => "dsc",
    stanag_emits_what_it_declares => "stanag",
    inmarsat_aero_emits_what_it_declares => "inmarsat_aero",
    inmarsat_stdc_emits_what_it_declares => "inmarsat_stdc",
    vdl2_emits_what_it_declares => "vdl2",
    hfdl_emits_what_it_declares => "hfdl",
    iridium_emits_what_it_declares => "iridium",
    wmbus_emits_what_it_declares => "wmbus",
}
