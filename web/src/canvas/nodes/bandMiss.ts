import { formatHz } from "../../components/format";
import type { ShortText } from "../../components/hunt";
import type { BandMiss } from "../../lib/types";

function roundedUp(hz: number): number {
  const scale = 10 ** (Math.floor(Math.log10(hz)) - 2);
  return Math.ceil(hz / scale) * scale;
}

function tooWideTitle(needsHz: number, topRateHz: number | null | undefined): string {
  if (topRateHz != null && topRateHz < needsHz) {
    return `Wider than this radio reaches. Its sample rate tops out at ${formatHz(topRateHz)}`;
  }
  return `Wider than the radio's window. Raise its sample rate to ${formatHz(roundedUp(needsHz))} or more`;
}

export function bandMissSaid(miss: BandMiss): ShortText {
  switch (miss.reason) {
    case "too_wide":
      return {
        label: `needs ${formatHz(roundedUp(miss.needs_hz))}`,
        title: tooWideTitle(miss.needs_hz, miss.top_rate_hz),
      };
    case "off_tuner":
      return { label: "off tuner", title: "Outside the radio's tuning range" };
    case "crowded":
      return {
        label: "crowded out",
        title:
          "Auto keeps the radio over the other decoders. Raise the sample rate or move this one to another radio",
      };
    case "tuned_away":
      return { label: "out of band", title: "The radio is tuned elsewhere" };
  }
}

export function bandMissOf({
  reported,
  reaches,
}: {
  reported: BandMiss | null | undefined;
  reaches: boolean;
}): BandMiss | null {
  if (reported !== undefined) {
    return reported;
  }
  return reaches ? null : { reason: "tuned_away" };
}
