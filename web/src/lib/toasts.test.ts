import { beforeEach, describe, expect, it } from "vitest";
import { clientEvents, resetEvents } from "./diagnostics";
import { toastRejection } from "./toasts";

const logged = (): string =>
  clientEvents()
    .map((event) => event.message)
    .join("\n");

describe("toastRejection", () => {
  beforeEach(resetEvents);

  it("surfaces a rejected task instead of discarding it", async () => {
    toastRejection(Promise.reject(new Error("saved sweep is gone")));
    await Promise.resolve();
    await Promise.resolve();

    expect(logged()).toContain("saved sweep is gone");
  });

  it("stays quiet for a task that settles", async () => {
    toastRejection(Promise.resolve("ok"));
    await Promise.resolve();
    await Promise.resolve();

    expect(logged()).toBe("");
  });
});
