import { createContext, useContext } from "react";
import type { UserRole } from "./types";

export const RoleContext = createContext<UserRole>("admin");

export function useRole(): UserRole {
  return useContext(RoleContext);
}

/** A viewer watches; every other role may change the workspace. */
export function useCanEdit(): boolean {
  return useRole() !== "viewer";
}
