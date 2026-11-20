---
title: Digital voice and trunking
description: Operating notes for MPT 1327, P25 Phase 2, Tetrapol, TETRA, DECT, and GSM downlink.
---

Shared trunking, encryption keys, and DVSI vocoder setup live in [Decoders](../decoders.md).

## MPT 1327

Decodes control channel grants (GTC), Alohas, acks, and broadcasts. Voice calls are analog, so [Trunk system](../decoders.md#trunking) follows grants with NFM. Talkgroups encode as `prefix × 8192 + ident` (`10/4105` → `86025`).

## P25 Phase 2

Tune the TDMA voice channel; WACN, system ID, and NAC auto-recover from SACCH. **Slot** picks TS1, TS2, or both. Encrypted calls stay muted.

## Tetrapol

12.5 kHz PMR downlinks; scrambling auto-detects (or pin **Scrambling**). Pick **VHF** for 70–87 MHz. Logs control channel cell/group/paging info and traffic burst durations. Voice audio and uplinks are not decoded.

## TETRA

25 kHz downlink carrier, no settings required. Logs cell identity (MCC/MNC/colour code/LA), call setup/grants, voice traffic markers, SDS (status, text, LIP positions on **Map**, raw hex), and mobility events. Voice audio is not decoded; air-encrypted cells log identity only.

## DECT

Surveys base stations and plays unencrypted 32 kbit/s G.726 calls (needs 1.9 GHz radio > 1.728 MHz wide: HackRF/SDRplay, not RTL-SDR).

- **EU:** 1880–1900 MHz (ch 0 = 1897.344 MHz counting down by 1.728 MHz; **Band** centre 1889.568 MHz at 20 MS/s).
- **US:** 1920–1930 MHz (counting up from 1921.536 MHz; **Band** centre 1924.992 MHz at 10 MS/s).
- **Span:** **Carrier** (2.304 MS/s) or **Band**. **Side:** Base, Handset, or Both.
- Encryption marks active only after an observed grant. Encrypted calls stay muted.

## GSM downlink

Cell monitor on one downlink C0 carrier (541.667 kS/s, 200 kHz). Decodes BCCH/CCCH (cell ID, neighbours, paging, assignments). Enable **Follow** to decode non-hopping SDCCH/8 or SDCCH/4 on the same ARFCN (LAPDm, RR/MM/CC/SS/SMS, A5 cipher command). Ciphered traffic and hopping channels are not decoded.
