import type { DecodedRecordOf, GsmAssignment, GsmCell, GsmFrame, GsmIdentity } from "./types";

type Dedicated = Extract<GsmFrame["message"], { type: "dedicated" }>;

export const GSM_BAND_LABELS: Record<NonNullable<GsmFrame["band"]>, string> = {
  gsm450: "GSM 450",
  gsm480: "GSM 480",
  gsm850: "GSM 850",
  gsm900: "GSM 900",
  dcs1800: "DCS 1800",
  pcs1900: "PCS 1900",
};

export const GSM_CHANNEL_LABELS: Record<GsmFrame["channel"], string> = {
  bcch: "BCCH",
  ccch: "CCCH",
  sdcch: "SDCCH",
  sacch: "SACCH",
};

const CHANNEL_TYPE_LABELS: Record<GsmAssignment["channel"], string> = {
  tch_f: "TCH/F",
  tch_h: "TCH/H",
  sdcch4: "SDCCH/4",
  sdcch8: "SDCCH/8",
  pdch: "PDCH",
};

const CCCH_LABELS: Record<NonNullable<GsmCell["ccch"]>, string> = {
  separate: "1 CCCH",
  combined: "1 CCCH with SDCCH/4",
  two: "2 CCCH",
  three: "3 CCCH",
  four: "4 CCCH",
};

export function siLabel(si: NonNullable<GsmCell["system_information"]>[number]): string {
  return `SI${si.slice(2)}`;
}

export function bsicLabel(bsic: number): string {
  return `${bsic >> 3}-${bsic & 7}`;
}

export function cipherLabel(algorithm: number): string {
  return algorithm === 0 ? "no cipher" : `A5/${algorithm}`;
}

export function identityLabel(identity: GsmIdentity): string {
  return `${identity.kind.toUpperCase()} ${identity.value}`;
}

export function ccchLabel(ccch: GsmCell["ccch"]): string | null {
  return ccch ? CCCH_LABELS[ccch] : null;
}

export function assignmentLabel(assignment: GsmAssignment): string {
  const parts = [`${CHANNEL_TYPE_LABELS[assignment.channel]} TS${assignment.timeslot}`];
  if (assignment.subchannel != null) parts.push(`SC${assignment.subchannel}`);
  if (assignment.arfcn != null) parts.push(`ARFCN ${assignment.arfcn}`);
  else if (assignment.hopping) parts.push(`hopping HSN ${assignment.hopping.hsn}`);
  parts.push(`TA ${assignment.timing_advance}`);
  return parts.join(" ");
}

function dedicatedSummary(message: Dedicated): string {
  const parts = [
    message.protocol && message.message
      ? `${message.protocol === "other" ? "L3" : message.protocol.toUpperCase()} ${message.message}`
      : `LAPDm ${message.frame === "unknown" ? "?" : message.frame.toUpperCase()}`,
  ];
  if (message.identity) parts.push(identityLabel(message.identity));
  if (message.cipher != null) parts.push(cipherLabel(message.cipher));
  return parts.join(" · ");
}

function cellSummary(cell: GsmCell): string[] {
  const parts: string[] = [];
  if (cell.mcc && cell.mnc) parts.push(`${cell.mcc}-${cell.mnc}`);
  if (cell.lac != null) parts.push(`LAC ${cell.lac}`);
  if (cell.cell_id != null) parts.push(`CI ${cell.cell_id}`);
  return parts;
}

export function gsmSummary(frame: GsmFrame): string {
  const message = frame.message;
  switch (message.type) {
    case "system_information": {
      const parts = [siLabel(message.si), ...cellSummary(message.cell)];
      if (["si2", "si2bis", "si2ter", "si5"].includes(message.si)) {
        parts.push(`${(message.cell.neighbours ?? []).length} neighbours`);
      }
      return parts.join(" · ");
    }
    case "paging":
      return `Paging · ${message.identities.map(identityLabel).join(", ")}`;
    case "immediate_assignment":
      return `Assignment · ${message.assignments.map(assignmentLabel).join(", ")}`;
    case "assignment_reject":
      return `Assignment reject · ${message.requests.length} requests`;
    case "dedicated":
      return `${GSM_CHANNEL_LABELS[frame.channel]} · ${dedicatedSummary(message)}`;
    case "other":
      return message.message_type === 0x20
        ? `${GSM_CHANNEL_LABELS[frame.channel]} · NCH notification`
        : `${GSM_CHANNEL_LABELS[frame.channel]} · RR 0x${message.message_type.toString(16).toUpperCase().padStart(2, "0")}`;
  }
}

export interface GsmCellView {
  key: string;
  cgi: string | null;
  arfcn: number | null;
  band: string | null;
  bsic: string;
  ccch: string | null;
  neighbours: number[];
  systemInformation: string[];
  frequencyErrorHz: number;
  snrDb: number;
  paged: number;
  assignments: number;
  at: string;
}

export function gsmCells(records: readonly DecodedRecordOf<"gsm">[]): GsmCellView[] {
  const cells = new Map<string, GsmCellView>();
  for (const record of records) {
    const frame = record.event.data;
    const key = `${record.device_set}:${record.channel}:${frame.arfcn ?? "-"}:${frame.bsic}`;
    const view = cells.get(key) ?? {
      key,
      cgi: null,
      arfcn: frame.arfcn ?? null,
      band: frame.band ? GSM_BAND_LABELS[frame.band] : null,
      bsic: bsicLabel(frame.bsic),
      ccch: null,
      neighbours: [],
      systemInformation: [],
      frequencyErrorHz: frame.frequency_error_hz,
      snrDb: frame.snr_db,
      paged: 0,
      assignments: 0,
      at: record.at,
    };
    view.cgi = frame.cgi ?? view.cgi;
    view.frequencyErrorHz = frame.frequency_error_hz;
    view.snrDb = frame.snr_db;
    view.at = record.at;
    const message = frame.message;
    if (message.type === "system_information") {
      view.ccch = ccchLabel(message.cell.ccch) ?? view.ccch;
      view.neighbours = message.cell.neighbours ?? view.neighbours;
      view.systemInformation = (message.cell.system_information ?? []).map(siLabel);
    } else if (message.type === "paging") {
      view.paged += message.identities.length;
    } else if (message.type === "immediate_assignment") {
      view.assignments += message.assignments.length;
    }
    cells.set(key, view);
  }
  return [...cells.values()].toSorted((a, b) => Date.parse(b.at) - Date.parse(a.at));
}
