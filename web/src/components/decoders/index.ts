import type { ReactNode } from "react";
import { AcarsChips } from "./acars";
import { AdsbChips } from "./adsb";
import { AirtimeChips } from "./airtime";
import { AisChips } from "./ais";
import { AmChips } from "./am";
import { AprsChips } from "./aprs";
import { AptChips } from "./apt";
import { AtvChips } from "./atv";
import { BleChips } from "./ble";
import { SkimmerChips } from "./cwSkimmer";
import { DabChips } from "./dab";
import { DatvChips } from "./datv";
import { DectChips } from "./dect";
import { DmrChips } from "./dmr";
import { DrmChips } from "./drm";
import { DvbtChips } from "./dvbt";
import { FreedvChips } from "./freedv";
import { GnssChips } from "./gnss";
import { GsmChips } from "./gsm";
import { IdentChips } from "./ident";
import { IlsChips } from "./ils";
import { AeroChips } from "./inmarsatAero";
import { IridiumChips } from "./iridium";
import { IsmChips } from "./ism";
import type { BroadcastMode, ChannelTypeId } from "./kit";
import { LoraChips } from "./lora";
import { LrptChips } from "./lrpt";
import { MorseChips } from "./morse";
import { NarrowBandwidthChips } from "./narrowband";
import { NavtexChips } from "./navtex";
import { NfmChips } from "./nfm";
import { NoiseFigureChips } from "./noiseFigure";
import { NxdnChips } from "./nxdn";
import { PagerChips } from "./pagers";
import type { Panels } from "./panelKit";
import { PerChips } from "./per";
import { PocsagChips } from "./pocsag";
import { PskChips } from "./psk";
import { RadioClockChips } from "./radioClock";
import { RadiosondeChips } from "./radiosonde";
import { RemoteIdChips } from "./remoteId";
import { RttyChips } from "./rtty";
import { SarsatChips } from "./sarsat";
import { SelcallChips } from "./selcall";
import { SidChips } from "./sid";
import { SsbChips } from "./ssb";
import { SstvChips } from "./sstv";
import { StanagChips } from "./stanag";
import { TetrapolChips } from "./tetrapol";
import { VorChips } from "./vor";
import { WefaxChips } from "./wefax";
import { WfmChips } from "./wfm";
import { WmbusChips } from "./wmbus";
import { WsjtChips } from "./wsjt";

export type DecoderChips<K extends ChannelTypeId> = (props: BroadcastMode<K>) => ReactNode;

export type ModeChipsRegistry = { [K in ChannelTypeId]: DecoderChips<K> | null };

export const MODE_CHIPS: ModeChipsRegistry = {
  nfm: NfmChips,
  selcall: SelcallChips,
  am: AmChips,
  ssb: SsbChips,
  wfm: WfmChips,
  pocsag: PocsagChips,
  flex: PagerChips,
  ermes: PagerChips,
  eot: NarrowBandwidthChips,
  same: NarrowBandwidthChips,
  ism: IsmChips,
  wmbus: WmbusChips,
  sarsat: SarsatChips,
  tetrapol: TetrapolChips,
  adsb: AdsbChips,
  ais: AisChips,
  inmarsat_aero: AeroChips,
  iridium: IridiumChips,
  aprs: AprsChips,
  rtty: RttyChips,
  morse: MorseChips,
  cw_skimmer: SkimmerChips,
  ft8: WsjtChips,
  ft4: WsjtChips,
  wspr: WsjtChips,
  psk: PskChips,
  navtex: NavtexChips,
  radio_clock: RadioClockChips,
  gnss: GnssChips,
  vor: VorChips,
  ils: IlsChips,
  acars: AcarsChips,
  atv: AtvChips,
  sstv: SstvChips,
  dab: DabChips,
  datv: DatvChips,
  dvbt: DvbtChips,
  drm: DrmChips,
  dmr: DmrChips,
  nxdn: NxdnChips,
  freedv: FreedvChips,
  ident: IdentChips,
  dect: DectChips,
  gsm: GsmChips,
  apt: AptChips,
  lrpt: LrptChips,
  wefax: WefaxChips,
  radiosonde: RadiosondeChips,
  lora: LoraChips,
  remote_id: RemoteIdChips,
  ble: BleChips,
  wifi_occupancy: AirtimeChips,
  ism_survey: AirtimeChips,
  noise_figure: NoiseFigureChips,
  per: PerChips,
  sid: SidChips,
  stanag: StanagChips,
  dstar: null,
  ysf: null,
  p25: null,
  dpmr: null,
  m17: null,
  tetra: null,
  dsc: null,
  inmarsat_stdc: null,
  vdl2: null,
  hfdl: null,
};

import { AdsbPanel } from "./adsb";
import { IsmSurveyView, WifiOccupancyView } from "./airtime";
import { AisPanel } from "./ais";
import { AprsWeatherView } from "./aprs";
import { BleView } from "./ble";
import { BroadcastView } from "./broadcast";
import { BroadcastDataPanel } from "./broadcastData";
import { CwSkimmerView } from "./cwSkimmer";
import { DectView } from "./dect";
import { GsmView } from "./gsm";
import { IdentView } from "./ident";
import { LoraView } from "./lora";
import { MorsePanel } from "./morse";
import { NoiseFigureView } from "./noiseFigure";
import { PerView } from "./per";
import { PskPanel } from "./psk";
import { RadiosondeView } from "./radiosonde";
import { RdsView } from "./rds";
import { RemoteIdView } from "./remoteId";
import { RttyPanel } from "./rtty";
import { SidView } from "./sid";
import { ToneView } from "./tone";
import { VorView } from "./vor";

export const DECODER_PANELS: Panels = {
  rds: RdsView,
  adsb: AdsbPanel,
  ais: AisPanel,
  rtty: RttyPanel,
  morse: MorsePanel,
  psk: PskPanel,
  cw_skimmer: CwSkimmerView,
  tone: ToneView,
  ident: IdentView,
  aprs: AprsWeatherView,
  broadcast: BroadcastView,
  broadcast_data: BroadcastDataPanel,
  radiosonde: RadiosondeView,
  vor: VorView,
  dect: DectView,
  gsm: GsmView,
  lora: LoraView,
  remote_id: RemoteIdView,
  ble: BleView,
  wifi_occupancy: WifiOccupancyView,
  ism_survey: IsmSurveyView,
  noise_figure: NoiseFigureView,
  per: PerView,
  sid: SidView,
};
