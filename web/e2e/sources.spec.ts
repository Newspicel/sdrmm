import { expect, type Page, test } from "@playwright/test";
import { addNode } from "./canvas";

async function pickGenerator(page: Page) {
  await addNode(page, "Device");
  const device = page.locator('.react-flow__node[data-id^="device:"]').last();
  await device.locator("header").click();
  await device.getByRole("radio", { name: /Virtual/ }).click();
  await device.getByRole("button", { name: "Signal generator" }).click();
  return device;
}

async function pickRecording(page: Page) {
  await addNode(page, "Device");
  const device = page.locator('.react-flow__node[data-id^="device:"]').last();
  await device.locator("header").click();
  await device.getByRole("radio", { name: "Recording" }).click();
  return device;
}

test.describe("the source nodes", () => {
  test.describe.configure({ mode: "serial" });

  test.beforeEach(async ({ page }) => {
    await page.route("https://tiles.openfreemap.org/**", (route) => route.abort());
    await page.goto("/");
  });

  test("a signal generator starts as soon as it is picked", async ({ page }) => {
    const generator = await pickGenerator(page);

    await expect(generator).toBeVisible();
    await expect(generator.getByRole("button", { name: "Start" })).toHaveCount(0);
    await expect(generator.locator('[id^="frequency-dial"]')).toBeVisible();
    await expect(generator.getByRole("button", { name: "Stop" })).toBeVisible();
  });

  test("the signal list is searchable rather than one long scroll", async ({ page }) => {
    const generator = await pickGenerator(page);

    await expect(generator.locator("header")).not.toContainText("CTCSS");

    await generator.getByRole("button", { name: "Signal", exact: true }).click();
    await page.getByRole("combobox", { name: "Signal", exact: true }).click();
    const search = page.getByRole("combobox", { name: "Search signal" });
    await expect(search).toBeVisible();
    await search.fill("adsb");
    const options = page.getByRole("option");
    await expect(options).toHaveCount(1);
    await expect(options.first()).toContainText("ADS-B");
    await page.screenshot({ path: "/tmp/shot-signals.png" });
    await options.first().click();

    await expect(generator.getByRole("button", { name: "Signal", exact: true })).toContainText(
      "ADS-B",
    );
    await expect(generator.getByRole("button", { name: "Sample rate" })).toContainText("2 MS/s");
  });

  test("a device node picks from the library and plays what it is given", async ({ page }) => {
    const recording = await pickRecording(page);

    await expect(recording.getByRole("searchbox", { name: "Search recordings" })).toBeVisible();
    await expect(recording.getByRole("button", { name: "Upload SigMF" })).toBeVisible();

    const first = recording
      .locator("button")
      .filter({ hasText: /MHz/ })
      .filter({ hasNotText: "array_" })
      .first();
    await expect(first).toBeVisible();
    await first.click();

    await expect(recording.getByRole("slider", { name: "Playback position" })).toBeVisible();
    await expect(recording.getByRole("button", { name: "Forget recording" })).toBeVisible();
    await expect(recording).toContainText("Centre");
    await expect(recording).toContainText("Length");
  });

  test("one device node offers every source", async ({ page }) => {
    await addNode(page, "Device");

    const device = page.locator('.react-flow__node[data-id^="device:"]').last();
    await device.locator("header").click();
    const source = device.getByRole("group", { name: "Radio source" });
    await expect(source).toBeVisible();
    await expect(source.getByRole("radio", { name: "Recording" })).toBeVisible();
  });
});
