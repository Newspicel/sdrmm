import labels from "../generated/labels.json";
import type { StanagInterleave, StanagModem, StanagRate, StanagReport } from "./types";

const MODEM_LABELS: Record<StanagModem, string> = labels.stanag_modem;
const INTERLEAVE_LABELS: Record<StanagInterleave, string> = labels.stanag_interleave;

export function stanagModem(modem: StanagModem): string {
  return MODEM_LABELS[modem];
}

export function stanagRate(rate: StanagRate): string {
  return `${rate.bps} bit/s${rate.voice ? " voice" : ""} ${INTERLEAVE_LABELS[rate.interleave]}`;
}

export function stanagSummary(report: StanagReport): string {
  if (report.modem == null) {
    return "modem gone";
  }
  return [
    stanagModem(report.modem),
    report.rate == null ? null : stanagRate(report.rate),
    `${Math.round(report.audio_hz)} Hz`,
  ]
    .filter((part) => part !== null)
    .join(" · ");
}
