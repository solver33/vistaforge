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
async function open(page: Page, hooks: MockHooks = {}) {
  const problems: string[] = [];
  page.on('pageerror', (e) => problems.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error') problems.push(m.text());
  });
  await page.addInitScript((h) => {
    (window as unknown as { __vistaforgeMock: MockHooks }).__vistaforgeMock = h;
    localStorage.setItem('vistaforge.render.v1', JSON.stringify({ scale: 0.25, samples: 64, fog: 0.3 }));
  }, hooks);
  await page.goto('/');
  await expect(page.getByTestId('preview-status')).toContainText('Preview 256×256', { timeout: 20_000 });
  return problems;
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

  // Moving the sun starts the picture over; the setting is remembered.
  await page.getByTestId('render-panel').getByRole('slider', { name: /^Height/ }).fill('10');
  await expect(page.getByTestId('render-status')).toContainText(/Rendering|Finished/);
  await expect
    .poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('vistaforge.render.v1') ?? '{}').sunElevation))
    .toBe(10);

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
