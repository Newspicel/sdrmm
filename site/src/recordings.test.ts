import { describe, expect, it } from "vitest";
import {
  donationFolder,
  isDonationKey,
  LIMITS,
  MAX_FILE_BYTES,
  MAX_FILES,
  MAX_PARTS,
  PART_BYTES,
  parseFinish,
  parsePartNumber,
  parseStart,
  partCount,
  safeName,
} from "./recordings";

const file = { name: "pocsag.sigmf-data", size: 1000 };
const valid = { signal: "POCSAG 466.075 MHz", notes: "", email: "", files: [file] };
const key = "recordings/2026-10-04/123e4567-e89b-12d3-a456-426614174000/pocsag.sigmf-data";

describe("parseStart", () => {
  it("accepts a donation without email or notes", () => {
    expect(parseStart(valid)).toEqual({ kind: "donation", donation: valid });
  });

  it("trims fields and keeps line breaks in notes", () => {
    const parsed = parseStart({ ...valid, signal: " ADS-B ", notes: "one\ntwo", email: "a@b.cd " });
    expect(parsed).toEqual({
      kind: "donation",
      donation: { ...valid, signal: "ADS-B", notes: "one\ntwo", email: "a@b.cd" },
    });
  });

  it("rejects a missing signal, a bad email and oversized notes", () => {
    expect(parseStart({ ...valid, signal: "" }).kind).toBe("invalid");
    expect(parseStart({ ...valid, signal: "a\nb" }).kind).toBe("invalid");
    expect(parseStart({ ...valid, email: "ada" }).kind).toBe("invalid");
    expect(parseStart({ ...valid, notes: "a".repeat(LIMITS.notes + 1) }).kind).toBe("invalid");
  });

  it("rejects empty, oversized, too many and duplicate files", () => {
    expect(parseStart({ ...valid, files: [] }).kind).toBe("invalid");
    expect(parseStart({ ...valid, files: [{ ...file, size: 0 }] }).kind).toBe("invalid");
    expect(parseStart({ ...valid, files: [{ ...file, size: MAX_FILE_BYTES + 1 }] }).kind).toBe(
      "invalid",
    );
    const many = Array.from({ length: MAX_FILES + 1 }, (_, i) => ({ ...file, name: `${i}.wav` }));
    expect(parseStart({ ...valid, files: many }).kind).toBe("invalid");
    expect(parseStart({ ...valid, files: [file, file] }).kind).toBe("invalid");
  });

  it("flags a filled trap field", () => {
    expect(parseStart({ ...valid, website: "spam" }).kind).toBe("trapped");
  });

  it("rejects bodies that are not objects", () => {
    expect(parseStart(null).kind).toBe("invalid");
    expect(parseStart("x").kind).toBe("invalid");
  });
});

describe("safeName", () => {
  it("keeps recording names and strips paths and odd characters", () => {
    expect(safeName("pocsag.sigmf-data")).toBe("pocsag.sigmf-data");
    expect(safeName("../../etc/passwd")).toBe("etc_passwd");
    expect(safeName("my rec (1).wav")).toBe("my_rec_1_.wav");
    expect(safeName("...")).toBe("recording");
  });
});

describe("keys", () => {
  it("builds dated folders the parser accepts", () => {
    const folder = donationFolder(
      new Date("2026-10-04T12:00:00Z"),
      "123e4567-e89b-12d3-a456-426614174000",
    );
    expect(`${folder}pocsag.sigmf-data`).toBe(key);
    expect(isDonationKey(key)).toBe(true);
  });

  it("rejects keys outside the recordings folder", () => {
    expect(isDonationKey("other/2026-10-04/x/y")).toBe(false);
    expect(isDonationKey(`${key}/../../x`)).toBe(false);
  });
});

describe("parts", () => {
  it("splits files into fixed size parts", () => {
    expect(partCount(1)).toBe(1);
    expect(partCount(PART_BYTES)).toBe(1);
    expect(partCount(PART_BYTES + 1)).toBe(2);
    expect(partCount(MAX_FILE_BYTES)).toBe(MAX_PARTS);
  });

  it("accepts part numbers in range only", () => {
    expect(parsePartNumber("1")).toBe(1);
    expect(parsePartNumber(String(MAX_PARTS))).toBe(MAX_PARTS);
    expect(parsePartNumber("0")).toBeNull();
    expect(parsePartNumber(String(MAX_PARTS + 1))).toBeNull();
    expect(parsePartNumber("1.5")).toBeNull();
    expect(parsePartNumber(null)).toBeNull();
  });

  it("parses a finish request", () => {
    const parts = [{ partNumber: 1, etag: "a" }];
    expect(parseFinish({ key, uploadId: "u", parts })).toEqual({ key, uploadId: "u", parts });
    expect(parseFinish({ key, uploadId: "u", parts: [] })).toBeNull();
    expect(parseFinish({ key: "x", uploadId: "u", parts })).toBeNull();
    expect(parseFinish({ key, uploadId: "u", parts: [{ partNumber: 1 }] })).toBeNull();
  });
});
