# ISM sensor recordings

Excerpts of `rtl_433_tests` (github.com/merbanan/rtl_433_tests, commit
`1aa8c29d6b9383739d1aeed495dbc5069b704392`), which states no license. Each excerpt keeps the
`cu8` samples of one burst, trimmed to the shortest stretch that still decodes. The SigMF
annotation names the source file, its SHA-256 and the sample range.

`ism::tests` reads them directly. Expected values match rtl_433's JSON for the same file.

| stem | expected |
|---|---|
| `acurite_5n1_433m92_250k` | Acurite-5n1 839 ch A, wind 1.2 m/s from 337.5°, rain 16.8 mm |
| `acurite_609txc_433m92_250k` | Acurite-609TXC 162, 21.8 °C, 67%, battery low |
| `acurite_tower_433m92_250k` | Acurite-Tower 9884 ch B 22.8 °C 66% battery low, and 12053 ch C 21.0 °C 73% |
| `ambient_f007th_433m92_250k` | Ambientweather-F007TH 37, 19.9 °C, 35% |
| `ambient_wh31e_915m_250k` | AmbientWeather-WH31E 195, 12.2 °C, 94% |
| `bresser_5in1_868m3_250k` | Bresser-5in1 118, 8.0 °C, 92%, wind 2.0 gust 3.0 m/s from 292.5°, rain 10.4 mm |
| `bresser_6in1_868m3_1000k` | Bresser-6in1 411042499, 11.8 °C, 81%, wind 4.0 gust 5.7 m/s from 276°, 1 MS/s |
| `ev1527_remote_433m92_250k` | Unknown, PWM, 25 bits, short pulse near 370 µs |
| `fineoffset_wh1080_433m92_250k` | Fineoffset-WHx080 5, 7.4 °C, 82% |
| `fineoffset_wh2_433m92_250k` | Fineoffset-WH2 209, 24.6 °C, 33% |
| `generic_remote_433m92_250k` | Generic-Remote 5069, cmd 192, tristate `0Z01101Z1000` |
| `honeywell_345m_250k` | Honeywell-Security 698779, open, tamper |
| `lacrosse_tx141_433m92_250k` | LaCrosse-TX141Bv2 117, 20.1 °C |
| `lacrosse_tx29_tx35_868m2_250k` | LaCrosse-TX29IT 10 23.8 °C, and TX35DTHIT 26 24.1 °C 34%, FSK |
| `nexus_th_433m92_250k` | Nexus-TH 181, 19.0 °C, 71% |
| `oregon_thgr122n_433m92_250k` | Oregon-THGR122N 248, −5.5 °C, 30% |
| `oregon_thn132n_433m92_250k` | Oregon-THN132N 206, −9.4 °C |
| `oregon_wgr800_433m92_250k` | Oregon-WGR800 224, no wind |
| `schrader_tpms_315m_250k` | Schrader 03A38B2, 23 °C, 0 kPa |
| `thermopro_tp12_433m92_250k` | Thermopro-TP12 56, 43.4 °C and 24.3 °C |

| stem | source |
|---|---|
| `acurite_5n1_433m92_250k` | `tests/acurite/Acurite_5-n-1/VN1TXC_02.cu8` |
| `acurite_609txc_433m92_250k` | `tests/acurite/Acurite_609A1TX/acurite_th_609_lowbatt.cu8` |
| `acurite_tower_433m92_250k` | `tests/acurite/Acurite_592TXR/acurite-592txr-low-batt.cu8` |
| `ambient_f007th_433m92_250k` | `tests/ambient_weather/01/f007th.cu8` |
| `ambient_wh31e_915m_250k` | `tests/ambient_weather/04/g001_915M_250k.cu8` |
| `bresser_5in1_868m3_250k` | `tests/bresser_5in1/01/g001_868.3M_250k.cu8` |
| `bresser_6in1_868m3_1000k` | `tests/bresser_6in1/01/g002_868.3M_1000k.cu8` |
| `ev1527_remote_433m92_250k` | `tests/EV1527-Universal-Remote/g024_433.92M_250k.cu8` |
| `fineoffset_wh1080_433m92_250k` | `tests/fineoffset/fineoffset_wh1080/01/gfile001.cu8` |
| `fineoffset_wh2_433m92_250k` | `tests/fineoffset/fineoffset_wh2/01/gfile001.cu8` |
| `generic_remote_433m92_250k` | `tests/generic_remote/01/gfile001.cu8` |
| `honeywell_345m_250k` | `tests/honeywell/01/honeywell-001.cu8` |
| `lacrosse_tx141_433m92_250k` | `tests/lacrosse/tx141-bv2/g001_433.92M_250k.cu8` |
| `lacrosse_tx29_tx35_868m2_250k` | `tests/lacrosse/tx29-it_tx35dth-it/tx35dth-it_868.2M_250k.cu8` |
| `nexus_th_433m92_250k` | `tests/nexus/01/gfile001.cu8` |
| `oregon_thgr122n_433m92_250k` | `tests/oregon_scientific/thgr122n/01/THGR122N_-550_30_ch1.cu8` |
| `oregon_thn132n_433m92_250k` | `tests/oregon_scientific/THN132N/THN132N_-940_ch1.cu8` |
| `oregon_wgr800_433m92_250k` | `tests/oregon_scientific/WGR800X/01/WGR800X-g001_433.92M_250k.cu8` |
| `schrader_tpms_315m_250k` | `tests/Schrader/01/gfile001.cu8` |
| `thermopro_tp12_433m92_250k` | `tests/thermopro-tp12/01/gfile001.cu8` |
