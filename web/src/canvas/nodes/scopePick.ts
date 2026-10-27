import { identify, suggestedAt } from "../../components/bandPlan";
import { channelFrequencyHz } from "../../components/channelSettings";
import { formatMhz } from "../../components/format";
import {
  offsetToSpan,
  type SpectrumView,
  spanToOffset,
  spanToView,
  viewToSpan,
  viewWidth,
} from "../../components/spectrumView";
import { heardHz, type SetLevels } from "../../lib/levels";
import type { BandPlan, ChannelInfo, ChannelParams } from "../../lib/types";

export function streamChannels(
  channels: readonly ChannelInfo[],
  stream: number,
): readonly ChannelInfo[] {
  return channels.filter((channel) => (channel.stream ?? 0) === stream);
}

export interface ScopePick {
  hz: number;
  offsetHz: number;
}

export function pickAt(
  centerHz: number,
  spanHz: number,
  view: SpectrumView,
  at: number,
): ScopePick {
  const offsetHz = Math.round(spanToOffset(viewToSpan(view, at), spanHz));
  return { hz: centerHz + offsetHz, offsetHz };
}

export function dragTuneHz(
  centerHz: number,
  spanHz: number,
  view: SpectrumView,
  deltaPx: number,
  widthPx: number,
): number {
  if (!(spanHz > 0) || !(widthPx > 0)) {
    return Math.round(centerHz);
  }
  return Math.round(centerHz + (deltaPx / widthPx) * spanHz * viewWidth(view));
}

export function pickText(pick: ScopePick): { frequency: string; offset: string } {
  const offsetHz = Math.round(pick.offsetHz);
  return {
    frequency: `${Math.round(pick.hz)} Hz`,
    offset: `${offsetHz < 0 ? "-" : "+"}${Math.abs(offsetHz)} Hz`,
  };
}

export function bookmarkDraft(
  hz: number,
  plan: BandPlan | null,
): { label: string; mode: string | null } {
  const found = plan === null ? [] : identify(plan, hz);
  return {
    label: found[0]?.allocation.name ?? formatMhz(hz),
    mode: suggestedAt(found)?.type ?? null,
  };
}

export function channelTypeAt(
  suggested: ChannelParams | null,
  listening: ChannelInfo | undefined,
): string {
  return suggested?.type ?? listening?.settings.params.type ?? "nfm";
}

const awaitingCreation = new Map<string, number>();

export function tuneOnCreate(node: string, frequencyHz: number): void {
  awaitingCreation.set(node, frequencyHz);
}

export function takeCreationTune(node: string): number | undefined {
  const frequencyHz = awaitingCreation.get(node);
  awaitingCreation.delete(node);
  return frequencyHz;
}

export function markerAt(
  channels: readonly ChannelInfo[],
  levels: SetLevels | undefined,
  view: SpectrumView,
  centerHz: number,
  spanHz: number,
  at: number,
  tolerance: number,
): ChannelInfo | null {
  let best: ChannelInfo | null = null;
  let bestDistance = tolerance;
  for (const channel of channels) {
    const offsetHz = heardHz(channelFrequencyHz(channel.settings), levels?.[channel.id]) - centerHz;
    const position = spanToView(view, offsetToSpan(offsetHz, spanHz));
    const distance = Math.abs(position - at);
    if (distance <= bestDistance) {
      best = channel;
      bestDistance = distance;
    }
  }
  return best;
}

export function markerFrom(
  target: EventTarget | null,
  channels: readonly ChannelInfo[],
): ChannelInfo | null {
  if (!(target instanceof Element)) {
    return null;
  }
  const id = target.closest("[data-marker]")?.getAttribute("data-marker");
  if (id == null) {
    return null;
  }
  return channels.find((channel) => String(channel.id) === id) ?? null;
}
