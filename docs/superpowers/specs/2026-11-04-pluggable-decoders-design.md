# Pluggable decoders

Goal: a decoder is one module in `channels` or one WASM plugin. Engine, server, wire and web never
name a decoder. The frontend renders settings, readouts and events from the registry.

## Registry (`channels::registry`)

- `trait Decoder: Send + Sync`: `descriptor()`, `create_rx`, `create_tx`, `layout(&params)`,
  `event_kinds()`, `presets()`, `signatures()`.
- `ChannelLayout { input_rate_hz, occupied_band, filter, audio_channels, click_profile }` replaces the
  `match ChannelParams` functions in `channels/lib.rs`.
- One process registry: `ArcSwap<Registry>`. Built-ins register at startup, plugins at load and
  unload. The DSP plane never reads it; it holds the `Box<dyn ChannelRx>` it was given.

## Settings

- Wire: `DecoderParams { type_id, settings: Value }`, serialized `{"type", "settings"}` as today.
- Each decoder owns its typed settings struct (`serde` + `schemars::JsonSchema`) and parses it on
  the control plane in `create_rx` / `apply`.
- `ChannelDescriptor.settings: Vec<SettingField>`, derived from the JSON Schema plus the decoder's
  limits, choices, units and hints. The web form renders only this.
- Live choices (a DAB service list) and face readouts come from `ChannelStatus`, a generic decoder
  output replacing `BroadcastStatus`.

## Events

- Wire: `DecoderEvent { kind, data: Value }`, serialized `{"kind", "data"}` as today.
- Decoder kinds are registered with `EventKindSpec { info, present, merge }`; the server presents
  through the registry. Default merge is the JSON overlay.
- Platform kinds that several decoders share stay typed in wire as contracts, without naming a
  decoder: voice frame (calls, trunking), transmission, call, df, radar, nav, satellite.
- Platform consumers read contracts or capabilities, never kinds: radar truth takes any `Track`
  event, exporters (Beast, IP tunnel) are registered by the decoder that owns the format.

## Platform features

- Trunking: one generic trunk node over decoders that declare a trunk capability.
- Monitor: decoders declare their own identification signatures and how to tune from one.
- Templates and band plan suggestions are data, filtered to registered types.
- Synth catalog is assembled from registrations.

## Plugins (`plugin-host`)

- wasmtime component model, epoch interruption per `process` call, compiled module cache.
- WIT generated from `wire`. Host imports expose `dsp` kernels (FIR, FFT, resampler) working on
  guest memory, so the inner loops run native.
- Loaded at startup and on change from the plugins directory; unload stops its channels with an
  error.
- `sdrmm-plugin` guest SDK re-exports `dsp` and `modem`.

## Order

1. Registry trait and layout methods.
2. Dynamic settings and generated forms.
3. Dynamic events, registry presenters, contracts.
4. Generic channel status.
5. Trunking, exporters, radar truth, monitor, templates, synth.
6. Frontend generic views, per-decoder code deleted.
7. Shared decoder code into `dsp` / `modem`.
8. WASM host, SDK, sample plugin.
