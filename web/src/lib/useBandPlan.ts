import { useQuery } from "@tanstack/react-query";
import { useWorkspaceNow, useWorkspaceSelect } from "../canvas/context";
import { bandPlanQuery, bandRegionsQuery } from "./api";
import type { BandPlan, BandRegion } from "./types";

export interface BandPlanState {
  region: string | null;
  regions: readonly BandRegion[];
  plan: BandPlan | null;
  ruler: boolean;
  setRegion: (region: string) => void;
  setRuler: (ruler: boolean) => void;
}

export function useBandPlan(): BandPlanState {
  const now = useWorkspaceNow();
  const stored = useWorkspaceSelect((workspace) => workspace.settings.band_region ?? null);
  const ruler = useWorkspaceSelect((workspace) => workspace.settings.band_ruler ?? true);
  const regions = useQuery(bandRegionsQuery());
  const region = stored ?? regions.data?.default_region ?? null;
  const plan = useQuery(bandPlanQuery(region));

  return {
    region,
    regions: regions.data?.regions ?? [],
    plan: plan.data ?? null,
    ruler,
    setRegion: (next) => now().editSettings({ band_region: next }),
    setRuler: (next) => now().editSettings({ band_ruler: next }),
  };
}
