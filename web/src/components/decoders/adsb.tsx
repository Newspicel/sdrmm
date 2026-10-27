import { type Mode, ToggleChip } from "./kit";

export function AdsbChips({ params, onParams }: Mode<"adsb">) {
  return (
    <ToggleChip
      label="CRC fix"
      title="Repair single-bit errors the checksum can pin down"
      on={params.settings.crc_fix}
      onChange={(crc_fix) => onParams({ type: "adsb", settings: { ...params.settings, crc_fix } })}
    />
  );
}

import { type PanelProps, TargetsView } from "./panelKit";

export function AdsbPanel({ entities }: PanelProps<"adsb">) {
  return <TargetsView entities={entities} />;
}
