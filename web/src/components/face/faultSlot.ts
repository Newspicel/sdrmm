import { createContext, useContext } from "react";

/** Where a framed face shows its faults: its footer, beside the stats, so a fault takes no row. */
export const FaultSlot = createContext<HTMLElement | null>(null);

/** How a framed face's footer offers its fault slot to the shell. */
export const FaultSlotHolder = createContext<((slot: HTMLElement | null) => void) | null>(null);

export function useFaultSlot(): HTMLElement | null {
  return useContext(FaultSlot);
}

export function useFaultSlotHolder(): ((slot: HTMLElement | null) => void) | null {
  return useContext(FaultSlotHolder);
}
