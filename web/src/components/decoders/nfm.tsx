import {
  BandwidthChip,
  ChoiceChip,
  CTCSS_DEFAULT_HZ,
  choicesOf,
  DCS_DEFAULT_CODE,
  HzChip,
  INVERSION_DEFAULT_HZ,
  limitOf,
  type Mode,
  ToggleChip,
} from "./kit";

export function NfmChips({ params, choices, limits, onParams }: Mode<"nfm">) {
  const settings = params.settings;
  const mode = settings.tone_mode;
  const scrambler = settings.scrambler_mode;
  const set = (next: typeof settings) => onParams({ type: "nfm", settings: next });
  return (
    <>
      <BandwidthChip
        valueHz={settings.bandwidth_hz}
        optionsHz={[12_500, 25_000]}
        onCommit={(bandwidth_hz) => set({ ...settings, bandwidth_hz })}
      />
      <ChoiceChip
        label="Tone"
        title="Tone squelch"
        value={mode}
        options={choicesOf(choices, params.settings, "tone_mode")}
        quiet={mode === "off"}
        onChange={(tone_mode) =>
          set({
            ...settings,
            tone_mode,
            ctcss_hz: settings.ctcss_hz ?? CTCSS_DEFAULT_HZ,
            dcs_code: settings.dcs_code ?? DCS_DEFAULT_CODE,
          })
        }
      />
      {mode === "ctcss" && (
        <ChoiceChip
          label="CTCSS"
          title="CTCSS tone"
          value={settings.ctcss_hz ?? CTCSS_DEFAULT_HZ}
          options={choicesOf(choices, params.settings, "ctcss_hz")}
          onChange={(ctcss_hz) => set({ ...settings, ctcss_hz })}
        />
      )}
      {mode === "dcs" && (
        <ChoiceChip
          label="DCS"
          title="DCS code"
          value={settings.dcs_code ?? DCS_DEFAULT_CODE}
          options={choicesOf(choices, params.settings, "dcs_code")}
          onChange={(dcs_code) => set({ ...settings, dcs_code })}
        />
      )}
      <ChoiceChip
        label="Scrambler"
        title="Voice scrambler"
        value={scrambler}
        options={choicesOf(choices, params.settings, "scrambler_mode")}
        quiet={scrambler === "off"}
        onChange={(scrambler_mode) =>
          set({
            ...settings,
            scrambler_mode,
            inversion_hz: settings.inversion_hz ?? INVERSION_DEFAULT_HZ,
          })
        }
      />
      {scrambler === "inversion" && (
        <HzChip
          label="Carrier"
          title="Inversion carrier"
          value={settings.inversion_hz ?? INVERSION_DEFAULT_HZ}
          limit={limitOf(limits, "inversion_hz")}
          onCommit={(inversion_hz) => set({ ...settings, inversion_hz })}
        />
      )}
      <ToggleChip
        label="Compander"
        title="Expand audio that was sent with 2:1 compression"
        on={settings.compander}
        onChange={(compander) => set({ ...settings, compander })}
      />
    </>
  );
}
