import { describe, expect, it } from "vitest";
import { latestOnly } from "./latest";

function gate() {
  const started: number[] = [];
  const release: (() => void)[] = [];
  const offer = latestOnly<number>((item) => {
    started.push(item);
    return new Promise<void>((resolve) => release.push(resolve));
  });
  const finish = async (): Promise<void> => {
    release.shift()?.();
    await new Promise((resolve) => setTimeout(resolve, 0));
  };
  return { started, offer, finish };
}

describe("latestOnly", () => {
  it("works on one item at a time and skips to the newest that waited", async () => {
    const { started, offer, finish } = gate();
    offer(1);
    offer(2);
    offer(3);
    expect(started).toEqual([1]);
    await finish();
    expect(started).toEqual([1, 3]);
    await finish();
    expect(started).toEqual([1, 3]);
    offer(4);
    expect(started).toEqual([1, 3, 4]);
  });

  it("keeps going after an item fails", async () => {
    const seen: number[] = [];
    const offer = latestOnly<number>(async (item) => {
      seen.push(item);
      if (item === 1) {
        throw new Error("unreadable");
      }
    });
    offer(1);
    offer(2);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(seen).toEqual([1, 2]);
  });
});
