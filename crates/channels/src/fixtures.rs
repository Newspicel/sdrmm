use num_complex::Complex;
use sdrmm_wire::{
    AcarsParams, AdsbParams, AirtimeSpan, AisParams, AprsMode, AprsParams, AptParams, AvhrrChannel,
    BleLink, BleParams, ChannelParams, ChannelSettings, DabParams, DabTransmissionMode, DatvParams,
    DatvStandard, DecoderEvent, DectParams, DpmrParams, DrmMode, DrmParams, DscParams, DstarParams,
    DvbtParams, EotArming, EotBattery, EotParams, EotStatus, ErmesParams, EventKind, FlexParams,
    FreeDvParams, GnssParams, GsmParams, IdentParams, IlsParams, InmarsatAeroParams, IsmParams,
    IsmSurveyParams, LoraParams, LrptMode, LrptParams, M17Params, MorseParams, NavtexParams,
    NfmParams, NfmToneMode, NoiseFigureParams, NxdnParams, P25Params, PerParams, PocsagParams,
    PskBaud, PskParams, RadioClockParams, RadiosondeParams, RemoteIdLink, RemoteIdParams,
    RttyParams, SarsatParams, SelcallParams, SelcallSystem, SidParams, SondeType, SstvMode,
    SstvParams, StanagParams, TetraParams, TetrapolParams, VorParams, WefaxIoc, WefaxLpm,
    WefaxParams, WfmParams, WifiOccupancyParams, WsjtParams, WsprParams, YsfParams,
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

#[derive(Default)]
struct Heard {
    kinds: Vec<EventKind>,
}

impl Heard {
    fn from_events(events: &[DecoderEvent]) -> Self {
        Self {
            kinds: events.iter().map(DecoderEvent::kind).collect(),
        }
    }

    fn take(&mut self, out: &mut ChannelOutputs) {
        self.kinds
            .extend(out.events.drain(..).map(|event| event.kind()));
        if out.broadcast.take().is_some() {
            self.kinds.push(EventKind::Broadcast);
        }
    }
}

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

fn check(type_id: &str) {
    let descriptor = crate::descriptor(type_id).expect("registered");
    let heard = heard(type_id).unwrap_or_else(|| panic!("{type_id} has no positive fixture"));
    assert!(
        !heard.kinds.is_empty(),
        "{type_id} produced nothing from its fixture"
    );
    for kind in heard.kinds {
        assert!(
            descriptor.emits.contains(&kind),
            "{type_id} emitted {kind} but declares {:?}",
            descriptor.emits
        );
    }
}

fn heard(type_id: &str) -> Option<Heard> {
    let fixture = match type_id {
        "ft8" | "ft4" | "wspr" => return Some(weak(type_id)),
        "wifi_occupancy" => return Some(wifi_occupancy()),
        "ism_survey" => return Some(ism_survey()),
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
        _ => return None,
    };
    Some(fixture.heard())
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
    tx.submit(TxPayload::Frame(aprs_frame("SDR FIXTURE")))
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
    Fixture::new(params, with_tail(iq, rate, 3.5))
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
    Fixture::new(params, with_tail(iq, rate, 1.0))
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
            dv::nxdn::transmission(&dv::nxdn::Shape::default(), 1, true, 48_000.0),
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
            30_000.0,
            10.0,
            rate,
            0x51D,
        ),
    )
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
        ChannelParams::Gnss(GnssParams::default()),
        synth::gnss::acquisition(7, 1_000.0, 317, 1_000),
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
    let iq = synth::dvbt::waveform(synth::dvbt::defaults(), 180);
    Fixture::new(params, with_tail(iq, rate, 0.3))
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
fn every_channel_that_emits_has_a_positive_fixture() {
    let missing: Vec<String> = descriptors()
        .into_iter()
        .filter(|descriptor| !descriptor.emits.is_empty())
        .map(|descriptor| descriptor.type_id)
        .filter(|type_id| !FIXTURED.contains(&type_id.as_str()))
        .collect();
    assert!(missing.is_empty(), "no positive fixture for {missing:?}");
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
}
