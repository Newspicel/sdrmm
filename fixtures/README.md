# IQ fixture library

This directory holds short IQ signals for decoder regression tests and playback in the app.
Each fixture has a known expected output. Generated waveforms test the decoder against the
project's modulators; recorded captures also exercise real transmitter and reception effects.

Every IQ fixture is a single-channel SigMF pair: `<stem>.sigmf-meta` and `<stem>.sigmf-data`,
using `cf32_le`. Open it as a `virtual:file:<stem>` source and set the channel to the
fixture's centre plus the offset listed below. Rates in the tables are samples per second; `k` means thousands and `M` means millions.

## Generated fixtures

Run `cargo xtask fixtures` to create these pairs. The task uses the same modulators as unit and
engine tests. Most use `channels::testgen`; APRS uses its `ChannelTx` implementation and resamples
to the fixture's device rate.

Generated pairs are ignored by Git. Commit generator and expected-output changes together.

| stem | rate | channel | expected |
|---|---|---|---|
| `siggen_2m4_1s` | 2.4 M | any demod | the virtual siggen's tones (record/replay fixture) |
| `pocsag_1200_240k` | 240 k | `pocsag` @ +50 kHz | address 1234567, `SDR-- FIXTURE` |
| `flex_1600_2_240k` | 240 k | `flex` @ +30 kHz | address 1234567, `SDR-- FLEX FIXTURE`, cycle 7 frame 83 |
| `ermes_alpha_240k` | 240 k | `ermes` @ −30 kHz | address 234567, urgent alert 5, `SDR-- ERMES FIXTURE` |
| `selcall_ccir1_48k` | 48 k | `selcall` @ +5 kHz, CCIR-1 | `12234`, including the repeat marker |
| `selcall_zvei1_48k` | 48 k | `selcall` @ −5 kHz, ZVEI-1 | `A11D0`, including group and repeat symbols |
| `ais_position_240k` | 240 k | `ais` @ +25 kHz | MMSI 211234560 at 53.5413, 9.9846 |
| `aprs_afsk1200_240k` | 240 k | `aprs` @ −40 kHz | `DL1ABC-9>APRS,WIDE1-1` at 52.5, 13.4 |
| `rtty_45_170_48k` | 48 k | `rtty` @ +5 kHz | `CQ CQ DE DL1ABC K` |
| `morse_20wpm_48k` | 48 k | `morse` @ −5 kHz | `CQ DE DL1ABC K` at 20 wpm |
| `cw_skimmer_dual_48k` | 48 k | `cw_skimmer` @ 0 Hz | simultaneous `DL1AAA` at −3.5 kHz/18 wpm and `G4BBB` at +4.2 kHz/27 wpm |
| `adsb_squitters_2m` | 2 M | `adsb` @ 0 Hz | `3C6444`/`DLH123`, FL380, a solved position |
| `rds_station_960k` | 960 k | `wfm` @ +200 kHz, `rds` on | PI `D3C2`, PS `SDR-M4`, 1 kHz audio |
| `navtex_518_48k` | 48 k | `navtex` @ +3 kHz | `DA07` navigational warning, `GALE WARNING` |
| `acars_downlink_240k` | 240 k | `acars` @ −40 kHz | `D-AIBC` / `LH0400` `[H1]`, `SDR-- FIXTURE` |
| `ysf_callsigns_48k` | 48 k | `ysf` @ 0 Hz | `DL1ABC` to `ALL` via `DB0XYZ` and `DB0ABC` |
| `atv_ccir625_2m4` | 2.4 M | `atv` @ +200 kHz | 625/25 AM, five vertical bars black to white |
| `sstv_robot36_48k` | 48 k | `sstv` @ +4 kHz | Robot 36, eight colour bars white to black |
| `dcf77_2026_2k` | 2 k | `radio_clock` / DCF77 @ 0 Hz | 2026-08-15 12:34 CET, valid parity |
| `gps_l1_ca_prn7_2m048` | 2.048 M | `gnss` / PRN 7 @ 0 Hz | +1 kHz Doppler, 158.3-chip code phase |
| `dect_base_2m304` | 2.304 M | `dect` @ 0 Hz | RFPI `01234D5E6D`, class A, carrier 4, standard authentication and ciphering advertised |
| `dect_call_2m304` | 2.304 M | `dect` @ 0 Hz | clear call on RFPI `01234D5E6D`: 1 kHz from the base, 600 Hz from the handset |

## Committed fixtures

These thirty-three pairs are not regenerated: nineteen are recordings, two are frozen synthetic waveforms,
and twelve are reference waveforms from generators that are independent of the Rust modulators.
They retain cases that the current generators do not reproduce.

`cargo xtask excerpt` trims a SigMF pair, WAV, or raw `cu8`/`cs8`/`cs16`/`cf32` capture. It shifts
and resamples through the engine's `Ddc`, writes the requested window, and records the source
SHA-256 in a SigMF annotation. `cargo xtask net-capture` records a public KiwiSDR or SpyServer to
a SigMF pair; `cargo xtask replay` runs a channel over it.

| stem | rate | channel | expected |
|---|---|---|---|
| `dmr_call_48k` | 48 k | `dmr` @ 0 Hz | colour code 1, group call, radio ID 12345678 to talkgroup 12345678 |
| `dmr_tier3_control_48k` | 48 k | `dmr` @ 0 Hz | colour code 10 Capacity Max control channel, grants for logical channels 22 (slot 2) and 42 (slot 1) between radios 9995 and 9999 |
| `dmr_capacity_plus_48k` | 48 k | `dmr` @ 0 Hz | colour code 5 Capacity Plus rest channel, a talkgroup 101 call handed from timeslot 1 to timeslot 2 and the rest channel moving 4 → 3 with it |
| `freedv_1600_8k` | 8 k | `freedv` @ 0 Hz, USB | FreeDV 1600 sync and decoded Codec2 speech |
| `ais_position_pre_cpm_240k` | 240 k | `ais` @ +25 kHz | MMSI 211234560 at 53.5413, 9.9846 |
| `nxdn_addressed_48k` | 48 k | `nxdn` @ 0 Hz | RAN 17, radio 12345 to talkgroup 234 via FACCH/SACCH |
| `adsb_offair_2m` | 2 M | `adsb` @ 0 Hz | 17 Mode S replies from four aircraft: DF4/5/11/17/20/21, FL370 and squawk 5245 from 4D2256, a TC11 position and a TC19 velocity from 3FF91D |
| `ft8_20m_busy_12k` | 12 k | `ft8` @ 0 Hz | 19 of the 20 decodes `ft8_lib` publishes for this slot |
| `acars_offair_48k` | 48 k | `acars` @ 0 Hz | `F-GTAE` / `AF7728` `[H1]` engine report `#DFB00000/V206,...`, then `LN-DYY` `[_d]` acknowledging block 5 |
| `inmarsat_stdc_egc_24k` | 24 k | `inmarsat_stdc` @ +216 Hz | NCS bulletin board for frame 5987 (LES 144), an LES 104 announcement, the signalling channel |
| `inmarsat_aero_offair_48k` | 48 k | `inmarsat_aero` @ 0 Hz | 600 bps P channel, `HL8217` `[_d]` |
| `iridium_prbs15_250k` | 250 k | `iridium` @ 0 Hz | one downlink burst whose payload holds the PRBS15 recurrence bit for bit |
| `lora_rn2483_sf7_250k` | 250 k | `lora` @ 0 Hz, 125 kHz | two RN2483 frames, SF7 CR 4/8, payload `deadbeef`, CRC ok |
| `lora_epfl_sf7_strong_250k` | 250 k | `lora` @ 0 Hz, 125 kHz | SF7 CR 4/5, payload `0000967`, 32 dB |
| `lora_epfl_sf7_offgrid_250k` | 250 k | `lora` @ 0 Hz, 125 kHz | SF7 CR 4/5, payload `0000158`, 34 dB |
| `lora_epfl_sf7_weak_250k` | 250 k | `lora` @ 0 Hz, 125 kHz | SF7 CR 4/5, payload `0001413`, −6 dB |
| `lora_epfl_sf10_500k` | 500 k | `lora` @ 0 Hz, 250 kHz | SF10 CR 4/5, 19 bytes from a drone, 6 dB |
| `lora_epfl_sf10_weak_500k` | 500 k | `lora` @ 0 Hz, 250 kHz | SF10 CR 4/5, 19 bytes from a drone, −14 dB |
| `lora_grlorasdr_sf7_250k` | 250 k | `lora` @ 0 Hz, 125 kHz | gr-lora_sdr transmitter, SF7 sync 0x12, `sdrflex lora fixture` twice |
| `dsc_offair_8k` | 8 k | `dsc` @ 0 Hz | Lyngby Radio `002191000` acknowledging a test call from `231700000` |
| `radio_clock_dcf77_offair_2k` | 2 k | `radio_clock` / DCF77 @ 0 Hz | 2026-10-03 22:04 CEST |
| `radio_clock_jjy_offair_2k` | 2 k | `radio_clock` / JJY @ 0 Hz | 2026-10-04 05:16 JST |
| `radio_clock_wwvb_offair_2k` | 2 k | `radio_clock` / WWVB @ 0 Hz | 2021-03-14 05:35 UTC |
| `flex_p2000_offair_48k` | 48 k | `flex` @ 0 Hz | P2000 page to capcode 2029574, `A2 DP2 Leidschendam-Voorburg ...` |
| `dvbt/qpsk_2k_reference` | 9.142857 M | `dvbt` @ 0 Hz | 2K QPSK, rate 1/2, guard 1/8, 1750 Hz offset; PID 0x123 packets, TPS cell 0x5a |
| `dab/mode_ii_reference_2m048` | 2.048 M | `dab` @ 0 Hz | Mode II frame: ensemble `0x4a2c`, service `0xc201`, no failed FIB CRCs |
| `dab/mode_iii_reference_2m048` | 2.048 M | `dab` @ 0 Hz | the same ensemble in Mode III |
| `dab/mode_iv_reference_2m048` | 2.048 M | `dab` @ 0 Hz | the same ensemble in Mode IV |
| `dvbs2x/pls132` | symbol rate | `datv` unit tests | PLS 132 QPSK, PID 0x123 packets with payload `(132 + i + j) % 256` |
| `dvbs2x/pls138` | symbol rate | `datv` unit tests | PLS 138 8APSK |
| `dvbs2x/pls184` | symbol rate | `datv` unit tests | PLS 184 64APSK |
| `dvbs2x/pls200` | symbol rate | `datv` unit tests | PLS 200 128APSK |
| `dvbs2x/pls214` | symbol rate | `datv` unit tests | PLS 214 256APSK |
| `dvbs2x/pls248` | symbol rate | `datv` unit tests | PLS 248 short-frame 32APSK |
| `dvbs2x/superframe0` | symbol rate | `datv` unit tests | 72 repeated 256APSK PLFRAMEs in one Annex E format 0 container |

## Playback notes

SSTV and ATV produce pictures. Connect their video output to a Video node. Completed SSTV pictures
also enter the server's picture store, with one decoder-log event per picture; `GET /api/images`
serves the PNGs.

ADS-B and GNSS process device-rate samples without the resampling DDC. ADS-B requires 2–4 MS/s;
GNSS requires 2.048 MS/s.

## Naming and review

Use `<decoder>_<description>_<rate>` for decoder fixtures. Signal-generator captures may also
include duration, as in `siggen_2m4_1s` for 2.4 MS/s over one second.

Keep recorded captures short and restrict them to the relevant band. Include expected output,
source provenance, and applicable license information. Committed IQ files require an explicit
force-add because `.gitignore` excludes SigMF pairs.

Give frozen renders a separate stem from generated fixtures. Otherwise regeneration could overwrite
the waveform a regression test is supposed to preserve.

## Provenance and regression coverage

### Independent reference waveforms: `dab/`, `dvbs2x/`, `dvbt/`

Python and NumPy generators outside the Rust modulators produce these pairs, so a broadcast
decoder cannot pass by agreeing with a transmitter that shares its mistake. `cargo xtask fixtures`
does not rebuild them and CI does not run their generators: the tests read them with
`include_bytes!`, so the pairs are force-added past `.gitignore`. Each directory's README records
its generator, the standard clauses it follows, and the expected output.

The `dvbt` and `dvbs2x` pairs are `ci16_le` rather than `cf32_le`, because their generators write
what the receivers read. The `dvbs2x` pairs carry one sample per symbol and feed the receiver
stages directly rather than a device-rate channel, so they have no playback offset.

### DMR direct mode: `dmr_call_48k`

A 1.7-second PMR446 channel 1 call captured with an RTL-SDR at 2.048 MS/s, then down-converted to
48 kS/s. It preserves transmitter keying gaps between TDMA bursts.
`dv::dmr::tests::decodes_a_recorded_call` reads it directly.

### DMR Tier III: `dmr_tier3_control_48k`

A 2.3-second Motorola Capacity Max control-channel excerpt from a 2.4 MS/s capture centred on
460.802929 MHz, down-converted to 48 kS/s. It contains two channel grants and CSBK trailers that
do not match the standard payload CRC despite clean BPTC decoding.

Tests cover direct decoding, delivery through the channel filter in engine-sized blocks, and
reception with default settings:

- `dv::dmr::tests::decodes_a_recorded_tier_three_control_channel`
- `dv::dmr::tests::a_live_control_channel_decodes_in_the_blocks_a_radio_delivers`
- `dv::dmr::tests::a_site_that_masks_its_checksums_is_read_without_being_asked_to_be`

### DMR Capacity Plus: `dmr_capacity_plus_48k`

A 2.4-second rest-channel excerpt at 460.800 MHz from the same source capture as the Tier III
fixture. Talkgroup 101 moves from timeslot 1 to timeslot 2 while the rest channel changes from
4 to 3. Its channel-status CSBKs pass the standard CRC. The capture contains only one talkgroup,
so it does not establish the full address-field width.
`dv::dmr::tests::decodes_a_recorded_capacity_plus_rest_channel` reads it directly.

### AIS frozen render: `ais_position_pre_cpm_240k`

A 0.03-second burst from the earlier AIS generator, including its stepped envelope, re-rendered
once with the corrected octet order on the wire.
`ais::tests::decodes_the_committed_fixture` checks that the general CPM receiver still decodes it.
This render has 6,250 samples; the current generator produces 6,425.

### NXDN frozen render: `nxdn_addressed_48k`

A 0.69-second reference-modulator render containing a complete four-quarter SACCH message and
FACCH call addressing. Keeping the waveform fixed prevents a decoder test from regenerating its
own reference input.

### ADS-B: `adsb_offair_2m`

A 200 ms excerpt of a 2 MS/s RTL-SDR recording over the Eifel. It includes overlapping replies,
amplitude variation, and roll-call replies whose addresses depend on earlier all-call messages.
The window contains six downlink formats and four aircraft in about 3.1 MB.

`adsb::tests::decodes_a_recorded_sky` reads it directly.
`a_recorded_squitter_places_its_aircraft_against_the_receiver` checks local CPR positioning against
the global position solved from the full recording.

### FT8: `ft8_20m_busy_12k`

A 15-second 20 m slot recorded on 2019-11-11 at 11:06:15 UTC. The source is the MIT-licensed
`ft8_lib` test recording `191111_110615.wav`, pinned in the SigMF annotation. Its 12 kHz mono audio
was converted to `cf32_le` with zero quadrature, centered on the 14.074 MHz dial frequency.

The upstream expected output contains twenty messages; this integration decodes nineteen.
`weak_signal::tests::a_recorded_slot_reads_the_band_the_reference_decoder_published` appends a
quiet tail to close the sliding slot window and records the missing decode explicitly.

### ACARS: `acars_offair_48k`

Channel 1 of acarsdec's four-channel `test.wav`, 0.5 s to 1.85 s: two real VHF bursts with the
2400 Hz pre-key, the key-up transient and the receiver's noise floor. The LGPL-2.0 source is
pinned by commit and SHA-256 in the SigMF annotation. Its 12.5 kHz AM-demodulated audio was
resampled to 48 kHz and re-modulated at 80 % depth onto a 0 Hz carrier; the RF channel is not
recorded upstream. `acars::tests::decodes_the_committed_recording` reads it directly.

### Inmarsat STD-C: `inmarsat_stdc_egc_24k`

A 14-second slice of sigidwiki's Inmarsat-C TDM/EGC IQ recording, resampled to 24 kHz `ci16_le`.
The same slice is the off-air test of xng-mode-stdc. sigidwiki content is CC BY-SA, so this pair
is test data under that license. `inmarsat_stdc::tests::decodes_the_offair_recording` reads it
directly.

### Inmarsat Aero: `inmarsat_aero_offair_48k`

2.8 s of JAERO's MIT-licensed `600bps_sample.ogg`: one 600 bps P channel frame with an
ACARS block from `HL8217`. The audio carrier at 1066 Hz was mixed to 0 Hz and low-passed at
1.2 kHz; the capture frequency is nominal. `ci16_le`, like the `dvbt` pairs.
`inmarsat_aero::tests::decodes_the_recorded_600_bps_channel` reads it directly.

### Iridium: `iridium_prbs15_250k`

A burst from gr-iridium's reference modulator (`test-data/prbs15-2M-20dB.sigmf-data`, GPL-3.0
test data), as vendored by xng: downconverted to 250 kHz and trimmed to 32 ms. The signed 16-bit
IQ was scaled to `cf32_le`; the annotation pins the source SHA-256.
`iridium::tests::demodulates_the_gr_iridium_reference_burst` reads it directly.

### LoRa: `lora_rn2483_sf7_250k`

Two frames from a Microchip RN2483 in gr-lora's example trace
(`usrp-868.1-sf7-cr4-bw125-crc-0`, CC0), shifted from 868.0 to 868.1 MHz and resampled from
1 MHz to 250 kHz. The annotation pins the source SHA-256.
`lora::tests::a_real_rn2483_capture_decodes_both_frames` reads it directly.

`lora_epfl_*` are single frames from the EPFL LoRaIQ dataset (CC BY 4.0,
zenodo.org/records/17708397), recorded by USRP-2920 rooftop receivers at 862.5 MHz with the
transmitted payload known. The SF10 frames are trimmed around the frame. They cover cases real
transmitters exposed: a one bin one chip offset after coarse sync, a half bin bias from folded sync
symbols, and timing over a long frame. `lora::tests::real_epfl_frames_decode_from_strong_to_near_the_sensitivity_limit`
reads them directly. `lora_grlorasdr_sf7_250k` is gr-lora_sdr's own transmitter output from the
sdr-flex repository (CC0), so the encoder is independent of ours.

### Public receivers: `dsc_offair_8k`, `radio_clock_*_offair_2k`, `flex_p2000_offair_48k`

Excerpts of `cargo xtask net-capture` recordings from public KiwiSDRs and a public SpyServer, made
on 2026-10-03. The SigMF metadata names the receiver and pins the source SHA-256.

- `dsc_offair_8k`: 2187.5 kHz, Cobh. Real DSC puts B (bit 0) on the higher tone.
  `dsc::tests::decodes_a_coast_station_off_air`
- `radio_clock_dcf77_offair_2k`: 77.5 kHz near Hamburg, KiwiSDR AGC on, with fades between pulses.
  `radio_clock::tests::dcf77_off_air_with_agc_decodes`
- `radio_clock_jjy_offair_2k`: 40 kHz, Chiba, manual gain. `radio_clock::tests::jjy_off_air_decodes`
- `radio_clock_wwvb_offair_2k`: a public domain archive.org KiwiSDR recording, USB with AGC, carrier at
  1 kHz. `radio_clock::tests::wwvb_off_air_through_receiver_agc_decodes`
- `flex_p2000_offair_48k`: 169.650 MHz, Netherlands. FLEX sync reads the low tone as 1, the FIW
  and data the high tone. `flex::tests::decodes_a_p2000_page_off_air`

### FreeDV: `freedv_1600_8k`

The first three seconds of the FreeDV GUI project's `wav/ve9qrp_1600.wav` receive test recording.
Signed 16-bit mono audio was converted to normalized `cf32_le` with zero quadrature. The SigMF
annotation records the source SHA-256, pinned upstream commit, and LGPL-2.1 license.
`dv::freedv::tests::decodes_the_upstream_receive_recording` reads it directly.

### Meteor LRPT: `lrpt/meteor_m2_qpsk72_symbols.bin`

Not IQ: 196,864 hard QPSK symbol bits, packed MSB first, from this receiver's demodulator on a
2018 Meteor-M N2 pass on 137.9 MHz (72k QPSK). They hold VCDUs `09BF68` to `09BF73`: one full
MCU row of APIDs 64, 65 and 68. The source IQ is sigidwiki's LRPT sample
(`LRPT_16-29-02_137900kHz.wav`), which states no license. `lrpt::tests::a_recorded_meteor_m2_row_decodes_cleanly` reads it.

### End-of-Train: `eot/pyeot_demo3_48k.wav`

Not IQ: PyEOT's `demo3eot.wav`, unmodified, 2.6 s of FM-discriminator audio from 457.9375 MHz
with three rear-unit packets. GPL-3.0, see `eot/README.md`. The test FM-modulates it at ±3 kHz.
`eot::tests::decodes_three_rear_units_from_an_off_air_recording` reads it.
