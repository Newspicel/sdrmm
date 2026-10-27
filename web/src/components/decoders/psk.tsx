import { ChoiceChip, choicesOf, InvertChip, type Mode } from "./kit";

export function PskChips({ params, choices, onParams }: Mode<"psk">) {
  const settings = params.settings;
  return (
    <>
      <ChoiceChip
        label="Mode"
        title="PSK symbol rate"
        value={settings.baud}
        options={choicesOf(choices, params.settings, "baud")}
        onChange={(baud) => onParams({ type: "psk", settings: { ...settings, baud } })}
      />
      <InvertChip
        on={settings.invert}
        onChange={(invert) => onParams({ type: "psk", settings: { ...settings, invert } })}
      />
    </>
  );
}

import { type PanelProps, TextView } from "./panelKit";

export function PskPanel({ records }: PanelProps<"psk">) {
  return <TextView kind="psk" records={records} />;
}
