import { BandwidthChip, InvertChip, type Mode } from "./kit";

export function PagerChips({ params, onParams }: Mode<"flex" | "ermes">) {
  const { type, settings } = params;
  return (
    <>
      <BandwidthChip
        valueHz={settings.bandwidth_hz}
        optionsHz={[12_500, 25_000]}
        onCommit={(bandwidth_hz) => onParams({ type, settings: { ...settings, bandwidth_hz } })}
      />
      <InvertChip
        on={settings.invert}
        onChange={(invert) => onParams({ type, settings: { ...settings, invert } })}
      />
    </>
  );
}
