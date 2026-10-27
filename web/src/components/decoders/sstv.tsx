import {
  ChoiceChip,
  choiceOptions,
  choicesOf,
  type Mode,
  SSTV_AUTO,
  type SstvMode,
  ToggleChip,
  unsetAs,
} from "./kit";

export function SstvChips({ params, choices, onParams }: Mode<"sstv">) {
  const settings = params.settings;
  const mode = settings.mode ?? SSTV_AUTO;
  return (
    <>
      <ChoiceChip
        label="Mod"
        title="Modulation"
        value={settings.modulation}
        options={choicesOf(choices, params.settings, "modulation")}
        onChange={(modulation) => onParams({ type: "sstv", settings: { ...settings, modulation } })}
      />
      <ChoiceChip
        label="Mode"
        title="Scanning mode"
        value={mode}
        options={unsetAs(choiceOptions<SstvMode | null>(choices, "mode"), SSTV_AUTO)}
        quiet={mode === SSTV_AUTO}
        onChange={(next) =>
          onParams({
            type: "sstv",
            settings: { ...settings, mode: next === SSTV_AUTO ? null : next },
          })
        }
      />
      <ToggleChip
        label="Slant"
        title="Straighten pictures from a sender whose clock runs off"
        on={settings.slant_correction}
        onChange={(slant_correction) =>
          onParams({ type: "sstv", settings: { ...settings, slant_correction } })
        }
      />
      <ToggleChip
        label="Partial"
        title="Keep unfinished pictures"
        on={settings.keep_partial}
        onChange={(keep_partial) =>
          onParams({ type: "sstv", settings: { ...settings, keep_partial } })
        }
      />
    </>
  );
}
