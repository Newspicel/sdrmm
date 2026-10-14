# Wireless M-Bus recordings

Excerpts at 400 kS/s centred on 868.95 MHz, cut with `cargo xtask excerpt`. Seven come from
`rtl_433_tests` (github.com/merbanan/rtl_433_tests, commit
`1aa8c29d6b9383739d1aeed495dbc5069b704392`), which states no license. The SigMF annotation names
the source file, its SHA-256 and the sample range. The `tests/m-bus/05` capture says 868.6 MHz but
carries the meter 157 kHz above it; the excerpt shifts by what the spectrum shows.

`wmbus::tests` reads them directly. The expected frames match rtl_433's `data` field.

| stem | expected |
|---|---|
| `kamstrup_heat_c1b_868m95_400k` | C1 format B, KAM 60978332 heat, ELL AES without a key |
| `kamstrup_heat_c1a_868m95_400k` | C1 format A, KAM 71372984 access request `09472d2c84293771340c` |
| `kamstrup_water_c1b_868m95_400k` | C1 format B, KAM 74433908 cold water, plain ELL, compact frame |
| `diehl_water_t1_868m95_400k` | T1, DME 84850129 water, then DME 01820778 |
| `imt_repeater_t1_868m95_400k` | T1, IMT 05555487 water via IMT 10025571, mode 5 without a key |
| `bmt_water_t1_868m95_400k` | T1, BMT 18160686 water, mode 5 without a key |
| `techem_hca_t1_868m95_400k` | T1, TCH 30717777, manufacturer frame |
| `techem_hca_offair_868m95_400k` | T1, TCH 24186304, about 9 dB SNR, own AntSDR recording |

| stem | source |
|---|---|
| `kamstrup_heat_c1b_868m95_400k` | `tests/m-bus/01/g002_868.95M_1200k.cu8` |
| `kamstrup_heat_c1a_868m95_400k` | `tests/m-bus/01/g020_868.95M_1200k.cu8` |
| `kamstrup_water_c1b_868m95_400k` | `tests/m-bus/05/g001_868.6M_1000k.cu8` |
| `diehl_water_t1_868m95_400k` | `tests/m_bus/01/g001_868.9M_1000k.cu8` |
| `imt_repeater_t1_868m95_400k` | `tests/m_bus/02/g001_868.9M_1000k.cu8` |
| `bmt_water_t1_868m95_400k` | `tests/m-bus/03/g001_868.9M_1600k.cu8` |
| `techem_hca_t1_868m95_400k` | `tests/m-bus/04/g001_868.9M_1000k.cu8` |
| `techem_hca_offair_868m95_400k` | AntSDR, 1.2 MS/s at 868.95 MHz, 2026-10-14 |
