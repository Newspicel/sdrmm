import { BandwidthChip, type Mode } from "./kit";

export function NarrowBandwidthChips({ params, onParams }: Mode<"eot" | "same">) {
  const { type, settings } = params;
  return (
    <BandwidthChip
      valueHz={settings.bandwidth_hz}
      optionsHz={[12_500, 25_000]}
      onCommit={(bandwidth_hz) => onParams({ type, settings: { ...settings, bandwidth_hz } })}
    />
  );
}
