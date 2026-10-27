import { describe, expect, it } from "vitest";
import { record } from "../../test/decoderRecords";
import { cwSignalRows } from "./cwSkimmer";

describe("cwSignalRows", () => {
  it("groups CW spots by carrier and keeps the text in the order it arrived", () => {
    const rows = cwSignalRows([
      record("cw_skimmer", { offset_hz: 4_210, text: "K", wpm: 27, snr_db: 14 }),
      record("cw_skimmer", { offset_hz: -3_500, text: "DE DL1AAA ", wpm: 18, snr_db: 21 }),
      record("cw_skimmer", { offset_hz: 4_180, text: "CQ ", wpm: 26, snr_db: 15 }),
      record("cw_skimmer", { offset_hz: -3_480, text: "CQ ", wpm: 17, snr_db: 20 }),
    ]);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toEqual({
      frequencyHz: 1_090_000_000 - 3_500,
      offsetHz: -3_500,
      wpm: 18,
      snrDb: 21,
      text: "CQ DE DL1AAA ",
    });
    expect(rows[1]).toEqual({
      frequencyHz: 1_090_000_000 + 4_210,
      offsetHz: 4_210,
      wpm: 27,
      snrDb: 14,
      text: "CQ K",
    });
  });

  it("keeps carriers a group apart separate and caps a long transcript", () => {
    const rows = cwSignalRows(
      [
        record("cw_skimmer", { offset_hz: 700, text: "TU", wpm: 20, snr_db: 9 }),
        record("cw_skimmer", { offset_hz: 600, text: "VVV", wpm: 21, snr_db: 8 }),
      ],
      4,
    );
    expect(rows.map((row) => row.offsetHz)).toEqual([600, 700]);
    expect(
      cwSignalRows(
        [record("cw_skimmer", { offset_hz: 0, text: "ABCDEF", wpm: 20, snr_db: 9 })],
        4,
      )[0]?.text,
    ).toBe("CDEF");
  });

  it("has no rows without spots", () => {
    expect(cwSignalRows([])).toEqual([]);
  });
});
