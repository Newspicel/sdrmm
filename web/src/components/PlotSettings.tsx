import { Settings2 } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "./BaseControls";
import { plotButton } from "./controls";
import { Icon } from "./Icon";
import { Popover } from "./Popover";
import { SettingsPanel } from "./SettingsPanel";

export function PlotSettings({
  title,
  changed,
  children,
}: {
  title: string;
  changed: boolean;
  children: ReactNode;
}) {
  return (
    <Popover
      label={<Icon glyph={Settings2} size={12} />}
      title={title}
      triggerClass={plotButton(changed)}
      width="w-76"
      padded={false}
    >
      {() => <SettingsPanel>{children}</SettingsPanel>}
    </Popover>
  );
}

export function Swatch({
  name,
  fill,
  on,
  onClick,
}: {
  name: string;
  fill: string;
  on: boolean;
  onClick: () => void;
}) {
  return (
    <Button
      type="button"
      aria-pressed={on}
      onClick={onClick}
      className={`group flex flex-col gap-1 rounded-[3px] border p-1 text-left transition-colors duration-100 ${
        on ? "border-accent-dim bg-accent/10" : "border-transparent hover:bg-panel-2"
      }`}
    >
      <span className="h-3 w-full rounded-[2px]" style={{ background: fill }} />
      <span
        className={`font-mono text-[10.5px] ${on ? "text-accent" : "text-ink-faint group-hover:text-ink"}`}
      >
        {name}
      </span>
    </Button>
  );
}
