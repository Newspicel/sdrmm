use sdrmm_wire::channel::{
    AeroChannel, AisChannel, AprsMode, AtvColor, AtvModulation, AtvStandard, ChannelParams,
    DabMode, DabTransmissionMode, DatvCodeRate, DatvRollOff, DatvStandard, DectBand, DectSides,
    DectSpan, DmrSlots, DrmMode, DvbtBandwidth, DvbtStandard, GnssParams, IlsComponent,
    IridiumSpan, NfmScramblerMode, NfmToneMode, NxdnBandwidth, PocsagBaud, PskBaud,
    RadioClockStandard, RttyStopBits, SelcallSystem, Sideband, SstvModulation,
};
use zgui::prelude::*;

use super::{
    controls::{
        CHANNEL_BANDWIDTH, Ctx, bandwidth, listed, number, presets, segmented, select,
        service_picker, titled, toggle,
    },
    more_modes,
    settings::{NumberLimit, scaled_limit},
    tables::{
        AERO_CHANNELS, AIS_CHANNELS, APRS_MODES, ATV_COLORS, ATV_MODULATIONS, ATV_STANDARDS,
        CTCSS_DEFAULT_HZ, DAB_MODES, DATV_CODE_RATES, DATV_ROLL_OFFS, DATV_STANDARDS,
        DCS_DEFAULT_CODE, DECT_BANDS, DECT_SIDES, DECT_SPANS, DEEMPHASIS_US, DMR_SLOTS, DRM_MODES,
        DRM_PLUS_BANDWIDTHS_HZ, DRM30_BANDWIDTHS_HZ, DVBT_BANDWIDTHS, DVBT_STANDARDS,
        ILS_COMPONENTS, INVERSION_DEFAULT_HZ, IRIDIUM_SPANS, NFM_SCRAMBLER_MODES, NFM_TONE_MODES,
        NXDN_WIDTHS, POCSAG_BAUDS, PSK_BAUDS, RADIO_CLOCK_STANDARDS, RTTY_BAUDS, RTTY_SHIFTS_HZ,
        RTTY_STOP_BITS, SELCALL_SYSTEMS, SIDEBANDS, SSTV_MODULATIONS, ctcss_options,
        dab_transmission_modes, dcs_options, drm_bandwidth_for, sstv_modes, tone_key,
    },
};
use crate::ui::kit_channel::{NumberSpec, setting_row, text_field};

pub const INVERT: Option<&str> = Some("Flip the signal's polarity; try it when nothing decodes");
pub const NARROW: &[f64] = &[12_500.0, 25_000.0];

pub fn rows(rows: Vec<AnyView>) -> AnyView {
    AnyView::new(view! { column(class = "kc-grid") {{rows}} })
}

pub fn mode_view(ctx: Ctx, kind: &str) -> AnyView {
    match kind {
        "nfm" => nfm(ctx),
        "selcall" => rows(vec![segmented(
            ctx,
            "Tone plan",
            Some("Selective calling tone plan"),
            SELCALL_SYSTEMS,
            ctx.read(get!(Selcall, p => p.system), SelcallSystem::Ccir1),
            set!(Selcall, p, v => p.system = v),
        )]),
        "am" => rows(vec![
            bandwidth(
                ctx,
                "Bandwidth",
                CHANNEL_BANDWIDTH,
                &[5_000.0, 8_000.0, 10_000.0],
                ctx.read(get!(Am, p => p.bandwidth_hz), 10_000.0),
                set!(Am, p, v => p.bandwidth_hz = v),
            ),
            toggle(
                ctx,
                "Sync",
                Some("Lock to the carrier; clearer under fading"),
                ctx.read(get!(Am, p => p.sync), false),
                set!(Am, p, v => p.sync = v),
            ),
        ]),
        "ssb" => ssb(ctx),
        "wfm" => wfm(ctx),
        "pocsag" => pocsag(ctx),
        "flex" => pager_flex(ctx),
        "ermes" => pager_ermes(ctx),
        "eot" => rows(vec![bandwidth(
            ctx,
            "Bandwidth",
            CHANNEL_BANDWIDTH,
            NARROW,
            ctx.read(get!(Eot, p => p.bandwidth_hz), 12_500.0),
            set!(Eot, p, v => p.bandwidth_hz = v),
        )]),
        "adsb" => rows(vec![toggle(
            ctx,
            "CRC fix",
            Some("Repair single-bit errors the checksum can pin down"),
            ctx.read(get!(Adsb, p => p.crc_fix), true),
            set!(Adsb, p, v => p.crc_fix = v),
        )]),
        "ais" => rows(vec![segmented(
            ctx,
            "Channel",
            Some("AIS channel"),
            AIS_CHANNELS,
            ctx.read(get!(Ais, p => p.ais_channel), AisChannel::A),
            set!(Ais, p, v => p.ais_channel = v),
        )]),
        "inmarsat_aero" => rows(vec![titled(
            ctx,
            "Channel",
            Some("Aero channel"),
            AERO_CHANNELS,
            ctx.read(get!(InmarsatAero, p => p.channel), AeroChannel::P),
            set!(InmarsatAero, p, v => p.channel = v),
        )]),
        "iridium" => rows(vec![titled(
            ctx,
            "Span",
            Some("Iridium span"),
            IRIDIUM_SPANS,
            ctx.read(get!(Iridium, p => p.span), IridiumSpan::Channel),
            set!(Iridium, p, v => p.span = v),
        )]),
        "aprs" => aprs(ctx),
        "rtty" => rtty(ctx),
        "morse" => morse(ctx),
        "cw_skimmer" => cw_skimmer(ctx),
        "ft8" | "ft4" | "wspr" => wsjt(ctx, kind == "wspr"),
        "psk" => psk(ctx),
        "navtex" => rows(vec![toggle(
            ctx,
            "Invert",
            INVERT,
            ctx.read(get!(Navtex, p => p.invert), false),
            set!(Navtex, p, v => p.invert = v),
        )]),
        "radio_clock" => radio_clock(ctx),
        "gnss" => gnss(ctx),
        "vor" => vor(ctx),
        "ils" => ils(ctx),
        "acars" => rows(vec![bandwidth(
            ctx,
            "Bandwidth",
            CHANNEL_BANDWIDTH,
            &[8_000.0, 12_500.0, 25_000.0],
            ctx.read(get!(Acars, p => p.bandwidth_hz), 12_500.0),
            set!(Acars, p, v => p.bandwidth_hz = v),
        )]),
        "atv" => atv(ctx),
        "sstv" => sstv(ctx),
        "dab" => dab(ctx),
        "datv" => datv(ctx),
        "dvbt" => dvbt(ctx),
        "drm" => drm(ctx),
        "dmr" => rows(vec![
            segmented(
                ctx,
                "Slot",
                Some("Slot"),
                DMR_SLOTS,
                ctx.read(get!(Dmr, p => p.slots), DmrSlots::Both),
                set!(Dmr, p, v => p.slots = v),
            ),
            toggle(
                ctx,
                "Ignore data CRC",
                Some("Ignore data CRC"),
                ctx.read(get!(Dmr, p => p.ignore_crc), false),
                set!(Dmr, p, v => p.ignore_crc = v),
            ),
        ]),
        "nxdn" => rows(vec![segmented(
            ctx,
            "Width",
            Some("Width"),
            NXDN_WIDTHS,
            ctx.read(get!(Nxdn, p => p.bandwidth), NxdnBandwidth::Narrow),
            set!(Nxdn, p, v => p.bandwidth = v),
        )]),
        "freedv" => rows(vec![segmented(
            ctx,
            "Sideband",
            Some("FreeDV sideband"),
            SIDEBANDS,
            ctx.read(get!(Freedv, p => p.sideband), Sideband::Usb),
            set!(Freedv, p, v => p.sideband = v),
        )]),
        "ident" => ident(ctx),
        "dect" => rows(vec![
            segmented(
                ctx,
                "Band",
                Some("DECT band"),
                DECT_BANDS,
                ctx.read(get!(Dect, p => p.band), DectBand::Eu),
                set!(Dect, p, v => p.band = v),
            ),
            titled(
                ctx,
                "Span",
                Some("DECT span"),
                DECT_SPANS,
                ctx.read(get!(Dect, p => p.span), DectSpan::Carrier),
                set!(Dect, p, v => p.span = v),
            ),
            segmented(
                ctx,
                "Side",
                Some("DECT side"),
                DECT_SIDES,
                ctx.read(get!(Dect, p => p.sides), DectSides::Both),
                set!(Dect, p, v => p.sides = v),
            ),
        ]),
        _ => more_modes::mode_view(ctx, kind),
    }
}

fn nfm(ctx: Ctx) -> AnyView {
    let tone = ctx.read(get!(Nfm, p => p.tone_mode), NfmToneMode::Off);
    let scrambler = ctx.read(get!(Nfm, p => p.scrambler_mode), NfmScramblerMode::Off);
    let ctcss = ctx.read(
        get!(Nfm, p => tone_key(p.ctcss_hz.unwrap_or(CTCSS_DEFAULT_HZ))),
        tone_key(CTCSS_DEFAULT_HZ),
    );
    let dcs = ctx.read(
        get!(Nfm, p => p.dcs_code.unwrap_or(DCS_DEFAULT_CODE)),
        DCS_DEFAULT_CODE,
    );
    let carrier = ctx.read(
        get!(Nfm, p => p.inversion_hz.or(Some(INVERSION_DEFAULT_HZ))),
        None,
    );
    let tone_rows = move || match tone.get() {
        NfmToneMode::Ctcss => Some(select(
            ctx,
            "CTCSS",
            Some("CTCSS tone"),
            ctcss_options(),
            ctcss,
            set!(Nfm, p, v => p.ctcss_hz = Some(f64::from(v) / 10.0)),
        )),
        NfmToneMode::Dcs => Some(select(
            ctx,
            "DCS",
            Some("DCS code"),
            dcs_options(),
            dcs,
            set!(Nfm, p, v => p.dcs_code = Some(v)),
        )),
        _ => None,
    };
    let carrier_row = move || {
        (scrambler.get() == NfmScramblerMode::Inversion).then(|| {
            number(
                ctx,
                "Carrier",
                Some("Inversion carrier"),
                NumberSpec::new("Inversion carrier")
                    .limit(ctx.limit("inversion_hz"))
                    .unit("Hz"),
                carrier,
                set!(Nfm, p, v => p.inversion_hz = v.or(p.inversion_hz)),
            )
        })
    };
    rows(vec![
        bandwidth(
            ctx,
            "Bandwidth",
            CHANNEL_BANDWIDTH,
            NARROW,
            ctx.read(get!(Nfm, p => p.bandwidth_hz), 12_500.0),
            set!(Nfm, p, v => p.bandwidth_hz = v),
        ),
        select(
            ctx,
            "Tone",
            Some("Tone squelch"),
            listed(NFM_TONE_MODES),
            tone,
            set!(Nfm, p, v => {
                p.tone_mode = v;
                p.ctcss_hz.get_or_insert(CTCSS_DEFAULT_HZ);
                p.dcs_code.get_or_insert(DCS_DEFAULT_CODE);
            }),
        ),
        AnyView::new(tone_rows),
        select(
            ctx,
            "Scrambler",
            Some("Voice scrambler"),
            listed(NFM_SCRAMBLER_MODES),
            scrambler,
            set!(Nfm, p, v => {
                p.scrambler_mode = v;
                p.inversion_hz.get_or_insert(INVERSION_DEFAULT_HZ);
            }),
        ),
        AnyView::new(carrier_row),
        toggle(
            ctx,
            "Compander",
            Some("Expand audio that was sent with 2:1 compression"),
            ctx.read(get!(Nfm, p => p.compander), false),
            set!(Nfm, p, v => p.compander = v),
        ),
    ])
}

fn ssb(ctx: Ctx) -> AnyView {
    rows(vec![
        segmented(
            ctx,
            "Sideband",
            Some("Sideband"),
            SIDEBANDS,
            ctx.read(get!(Ssb, p => p.sideband), Sideband::Usb),
            set!(Ssb, p, v => p.sideband = v),
        ),
        number(
            ctx,
            "Bandwidth",
            Some("SSB bandwidth"),
            NumberSpec::new("SSB bandwidth")
                .limit(ctx.limit("bandwidth_hz"))
                .unit("Hz"),
            ctx.read(get!(Ssb, p => Some(p.bandwidth_hz)), Some(2_700.0)),
            set!(Ssb, p, v => if let Some(v) = v { p.bandwidth_hz = v }),
        ),
    ])
}

fn wfm(ctx: Ctx) -> AnyView {
    rows(vec![
        select(
            ctx,
            "De-emphasis",
            Some("De-emphasis"),
            listed(DEEMPHASIS_US),
            ctx.read(get!(Wfm, p => p.deemphasis_us.round() as u32), 50),
            set!(Wfm, p, v => p.deemphasis_us = v as f32),
        ),
        toggle(
            ctx,
            "Stereo",
            Some("Decode the stereo pilot; mono is quieter on weak signals"),
            ctx.read(get!(Wfm, p => p.stereo), true),
            set!(Wfm, p, v => p.stereo = v),
        ),
    ])
}

fn pocsag(ctx: Ctx) -> AnyView {
    rows(vec![
        select(
            ctx,
            "Baud",
            Some("POCSAG baud"),
            listed(POCSAG_BAUDS),
            ctx.read(get!(Pocsag, p => p.baud), PocsagBaud::Auto),
            set!(Pocsag, p, v => p.baud = v),
        ),
        bandwidth(
            ctx,
            "Bandwidth",
            CHANNEL_BANDWIDTH,
            NARROW,
            ctx.read(get!(Pocsag, p => p.bandwidth_hz), 12_500.0),
            set!(Pocsag, p, v => p.bandwidth_hz = v),
        ),
        toggle(
            ctx,
            "Invert",
            INVERT,
            ctx.read(get!(Pocsag, p => p.invert), false),
            set!(Pocsag, p, v => p.invert = v),
        ),
    ])
}

fn pager_flex(ctx: Ctx) -> AnyView {
    rows(vec![
        bandwidth(
            ctx,
            "Bandwidth",
            CHANNEL_BANDWIDTH,
            NARROW,
            ctx.read(get!(Flex, p => p.bandwidth_hz), 12_500.0),
            set!(Flex, p, v => p.bandwidth_hz = v),
        ),
        toggle(
            ctx,
            "Invert FLEX",
            INVERT,
            ctx.read(get!(Flex, p => p.invert), false),
            set!(Flex, p, v => p.invert = v),
        ),
    ])
}

fn pager_ermes(ctx: Ctx) -> AnyView {
    rows(vec![
        bandwidth(
            ctx,
            "Bandwidth",
            CHANNEL_BANDWIDTH,
            NARROW,
            ctx.read(get!(Ermes, p => p.bandwidth_hz), 12_500.0),
            set!(Ermes, p, v => p.bandwidth_hz = v),
        ),
        toggle(
            ctx,
            "Invert ERMES",
            INVERT,
            ctx.read(get!(Ermes, p => p.invert), false),
            set!(Ermes, p, v => p.invert = v),
        ),
    ])
}

fn aprs(ctx: Ctx) -> AnyView {
    rows(vec![
        select(
            ctx,
            "Mode",
            Some("APRS mode"),
            listed(APRS_MODES),
            ctx.read(get!(Aprs, p => p.mode), AprsMode::Afsk1200),
            set!(Aprs, p, v => p.mode = v),
        ),
        bandwidth(
            ctx,
            "Bandwidth",
            CHANNEL_BANDWIDTH,
            NARROW,
            ctx.read(get!(Aprs, p => p.bandwidth_hz), 12_500.0),
            set!(Aprs, p, v => p.bandwidth_hz = v),
        ),
    ])
}

fn rtty(ctx: Ctx) -> AnyView {
    rows(vec![
        presets(
            ctx,
            "Baud",
            Some("RTTY baud"),
            NumberSpec::new("RTTY baud").limit(ctx.limit("baud")),
            RTTY_BAUDS,
            ctx.read(get!(Rtty, p => p.baud), 45.45),
            set!(Rtty, p, v => p.baud = v),
        ),
        presets(
            ctx,
            "Shift",
            Some("RTTY shift"),
            NumberSpec::new("RTTY shift")
                .limit(ctx.limit("shift_hz"))
                .unit("Hz"),
            RTTY_SHIFTS_HZ,
            ctx.read(get!(Rtty, p => p.shift_hz), 170.0),
            set!(Rtty, p, v => p.shift_hz = v),
        ),
        select(
            ctx,
            "Stop bits",
            Some("RTTY stop bits"),
            listed(RTTY_STOP_BITS),
            ctx.read(get!(Rtty, p => p.stop_bits), RttyStopBits::OneAndHalf),
            set!(Rtty, p, v => p.stop_bits = v),
        ),
        toggle(
            ctx,
            "Invert",
            INVERT,
            ctx.read(get!(Rtty, p => p.invert), false),
            set!(Rtty, p, v => p.invert = v),
        ),
        toggle(
            ctx,
            "Unshift on space",
            Some("Drop back to letters after a space, as most stations expect"),
            ctx.read(get!(Rtty, p => p.unshift_on_space), true),
            set!(Rtty, p, v => p.unshift_on_space = v),
        ),
    ])
}

fn morse(ctx: Ctx) -> AnyView {
    rows(vec![
        number(
            ctx,
            "Bandwidth",
            Some("CW filter bandwidth"),
            NumberSpec::new("CW filter bandwidth")
                .limit(ctx.limit("bandwidth_hz"))
                .unit("Hz"),
            ctx.read(get!(Morse, p => Some(p.bandwidth_hz)), Some(400.0)),
            set!(Morse, p, v => if let Some(v) = v { p.bandwidth_hz = v }),
        ),
        number(
            ctx,
            "Speed",
            Some("Morse speed, empty to auto-track"),
            NumberSpec::new("Morse speed")
                .limit(ctx.limit("wpm"))
                .unit("WPM")
                .optional("auto"),
            ctx.read(get!(Morse, p => p.wpm.map(f64::from)), None),
            set!(Morse, p, v => p.wpm = v.map(|v| v as f32)),
        ),
    ])
}

fn cw_skimmer(ctx: Ctx) -> AnyView {
    rows(vec![
        number(
            ctx,
            "Passband",
            Some("CW skimmer passband"),
            NumberSpec::new("CW skimmer passband")
                .limit(ctx.limit("bandwidth_hz"))
                .unit("Hz"),
            ctx.read(get!(CwSkimmer, p => Some(p.bandwidth_hz)), Some(24_000.0)),
            set!(CwSkimmer, p, v => if let Some(v) = v { p.bandwidth_hz = v }),
        ),
        number(
            ctx,
            "Acquire",
            Some("Carrier threshold above the noise floor"),
            NumberSpec::new("Carrier threshold")
                .limit(ctx.limit("threshold_db"))
                .unit("dB SNR"),
            ctx.read(
                get!(CwSkimmer, p => Some(f64::from(p.threshold_db))),
                Some(10.0),
            ),
            set!(CwSkimmer, p, v => if let Some(v) = v { p.threshold_db = v as f32 }),
        ),
        number(
            ctx,
            "Signals",
            Some("Maximum simultaneous CW signals"),
            NumberSpec::new("Maximum signals").limit(ctx.limit("max_signals")),
            ctx.read(
                get!(CwSkimmer, p => Some(f64::from(p.max_signals))),
                Some(32.0),
            ),
            set!(CwSkimmer, p, v => if let Some(v) = v { p.max_signals = v as u16 }),
        ),
        number(
            ctx,
            "Speed",
            Some("Morse speed, empty to track each signal"),
            NumberSpec::new("Morse speed")
                .limit(ctx.limit("wpm"))
                .unit("WPM")
                .optional("auto"),
            ctx.read(get!(CwSkimmer, p => p.wpm.map(f64::from)), None),
            set!(CwSkimmer, p, v => p.wpm = v.map(|v| v as f32)),
        ),
    ])
}

fn wsjt(ctx: Ctx, wspr: bool) -> AnyView {
    let (low, high) = if wspr {
        (1_400.0, 1_600.0)
    } else {
        (200.0, 3_000.0)
    };
    let read_low = ctx.read(
        |params: &ChannelParams| match params {
            ChannelParams::Ft8(p) | ChannelParams::Ft4(p) => Some(Some(f64::from(p.audio_low_hz))),
            ChannelParams::Wspr(p) => Some(Some(f64::from(p.audio_low_hz))),
            _ => None,
        },
        Some(low),
    );
    let read_high = ctx.read(
        |params: &ChannelParams| match params {
            ChannelParams::Ft8(p) | ChannelParams::Ft4(p) => Some(Some(f64::from(p.audio_high_hz))),
            ChannelParams::Wspr(p) => Some(Some(f64::from(p.audio_high_hz))),
            _ => None,
        },
        Some(high),
    );
    let read_candidates = ctx.read(
        |params: &ChannelParams| match params {
            ChannelParams::Ft8(p) | ChannelParams::Ft4(p) => {
                Some(Some(f64::from(p.max_candidates)))
            }
            ChannelParams::Wspr(p) => Some(Some(f64::from(p.max_candidates))),
            _ => None,
        },
        Some(200.0),
    );
    rows(vec![
        number(
            ctx,
            "Audio from",
            Some("Lowest USB audio frequency searched"),
            NumberSpec::new("Audio from")
                .limit(ctx.limit("audio_low_hz"))
                .unit("Hz"),
            read_low,
            |params: &mut ChannelParams, v: Option<f64>| {
                let Some(v) = v else { return };
                match params {
                    ChannelParams::Ft8(p) | ChannelParams::Ft4(p) => p.audio_low_hz = v as f32,
                    ChannelParams::Wspr(p) => p.audio_low_hz = v as f32,
                    _ => {}
                }
            },
        ),
        number(
            ctx,
            "Audio to",
            Some("Highest USB audio frequency searched"),
            NumberSpec::new("Audio to")
                .limit(ctx.limit("audio_high_hz"))
                .unit("Hz"),
            read_high,
            |params: &mut ChannelParams, v: Option<f64>| {
                let Some(v) = v else { return };
                match params {
                    ChannelParams::Ft8(p) | ChannelParams::Ft4(p) => p.audio_high_hz = v as f32,
                    ChannelParams::Wspr(p) => p.audio_high_hz = v as f32,
                    _ => {}
                }
            },
        ),
        number(
            ctx,
            "Candidates",
            Some("Maximum synchronized signals tried per decode pass"),
            NumberSpec::new("Candidates").limit(ctx.limit("max_candidates")),
            read_candidates,
            |params: &mut ChannelParams, v: Option<f64>| {
                let Some(v) = v else { return };
                match params {
                    ChannelParams::Ft8(p) | ChannelParams::Ft4(p) => p.max_candidates = v as u16,
                    ChannelParams::Wspr(p) => p.max_candidates = v as u16,
                    _ => {}
                }
            },
        ),
    ])
}

fn psk(ctx: Ctx) -> AnyView {
    rows(vec![
        select(
            ctx,
            "Mode",
            Some("PSK symbol rate"),
            listed(PSK_BAUDS),
            ctx.read(get!(Psk, p => p.baud), PskBaud::Psk31),
            set!(Psk, p, v => p.baud = v),
        ),
        toggle(
            ctx,
            "Invert",
            INVERT,
            ctx.read(get!(Psk, p => p.invert), false),
            set!(Psk, p, v => p.invert = v),
        ),
    ])
}

fn radio_clock(ctx: Ctx) -> AnyView {
    rows(vec![
        select(
            ctx,
            "Service",
            Some("Radio clock service"),
            listed(RADIO_CLOCK_STANDARDS),
            ctx.read(get!(RadioClock, p => p.standard), RadioClockStandard::Dcf77),
            set!(RadioClock, p, v => p.standard = v),
        ),
        toggle(
            ctx,
            "Invert",
            INVERT,
            ctx.read(get!(RadioClock, p => p.invert), false),
            set!(RadioClock, p, v => p.invert = v),
        ),
    ])
}

fn gnss(ctx: Ctx) -> AnyView {
    rows(vec![
        number(
            ctx,
            "GPS PRN",
            Some("GPS L1 C/A satellite PRN"),
            NumberSpec::new("GPS PRN").limit(ctx.limit("prn")),
            ctx.read(get!(Gnss, p => Some(f64::from(p.prn))), Some(1.0)),
            set!(Gnss, p, v => if let Some(v) = v { p.prn = v as u8 }),
        ),
        number(
            ctx,
            "Doppler",
            Some("Symmetric Doppler search span"),
            NumberSpec::new("Doppler")
                .limit(ctx.limit("doppler_hz"))
                .unit("Hz"),
            ctx.read(
                get!(Gnss, p => Some(f64::from(p.doppler_hz))),
                Some(10_000.0),
            ),
            set!(Gnss, p, v => if let Some(v) = v { p.doppler_hz = v as u32 }),
        ),
        number(
            ctx,
            "Acquire above",
            Some("Correlation peak-to-floor acquisition threshold"),
            NumberSpec::new("Acquire above")
                .limit(ctx.limit("threshold"))
                .unit("× floor"),
            ctx.read(
                get!(Gnss, p => Some(f64::from(p.threshold))),
                Some(f64::from(GnssParams::default().threshold)),
            ),
            set!(Gnss, p, v => if let Some(v) = v { p.threshold = v as f32 }),
        ),
    ])
}

fn vor(ctx: Ctx) -> AnyView {
    let station = ctx.read(
        get!(Vor, p => p.station.clone().unwrap_or_default()),
        String::new(),
    );
    rows(vec![
        setting_row(
            "Station",
            Some("VOR station identifier"),
            text_field(
                station,
                "VOR station identifier",
                "Optional identifier",
                "kc-num kc-num--wide",
                move |text| {
                    ctx.write(move |params| {
                        if let ChannelParams::Vor(p) = params {
                            p.station = (!text.is_empty()).then_some(text);
                        }
                    });
                    true
                },
            ),
        ),
        number(
            ctx,
            "Station latitude",
            Some("VOR station latitude"),
            NumberSpec::new("VOR station latitude")
                .limit(ctx.limit("station_lat"))
                .unit("°")
                .optional("Unknown"),
            ctx.read(get!(Vor, p => p.station_lat), None),
            set!(Vor, p, v => p.station_lat = v),
        ),
        number(
            ctx,
            "Station longitude",
            Some("VOR station longitude"),
            NumberSpec::new("VOR station longitude")
                .limit(ctx.limit("station_lon"))
                .unit("°")
                .optional("Unknown"),
            ctx.read(get!(Vor, p => p.station_lon), None),
            set!(Vor, p, v => p.station_lon = v),
        ),
        number(
            ctx,
            "Declination",
            Some("East-positive magnetic declination at the VOR"),
            NumberSpec::new("Declination")
                .limit(ctx.limit("magnetic_declination_deg"))
                .unit("°"),
            ctx.read(get!(Vor, p => Some(p.magnetic_declination_deg)), Some(0.0)),
            set!(Vor, p, v => if let Some(v) = v { p.magnetic_declination_deg = v }),
        ),
        number(
            ctx,
            "Report every",
            Some("VOR report interval"),
            NumberSpec::new("VOR report interval")
                .limit(ctx.limit("report_ms"))
                .unit("ms"),
            ctx.read(get!(Vor, p => Some(f64::from(p.report_ms))), Some(500.0)),
            set!(Vor, p, v => if let Some(v) = v { p.report_ms = v as u32 }),
        ),
    ])
}

fn ils(ctx: Ctx) -> AnyView {
    rows(vec![
        segmented(
            ctx,
            "Component",
            Some("ILS component"),
            ILS_COMPONENTS,
            ctx.read(get!(Ils, p => p.component), IlsComponent::Localizer),
            set!(Ils, p, v => p.component = v),
        ),
        number(
            ctx,
            "Report every",
            Some("ILS report interval"),
            NumberSpec::new("ILS report interval")
                .limit(ctx.limit("report_ms"))
                .unit("ms"),
            ctx.read(get!(Ils, p => Some(f64::from(p.report_ms))), Some(500.0)),
            set!(Ils, p, v => if let Some(v) = v { p.report_ms = v as u32 }),
        ),
    ])
}

fn atv(ctx: Ctx) -> AnyView {
    rows(vec![
        segmented(
            ctx,
            "Modulation",
            Some("Modulation"),
            ATV_MODULATIONS,
            ctx.read(get!(Atv, p => p.modulation), AtvModulation::Am),
            set!(Atv, p, v => p.modulation = v),
        ),
        select(
            ctx,
            "Lines",
            Some("Scanning standard"),
            listed(ATV_STANDARDS),
            ctx.read(get!(Atv, p => p.standard), AtvStandard::Ccir625),
            set!(Atv, p, v => p.standard = v),
        ),
        bandwidth(
            ctx,
            "Bandwidth",
            CHANNEL_BANDWIDTH,
            &[500_000.0, 1_000_000.0, 1_500_000.0, 1_600_000.0],
            ctx.read(get!(Atv, p => p.bandwidth_hz), 1_500_000.0),
            set!(Atv, p, v => p.bandwidth_hz = v),
        ),
        select(
            ctx,
            "Colour",
            Some("Composite colour system"),
            listed(ATV_COLORS),
            ctx.read(get!(Atv, p => p.color), AtvColor::Monochrome),
            set!(Atv, p, v => p.color = v),
        ),
        number(
            ctx,
            "Sound",
            Some("FM sound subcarrier, empty for none"),
            NumberSpec::new("Sound subcarrier")
                .limit(scaled_limit(ctx.limit("sound_subcarrier_hz"), 1e-6))
                .unit("MHz")
                .optional("off"),
            ctx.read(
                get!(Atv, p => p.sound_subcarrier_hz.map(|hz| hz / 1e6)),
                None,
            ),
            set!(Atv, p, v => p.sound_subcarrier_hz = v.map(|mhz| (mhz * 1e6).round())),
        ),
        toggle(
            ctx,
            "Interlace",
            Some("Weave both fields into one frame"),
            ctx.read(get!(Atv, p => p.interlace), true),
            set!(Atv, p, v => p.interlace = v),
        ),
        toggle(
            ctx,
            "Invert",
            INVERT,
            ctx.read(get!(Atv, p => p.invert), false),
            set!(Atv, p, v => p.invert = v),
        ),
    ])
}

fn sstv(ctx: Ctx) -> AnyView {
    rows(vec![
        segmented(
            ctx,
            "Modulation",
            Some("Modulation"),
            SSTV_MODULATIONS,
            ctx.read(get!(Sstv, p => p.modulation), SstvModulation::Usb),
            set!(Sstv, p, v => p.modulation = v),
        ),
        select(
            ctx,
            "Mode",
            Some("Scanning mode"),
            sstv_modes(),
            ctx.read(get!(Sstv, p => p.mode), None),
            set!(Sstv, p, v => p.mode = v),
        ),
        toggle(
            ctx,
            "Slant correction",
            Some("Straighten pictures from a sender whose clock runs off"),
            ctx.read(get!(Sstv, p => p.slant_correction), true),
            set!(Sstv, p, v => p.slant_correction = v),
        ),
        toggle(
            ctx,
            "Keep unfinished pictures",
            Some("Keep unfinished pictures"),
            ctx.read(get!(Sstv, p => p.keep_partial), true),
            set!(Sstv, p, v => p.keep_partial = v),
        ),
    ])
}

fn dab(ctx: Ctx) -> AnyView {
    let mode = ctx.read(get!(Dab, p => p.transmission_mode), DabTransmissionMode::I);
    let detected = Memo::new(move |_| {
        ctx.broadcast
            .get()
            .and_then(|status| status.transmission_mode)
    });
    let transmission = move || {
        select(
            ctx,
            "Transmission",
            Some("DAB transmission mode"),
            dab_transmission_modes(detected.get()),
            mode,
            set!(Dab, p, v => p.transmission_mode = v),
        )
    };
    rows(vec![
        segmented(
            ctx,
            "Generation",
            Some("DAB generation"),
            DAB_MODES,
            ctx.read(get!(Dab, p => p.mode), DabMode::Auto),
            set!(Dab, p, v => p.mode = v),
        ),
        AnyView::new(transmission),
        service_picker(
            ctx,
            f64::from(u32::MAX),
            ctx.read(get!(Dab, p => p.service_id), None),
            set!(Dab, p, v => p.service_id = v),
        ),
    ])
}

fn superframe_rows(ctx: Ctx) -> AnyView {
    rows(vec![
        number(
            ctx,
            "Ref code",
            Some("Superframe reference scrambling code n_Ref, 0 by default"),
            NumberSpec::new("Superframe reference code").limit(ctx.limit("superframe_reference")),
            ctx.read(
                get!(Datv, p => Some(f64::from(p.superframe_reference))),
                Some(0.0),
            ),
            set!(Datv, p, v => if let Some(v) = v { p.superframe_reference = v as u32 }),
        ),
        number(
            ctx,
            "Data code",
            Some("Superframe payload scrambling code n_Pay, 0 by default"),
            NumberSpec::new("Superframe payload code").limit(ctx.limit("superframe_payload")),
            ctx.read(
                get!(Datv, p => Some(f64::from(p.superframe_payload))),
                Some(0.0),
            ),
            set!(Datv, p, v => if let Some(v) = v { p.superframe_payload = v as u32 }),
        ),
        toggle(
            ctx,
            "Code search",
            Some("Find unknown scrambling codes from the signal; needs a clean carrier"),
            ctx.read(get!(Datv, p => p.superframe_search), false),
            set!(Datv, p, v => p.superframe_search = v),
        ),
    ])
}

fn dvb_s2(ctx: Ctx) -> AnyView {
    let superframes = ctx.read(get!(Datv, p => p.superframes), false);
    let codes = move || superframes.get().then(|| superframe_rows(ctx));
    rows(vec![
        select(
            ctx,
            "Roll-off",
            Some("Match the transmitter's filter shape; DVB-S2 signals it in the base band header"),
            listed(DATV_ROLL_OFFS),
            ctx.read(get!(Datv, p => p.roll_off), DatvRollOff::Pct35),
            set!(Datv, p, v => p.roll_off = v),
        ),
        toggle(
            ctx,
            "Superframes",
            Some("Receive DVB-S2X Annex E superframes, formats 0 to 7"),
            superframes,
            set!(Datv, p, v => p.superframes = v),
        ),
        AnyView::new(codes),
        number(
            ctx,
            "Input stream",
            Some("Choose an input stream identifier on a multistream carrier"),
            NumberSpec::new("DVB-S2 input stream")
                .limit(NumberLimit::new(0.0, 255.0, 1.0))
                .optional("Auto"),
            ctx.read(get!(Datv, p => p.input_stream.map(f64::from)), None),
            set!(Datv, p, v => p.input_stream = v.map(|v| v as u8)),
        ),
    ])
}

fn datv(ctx: Ctx) -> AnyView {
    let standard = ctx.read(get!(Datv, p => p.standard), DatvStandard::DvbS);
    let by_standard = move || {
        if standard.get() == DatvStandard::DvbS2 {
            return dvb_s2(ctx);
        }
        select(
            ctx,
            "Code rate",
            Some("DVB-S code rate"),
            listed(DATV_CODE_RATES),
            ctx.read(get!(Datv, p => p.code_rate), DatvCodeRate::Auto),
            set!(Datv, p, v => p.code_rate = v),
        )
    };
    rows(vec![
        segmented(
            ctx,
            "Standard",
            Some("DATV standard"),
            DATV_STANDARDS,
            standard,
            set!(Datv, p, v => p.standard = v),
        ),
        number(
            ctx,
            "Symbol rate",
            Some("DATV symbol rate"),
            NumberSpec::new("DATV symbol rate")
                .limit(ctx.limit("symbol_rate"))
                .unit("Bd"),
            ctx.read(get!(Datv, p => Some(p.symbol_rate)), Some(333_000.0)),
            set!(Datv, p, v => if let Some(v) = v { p.symbol_rate = v }),
        ),
        service_picker(
            ctx,
            65_535.0,
            ctx.read(get!(Datv, p => p.program.map(u32::from)), None),
            set!(Datv, p, v => p.program = v.and_then(|v| u16::try_from(v).ok())),
        ),
        AnyView::new(by_standard),
    ])
}

fn dvbt(ctx: Ctx) -> AnyView {
    let standard = ctx.read(get!(Dvbt, p => p.standard), DvbtStandard::DvbT);
    let by_standard = move || {
        if standard.get() == DvbtStandard::DvbT2 {
            return number(
                ctx,
                "PLP",
                Some("PLP ID, empty for automatic selection"),
                NumberSpec::new("PLP")
                    .limit(NumberLimit::new(0.0, 255.0, 1.0))
                    .optional("auto"),
                ctx.read(get!(Dvbt, p => p.plp.map(f64::from)), None),
                set!(Dvbt, p, v => p.plp = v.map(|v| v as u8)),
            );
        }
        toggle(
            ctx,
            "Low priority stream",
            Some("Decode the low priority transport stream of a hierarchical DVB-T multiplex"),
            ctx.read(get!(Dvbt, p => p.low_priority), false),
            set!(Dvbt, p, v => p.low_priority = v),
        )
    };
    rows(vec![
        segmented(
            ctx,
            "Standard",
            Some("Terrestrial standard"),
            DVBT_STANDARDS,
            standard,
            set!(Dvbt, p, v => p.standard = v),
        ),
        select(
            ctx,
            "Bandwidth",
            Some("DVB-T bandwidth"),
            listed(DVBT_BANDWIDTHS),
            ctx.read(get!(Dvbt, p => p.bandwidth), DvbtBandwidth::Mhz8),
            set!(Dvbt, p, v => p.bandwidth = v),
        ),
        AnyView::new(by_standard),
        service_picker(
            ctx,
            65_535.0,
            ctx.read(get!(Dvbt, p => p.program.map(u32::from)), None),
            set!(Dvbt, p, v => p.program = v.and_then(|v| u16::try_from(v).ok())),
        ),
    ])
}

fn drm(ctx: Ctx) -> AnyView {
    let mode = ctx.read(get!(Drm, p => p.mode), DrmMode::Auto);
    let width = move || {
        let options = if mode.get() == DrmMode::Drm30 {
            DRM30_BANDWIDTHS_HZ
        } else {
            DRM_PLUS_BANDWIDTHS_HZ
        };
        bandwidth(
            ctx,
            "Bandwidth",
            CHANNEL_BANDWIDTH,
            options,
            ctx.read(get!(Drm, p => p.bandwidth_hz), 100_000.0),
            set!(Drm, p, v => p.bandwidth_hz = v),
        )
    };
    rows(vec![
        segmented(
            ctx,
            "Mode",
            Some("DRM mode"),
            DRM_MODES,
            mode,
            set!(Drm, p, v => {
                p.bandwidth_hz = drm_bandwidth_for(v, p.mode, p.bandwidth_hz);
                p.mode = v;
            }),
        ),
        AnyView::new(width),
        service_picker(
            ctx,
            3.0,
            ctx.read(get!(Drm, p => p.service.map(u32::from)), None),
            set!(Drm, p, v => p.service = v.and_then(|v| u8::try_from(v).ok())),
        ),
    ])
}

fn ident(ctx: Ctx) -> AnyView {
    rows(vec![
        bandwidth(
            ctx,
            "Search width",
            Some("Search bandwidth"),
            &[12_500.0, 50_000.0, 100_000.0, 192_000.0],
            ctx.read(get!(Ident, p => p.bandwidth_hz), 192_000.0),
            set!(Ident, p, v => p.bandwidth_hz = v),
        ),
        number(
            ctx,
            "Report every",
            Some("Milliseconds of signal each report is measured from"),
            NumberSpec::new("Report every")
                .limit(ctx.limit("interval_ms"))
                .unit("ms"),
            ctx.read(
                get!(Ident, p => Some(f64::from(p.interval_ms))),
                Some(1_000.0),
            ),
            set!(Ident, p, v => if let Some(v) = v { p.interval_ms = v as u32 }),
        ),
        number(
            ctx,
            "Detect above",
            Some("Decibels above the noise floor a signal must reach"),
            NumberSpec::new("Detect above")
                .limit(ctx.limit("threshold_db"))
                .unit("dB"),
            ctx.read(get!(Ident, p => Some(f64::from(p.threshold_db))), Some(8.0)),
            set!(Ident, p, v => if let Some(v) = v { p.threshold_db = v as f32 }),
        ),
    ])
}
