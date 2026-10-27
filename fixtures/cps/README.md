# Codeplug images

Radio memory images for the CPS tests, in the form `Image::to_bytes` writes: for each segment a
4-byte little-endian address, a 4-byte little-endian length, then that many bytes.

| File | Source |
|---|---|
| `anytone-d890uv-v100.img` | Read from a real D890UV |
| `radtel-rt4d-handbuilt.img` | Not a device dump. The RT-4D regions written by hand from the addresses, strides and field offsets in `crates/cps/src/radtel/`, holding four channels, three contacts, one group list and two zones. Every byte no field claims is left erased. |

`crates/cps/tests/` reads both with `include_bytes!`, so Git tracks them.
