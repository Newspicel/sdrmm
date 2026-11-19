import type { Air } from "./airState";

/** The header's word for a transmission: armed, or on air in red. */
export function AirLabel({ air }: { air: Air }) {
  return air === "on air" ? (
    <span role="status" className="font-semibold text-danger" title="Transmitting now">
      on air
    </span>
  ) : (
    <span role="status">armed</span>
  );
}
