import { expect, test, type Page } from '@playwright/test';
import type { MockHooks } from '../src/lib/backend/mock';

/** Opens the app with the mock backend set up by `hooks`, and fails on any page error. */
async function open(page: Page, hooks: MockHooks = {}) {
  const problems: string[] = [];
  page.on('pageerror', (e) => problems.push(e.message));
  page.on('console', (m) => {
    // CSP violations and other errors land here.
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

/** The square's north-west corner, from the status bar. */
async function corner(page: Page): Promise<[number, number]> {
  const text = (await page.getByTestId('preview-status').textContent()) ?? '';
  const m = /from X (-?\d+), Z (-?\d+)/.exec(text);
  if (!m) throw new Error(`no area in "${text}"`);
  return [Number(m[1]), Number(m[2])];
}

test('open a Minecraft world, choose a square on its map and render it', async ({ page }) => {
  const problems = await open(page, { calls: {} });

  await page.getByTestId('open-world').click();
  await expect(page.getByTestId('world-title')).toHaveText('Kingdom of Ashford');
  await expect(page.getByTestId('opened-world')).toContainText('Minecraft 1.20.1');
  // The generator's controls step aside while a world is open.
  await expect(page.getByTestId('export')).toHaveCount(0);
  await expect(page.getByTestId('world-name')).toHaveCount(0);
  await expect(page.getByTestId('view-map')).toHaveCount(0);

  // The map of saved chunks, and a square around the player read for the 3D view.
  await expect(page.getByTestId('view-world-map')).toBeVisible();
  await expect(page.getByTestId('world-map-busy')).toHaveCount(0);
  await expect(page.getByTestId('preview-status')).toContainText('Area 512×512 from X 160, Z 0');

  await page.getByRole('button', { name: 'Go to spawn' }).click();
  await expect(page.getByTestId('preview-status')).toContainText('Area 512×512 from X -160, Z -288');

  // A bigger square keeps its centre.
  await page.getByTestId('world-area-sizes').getByRole('button', { name: '1024' }).click();
  await expect(page.getByTestId('preview-status')).toContainText('Area 1024×1024 from X -416, Z -544');

  // A click on the map moves the square there; dragging it moves it on.
  const box = (await page.getByTestId('world-map-canvas').boundingBox())!;
  const at = { x: box.x + box.width * 0.22, y: box.y + box.height * 0.66 };
  await page.mouse.click(at.x, at.y);
  await expect(page.getByTestId('preview-status')).not.toContainText('from X -416, Z -544');
  await expect(page.getByTestId('preview-status')).toContainText('Area 1024×1024');
  const [x0, z0] = await corner(page);
  await page.mouse.move(at.x, at.y);
  await page.mouse.down();
  await page.mouse.move(at.x + 30, at.y, { steps: 4 });
  await page.mouse.move(at.x + 60, at.y, { steps: 4 });
  await page.mouse.up();
  await expect.poll(async () => (await corner(page).catch(() => [x0, z0]))[0]).toBeGreaterThan(x0 + 64);
  expect((await corner(page))[1]).toBe(z0);
  // Over the square, once it is read, the status bar gives the ground there.
  await expect
    .poll(async () => {
      await page.mouse.move(at.x + 60, at.y + 1);
      await page.mouse.move(at.x + 60, at.y);
      return page.getByTestId('cursor').textContent();
    })
    .toMatch(/X -?\d+ · Z -?\d+ · Y \d+/);

  // Render mode renders the square, from the world.
  await page.getByTestId('view-mode-render').click();
  await page.getByTestId('render-build').click();
  await expect(page.getByTestId('render-status')).toContainText(/Rendering|Finished/, { timeout: 30_000 });
  expect((await calls(page)).worldScene).toBe(1);
  expect((await calls(page)).renderScene ?? 0).toBe(0);
  await page.getByTestId('view-mode-preview').click();

  // Another dimension of the same world.
  await page.getByTestId('dimension').selectOption('the_nether');
  await expect(page.getByTestId('status-dimension')).toHaveText(/The Nether/);
  expect((await calls(page)).openWorld).toBe(2);

  // Closing brings the generated map back.
  await page.getByTestId('close-world').click();
  await expect(page.getByTestId('world-name')).toBeVisible();
  await expect(page.getByTestId('export')).toBeVisible();
  await expect(page.getByTestId('preview-status')).toContainText('Preview 256×256');
  expect((await calls(page)).closeWorld).toBe(1);
  expect(problems).toEqual([]);
});

test('a folder that is not a world says what to choose', async ({ page }) => {
  const problems = await open(page, { pick: 'C:\\Users\\you\\saves\\empty folder' });
  await page.getByTestId('open-world').click();
  await expect(page.getByTestId('toast')).toContainText('No saved chunks');
  await expect(page.getByTestId('toast')).toContainText('level.dat');
  await expect(page.getByTestId('world-title')).toHaveCount(0);
  await expect(page.getByTestId('export')).toBeVisible();
  expect(problems).toEqual([]);
});
