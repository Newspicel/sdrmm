import { HzChip, limitOf, type Mode, OptionalNumberChip } from "./kit";

export function MorseChips({ params, limits, onParams }: Mode<"morse">) {
  const settings = params.settings;
  return (
    <>
      <HzChip
        label="BW"
        title="CW filter bandwidth"
        value={settings.bandwidth_hz}
        limit={limitOf(limits, "bandwidth_hz")}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "morse", settings: { ...settings, bandwidth_hz } })
        }
      />
      <OptionalNumberChip
        label="Speed"
        title="Morse speed, empty to auto-track"
        placeholder="auto"
        value={settings.wpm ?? null}
        {...limitOf(limits, "wpm")}
        unit="WPM"
        onCommit={(wpm) => onParams({ type: "morse", settings: { ...settings, wpm } })}
      />
    </>
  );
}

import { type PanelProps, TextView } from "./panelKit";

export function MorsePanel({ records }: PanelProps<"morse">) {
  return <TextView kind="morse" records={records} />;
}
