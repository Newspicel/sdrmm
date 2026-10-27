import { BandwidthChip, type Mode } from "./kit";

export function AcarsChips({ params, onParams }: Mode<"acars">) {
  return (
    <BandwidthChip
      valueHz={params.settings.bandwidth_hz}
      optionsHz={[8_000, 12_500, 25_000]}
      onCommit={(bandwidth_hz) =>
        onParams({ type: "acars", settings: { ...params.settings, bandwidth_hz } })
      }
    />
  );
}
