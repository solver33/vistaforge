import { expect, test, type Page } from '@playwright/test';
import type { MockHooks } from '../src/lib/backend/mock';

interface Tracer {
  picture(): Promise<Blob>;
  quickFrames: number;
}

/** What the tracer is doing, read past its private fields. */
async function tracerState(page: Page): Promise<{ samples: number; quick: number; mix: number }> {
  return page.evaluate(() => {
    const t = (window as unknown as { __vistaforgeTracer?: Tracer & { samples: number; mix: number } }).__vistaforgeTracer;
    return { samples: t?.samples ?? -1, quick: t?.quickFrames ?? -1, mix: t?.mix ?? -1 };
  });
}

/** Opens the app with the mock backend and render mode set small and quick. */
async function open(page: Page, hooks: MockHooks = {}, settings: Record<string, unknown> = {}) {
  const problems: string[] = [];
  page.on('pageerror', (e) => problems.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error') problems.push(m.text());
  });
  await page.addInitScript(
    ([h, s]) => {
      (window as unknown as { __vistaforgeMock: MockHooks }).__vistaforgeMock = h;
      localStorage.setItem('vistaforge.render.v1', JSON.stringify({ scale: 0.25, samples: 64, fog: 0.3, ...s }));
    },
    [hooks, settings] as const,
  );
  await page.goto('/');
  await expect(page.getByTestId('preview-status')).toContainText('Preview 256×256', { timeout: 20_000 });
  return problems;
}

/** The render settings as remembered. */
async function saved(page: Page): Promise<Record<string, unknown>> {
  return page.evaluate(() => JSON.parse(localStorage.getItem('vistaforge.render.v1') ?? '{}'));
}

async function calls(page: Page): Promise<Record<string, number>> {
  return page.evaluate(() => (window as unknown as { __vistaforgeMock: MockHooks }).__vistaforgeMock.calls ?? {});
}

/** Spread of the picture's pixel values: zero for a blank picture. */
async function spread(page: Page): Promise<number> {
  return page.evaluate(async () => {
    const tracer = (window as unknown as { __vistaforgeTracer?: Tracer }).__vistaforgeTracer;
    if (!tracer) return 0;
    const bitmap = await createImageBitmap(await tracer.picture());
    const c = new OffscreenCanvas(bitmap.width, bitmap.height);
    const ctx = c.getContext('2d')!;
    ctx.drawImage(bitmap, 0, 0);
    const px = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data;
    let sum = 0;
    let sq = 0;
    const n = px.length / 4;
    for (let i = 0; i < px.length; i += 4) {
      const l = (px[i] + px[i + 1] + px[i + 2]) / 3;
      sum += l;
      sq += l * l;
    }
    return Math.sqrt(sq / n - (sum / n) ** 2);
  });
}

test('render mode builds a scene and renders it', async ({ page }) => {
  const problems = await open(page, { calls: {} });
  await page.getByTestId('view-mode-render').click();
  await expect(page.getByTestId('render-view')).toBeVisible();
  await page.getByTestId('render-build').click();
  await expect(page.getByTestId('render-status')).toContainText(/Rendering|Finished/, { timeout: 30_000 });
  // A few samples in, the picture shows the hills, not one flat colour.
  await expect.poll(() => spread(page), { timeout: 30_000 }).toBeGreaterThan(8);
  expect((await calls(page)).renderScene).toBe(1);

  // A time of day button moves the sun and starts the picture over; the setting is remembered.
  await page.locator('[data-time="Sunset"]').click();
  await expect(page.getByTestId('render-status')).toContainText(/Rendering|Finished/);
  await expect.poll(async () => (await saved(page)).sunElevation).toBe(0.8);
  expect((await saved(page)).sunAzimuth).toBe(272);
  await expect(page.locator('[data-time="Sunset"]')).toHaveClass(/on/);

  // On the dial, the middle is straight overhead and the top edge is the northern horizon.
  const dial = (await page.getByTestId('sun-dial').boundingBox())!;
  await page.mouse.click(dial.x + dial.width / 2, dial.y + dial.height / 2);
  await expect.poll(async () => (await saved(page)).sunElevation).toBeGreaterThan(80);
  await page.mouse.click(dial.x + dial.width / 2, dial.y + dial.height * 0.1);
  await expect.poll(async () => (await saved(page)).sunElevation).toBeLessThan(5);
  const north = Number((await saved(page)).sunAzimuth);
  expect(north < 5 || north > 355).toBe(true);
  await expect(page.locator('[data-time="Sunset"]')).not.toHaveClass(/on/);

  await page.getByRole('button', { name: 'Save image…' }).click();
  await expect.poll(async () => (await calls(page)).saveImage ?? 0).toBe(1);

  // Back to the preview and again: the scene is kept.
  await page.getByTestId('view-mode-preview').click();
  await expect(page.getByTestId('render-view')).toHaveCount(0);
  await page.getByTestId('view-mode-render').click();
  await expect(page.getByTestId('render-status')).toContainText(/Rendering|Finished/, { timeout: 30_000 });
  expect((await calls(page)).renderScene).toBe(1);
  expect(problems).toEqual([]);
});

test('moving the camera shows the quick view, and the render fades in once it stops', async ({ page }) => {
  const problems = await open(page, { calls: {} });
  await page.getByTestId('view-mode-render').click();
  await page.getByTestId('render-build').click();
  await expect.poll(async () => (await tracerState(page)).mix, { timeout: 30_000 }).toBe(1);

  // While the mouse drags, only quick views are drawn and the render waits.
  const box = (await page.getByTestId('render-canvas').boundingBox())!;
  const x = box.x + box.width * 0.4;
  const y = box.y + box.height * 0.5;
  await page.mouse.move(x, y);
  await page.mouse.down();
  const before = await tracerState(page);
  for (let i = 1; i <= 6; i++) {
    await page.mouse.move(x + i * 15, y + i * 3);
    await page.waitForTimeout(60);
  }
  const during = await tracerState(page);
  expect(during.quick).toBeGreaterThan(before.quick);
  expect(during.samples).toBe(0);
  expect(during.mix).toBe(0);
  await page.mouse.up();

  // Then the render starts and covers the quick view.
  await expect.poll(async () => (await tracerState(page)).samples, { timeout: 30_000 }).toBeGreaterThan(0);
  await expect.poll(async () => (await tracerState(page)).mix, { timeout: 30_000 }).toBe(1);
  expect(problems).toEqual([]);
});

test('after the graphics card drops the render, it carries on once the card is back', async ({ page }) => {
  const problems = await open(page, { calls: {} });
  await page.getByTestId('view-mode-render').click();
  await page.getByTestId('render-build').click();
  await expect.poll(async () => (await tracerState(page)).samples, { timeout: 30_000 }).toBeGreaterThan(0);

  await page.evaluate(() => {
    const t = (window as unknown as { __vistaforgeTracer: { gl: WebGL2RenderingContext } }).__vistaforgeTracer;
    const ext = t.gl.getExtension('WEBGL_lose_context')!;
    (window as unknown as { __lose: WEBGL_lose_context }).__lose = ext;
    ext.loseContext();
  });
  await expect(page.getByText('The graphics card stopped the render')).toBeVisible();
  await page.evaluate(() => (window as unknown as { __lose: WEBGL_lose_context }).__lose.restoreContext());

  // A new tracer on the same canvas picks the scene up again, with no click.
  await expect(page.getByText('The graphics card stopped the render')).toHaveCount(0);
  await expect.poll(async () => (await tracerState(page)).samples, { timeout: 30_000 }).toBeGreaterThan(0);
  await expect.poll(() => spread(page), { timeout: 30_000 }).toBeGreaterThan(8);
  expect((await calls(page)).renderScene).toBe(1);
  expect(problems).toEqual([]);
});

test('soft clouds, the night sky and the glow render', async ({ page }) => {
  const problems = await open(page, { calls: {} }, { clouds: 0.6, cloudStyle: 'soft', glow: 1, vignette: 0.5 });
  await page.getByTestId('view-mode-render').click();
  await page.getByTestId('render-build').click();
  await expect.poll(async () => (await tracerState(page)).samples, { timeout: 60_000 }).toBeGreaterThan(1);
  const noise = () =>
    page.evaluate(
      () => (window as unknown as { __vistaforgeTracer: { noiseReady: boolean } }).__vistaforgeTracer.noiseReady,
    );
  expect(await noise()).toBe(true);
  await expect.poll(() => spread(page), { timeout: 30_000 }).toBeGreaterThan(8);

  // The Night button lights the scene by the moon and shows the stars.
  await page.locator('[data-time="Night"]').click();
  await expect.poll(async () => (await saved(page)).night).toBe(true);
  await expect(page.getByTestId('sun-dial')).toHaveAttribute('aria-valuetext', /^Moon/);
  await expect.poll(async () => (await tracerState(page)).samples, { timeout: 60_000 }).toBeGreaterThan(1);
  await expect.poll(() => spread(page), { timeout: 30_000 }).toBeGreaterThan(2);

  // Minecraft's own clouds are still there.
  await page.getByRole('button', { name: 'Blocky' }).click();
  await expect.poll(async () => (await saved(page)).cloudStyle).toBe('blocky');
  await expect.poll(async () => (await tracerState(page)).samples, { timeout: 60_000 }).toBeGreaterThan(1);
  expect(problems).toEqual([]);
});

test('render mode says when the map changed', async ({ page }) => {
  const problems = await open(page, { calls: {} });
  await page.getByTestId('view-mode-render').click();
  await page.getByTestId('render-build').click();
  await expect(page.getByTestId('render-status')).toContainText(/Rendering|Finished/, { timeout: 30_000 });
  await page.locator('[data-preset="mountains"]').click();
  await expect(page.getByText('The map has changed since this render.')).toBeVisible();
  // The prompt sits clear of the settings panel.
  const button = (await page.getByTestId('render-again').boundingBox())!;
  const panel = (await page.getByTestId('render-panel').boundingBox())!;
  expect(button.x + button.width).toBeLessThanOrEqual(panel.x);
  await page.getByTestId('render-again').click();
  await expect.poll(async () => (await calls(page)).renderScene).toBe(2);
  await expect(page.getByText('The map has changed since this render.')).toHaveCount(0);
  expect(problems).toEqual([]);
});
