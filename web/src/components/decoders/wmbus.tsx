import { type Mode, WmbusKeysChip } from "./kit";

export function WmbusChips({ params, onParams }: Mode<"wmbus">) {
  const settings = params.settings;
  return (
    <WmbusKeysChip
      keys={settings.keys ?? []}
      onChange={(keys) => onParams({ type: "wmbus", settings: { ...settings, keys } })}
    />
  );
}
