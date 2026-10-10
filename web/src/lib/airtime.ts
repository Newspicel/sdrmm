import type { IsmKind, IsmSurveyReport, WifiChannelLoad, WifiOccupancyReport } from "./types";

export const ISM_KIND_LABELS: Record<IsmKind, string> = {
  wifi: "Wi-Fi",
  bluetooth: "Bluetooth",
  ieee802154: "802.15.4",
  microwave_oven: "Microwave oven",
  narrowband: "Narrowband",
  wideband: "Wideband",
  continuous: "Continuous",
};

export const WIFI_BAND_LABELS: Record<WifiChannelLoad["band"], string> = {
  ghz2_4: "2.4 GHz",
  ghz5: "5 GHz",
  ghz6: "6 GHz",
};

export function percentOf(share: number): string {
  return `${(share * 100).toFixed(share < 0.1 ? 1 : 0)}%`;
}

export function wifiOccupancySummary(report: WifiOccupancyReport): string {
  if (report.channels.length === 0) {
    return "no Wi-Fi channel in view";
  }
  return report.channels
    .map((channel) => `ch ${channel.number} ${percentOf(channel.busy)}`)
    .join(" · ");
}

export function ismSurveySummary(report: IsmSurveyReport): string {
  return [
    `${percentOf(report.busy)} busy`,
    ...report.kinds.map(
      (load) => `${ISM_KIND_LABELS[load.kind]} ${load.bursts} · ${percentOf(load.airtime)}`,
    ),
  ].join(" · ");
}
