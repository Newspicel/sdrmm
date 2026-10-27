import { test as base, expect, type Locator, type Page } from "@playwright/test";
import { serverURL } from "../playwright.screenshots.config";
import { listen, SCENES, type Scene } from "./scenes";

const SHOTS = "../assets/screenshots";
const REPAINT_MS = 2_000;
const MIN_SPREAD = 0.05;

const test = base.extend({
  baseURL: async ({ baseURL }, use, testInfo) => {
    await use(serverURL(baseURL ?? "", testInfo.parallelIndex));
  },
});

async function spread(page: Page, shot: string, shown: Locator): Promise<number> {
  const box = await shown.boundingBox();
  if (box === null) {
    throw new Error("a box for what the scene shows");
  }
  return page.evaluate(
    async ({ encoded, frame }) => {
      const bytes = Uint8Array.from(atob(encoded), (letter) => letter.charCodeAt(0));
      const picture = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const ratio = window.devicePixelRatio;
      const crop = new OffscreenCanvas(
        Math.max(1, Math.round(frame.width * ratio)),
        Math.max(1, Math.round(frame.height * ratio)),
      );
      const context = crop.getContext("2d");
      if (context === null) {
        return 0;
      }
      context.drawImage(
        picture,
        Math.round(frame.x * ratio),
        Math.round(frame.y * ratio),
        crop.width,
        crop.height,
        0,
        0,
        crop.width,
        crop.height,
      );
      const { data } = context.getImageData(0, 0, crop.width, crop.height);
      const tally = new Map<number, number>();
      for (let at = 0; at < data.length; at += 4) {
        const colour = ((data[at] ?? 0) << 16) | ((data[at + 1] ?? 0) << 8) | (data[at + 2] ?? 0);
        tally.set(colour, (tally.get(colour) ?? 0) + 1);
      }
      let flattest = 0;
      for (const count of tally.values()) {
        flattest = count > flattest ? count : flattest;
      }
      const pixels = data.length / 4;
      return (pixels - flattest) / pixels;
    },
    { encoded: shot, frame: box },
  );
}

async function capture(page: Page, scene: Scene, path: string): Promise<void> {
  const toasts = page.getByLabel("Dismiss", { exact: true });
  const shown = scene.shows(page);
  for (const element of shown) {
    await expect(element).toBeVisible();
  }
  let shot = "";
  await expect(async () => {
    expect(await toasts.count()).toBe(0);
    shot = (await page.screenshot({ path })).toString("base64");
    expect(await toasts.count()).toBe(0);
  }).toPass({ timeout: 90_000 });
  for (const element of shown) {
    expect(
      await spread(page, shot, element),
      `${path} is flat where ${scene.id} shows`,
    ).toBeGreaterThanOrEqual(MIN_SPREAD);
  }
}

for (const scene of SCENES) {
  test(scene.title, async ({ page }) => {
    await page.emulateMedia({ colorScheme: "dark" });
    await page.goto("/");
    await scene.stage(page);
    await scene.ready(page);
    if (scene.speaker !== undefined) {
      await listen(page, scene.speaker);
    }
    await page.waitForTimeout(scene.settleSeconds * 1000);
    await scene.settle?.(page);
    await capture(page, scene, `${SHOTS}/${scene.id}-dark.png`);
    await page.emulateMedia({ colorScheme: "light" });
    await page.waitForTimeout(REPAINT_MS);
    await scene.settle?.(page);
    await capture(page, scene, `${SHOTS}/${scene.id}-light.png`);
  });
}
