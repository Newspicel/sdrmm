# Own AntSDR firmware: plan

## Goal

Use the full 56 MHz without streaming it raw:

- the FPGA computes a full-band FFT and sends only spectrum frames (~0.5 MB/s)
- a DDC bank in the FPGA gives each decoder its own narrow IQ stream
- decoding stays on the host, in Rust

## What we measured (E310, 2026-10)

### Link

| Path | Result |
|---|---|
| UDP with GSO, 64 KB per send | 116.6 MB/s, no loss (gigabit tops out near 120) |
| UDP, one datagram per send | 72 MB/s |
| TCP | 108 MB/s |
| Jumbo frames | No: the Mac USB adapter and board `eth0` both stop at MTU 1500 |
| USB 2.0 via iiod | ~33 MB/s, ~8.3 MS/s |

### Board

- Zynq 7020, 2x Cortex-A9, 1 GB RAM, CMA 256 MB, kernel 5.15 (ADI), Pluto firmware fork v0.20, iiod 0.25
- shipped with `maxcpus=1`; now `fw_setenv maxcpus 2`. The streamer needs both cores
- `mode=2r2t` caps `sampling_frequency_available` at 30.72 MS/s
- `/mnt/jffs2/autorun.sh` runs at boot, 700 KB free on jffs2
- SSH `root` / `analog`, iiod on 30431, iqlinkd on 30432

### Getting samples out of the DMA

| Method | Rate |
|---|---|
| iiod | 62 MB/s, one core full |
| `read()` on `/dev/iio:deviceN` | 85 MB/s, one core full |
| mmapped blocks, `memcpy` | 254 MB/s |
| mmapped blocks, `vldm` | 300 MB/s |
| `sendmsg` straight from mapped blocks | 67 MB/s |
| iqlinkd: copy core 0, send core 1, pinned | 80 MB/s, ~20 MS/s 1 lane, 9.8 MS/s 2 lanes |

- Block ioctls (`0xa0`..`0xa4`) only work on the buffer fd from `IIO_BUFFER_GET_FD_IOCTL`,
  not on `/dev/iio:deviceN` (EINVAL)
- The send core is the bottleneck now; the copy core has headroom

### Dead ends

- Overflow bit `0x80000088` bit 2 is live, not sticky; reads miss most overflows. Loss is
  estimated from timing instead
- Blocks carry no timestamps
- `SO_NO_CHECK` changed nothing
- Smaller chunks (64 KB to 2 MB) changed nothing once threads were pinned

### Full 56 MHz as raw IQ

61.44 MS/s needs 246 MB/s at 16 bit, 184 at 12 bit, 123 at 8 bit. None fits the 116 MB/s link.
Raw full band is impossible over this Ethernet; the FPGA has to reduce the data.

### FPGA room (estimate, not measured)

The ADI Pluto design fits a Z-7010, so on the 7020 it uses at most ~33% LUT, ~36% DSP
(of 220), ~43% BRAM (of 140 BRAM36). Real numbers need a Vivado report.

### Tooling

- Vivado runs only on x86 Linux or Windows. Everything else runs on the Mac
- Board binaries: `cargo build --release --target armv7-unknown-linux-musleabihf -p sdrmm-iqlinkd`
  (`.cargo/config.toml` links with `rust-lld`)

## Target architecture

| Layer | Job |
|---|---|
| FPGA | AD9361 at 61.44 MS/s, DDC bank, FFT, packetizer, one DMA |
| ARM | iqlinkd sets FPGA registers, forwards packets over UDP |
| Host | Allocates DDC channels for the decoders wired to the Device node |

No demodulation in the FPGA. After the DDC the data is already small, and decoders change too
often for Verilog.

### Data path

- Own protocol end to end, no IIO for samples. The ADI `ad9361` driver stays for RF control only
- The FPGA packetizer writes finished packets (header + payload) into one DMA ring, mapped via UIO.
  iqlinkd forwards them without touching the payload
- Header: stream id, 64-bit sample counter, sticky overflow counter, payload length. Loss and
  overflow become exact, and spectrum and channels share one timebase
- Header layout lives in `crates/iqlink`; `cargo xtask` generates the Verilog constants from it
- Packetizer flushes on full datagram or timeout, so narrow channels keep low latency
- iqlink v2 replaces v1. No compatibility with stock firmware

### DDC bank

- Per channel: NCO, mixer, CIC, then a shared FIR (compensation + half-band /2)
- Mixers time-multiplexed: fabric clock / 61.44 MHz channels per DSP mixer. FIR runs at the low
  rate, one MAC engine serves all channels
- Output rates 61.44 MS/s / 2^k, k >= 4 (3.84 MS/s down to ~30 kS/s). Host resamples to the
  decoder's exact rate
- 16-bit IQ out: decimation adds dynamic range beyond the ADC's 12 bits
- Start target: 16 channels. Real count from the utilization report
- If channel count runs short: polyphase channelizer as a first stage, DDCs on its outputs

### Spectrum

- Xilinx FFT IP (free), size set at runtime, 1k to 16k bins
- Linear power averaged in the FPGA, 32 bit per bin. Host converts to dB
- 4096 bins x 4 B x 30 fps = 0.5 MB/s

### Host: under the hood

The graph does not change. No new node, no new output, no visible hardware channels.

- The Device node keeps its IQ output. Its own stream is DDC channel 0 at the center frequency and
  the chosen sample rate
- For each decoder wired to the Device node, the engine requests a DDC channel at the decoder's
  frequency and input rate. The decoder gets narrow IQ instead of the wide stream
- Spectrum and waterfall consumers wired to the Device node get FPGA frames
- iqlinkd admits channels against a link budget (~100 MB/s) and the channel count. A refused
  channel shows as decoder state on its face, never silently

## Verification

- No bit-exact Rust models. Testbenches compare against float references with tolerances (SNR,
  spurs, passband ripple)
- Verilator + cocotb testbenches per block (NCO, CIC, FIR, FFT, packetizer), run in Linux CI.
  A Vivado build is 20 to 40 min per try; most bugs must die before synthesis
- Test source in the FPGA: register-selectable tone generator in front of the DDC bank. Rust
  integration tests against a live board check frequency, level and sample counter of each stream

## Phases

0. **Build box.** x86 Linux with the Vivado version ADI HDL needs, plus Buildroot. Build the stock
   MicroPhase firmware (`antsdr-fw-patch`), flash it, check nothing changed. Recover a bricked
   board via DFU to stock `.frm` before anything else.
1. **iqlink v2 + host.** Protocol, under-the-hood allocation and decoder state, driven by a
   software DDC in iqlinkd (few channels, today's rates). Useful even if the firmware stalls.
2. **Full rate.** 61.44 MS/s on the E310 in `1r1t` (`2r2t` caps at 30.72). Exit: timing closes,
   utilization report read, test source works. The ARM cannot keep up yet, short snapshots only.
3. **Data path.** Packetizer, UIO DMA ring, sample and overflow counters. Channel 0 only.
4. **DDC bank.** All channels, AXI-Lite registers. iqlinkd swaps software DDC for hardware.
5. **FFT.** Spectrum packets on the same ring.
6. **Shipping.** Scripted `.frm` build on the build box, install docs, iqlinkd update path.

## Before the firmware

- Install iqlinkd for good: copy to `/mnt/jffs2`, start it from `autorun.sh`
- Optional: 12-bit packing on the copy core. 25 MS/s x 3 bytes = 75 MB/s fits under the 80 MB/s
  send ceiling

## Risks and open questions

- Bricking: DFU recovery proven in phase 0
- Vivado license: free edition should cover the 7020, confirm
- Timing closure at 61.44 MS/s inside the ADI HDL framework; fabric clock sets DDC density
- One image per board: E310 first, then E200. Pluto (7010, USB only) maybe never
- Decoders needing more than 3.84 MS/s use channel 0 and share its rate
