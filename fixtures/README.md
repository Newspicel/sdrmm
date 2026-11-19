# IQ fixture library

This directory holds short IQ signals for decoder regression tests and playback in the app.
Each fixture has a known expected output. Generated waveforms test the decoder against the
project's modulators; recorded captures also exercise real transmitter and reception effects.

Every IQ fixture is a single-channel SigMF pair: `<stem>.sigmf-meta` and `<stem>.sigmf-data`,
using `cf32_le`. Open it as a `virtual:file:<stem>` source and set the channel to the
fixture's centre plus the offset listed below. Rates in the tables are samples per second; `k` means thousands and `M` means millions.

## Generated fixtures

Run `cargo xtask fixtures` to create these pairs. The task uses the same modulators as unit and
engine tests. Most use `channels::synth`; APRS uses its `ChannelTx` implementation and resamples
to the fixture's device rate.

Generated pairs are ignored by Git. Commit generator and expected-output changes together.

| stem | rate | channel | expected |
|---|---|---|---|
| `siggen_2m4_1s` | 2.4 M | any demod | the virtual siggen's tones (record/replay fixture) |
| `pocsag_1200_240k` | 240 k | `pocsag` @ +50 kHz | address 1234567, `SDRmm FIXTURE` |
| `flex_1600_2_240k` | 240 k | `flex` @ +30 kHz | address 1234567, `SDRmm FLEX FIXTURE`, cycle 7 frame 83 |
| `ermes_alpha_240k` | 240 k | `ermes` @ −30 kHz | address 234567, urgent alert 5, `SDRmm ERMES FIXTURE` |
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
| `acars_downlink_240k` | 240 k | `acars` @ −40 kHz | `D-AIBC` / `LH0400` `[H1]`, `SDRmm FIXTURE` |
| `ysf_callsigns_48k` | 48 k | `ysf` @ 0 Hz | `DL1ABC` to `ALL` via `DB0XYZ` and `DB0ABC` |
| `atv_ccir625_2m4` | 2.4 M | `atv` @ +200 kHz | 625/25 AM, five vertical bars black to white |
| `sstv_robot36_48k` | 48 k | `sstv` @ +4 kHz | Robot 36, eight colour bars white to black |
| `dcf77_2026_2k` | 2 k | `radio_clock` / DCF77 @ 0 Hz | 2026-08-15 12:34 CET, valid parity |
| `gps_l1_ca_prn7_2m048` | 2.048 M | `gnss` @ 0 Hz | PRN 7, +1 kHz Doppler, 158.3-chip code phase |
| `dect_base_2m304` | 2.304 M | `dect` @ 0 Hz | RFPI `01234D5E6D`, class A, carrier 4, standard authentication and ciphering advertised |
| `dect_call_2m304` | 2.304 M | `dect` @ 0 Hz | clear call on RFPI `01234D5E6D`: 1 kHz from the base, 600 Hz from the handset |

## Committed fixtures

These pairs are not regenerated. They retain cases that the current generators do not reproduce.
Provenance and regression coverage below says which are recordings, which are frozen synthetic
waveforms, and which come from generators independent of the Rust modulators.

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
| `msk144_wa4cqg_12k` | 12 k | `msk144` @ 0 Hz | `K1JT WA4CQG EM72`, 1488 Hz, 8.7 s into the 12:05:00 period |
| `msk144_w4imd_kd9vv_12k` | 12 k | `msk144` @ 0 Hz | `CQ W4IMD EM84` at 4.6 s and `CQ KD9VV EN71` at 12.2 s |
| `q65_60a_eme_12k` | 12 k | `q65` @ 0 Hz, 60 s A | `W7GJ W1VD FN31`, `W7GJ VE1JF RRR`, `W7GJ N8JX EN73`, `W7GJ N0TB -15` |
| `fst4_60_630m_12k` | 12 k | `fst4` @ 0 Hz, 60 s | `CQ K9KFR EN71` 1331 Hz, `CQ N5TM EL29` 1101 Hz, `CQ N3HSK FM28` 1135 Hz (CQ assumed) |
| `fst4w_1800_750` | 750 | FST4W decoder, 1800 s, audio 1450 Hz at 0 Hz | `DL0HOT JO60 30` at 1433 Hz, −44 dB |
| `jt9_20m_12k` | 12 k | `jt9` @ 0 Hz | all seven JT9 decodes of the WSJT-X sample `130418_1742` |
| `jt65_jt9_20m_12k` | 12 k | `jt65` / `jt9` @ 0 Hz | the nine JT65 and seven of the eight JT9 decodes of the WSJT-X sample `130610_2343`, plus one JT65 station they miss |
| `js8_normal_heartbeats_12k` | 12 k | `js8` @ 0 Hz | 5 JS8 Normal frames: heartbeat replies to KN4CRD and `KN4ZXG: @HB HEARTBEAT FM16` |
| `js8_normal_data_12k` | 12 k | `js8` @ 0 Hz | 7 JS8 Normal frames: six heartbeat replies to AC9BA and a data frame |
| `js8_normal_busy_12k` | 12 k | `js8` @ 0 Hz | 10 JS8 Normal heartbeat replies to K0EIA |
| `js8_slow_12k` | 12 k | `js8` @ 0 Hz | one JS8 Slow data frame `TrMcT8++++++` |
| `acars_offair_48k` | 48 k | `acars` @ 0 Hz | `F-GTAE` / `AF7728` `[H1]` engine report `#DFB00000/V206,...`, then `LN-DYY` `[_d]` acknowledging block 5 |
| `inmarsat_stdc_egc_24k` | 24 k | `inmarsat_stdc` @ +216 Hz | NCS bulletin board for frame 5987 (LES 144), an LES 104 announcement, the signalling channel |
| `inmarsat_aero_offair_48k` | 48 k | `inmarsat_aero` @ 0 Hz | 600 bps P channel, `HL8217` `[_d]` |
| `iridium_prbs15_250k` | 250 k | `iridium` @ 0 Hz | one downlink burst whose payload holds the PRBS15 recurrence bit for bit |
| `gsm_dcs1800_arfcn725_541k` | 541.667 k | `gsm` @ 0 Hz, 1847.8 MHz | cell 262-02-318-11225, BSIC 6-0: TMSI paging, SDCCH/8 TS1 SC1 assignment, paging response from TMSI `015C5703`, A5/1 ciphering command |
| `hd_radio/kut_558k` | 558.141 k | `hd_radio` @ 0 Hz | KUT, "The University of Texas at Austin", facility ID 21; one P1 frame: 32 HDC packets of HD1, 31 of HD2 (Public), no CRC errors |
| `lora_rn2483_sf7_250k` | 250 k | `lora` @ 0 Hz, 125 kHz | two RN2483 frames, SF7 CR 4/8, payload `deadbeef`, CRC ok |
| `lora_epfl_sf7_strong_250k` | 250 k | `lora` @ 0 Hz, 125 kHz | SF7 CR 4/5, payload `0000967`, 32 dB |
| `lora_epfl_sf7_offgrid_250k` | 250 k | `lora` @ 0 Hz, 125 kHz | SF7 CR 4/5, payload `0000158`, 34 dB |
| `lora_epfl_sf7_weak_250k` | 250 k | `lora` @ 0 Hz, 125 kHz | SF7 CR 4/5, payload `0001413`, −6 dB |
| `lora_epfl_sf10_500k` | 500 k | `lora` @ 0 Hz, 250 kHz | SF10 CR 4/5, 19 bytes from a drone, 6 dB |
| `lora_epfl_sf10_weak_500k` | 500 k | `lora` @ 0 Hz, 250 kHz | SF10 CR 4/5, 19 bytes from a drone, −14 dB |
| `lora_grlorasdr_sf7_250k` | 250 k | `lora` @ 0 Hz, 125 kHz | gr-lora_sdr transmitter, SF7 sync 0x12, `sdrflex lora fixture` twice |
| `remote_id_ble_dji_mini4_4m` | 4 M | Bluetooth channel 38 | 8 CRC-valid adverts, including `DJI-MINI4-Pro-98F8` from `E4:7A:2C:AB:98:F9` |
| `remote_id_wifi_dsss_beacon_20m` | 20 M | Wi-Fi channel 1 | 1 Mbit/s beacon `Xiaomi 13`, FCS ok |
| `remote_id_wifi_cck_20m` | 20 M | Wi-Fi channel 1 | 220-byte CCK 11 Mbit/s data frame, FCS ok |
| `remote_id_wifi_ofdm_beacon_20m` | 20 M | Wi-Fi channel 1 | 6 Mbit/s beacon `MipsTucker`, FCS ok |
| `mdc1200_unit1777_offair_48k` | 48 k | `mdc1200` @ 0 Hz | end-of-transmission PTT ID, unit `1777`, op `01`, arg `00` |
| `fleetsync1_fleet107_offair_48k` | 48 k | `fleetsync` @ 0 Hz | FleetSync I ANI from fleet 107, unit 1772, at key-up and key-down |
| `fleetsync2_fleet107_offair_48k` | 48 k | `fleetsync` @ 0 Hz | the same ANI pair in FleetSync II |
| `uat_reference_2m083` | 2.083334 M | `uat` @ 0 Hz | 5 aircraft (`A66EF1`, `N70FC`, squawk 0325 from `AD7233`, TIS-B `N8498F`, `AC46A0`) and 7 uplinks with METARs from KPOC, KMHV, KSPI, KGSH, KLHQ, KPAH |
| `hrpt_meteor_offair_2m66` | 2.6616 M | `hrpt` @ 0 Hz, Meteor-M, 1700 MHz | 60 CADUs identical to SatDump's, four MSU-MR lines from 05:22:29 UTC, infrared picture |
| `goes_hrit_offair_1m854` | 1.854 M | `goes_hrit` @ +6.4 kHz, GOES-19, 1694.1 MHz | three EMWIN bulletins (`SVRW13CN`, two `SAHOURLY`) identical to SatDump's |
| `dsc_offair_8k` | 8 k | `dsc` @ 0 Hz | Lyngby Radio `002191000` acknowledging a test call from `231700000` |
| `radio_clock_dcf77_offair_2k` | 2 k | `radio_clock` / DCF77 @ 0 Hz | 2026-10-03 22:04 CEST |
| `radio_clock_jjy_offair_2k` | 2 k | `radio_clock` / JJY @ 0 Hz | 2026-10-04 05:16 JST |
| `radio_clock_wwvb_offair_2k` | 2 k | `radio_clock` / WWVB @ 0 Hz | 2021-03-14 05:35 UTC |
| `flex_p2000_offair_48k` | 48 k | `flex` @ 0 Hz | P2000 page to capcode 2029574, `A2 DP2 Leidschendam-Voorburg ...` |
| `p25p2_mmr_idle_48k` | 48 k | `p25p2` @ 0 Hz | WACN BEE00, system 164, NAC 164, CRC-valid SACCH `81900b0c8200c900c900c9`: supergroup 3202 patching TG 201 |
| `tetra_scbs_offair_50k` | 50 k | `tetra` @ −1.4 kHz | MCC 250, MNC 13, colour code 44, LA 1052, carrier 2716 (467.9125 MHz) |
| `tetra_weak_offair_144k` | 144 k | `tetra` @ −0.9 kHz | the same cell at about 10 dB SNR |
| `tetra_call_offair_48k` | 48 k | `tetra` @ +0.3 kHz | MCC 228, MNC 8889, D-SETUP 100701 to group 100601, D-TX GRANTED to 100702 |
| `tetrapol_control_48k` | 48 k | `tetrapol` @ 0 Hz | scrambling 70, system info for cell 28.2.7 at BCH frame 0, group 393 active on channel 2890, PCH paging CGI 125 and 399 |
| `tetrapol_traffic_48k` | 48 k | `tetrapol` @ 0 Hz | group idle (cause 0x12) under scrambling 115, then voice under 118 |
| `wfm_offair_99m9_240k` | 240 k | `wfm` @ 0 Hz | 99.9 MHz broadcast audio, the wanted station in the IF filter tests |
| `wfm_offair_96m5_240k` | 240 k | `wfm` @ 0 Hz | 96.5 MHz broadcast audio, the interferer in the IF filter tests |
| `stanag/stanag_4285_9k6` | 9.6 k | `stanag` @ 0 Hz, USB | STANAG 4285 |
| `stanag/stanag_4529_9k6` | 9.6 k | `stanag` @ 0 Hz, USB | STANAG 4529 |
| `stanag/stanag_110a_600s_9k6` | 9.6 k | `stanag` @ 0 Hz, USB | MIL-STD-188-110A, 600 bps short |
| `stanag/stanag_110a_2400l_9k6` | 9.6 k | `stanag` @ 0 Hz, USB | MIL-STD-188-110A, 2400 bps long |
| `stanag/stanag_110a_150l_9k6` | 9.6 k | `stanag` @ 0 Hz, USB | MIL-STD-188-110A, 150 bps long |
| `stanag/stanag_4539_9k6` | 9.6 k | `stanag` @ 0 Hz, USB | STANAG 4539 |
| `stanag/stanag_39tone_9k6` | 9.6 k | `stanag` @ 0 Hz, USB | 39-tone |
| `stanag/stanag_16tone_9k6` | 9.6 k | `stanag` @ 0 Hz, USB | 16-tone |
| `stanag/stanag_ale_9k6` | 9.6 k | `stanag` @ 0 Hz, USB | 2G ALE |
| `horus/horus_v1_iq_8k` | 8 k | `horus` @ 0 Hz | v1 `000900071E2A000000000000000000000000259A6B14`: payload 0, sequence 9, 07:30:42 |
| `horus/horus_v1_4db5_8k` | 8 k | `horus` unit tests | v1 at 4.5 dB `0112000000230000000000000000000000001C9A9545`: payload 1, sequence 18 |
| `horus/horus_v2_8k` | 8 k | `horus` unit tests | v2 `000108000C2238...0102030405060708091DBB`: payload 256, sequence 8 |
| `horus/horus_v3_8k` | 8 k | `horus` unit tests | v3 `4FSKTEST-V2`, sequence 0, four `meowmeow` sensors, `WOOF` custom data |
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
| `gps_l1_opensky_2m048` | 2.048 M | `gnss` @ 0 Hz, 1575.42 MHz | GPS PRNs 5, 7, 13, 20 and 30 acquired and tracked above 40 dB-Hz |

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

### GPS L1: `gps_l1_opensky_2m048`

The first 1.2 s of the FGI OSNMA open-sky capture (doi:10.23729/9559efea-22fc-48ac-8de4-c4d1cba367be,
CC BY 4.0, Finnish Geospatial Research Institute), an NSL Stereo front end at 26 MS/s real with
L1 at +6.39 MHz, shifted to baseband and resampled with `cargo xtask excerpt --format rs8`. The
annotation pins the source SHA-256. `gnss::tests::an_open_sky_excerpt_acquires_and_tracks_five_satellites`
reads it directly. The whole 54 s capture is too large to commit:
`sdrmm_test_support::reference_data::gps_l1_open_sky` says how to fetch and convert it, and with
`GPS_L1_OPEN_SKY` set `an_open_sky_capture_fixes_within_thirty_metres_of_the_survey` checks every
fix (11 satellites, PDOP 1.5) against the published survey. `gnss::open_sky` keeps the navigation
words and one epoch of measurements from that run, so CI solves the fix from real data.

### ADS-B: `adsb_offair_2m`

A 200 ms excerpt of a 2 MS/s RTL-SDR recording over the Eifel. It includes overlapping replies,
amplitude variation, and roll-call replies whose addresses depend on earlier all-call messages.
The window contains six downlink formats and four aircraft in about 3.1 MB.

`adsb::tests::decodes_a_recorded_sky` reads it directly.
`a_recorded_squitter_places_its_aircraft_against_the_receiver` checks local CPR positioning against
the global position solved from the full recording.

### UAT: `uat_reference_2m083`

A frozen render of `crates/modem-test-support/scripts/uat_reference.py` (standard-library
Python, independent of the Rust code), about 0.95 MB. Not an antenna recording.

- Five frames from dump978's `sample-data.txt.gz` (lines 1, 547, 1003, 135, 138), messages
  decoded off air near KPAO in 2015. The script adds sync and its own Reed-Solomon parity and
  keys them as CPFSK (h = 0.6) with a 30 kHz offset and noise. Source:
  https://github.com/mutability/dump978 at `c7c4ad2f953692eadc5702aa8f059ea93fba2934`,
  GPL-2.0-or-later.
- The seven packets of fisb-978's `eye-samples`: FM discriminator output captured off air at
  2.083334 MS/s, after the sync word, from very noisy to clean. The script re-modulates each
  sample as one phase step behind a clean sync word, so the bit errors are the captured ones
  (one uplink needs 46 symbol repairs). Source: https://github.com/rand-projects/fisb-978 at
  `7a628016c6897d3ae88286c2fa9c2f00f8d829af`, BSD-2-Clause-Patent, Copyright (C) 2020-2021
  Michael Barnes.

dump978 (`c7c4ad2`) decodes the fixture, converted to `cu8`, to the same twelve frames its
sources hold, with 2, 46, 21 and 6 repairs. `uat2text` gives the expected fields and METAR text.
`uat::tests::the_reference_fixture_*` read it directly.

### FT8: `ft8_20m_busy_12k`

A 15-second 20 m slot recorded on 2019-11-11 at 11:06:15 UTC. The source is the MIT-licensed
`ft8_lib` test recording `191111_110615.wav`, pinned in the SigMF annotation. Its 12 kHz mono audio
was converted to `cf32_le` with zero quadrature, centered on the 14.074 MHz dial frequency.

The upstream expected output contains twenty messages; this integration decodes nineteen.
`weak_signal::tests::a_recorded_slot_reads_the_band_the_reference_decoder_published` appends a
quiet tail to close the sliding slot window and records the missing decode explicitly.

### Horus Binary: `horus/`

Test samples from horusdemodlib `samples/` at commit 8cc43de5 (LGPL-2.1,
github.com/projecthorus/horusdemodlib); each annotation pins the source SHA-256. The expected
payloads are the ones its CTest suite matches. `horus_v1_iq_8k` is the cs16 IQ capture, shifted
from 6.1 kHz and resampled to 8 kS/s; whether it was off air is unknown. The other three are
synthetic 8 kHz audio, trimmed to one frame and stored with zero quadrature; their tests search
for tones above 0 Hz only, as horusdemodlib does, to keep the mirror image out.
### MSK144: `msk144_wa4cqg_12k`, `msk144_w4imd_kd9vv_12k`

Meteor pings on 6 m from the WSJT-X samples `MSK144/181211_120500.wav` (6 to 11 s) and
`MSK144/181211_120800.wav` (2 to 14 s), [WSJT-X](https://github.com/WSJTX/wsjtx) commit
`967c85a6`, GPL-3.0, by K1JT and the WSJT-X team. The 12 kHz audio is I, quadrature zero.
`msk144::tests::recorded_pings_read_as_wsjtx_reads_them` expects the messages, times and
frequencies WSJT-X prints for these files.

### Q65: `q65_60a_eme_12k`

One Q65-60A period of 6 m EME at W7GJ, the WSJT-X sample `Q65/60A_EME_6m/210106_1621.wav`
(same commit, licence and authors as the MSK144 pair), whole. Four stations answer at about 2.8 s
DT. `q65::tests` reads all four blind, with no a-priori information.

### FST4: `fst4_60_630m_12k`, `fst4w_1800_750`

The WSJT-X samples `FST4+FST4W/210115_0058.wav` (one FST4-60 minute on 630 m, whole) and
`FST4+FST4W/201230_0300.wav` (one FST4W-1800 period), same commit, licence and authors as the
MSK144 pair. The first is 12 kHz audio as I, quadrature zero, on a nominal 474.2 kHz dial. The
second is down-converted by 1450 Hz to 750 S/s, the rate the channel narrows 1800 s periods to.
`fst4::tests` reads the three CQs, the third only with CQ assumed, and the beacon.
### JT9 and JT65: `jt9_20m_12k`, `jt65_jt9_20m_12k`

The first 52 s of two WSJT-X sample minutes, `samples/JT9/130418_1742.wav` and
`samples/JT9+JT65/130610_2343.wav` at WSJT-X commit `967c85a6` (GPL-3.0, K1JT et al.), pinned by
SHA-256 in each annotation. Their 12 kHz mono audio was converted to `cf32_le` with zero
quadrature, centered on the 14.078 and 14.076 MHz dial frequencies.

The expected decodes are those the WSJT-X and JTDX user guides show: seven JT9 for the first, and
for the second nine JT65 (K2OI at 2065 Hz is flagged `@` in the screenshot, but the guide counts
nine JT65 and decodes JT9 only above 2500 Hz) and eight JT9. `weak_signal::minute::tests` pads each to a full minute and decodes all but `CQ TA4A KM37`,
a −20 dB JT9 signal that starts 0.4 s before the recording. It also reads `WU7B K9EEI 73`, JT65
at 1064 Hz and −22 dB under stronger stations; both calls are in WSJT-X's `CALL3.TXT`.

### JS8: `js8_normal_heartbeats_12k`, `js8_normal_data_12k`, `js8_normal_busy_12k`, `js8_slow_12k`

Off-air audio from JS8Call's test set (`media/tests` at
[js8call commit a7ff1be0](https://github.com/js8call/js8call/tree/a7ff1be0/media/tests),
GPL-3.0-or-later): `A_2_5.wav`, `A_2_6.wav`, `A_2_9.wav` and `E_2_1.wav`, in that order. The
12 kHz mono audio was converted to `cf32_le` with zero quadrature; the SigMF annotation pins each
source's SHA-256. The dial frequency was not recorded, so the captures name 14.078 MHz.

A file name gives the speed (`A` Normal, `E` Slow), the JS8Call decoder depth and the frames its
CLI reads. This decoder reads the same 5 and 1 frames, and one more than the CLI in `A_2_6`
(7, not 6) and `A_2_9` (10, not 9), each CRC-valid and addressed like its neighbours.
`weak_signal::js8::tests` asserts every frame's text.

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

### P25 Phase 2: `p25p2_mmr_idle_48k`

GopherTrunk's Apache-2.0 capture `mmr-s9-420650k-p2-48k.cs16` (commit 5454622771dc,
`internal/radio/p25/phase2/receiver/testdata/`), off-air, unchanged but for `cf32_le`: 1.09 s of
an idle two-slot TDMA voice channel at 420.650 MHz. The site's Phase 1 control channel gives WACN
BEE00, system 164, NAC 164; the decoder works the same out of the scrambled SACCH. The SACCH
carries a Motorola patch, supergroup 3202 over TG 201. `p25::phase2::tests` reads it directly.

### TETRA: `tetra_scbs_offair_50k`, `tetra_weak_offair_144k`, `tetra_call_offair_48k`

The first two are GopherTrunk's Apache-2.0 test captures of a 467.9125 MHz control channel,
unchanged. Its own test asserts the same MCC and MNC. The third is seconds 22 to 29 of sigidwiki's
`TETRA_IQ.zip` (CC BY-SA) as recorded, with an inverted spectrum. `tetra::tests` reads all three
directly and resamples them to 72 kHz.

### Iridium: `iridium_prbs15_250k`

A burst from gr-iridium's reference modulator (`test-data/prbs15-2M-20dB.sigmf-data`, GPL-3.0
test data), as vendored by xng: downconverted to 250 kHz and trimmed to 32 ms. The signed 16-bit
IQ was scaled to `cf32_le`; the annotation pins the source SHA-256.
`iridium::tests::demodulates_the_gr_iridium_reference_burst` reads it directly.

### GSM: `gsm_dcs1800_arfcn725_541k`

0.72 s of the SRLabs airprobe capture `vf_call6_a725_d174_g5` (cf32 at 100/174 MS/s, as mirrored
in `ptrkrysik/test_data` for gr-gsm), resampled to 541.667 kHz. A DCS 1800 cell on ARFCN 725 with
its real 9.8 kHz offset. The annotation pins the source SHA-256.
`gsm::tests::decodes_the_recorded_cell` reads it directly.

### HD Radio: `hd_radio/kut_558k`, `hd_radio/kut_hd1_hdc.aus`

2.25 s of the nrsc5 project's sample capture (github.com/theori-io/nrsc5 `support/sample.xz` at
6d07a1e5fb, GPL-3.0), cu8 at 1 488 375 S/s, resampled to 558 140.625 S/s from 0.2 s on, past a
sample slip at 0.15 s. Off air: the analog FM host of KUT 90.5 Austin with MP1 sidebands. Every
PIDS frame passes its CRC. Over the whole capture the HD1 audio envelope follows the analog
program (correlation 0.87).
`kut_hd1_hdc.aus` holds twelve HD1 HDC frames from the same capture, two in short windows,
each led by a 16-bit big-endian length. `hd_radio::tests` and `aac::hdc::tests` read them.

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

### Remote ID PHYs: `remote_id_*`

No public IQ recording holds a real Remote ID transmission, so these check the Bluetooth and Wi-Fi
receivers on real air and the message parser runs on real frames from packet captures. Three come
from the RFUAV dataset (Apache-2.0, huggingface.co/datasets/alley66/RFUAV), a USRP X310 at
100 MS/s beside a DJI Mini 4 Pro. `remote_id_wifi_ofdm_beacon_20m` comes from IQEngine
`ism_band_24` by Marc Lichtman (CC BY-SA 4.0), a USRP B210 at 56 MS/s. That capture holds only
about 6% of the time it spans: beacon timestamps advance 1.02 s where the samples advance 62 ms,
so long frames break at the gaps. The excerpt is one beacon inside an unbroken stretch.
`remote_id::tests::air` reads them directly.

### ISM sensors: `ism/`

Twenty bursts from `rtl_433_tests`, which states no license, trimmed to the shortest stretch that
still decodes. `ism/README.md` lists each source and the expected readings, which match rtl_433.
`ism::tests` reads them directly.

### Wireless M-Bus: `wmbus/`

Seven bursts from `rtl_433_tests`, which states no license, and one off-air Techem frame recorded
with an AntSDR. `wmbus/README.md` lists each source and the expected frame, which matches rtl_433.
`wmbus::tests` reads them directly.

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

### MDC-1200 and FleetSync: `mdc1200_*`, `fleetsync*_offair_48k`

GopherTrunk's Apache-2.0 test captures at commit `5454622771dc5377f48244e4a94c6009a18445ec`
(`internal/radio/mdc1200/afsk/testdata/`, `internal/radio/fleetsync/afsk/testdata/`), converted
from `cs16` to `cf32_le` unchanged. All are off-air, channelized to 48 kHz at 0 Hz.

- `mdc1200_unit1777_offair_48k`: 0.7 s at 447.100 MHz, clipped at the ADC.
- `fleetsync1_fleet107_offair_48k`, `fleetsync2_fleet107_offair_48k`: 1.3 s each at
  462.5625 MHz, one Kenwood radio in each mode.
- `mdc1200_ref_01_80_1234_48k.s16`: not IQ. Discriminator audio from Matthew Kaufman's reference
  encoder, 48 kHz signed 16-bit mono: unit `1234`, op `01`, arg `80`.

`mdc1200::tests` and `fleetsync::tests` read them directly.

### Broadcast FM: `wfm_offair_99m9_240k`, `wfm_offair_96m5_240k`

One second each of two stations from `fm_98.1MHz_8Msps.iq` in LC-Linkous/hackRF_python
(GPL-2.0), a HackRF One capture of US band II at 8 MS/s. The annotations pin the source SHA-256
and sample range. `if_stage::offair` mixes the 96.5 MHz station onto the 99.9 MHz station's
adjacent channel, or adds a carrier, and checks that the IF filter brings the audio back.

### FreeDV: `freedv_1600_8k`

The first three seconds of the FreeDV GUI project's `wav/ve9qrp_1600.wav` receive test recording.
Signed 16-bit mono audio was converted to normalized `cf32_le` with zero quadrature. The SigMF
annotation records the source SHA-256, pinned upstream commit, and LGPL-2.1 license.
`dv::freedv::tests::decodes_the_upstream_receive_recording` reads it directly.

### Tetrapol: `tetrapol_control_48k`, `tetrapol_traffic_48k`

Excerpts of sigidwiki's `Tetrapol_2_channels.zip` (`Tetrapol.wav`, 48 kHz IQ, states no license),
which holds a control channel near +5.8 kHz and a traffic channel near -6.6 kHz. Each is shifted to
0 Hz; the annotations hold the source SHA-256 and sample range. `tetrapol::tests` reads both.

### Meteor HRPT: `hrpt_meteor_offair_2m66`

0.75 s of a Meteor-M N2-4 HRPT pass on 2026-03-06 from Meti's sat-archive
(`sat-archive.metislair.com`, CC BY-SA 4.0), received by Seler1500: `2026-03-06_05-19-03_3000000SPS_1700000000Hz_pass2_(northwest).cs8.zst`
from 175.0 s, resampled from 3 MS/s `cs8` with `cargo xtask excerpt` and stored as `ci16_le`.
The pair is test data under that license. The carrier sits near +5 kHz. It holds 60 CADUs,
byte-identical to numbers 8889 to 8948 of the archive's SatDump `meteor_hrpt.cadu`, and four
MSU-MR lines. `hrpt::tests::a_recorded_meteor_pass_decodes_as_satdump_does` reads it.

### GOES HRIT: `goes_hrit_offair_1m854`

1.25 s of a GOES-19 HRIT pass on 2025-06-26 from Meti's sat-archive (`sat-archive.metislair.com`,
CC BY-SA 4.0), received by Patrick Cunningham:
`2025-06-26_22-31-51_3000000SPS_1694100000Hz.cs8.zst` from 14.75 s, resampled from 3 MS/s `cs8`
with `cargo xtask excerpt` and stored as `ci16_le`. The source SHA-256 is of the first 600 MB of
the unpacked `cs8`. The pair is test data under that license. It holds 70 CADUs and three whole
EMWIN bulletins, a Saskatchewan thunderstorm warning and two METAR collections, each matching the
pass's SatDump `goes_hrit.cadu` byte for byte.
`goes_hrit::tests::a_recorded_goes19_second_gives_satdumps_files` reads it.
### MetOp AHRPT: `hrpt/metop_b_avhrr_cadus.bin`

Not IQ: 62 CADUs of virtual channel 9 from a MetOp-B AHRPT pass on 2024-02-29 in Meti's
sat-archive (`sat-archive.metislair.com`, CC BY-SA 4.0), received by Meti:
`L-band/Metop/MetOp-B/2024-02-29_08-59_metop-ahrpt_Meti/metop_ahrpt.cadu`, the VCID 9 units among
CADUs 153714 to 154006 (unit counters 6216516 to 6216577). As SatDump wrote them: derandomized,
Reed-Solomon parity in place. They hold AVHRR lines 997 to 1000 (APID 103) from 09:06:56.483 UTC.
The file is test data under that license. `hrpt::tests` reads it straight and through the project's
AHRPT modulator and the full receiver.

### Meteor LRPT: `lrpt/meteor_m2_qpsk72_symbols.bin`

Not IQ: 196,864 hard QPSK symbol bits, packed MSB first, from this receiver's demodulator on a
2018 Meteor-M N2 pass on 137.9 MHz (72k QPSK). They hold VCDUs `09BF68` to `09BF73`: one full
MCU row of APIDs 64, 65 and 68. The source IQ is sigidwiki's LRPT sample
(`LRPT_16-29-02_137900kHz.wav`), which states no license. `lrpt::tests::a_recorded_meteor_m2_row_decodes_cleanly` reads it.

### End-of-Train: `eot/pyeot_demo3_48k.wav`

Not IQ: PyEOT's `demo3eot.wav`, unmodified, 2.6 s of FM-discriminator audio from 457.9375 MHz
with three rear-unit packets. GPL-3.0, see `eot/README.md`. The test FM-modulates it at ±3 kHz.
`eot::tests::decodes_three_rear_units_from_an_off_air_recording` reads it.

### HF modems: `stanag/`

Four-second slices of sigidwiki audio samples (CC BY-SA), so these pairs are test data under that
license. The USB audio was resampled to 9.6 kHz `ci16_le` with zero quadrature; each annotation
names the source URL, slice and SHA-256. `stanag::tests::air::names_recorded_modems` reads them.
`names_the_sigidwiki_library` runs 49 full samples when `STANAG_SIGID_DIR` points at them.
