import { type Mode, ToggleChip } from "./kit";

export function AptChips({ params, onParams }: Mode<"apt">) {
  const settings = params.settings;
  return (
    <ToggleChip
      label="Partial"
      title="Keep unfinished passes"
      on={settings.keep_partial}
      onChange={(keep_partial) =>
        onParams({ type: "apt", settings: { ...settings, keep_partial } })
      }
    />
  );
}
