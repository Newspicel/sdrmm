import type { ReactNode } from "react";
import { useCanEdit } from "../lib/role";

/** Holds the canvas and inspector. For a viewer every control inside is disabled. */
export function ViewOnly({ children }: { children: ReactNode }) {
  return (
    <fieldset disabled={!useCanEdit()} className="flex min-h-0 min-w-0 flex-1">
      {children}
    </fieldset>
  );
}
