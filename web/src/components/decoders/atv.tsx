import {
  BandwidthChip,
  ChoiceChip,
  choicesOf,
  InvertChip,
  limitOf,
  type Mode,
  OptionalNumberChip,
  scaledLimit,
  ToggleChip,
} from "./kit";

export function AtvChips({ params, choices, limits, onParams }: Mode<"atv">) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "atv", settings: next });
  return (
    <>
      <ChoiceChip
        label="Mod"
        title="Modulation"
        value={settings.modulation}
        options={choicesOf(choices, params.settings, "modulation")}
        onChange={(modulation) => set({ ...settings, modulation })}
      />
      <ChoiceChip
        label="Lines"
        title="Scanning standard"
        value={settings.standard}
        options={choicesOf(choices, params.settings, "standard")}
        onChange={(standard) => set({ ...settings, standard })}
      />
      <BandwidthChip
        valueHz={settings.bandwidth_hz}
        optionsHz={[500_000, 1_000_000, 1_500_000, 1_600_000]}
        onCommit={(bandwidth_hz) => set({ ...settings, bandwidth_hz })}
      />
      <ChoiceChip
        label="Colour"
        title="Composite colour system"
        value={settings.color}
        options={choicesOf(choices, params.settings, "color")}
        onChange={(color) => set({ ...settings, color })}
      />
      <OptionalNumberChip
        label="Sound"
        title="FM sound subcarrier, empty for none"
        placeholder="off"
        value={
          settings.sound_subcarrier_hz == null ? null : settings.sound_subcarrier_hz / 1_000_000
        }
        {...scaledLimit(limitOf(limits, "sound_subcarrier_hz"), 1e-6)}
        unit="MHz"
        onCommit={(mhz) =>
          set({ ...settings, sound_subcarrier_hz: mhz === null ? null : mhz * 1_000_000 })
        }
      />
      <ToggleChip
        label="Interlace"
        title="Weave both fields into one frame"
        on={settings.interlace}
        onChange={(interlace) => set({ ...settings, interlace })}
      />
      <InvertChip on={settings.invert} onChange={(invert) => set({ ...settings, invert })} />
    </>
  );
}
