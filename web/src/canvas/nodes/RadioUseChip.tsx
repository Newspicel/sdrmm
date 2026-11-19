import { Chips, ChoiceChip } from "../../components/face/Chips";
import type { DeviceSet, RadioUse } from "../../lib/types";
import { USE_LABEL } from "./radioUse";

const USE_TITLE =
  "Receive keeps transmitters off. Send runs no receiver. Both: a half-duplex radio takes turns";

/** The use a radio is put to, offered only when its duplex leaves a choice. */
export function RadioUseChip({
  set,
  value,
  onChange,
}: {
  set: DeviceSet;
  value: RadioUse;
  onChange: (radio_use: RadioUse) => void;
}) {
  const uses = set.uses ?? [];
  if (uses.length < 2) {
    return null;
  }
  return (
    <Chips inset="none">
      <ChoiceChip
        label="Use"
        title={USE_TITLE}
        value={value}
        options={uses.map((use) => ({ value: use, label: USE_LABEL[use] }))}
        onChange={onChange}
      />
    </Chips>
  );
}
