#!/usr/bin/env python3
"""Independent UAT (978 MHz) reference waveform for the `uat` decoder.

Writes a cf32_le SigMF pair at 2.083334 MS/s, two samples per bit, holding:

* clean frames rebuilt from dump978's sample data (decoded off-air messages near KPAO,
  GPL-2.0-or-later): this script adds sync and its own Reed-Solomon parity, then keys
  phase-continuous binary FSK with modulation index 0.6, a 30 kHz offset and noise;
* the seven off-air packets of fisb-978's eye samples (BSD-2-Clause-Patent): those are
  FM discriminator output at 2.083334 MS/s after the sync word. Each sample becomes the
  phase step of one IQ sample after a clean sync word, so the received bits, errors included,
  are the ones fisb-978 captured.

Standard library only. Usage:

    uat_reference.py EYE_SAMPLE_DIR SAMPLE_DATA_TXT_GZ OUT_STEM
"""

import gzip
import hashlib
import json
import math
import os
import random
import struct
import sys

RATE = 2_083_334
SYNC_BITS = 36
ADSB_SYNC = 0xEACDDA4E2
UPLINK_SYNC = 0x153225B1D
STEP = 0.3 * math.pi
OFFSET_HZ = 30_000.0
NOISE = 0.12
GAP = 4_000

SAMPLE_LINES = [1, 547, 1003, 135, 138]
EYE_FILES = [
    "1646349679.663.A.i32",
    "1648135720.429.A.i32",
    "1646638263.105.F.i32",
    "1646638265.089.F.i32",
    "1646638270.108.F.i32",
    "1647067220.215.F.i32",
    "1648135761.799.F.i32",
]


def gf_tables(poly):
    exp, log = [0] * 512, [0] * 256
    x = 1
    for i in range(255):
        exp[i] = x
        log[x] = i
        x <<= 1
        if x & 0x100:
            x ^= poly
    for i in range(255, 512):
        exp[i] = exp[i - 255]
    return exp, log


EXP, LOG = gf_tables(0x187)


def mul(a, b):
    if a == 0 or b == 0:
        return 0
    return EXP[LOG[a] + LOG[b]]


def generator(nroots, fcr=120):
    g = [1]
    for i in range(nroots):
        root = EXP[(fcr + i) % 255]
        out = [0] * (len(g) + 1)
        for j, c in enumerate(g):
            out[j] ^= c
            out[j + 1] ^= mul(c, root)
        g = out
    return g


def rs_encode(data, nroots):
    g = generator(nroots)
    rem = [0] * nroots
    for byte in data:
        feedback = byte ^ rem[0]
        rem = rem[1:] + [0]
        for j in range(nroots):
            rem[j] ^= mul(feedback, g[j + 1])
    return list(data) + rem


def adsb_codeword(data):
    return rs_encode(data, 14 if len(data) == 34 else 12)


def uplink_codeword(data):
    blocks = [rs_encode(data[b * 72:(b + 1) * 72], 20) for b in range(6)]
    return [blocks[b][i] for i in range(92) for b in range(6)]


def bits_of(value, width):
    return [(value >> (width - 1 - i)) & 1 for i in range(width)]


def byte_bits(data):
    return [b for byte in data for b in bits_of(byte, 8)]


def steps_of(bits):
    return [STEP if bit else -STEP for bit in bits for _ in range(2)]


def sample_frames(path):
    with gzip.open(path, "rt") as f:
        lines = f.read().splitlines()
    frames = []
    for number in SAMPLE_LINES:
        text = lines[number - 1].split(";")[0]
        data = bytes.fromhex(text[1:])
        if text[0] == "-":
            frames.append(steps_of(bits_of(ADSB_SYNC, 36) + byte_bits(adsb_codeword(data))))
        else:
            frames.append(steps_of(bits_of(UPLINK_SYNC, 36) + byte_bits(uplink_codeword(data))))
    return frames


def eye_frame(path, uplink):
    with open(path, "rb") as f:
        raw = f.read()
    values = list(struct.unpack("<%di" % (len(raw) // 4), raw))
    centres = values[1::2]
    scale = sum(abs(v) for v in centres) / len(centres)
    steps = [max(-0.95 * math.pi, min(0.95 * math.pi, STEP * v / scale)) for v in values]
    sync = UPLINK_SYNC if uplink else ADSB_SYNC
    return steps_of(bits_of(sync, 36)) + steps


def noise_gap(rng, count):
    return [complex(rng.gauss(0, NOISE), rng.gauss(0, NOISE)) for _ in range(count)]


def render(frames, noisy, rng):
    out = noise_gap(rng, GAP)
    turn = 2 * math.pi * OFFSET_HZ / RATE
    for steps, add_noise in zip(frames, noisy):
        phase = 0.0
        for step in steps:
            phase = (phase + step + (turn if add_noise else 0.0)) % (2 * math.pi)
            sample = complex(math.cos(phase), math.sin(phase))
            if add_noise:
                sample += complex(rng.gauss(0, NOISE), rng.gauss(0, NOISE))
            out.append(sample)
        out.extend(noise_gap(rng, GAP))
    return out


def write_sigmf(stem, samples, description, label):
    data = b"".join(struct.pack("<ff", s.real, s.imag) for s in samples)
    with open(stem + ".sigmf-data", "wb") as f:
        f.write(data)
    meta = {
        "global": {
            "core:datatype": "cf32_le",
            "core:sample_rate": RATE,
            "core:version": "1.2.5",
            "core:license": "https://www.gnu.org/licenses/agpl-3.0.html",
            "core:description": description,
            "core:sha512": hashlib.sha512(data).hexdigest(),
        },
        "captures": [{"core:sample_start": 0, "core:frequency": 978000000}],
        "annotations": [
            {"core:sample_start": 0, "core:sample_count": len(samples), "core:label": label}
        ],
    }
    with open(stem + ".sigmf-meta", "w") as f:
        json.dump(meta, f, indent=2)
        f.write("\n")


def main():
    eye_dir, sample_data, stem = sys.argv[1:4]
    rng = random.Random(978)
    clean = sample_frames(sample_data)
    eye = [eye_frame(os.path.join(eye_dir, name), ".F." in name) for name in EYE_FILES]
    samples = render(clean + eye, [True] * len(clean) + [False] * len(eye), rng)
    write_sigmf(
        stem,
        samples,
        "Frozen UAT reference: dump978 sample frames keyed with fresh RS parity, then fisb-978 "
        "off-air discriminator packets re-modulated to IQ. Not an antenna recording.",
        "3 ADS-B and 2 uplinks from dump978 sample-data.txt.gz lines 1, 547, 1003, 135, 138; "
        "2 ADS-B and 5 FIS-B uplinks from fisb-978 eye-samples",
    )


if __name__ == "__main__":
    main()
