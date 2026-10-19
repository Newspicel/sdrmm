import { describe, expect, it } from "vitest";
import { RowQueue } from "./rowQueue";

function spans(queue: RowQueue): number[][] {
  const seen: number[][] = [];
  queue.drain((rows, count) => {
    expect(rows.length).toBe(count * queue.bins);
    seen.push([...rows]);
  });
  return seen;
}

describe("RowQueue", () => {
  it("drains what it holds in arrival order", () => {
    const queue = new RowQueue(2, 4);
    queue.push(new Uint8Array([1, 2]));
    queue.push(new Uint8Array([3, 4]));
    expect(queue.pending).toBe(2);
    expect(spans(queue)).toEqual([[1, 2, 3, 4]]);
    expect(queue.pending).toBe(0);
  });

  it("drains a wrapped ring as two runs", () => {
    const queue = new RowQueue(1, 3);
    queue.push(new Uint8Array([1]));
    queue.push(new Uint8Array([2]));
    expect(spans(queue)).toEqual([[1, 2]]);
    queue.push(new Uint8Array([3]));
    queue.push(new Uint8Array([4]));
    expect(spans(queue)).toEqual([[3], [4]]);
  });

  it("drops the oldest row once full", () => {
    const queue = new RowQueue(1, 2);
    for (const value of [1, 2, 3, 4]) {
      queue.push(new Uint8Array([value]));
    }
    expect(queue.pending).toBe(2);
    expect(spans(queue).flat()).toEqual([3, 4]);
  });

  it("forgets its backlog when cleared", () => {
    const queue = new RowQueue(1, 2);
    queue.push(new Uint8Array([7]));
    queue.clear();
    expect(queue.pending).toBe(0);
    expect(spans(queue)).toEqual([]);
  });
});
