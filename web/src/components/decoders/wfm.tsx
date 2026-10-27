import { ChoiceChip, choicesOf, type Mode, ToggleChip } from "./kit";

export function WfmChips({ params, choices, onParams }: Mode<"wfm">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="De-emph"
        title="De-emphasis"
        value={settings.deemphasis_us}
        options={choicesOf(choices, params.settings, "deemphasis_us")}
        onChange={(deemphasis_us) =>
          onParams({ type: "wfm", settings: { ...settings, deemphasis_us } })
        }
      />
      <ToggleChip
        label="Stereo"
        title="Decode the stereo pilot; mono is quieter on weak signals"
        on={settings.stereo}
        onChange={(stereo) => onParams({ type: "wfm", settings: { ...settings, stereo } })}
      />
    </>
  );
}
