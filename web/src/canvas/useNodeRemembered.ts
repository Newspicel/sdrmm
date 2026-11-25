import type { Dispatch, SetStateAction } from "react";
import { rememberedKey, useRemembered, type Valid } from "../lib/remembered";
import { useWorkspaceSelect } from "./context";

/** A view choice kept in this browser for one node of the open workspace. */
export function useNodeRemembered<T>(
  node: string,
  name: string,
  fallback: T,
  valid: Valid<T>,
): [T, Dispatch<SetStateAction<T>>] {
  const workspaceId = useWorkspaceSelect((workspace) => workspace.id);
  return useRemembered(rememberedKey(name, workspaceId, node), fallback, valid);
}
