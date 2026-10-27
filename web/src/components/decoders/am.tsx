import { BandwidthChip, type Mode, ToggleChip } from "./kit";

export function AmChips({ params, onParams }: Mode<"am">) {
  const settings = params.settings;
  return (
    <>
      <BandwidthChip
        valueHz={settings.bandwidth_hz}
        optionsHz={[5_000, 8_000, 10_000]}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "am", settings: { ...settings, bandwidth_hz } })
        }
      />
      <ToggleChip
        label="Sync"
        on={settings.sync}
        title="Lock to the carrier; clearer under fading"
        onChange={(sync) => onParams({ type: "am", settings: { ...settings, sync } })}
      />
    </>
  );
}
