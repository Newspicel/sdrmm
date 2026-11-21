---
bump: patch
---

GPU FFT: Stockham kernels, 3-4x faster. Spectrum and CAF load samples inside the FFT and, on shared-memory GPUs, upload in place. Radar clutter solve uses Levinson for one Doppler tap.
