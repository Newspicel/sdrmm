import { limitOf, type Mode, NumberChip } from "./kit";

export function StanagChips({ params, limits, onParams }: Mode<"stanag">) {
  return (
    <NumberChip
      label="Window"
      title="Seconds of signal behind each verdict"
      value={params.settings.window_s}
      {...limitOf(limits, "window_s")}
      unit="s"
      onCommit={(window_s) => onParams({ type: "stanag", settings: { window_s } })}
    />
  );
}
