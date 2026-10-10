# End-of-Train off-air audio

`pyeot_demo3_48k.wav` is `demo3eot.wav` from [PyEOT](https://github.com/ereuter/PyEOT) by Eric
Reuter, unmodified: 2.6 s of FM-discriminator audio from 457.9375 MHz, 8-bit mono at 48 kHz.
It is licensed GPL-3.0, which may be combined with this AGPL-3.0-or-later project.

It is audio, not IQ: the test FM-modulates it at ±3 kHz before the channel sees it, so it checks
the FFSK demodulator, framing and BCH on real transmitters but not the RF front end.

Expected: three rear units, each with a clean BCH check.

| unit | pressure | motion | marker light | charge |
|---|---|---|---|---|
| 39572 | 89 psig | stopped | on | 83% |
| 10690 | 66 psig | stopped | off | 91% |
| 19472 | 89 psig | moving | off | 71% |

SHA-256 `491ea18abc20fb6e6d652860b33f9e11739f1d5bd521e096073cdaf0cfc6e912`.
