import { Play, Square } from "lucide-react";
import { Button } from "../../components/BaseControls";
import { ICON_BTN } from "../../components/controls";
import type { MeterTone } from "../../components/dbfs";
import { Icon } from "../../components/Icon";
import { Tip } from "../../components/Tip";

export const METER_FILL: Record<MeterTone, string> = {
  ok: "bg-ok",
  warn: "bg-warn",
  danger: "bg-danger",
};

export function PlayToggle({ active, onToggle }: { active: boolean; onToggle: () => void }) {
  return (
    <Tip
      text={active ? "Stop listening" : "Listen"}
      render={
        <Button
          type="button"
          className={`${ICON_BTN} ${active ? "bg-accent/15 text-accent" : ""}`}
          aria-label={active ? "Stop" : "Play"}
          aria-pressed={active}
          onClick={onToggle}
        />
      }
    >
      <Icon glyph={active ? Square : Play} size={14} filled={active} />
    </Tip>
  );
}
