import { ChoiceChip, choicesOf, HzChip, limitOf, type Mode } from "./kit";

export function SsbChips({ params, choices, limits, onParams }: Mode<"ssb">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Side"
        title="Sideband"
        value={settings.sideband}
        options={choicesOf(choices, params.settings, "sideband")}
        onChange={(sideband) => onParams({ type: "ssb", settings: { ...settings, sideband } })}
      />
      <HzChip
        label="BW"
        title="SSB bandwidth"
        value={settings.bandwidth_hz}
        limit={limitOf(limits, "bandwidth_hz")}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "ssb", settings: { ...settings, bandwidth_hz } })
        }
      />
    </>
  );
}
