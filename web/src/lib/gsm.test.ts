import { describe, expect, it } from "vitest";
import { gsmCells, gsmSummary } from "./gsm";
import type { DecodedRecordOf, GsmFrame } from "./types";

function frame(message: GsmFrame["message"], over: Partial<GsmFrame> = {}): GsmFrame {
  return {
    arfcn: 725,
    band: "dcs1800",
    bsic: 48,
    frame_number: 862_212,
    timeslot: 0,
    channel: "ccch",
    cgi: "262-02-318-11225",
    message,
    snr_db: 22.4,
    frequency_error_hz: 9_800,
    raw: "",
    ...over,
  };
}

function record(data: GsmFrame, at: string): DecodedRecordOf<"gsm"> {
  return { at, device_set: 1, channel: 0, freq_hz: 1_847_800_000, event: { kind: "gsm", data } };
}

describe("gsm", () => {
  it("summarises paging, assignments and dedicated signalling like the server", () => {
    expect(
      gsmSummary(
        frame({
          type: "paging",
          identities: [
            { kind: "imsi", value: "234159082506421" },
            { kind: "tmsi", value: "015C5703" },
          ],
        }),
      ),
    ).toBe("Paging · IMSI 234159082506421, TMSI 015C5703");
    expect(
      gsmSummary(
        frame({
          type: "immediate_assignment",
          assignments: [
            {
              channel: "sdcch8",
              timeslot: 1,
              subchannel: 1,
              arfcn: 725,
              tsc: 0,
              request: 0x26,
              timing_advance: 1,
            },
          ],
        }),
      ),
    ).toBe("Assignment · SDCCH/8 TS1 SC1 ARFCN 725 TA 1");
    expect(
      gsmSummary(
        frame(
          {
            type: "dedicated",
            sapi: 0,
            frame: "i",
            protocol: "rr",
            message: "Ciphering Mode Command",
            cipher: 1,
          },
          { channel: "sdcch", timeslot: 1, subchannel: 1 },
        ),
      ),
    ).toBe("SDCCH · RR Ciphering Mode Command · A5/1");
    expect(gsmSummary(frame({ type: "other", message_type: 0x20 }))).toBe(
      "CCCH · NCH notification",
    );
  });

  it("gathers a cell from its broadcasts and counts who was paged", () => {
    const cells = gsmCells([
      record(
        frame(
          {
            type: "system_information",
            si: "si2",
            cell: { ccch: "separate", neighbours: [730, 733], system_information: ["si2"] },
          },
          { cgi: null },
        ),
        "2026-10-12T01:00:00Z",
      ),
      record(
        frame({ type: "paging", identities: [{ kind: "tmsi", value: "015C5703" }] }),
        "2026-10-12T01:00:01Z",
      ),
    ]);
    expect(cells).toHaveLength(1);
    expect(cells[0]).toMatchObject({
      cgi: "262-02-318-11225",
      band: "DCS 1800",
      bsic: "6-0",
      ccch: "1 CCCH",
      neighbours: [730, 733],
      paged: 1,
    });
  });
});
