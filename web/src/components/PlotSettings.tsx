import { Button } from "./BaseControls";

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
      className={`group flex flex-col gap-1 rounded-control border p-1 text-left transition-colors duration-100 ${
        on ? "border-accent-dim bg-accent/10" : "border-transparent hover:bg-panel-2"
      }`}
    >
      <span className="h-3 w-full rounded-inset" style={{ background: fill }} />
      <span
        className={`font-mono text-2xs ${on ? "text-accent" : "text-ink-faint group-hover:text-ink"}`}
      >
        {name}
      </span>
    </Button>
  );
}
