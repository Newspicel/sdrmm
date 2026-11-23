import { queryOptions, useQuery } from "@tanstack/react-query";
import { client, unwrap } from "./api";
import type { ChannelBand, ChannelInfo, ChannelParams, ChannelSettings } from "./types";

export function channelBandQuery(params: ChannelParams) {
  return queryOptions({
    queryKey: ["post", "/api/channeltypes/band", params] as const,
    queryFn: async (): Promise<ChannelBand> =>
      unwrap(await client.POST("/api/channeltypes/band", { body: params })),
    staleTime: Number.POSITIVE_INFINITY,
  });
}

/**
 * Where a channel sits around its frequency, as its decoder lays it out: the running decoder's
 * band, else the band the server works out for the settings the node holds.
 */
export function useChannelBand(
  settings: ChannelSettings | null,
  live: ChannelInfo | null,
): ChannelBand | null {
  const running = live?.band ?? null;
  const asked = useQuery({
    ...channelBandQuery(settings?.params ?? { type: "", settings: {} }),
    enabled: running === null && settings !== null,
  });
  return running ?? asked.data ?? null;
}
