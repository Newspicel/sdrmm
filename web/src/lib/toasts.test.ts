import { beforeEach, describe, expect, it } from "vitest";
import { clientEvents, resetEvents } from "./diagnostics";
import { closeToast, pushToast, type ToastData, toaster, toastRejection } from "./toasts";

const logged = (): string =>
  clientEvents()
    .map((event) => event.message)
    .join("\n");

const shown = () =>
  toaster.toasts().map((toast) => ({
    id: toast.id,
    type: toast.type,
    title: toast.title,
    repeats: (toast.data as ToastData).repeats,
  }));

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

describe("pushToast", () => {
  beforeEach(() => {
    toaster.dismiss();
  });

  it("counts a repeat on the toast already up", () => {
    pushToast("radio gone");
    pushToast("radio gone");
    pushToast("saved", "info");

    expect(shown()).toEqual([
      { id: "info:saved", type: "info", title: "saved", repeats: 0 },
      { id: "error:radio gone", type: "error", title: "radio gone", repeats: 1 },
    ]);
  });

  it("starts the count over once the toast is closed", () => {
    pushToast("radio gone");
    pushToast("radio gone");
    closeToast("radio gone");
    expect(shown()).toEqual([]);

    pushToast("radio gone");
    expect(shown()).toEqual([
      { id: "error:radio gone", type: "error", title: "radio gone", repeats: 0 },
    ]);
  });

  it("keeps the server's code with the toast", () => {
    pushToast("busy", "error", { code: "conflict" });

    expect(toaster.toasts()[0]?.data).toEqual({
      repeats: 0,
      code: "conflict",
      detail: undefined,
    });
  });
});
