import { ChoiceChip, choicesOf, limitOf, type Mode, OptionalNumberChip } from "./kit";

export function TetrapolChips({ params, choices, limits, onParams }: Mode<"tetrapol">) {
  const { settings } = params;
  const set = (next: typeof settings) => onParams({ type: "tetrapol", settings: next });
  return (
    <>
      <ChoiceChip
        label="Band"
        title="Tetrapol band"
        value={settings.band}
        options={choicesOf(choices, params.settings, "band")}
        onChange={(band) => set({ ...settings, band })}
      />
      <OptionalNumberChip
        label="Scrambling"
        title="Scrambling code, empty to find it"
        placeholder="auto"
        value={settings.scrambling ?? null}
        {...limitOf(limits, "scrambling")}
        onCommit={(scrambling) => set({ ...settings, scrambling })}
      />
    </>
  );
}
