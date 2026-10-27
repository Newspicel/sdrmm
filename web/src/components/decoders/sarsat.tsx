import { BandwidthChip, type Mode } from "./kit";

export function SarsatChips({ params, onParams }: Mode<"sarsat">) {
  const { settings } = params;
  return (
    <BandwidthChip
      valueHz={settings.bandwidth_hz}
      optionsHz={[10_000, 20_000]}
      onCommit={(bandwidth_hz) =>
        onParams({ type: "sarsat", settings: { ...settings, bandwidth_hz } })
      }
    />
  );
}
