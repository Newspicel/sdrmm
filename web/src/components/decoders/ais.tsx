import { ChoiceChip, choicesOf, type Mode } from "./kit";

export function AisChips({ params, choices, onParams }: Mode<"ais">) {
  return (
    <ChoiceChip
      label="Channel"
      title="AIS channel"
      value={params.settings.ais_channel}
      options={choicesOf(choices, params.settings, "ais_channel")}
      onChange={(ais_channel) =>
        onParams({ type: "ais", settings: { ...params.settings, ais_channel } })
      }
    />
  );
}

import { type PanelProps, TargetsView } from "./panelKit";

export function AisPanel({ entities }: PanelProps<"ais">) {
  return <TargetsView entities={entities} />;
}
