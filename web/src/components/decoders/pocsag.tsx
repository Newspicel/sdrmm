import { BandwidthChip, ChoiceChip, choicesOf, InvertChip, type Mode } from "./kit";

export function PocsagChips({ params, choices, onParams }: Mode<"pocsag">) {
  const settings = params.settings;
  const baud = settings.baud;
  return (
    <>
      <ChoiceChip
        label="Baud"
        title="POCSAG baud"
        value={baud}
        options={choicesOf(choices, params.settings, "baud")}
        quiet={baud === "auto"}
        onChange={(next) => onParams({ type: "pocsag", settings: { ...settings, baud: next } })}
      />
      <BandwidthChip
        valueHz={settings.bandwidth_hz}
        optionsHz={[12_500, 25_000]}
        onCommit={(bandwidth_hz) =>
          onParams({ type: "pocsag", settings: { ...settings, bandwidth_hz } })
        }
      />
      <InvertChip
        on={settings.invert}
        onChange={(invert) => onParams({ type: "pocsag", settings: { ...settings, invert } })}
      />
    </>
  );
}
