import { describe, expect, it } from "vitest";
import type { RdsUpdate } from "../../lib/types";
import { record } from "../../test/decoderRecords";
import { formatAltFreqs, ptyLabel, rdsPicture, rdsQuality } from "./rds";

describe("RDS", () => {
  const base: RdsUpdate = { groups: 0, blocks: 0, block_errors: 0 };

  it("folds frames forward without a later frame erasing an earlier field", () => {
    const records = [
      record("rds", {
        ...base,
        groups: 100,
        blocks: 401,
        block_errors: 1,
        radiotext: "Now playing",
      }),
      record("rds", { ...base, groups: 50, blocks: 200, ps: "RADIO 1", pi: "D389" }),
    ];
    expect(rdsPicture(records)).toEqual({
      groups: 100,
      blocks: 401,
      block_errors: 1,
      ps: "RADIO 1",
      pi: "D389",
      radiotext: "Now playing",
    });
    expect(rdsPicture([])).toBeNull();
  });

  it("drops the frames a retune left behind instead of blending two stations", () => {
    const records = [
      record("rds", { ...base, groups: 4, blocks: 16, pi: "D392", ps: "WDR 2" }),
      record("rds", { ...base, groups: 900, blocks: 3600, pi: "D3A3", radiotext: "SWR3 news" }),
    ];
    expect(rdsPicture(records)?.radiotext).toBeUndefined();
    expect(rdsPicture(records)?.ps).toBe("WDR 2");
  });

  it("drops the frames of an earlier station that never sent a PI code", () => {
    const records = [
      record("rds", { ...base, groups: 4, blocks: 16, ps: "WDR 2" }),
      record("rds", { ...base, groups: 900, blocks: 3600, radiotext: "SWR3 news" }),
    ];
    expect(rdsPicture(records)?.radiotext).toBeUndefined();
  });

  it("grades block errors against every block the decoder read", () => {
    expect(rdsQuality(base).label).toBe("no lock");
    expect(rdsQuality({ ...base, groups: 1000, blocks: 4010, block_errors: 10 }).label).toBe(
      "good",
    );
    expect(rdsQuality({ ...base, groups: 1000, blocks: 4200, block_errors: 200 }).label).toBe(
      "fair",
    );
    expect(rdsQuality({ ...base, groups: 100, blocks: 600, block_errors: 200 }).label).toBe("poor");
    expect(rdsQuality({ ...base, groups: 1000, blocks: 4000, block_errors: 0 }).errorRate).toBe(0);
  });

  it("counts the blocks of half-received groups even before the wire reports them", () => {
    const partial = rdsQuality({ ...base, groups: 100, blocks: 0, block_errors: 200 });
    expect(partial.errorRate).toBeCloseTo(200 / 600, 6);
  });

  it("prefers the wire's PTY name and falls back to the code", () => {
    expect(ptyLabel({ ...base, pty: 10, pty_name: "Pop Music" })).toBe("Pop Music");
    expect(ptyLabel({ ...base, pty: 10 })).toBe("PTY 10");
    expect(ptyLabel(base)).toBe("-");
  });

  it("sorts alternative frequencies ascending", () => {
    expect(formatAltFreqs([100_300_000, 98_500_000])).toEqual(["98.5 MHz", "100.3 MHz"]);
    expect(formatAltFreqs(undefined)).toEqual([]);
  });
});
