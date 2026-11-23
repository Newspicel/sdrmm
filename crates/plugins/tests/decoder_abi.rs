//! What crosses the decoder ABI beyond audio and events: pictures, symbols, the symbol request
//! and the squelch default, which ABIs load, and what a manifest may not declare.

use num_complex::Complex;
use sdrmm_decoder::{ChannelCtx, ChannelOutputs, Decoder};
use sdrmm_plugins::{PluginModule, WasmDecoder};
use sdrmm_wire::{
    SymbolPlane,
    plugin::{ABI_VERSION, OutputHeader, Picture, PictureImage, plane_code},
};

mod common;

const MANIFEST: &str = r#"{"abi":5,"decoder":{"descriptor":{"type_id":"painter","name":"Painter","bandwidth_hz":1000.0,"input_rate_hz":48000.0}}}"#;
const HEADER_AT: u32 = 20_000;
const PICTURES_AT: u32 = 21_000;
const SYMBOLS_AT: u32 = 22_000;
const REFERENCE_AT: u32 = 22_500;
const SYMBOLS: [f32; 4] = [0.5, -0.5, 1.0, -1.0];
const REFERENCE: [f32; 2] = [1.0, -1.0];

fn pictures() -> Vec<u8> {
    let mut buffer = Vec::new();
    let frame = Picture {
        width: 2,
        height: 1,
        luma: &[10, 20],
        rgb: &[],
        image: None,
    };
    let image = Picture {
        width: 1,
        height: 1,
        luma: &[30],
        rgb: &[1, 2, 3],
        image: Some(PictureImage {
            source: "painter".to_owned(),
            mode: "Test".to_owned(),
            complete: true,
            lines: 1,
        }),
    };
    frame
        .write(&mut buffer)
        .unwrap_or_else(|error| panic!("{error}"));
    image
        .write(&mut buffer)
        .unwrap_or_else(|error| panic!("{error}"));
    buffer
}

fn floats(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn painter() -> WasmDecoder {
    painting(common::ABI, plane_code(Some(SymbolPlane::Level)))
        .unwrap_or_else(|error| panic!("{error}"))
}

/// A plugin built for `abi` whose every block draws a frame and an image, and taps symbols on
/// plane word `plane` only when asked.
fn painting(abi: u32, plane: u32) -> Result<WasmDecoder, String> {
    let pictures = pictures();
    let header = OutputHeader {
        pictures_ptr: PICTURES_AT,
        pictures_len: pictures.len() as u32,
        symbols_ptr: SYMBOLS_AT,
        symbols_len: SYMBOLS.len() as u32,
        reference_ptr: REFERENCE_AT,
        reference_len: REFERENCE.len() as u32,
        plane,
        symbol_rate: 4_800.0,
        mer_db: 21.0,
        freq_error_hz: -7.5,
        ..OutputHeader::default()
    };
    let data = |at: u32, bytes: &[u8]| {
        format!(r#"(data (i32.const {at}) "{}")"#, common::wat_bytes(bytes))
    };
    let extra = [
        data(HEADER_AT, &header.to_bytes()),
        data(PICTURES_AT, &pictures),
        data(SYMBOLS_AT, &floats(&SYMBOLS)),
        data(REFERENCE_AT, &floats(&REFERENCE)),
    ]
    .join("\n  ");
    let process = format!(
        "local.get 2\n    i32.const 1\n    i32.and\n    if (result i32) i32.const {HEADER_AT} else i32.const 0 end"
    );
    let text = common::wat_text(abi, MANIFEST, &process, &extra);
    let module = PluginModule::from_bytes("painter".to_owned(), text.as_bytes(), None)
        .map_err(|error| error.to_string())?;
    Ok(WasmDecoder::of(&module).unwrap_or_else(|| panic!("the plugin declares a decoder")))
}

fn run(wanted: bool) -> ChannelOutputs {
    run_on(&painter(), wanted)
}

fn run_on(decoder: &WasmDecoder, wanted: bool) -> ChannelOutputs {
    let Some(settings) = decoder.descriptor().defaults.clone() else {
        panic!("defaults");
    };
    let mut channel = decoder
        .open(
            ChannelCtx {
                input_rate: 48_000.0,
            },
            &settings,
        )
        .unwrap_or_else(|error| panic!("opens: {error}"));
    assert!(
        channel.needs_gated_input(),
        "a plugin without the hook is gated"
    );
    let mut out = ChannelOutputs::default();
    out.symbols.set_wanted(wanted);
    channel.process(&[Complex::new(0.0, 0.0); 64], &mut out);
    out
}

#[test]
fn pictures_and_symbols_cross_when_symbols_are_wanted() {
    let out = run(true);
    assert_eq!(out.video.len(), 1);
    assert_eq!(out.video[0].luma, [10, 20]);
    assert_eq!(out.images.len(), 1);
    let image = &out.images[0];
    assert_eq!((image.source.as_ref(), image.lines), ("painter", 1));
    assert_eq!(image.picture.rgb, [1, 2, 3]);
    let tap = &out.symbols;
    assert_eq!(tap.plane, Some(SymbolPlane::Level));
    assert_eq!(tap.symbols, SYMBOLS);
    assert_eq!(tap.reference, REFERENCE);
    assert_eq!(tap.symbol_rate, 4_800.0);
    assert_eq!((tap.mer_db, tap.freq_error_hz), (21.0, -7.5));
}

#[test]
fn the_symbol_request_reaches_the_plugin() {
    let out = run(false);
    assert!(out.video.is_empty() && out.images.is_empty());
    assert_eq!(out.symbols.plane, None);
    assert!(out.status.is_none(), "{:?}", out.status);
}

#[test]
fn carriers_cross_on_their_own_plane() {
    let decoder = painting(ABI_VERSION, plane_code(Some(SymbolPlane::Carriers)))
        .unwrap_or_else(|error| panic!("{error}"));
    let out = run_on(&decoder, true);
    assert_eq!(out.symbols.plane, Some(SymbolPlane::Carriers));
    assert_eq!(out.symbols.symbols, SYMBOLS);
}

#[test]
fn a_plugin_newer_or_older_than_the_host_is_refused() {
    for abi in [ABI_VERSION - 1, ABI_VERSION + 1] {
        let refused = painting(abi, 0);
        let Err(error) = refused else {
            panic!("ABI {abi} must be refused");
        };
        assert!(error.contains(&format!("ABI {abi}")), "{error}");
    }
}

#[test]
fn an_unknown_plane_stops_the_channel_on_its_face() {
    let decoder = painting(ABI_VERSION, 9).unwrap_or_else(|error| panic!("{error}"));
    let out = run_on(&decoder, true);
    assert_eq!(out.symbols.plane, None);
    let face = format!("{:?}", out.status);
    assert!(face.contains("unknown symbol plane 9"), "{face}");
}

#[test]
fn a_plugin_that_declares_trunking_is_refused() {
    let manifest = r#"{"abi":5,"decoder":{"descriptor":{"type_id":"trunker","name":"Trunker","bandwidth_hz":1000.0,"input_rate_hz":48000.0,"trunking":{"slots":2,"schemes":[]}}}}"#;
    let text = common::wat_text(common::ABI, manifest, "i32.const 0", "");
    let refused = PluginModule::from_bytes("trunker".to_owned(), text.as_bytes(), None);
    let Err(error) = refused else {
        panic!("a trunking plugin must be refused");
    };
    assert!(error.to_string().contains("trunking"), "{error}");
}

#[test]
fn a_decoder_without_the_control_call_is_refused() {
    let text = common::wat_text(common::ABI, MANIFEST, "i32.const 0", "").replace(
        r#"(func (export "sdrmm_call")"#,
        r#"(func (export "sdrmm_unused_call")"#,
    );
    let refused = PluginModule::from_bytes("no-call".to_owned(), text.as_bytes(), None);
    let Err(error) = refused else {
        panic!("a decoder without sdrmm_call must be refused");
    };
    assert!(error.to_string().contains("sdrmm_call"), "{error}");
}
