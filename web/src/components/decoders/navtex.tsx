import { InvertChip, type Mode } from "./kit";

export function NavtexChips({ params, onParams }: Mode<"navtex">) {
  return (
    <InvertChip
      on={params.settings.invert}
      onChange={(invert) => onParams({ type: "navtex", settings: { ...params.settings, invert } })}
    />
  );
}
