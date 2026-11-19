import type { ReactNode } from "react";
import type { Needs } from "../lib/settingBlocks";
import { Button } from "./BaseControls";

const RING: Keyframe[] = [
  { boxShadow: "0 0 0 2px var(--color-accent)" },
  { boxShadow: "0 0 0 2px var(--color-accent)", offset: 0.6 },
  { boxShadow: "0 0 0 2px transparent" },
];

const CONTROL = '[role="combobox"], [role="listbox"], input, button';

/** Rings the control of the setting at `key` near `from`, so a greyed field shows what turns it on. */
export function flashSetting(from: Element, key: string): void {
  const scope = from.closest("aside") ?? document;
  const setting = scope.querySelector(`[data-setting="${CSS.escape(key)}"]`);
  const control = setting?.matches(CONTROL) ? setting : setting?.querySelector(CONTROL);
  control?.animate(RING, { duration: 1000, easing: "ease-out" });
}

/** The name `flashSetting` finds a setting by, and the rounding its blink shows in. */
export function settingMark(key: string): { "data-setting": string; className: string } {
  return { "data-setting": key, className: "rounded-control" };
}

/**
 * A setting the others make moot for now: in its place, greyed out and inert. Clicking it
 * blinks the setting that turns it on.
 */
export function Unused({ needs, children }: { needs: Needs | null; children: ReactNode }) {
  return (
    <div className="relative">
      <fieldset disabled className="m-0 min-w-0 border-0 p-0 opacity-45">
        {children}
      </fieldset>
      {needs !== null && (
        <Button
          type="button"
          aria-label={needs.hint}
          title={needs.hint}
          className="absolute inset-0 cursor-help outline-none"
          onClick={(event) => flashSetting(event.currentTarget, needs.key)}
        />
      )}
    </div>
  );
}
