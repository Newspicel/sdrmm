import {
  ChoiceChip,
  choicesOf,
  InvertChip,
  limitOf,
  type Mode,
  PresetChip,
  ToggleChip,
} from "./kit";

export function RttyChips({ params, choices, limits, onParams }: Mode<"rtty">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "rtty", settings: next });
  return (
    <>
      <PresetChip
        label="Baud"
        title="RTTY baud"
        value={settings.baud}
        presets={choicesOf(choices, params.settings, "baud")}
        limit={limitOf(limits, "baud")}
        onCommit={(baud) => set({ ...settings, baud })}
      />
      <PresetChip
        label="Shift"
        title="RTTY shift"
        unit="Hz"
        value={settings.shift_hz}
        presets={choicesOf(choices, params.settings, "shift_hz")}
        limit={limitOf(limits, "shift_hz")}
        onCommit={(shift_hz) => set({ ...settings, shift_hz })}
      />
      <ChoiceChip
        label="Stop"
        title="RTTY stop bits"
        value={settings.stop_bits}
        options={choicesOf(choices, params.settings, "stop_bits")}
        onChange={(stop_bits) => set({ ...settings, stop_bits })}
      />
      <InvertChip on={settings.invert} onChange={(invert) => set({ ...settings, invert })} />
      <ToggleChip
        label="Unshift"
        title="Drop back to letters after a space, as most stations expect"
        on={settings.unshift_on_space}
        onChange={(unshift_on_space) => set({ ...settings, unshift_on_space })}
      />
    </>
  );
}

import { type PanelProps, TextView } from "./panelKit";

export function RttyPanel({ records }: PanelProps<"rtty">) {
  return <TextView kind="rtty" records={records} />;
}
