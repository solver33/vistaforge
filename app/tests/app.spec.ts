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
  }, hooks);
  await page.goto('/');
  await expect(page.getByTestId('preview-status')).toContainText('Preview 256×256', { timeout: 20_000 });
  return problems;
}

/** Screen position of map block (x, z) on the map canvas, for a map of `size` shown fitted. */
async function mapPoint(page: Page, x: number, z: number, size = 1024) {
  const box = (await page.getByTestId('view-map').locator('canvas').boundingBox())!;
  const scale = (Math.min(box.width, box.height) * 0.92) / size;
  return { x: box.x + box.width / 2 + (x - size / 2) * scale, y: box.y + box.height / 2 + (z - size / 2) * scale };
}

/** The value of an inspector field. */
async function field(page: Page, key: string): Promise<number> {
  return Number(await page.getByTestId('inspector').locator(`[data-field="${key}"]`).getAttribute('aria-valuenow'));
}

/** Types a number into an inspector field: a click without dragging opens it for typing. */
async function typeField(page: Page, key: string, value: number) {
  const f = page.getByTestId('inspector').locator(`[data-field="${key}"]`);
  await f.click();
  await f.locator('input').fill(String(value));
  await f.locator('input').press('Enter');
}

async function hooks(page: Page): Promise<MockHooks> {
  return page.evaluate(() => (window as unknown as { __vistaforgeMock: MockHooks }).__vistaforgeMock);
}

test('starts with the detected game and draws the map', async ({ page }) => {
  const problems = await open(page);
  await expect(page.getByTestId('catalog')).toContainText('Conquest Reforged 1.7.0 · 19,168 blocks');
  await expect(page.getByTestId('saves')).toContainText('saves');
  await expect(page.locator('[data-preset]')).toHaveCount(11);
  await expect(page.getByTestId('export')).toBeEnabled();
  // The 3D view has rendered frames.
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __vistaforgeScene?: { frames: number } }).__vistaforgeScene?.frames ?? 0))
    .toBeGreaterThan(0);
  expect(problems).toEqual([]);
});

test('presets, sliders and undo', async ({ page }) => {
  const problems = await open(page);
  await page.locator('[data-preset="mountains"]').click();
  await expect(page.locator('[data-preset="mountains"]')).toHaveClass(/on/);
  await expect(page.locator('[data-biome="mountains"] .pct')).toHaveText('43%');
  await expect(page.getByTestId('slider-relief')).toHaveValue('0.85');

  const calls = (await hooks(page)).calls?.preview ?? 0;
  await page.getByTestId('slider-trees').fill('0.9');
  await expect(page.getByText('modified')).toBeVisible();
  await expect.poll(async () => (await hooks(page)).calls?.preview ?? 0).toBeGreaterThan(calls);

  await page.getByTestId('undo').click();
  await expect(page.getByTestId('slider-trees')).toHaveValue('0.5');
  await expect(page.getByText('modified')).toHaveCount(0);
  await page.getByTestId('redo').click();
  await expect(page.getByTestId('slider-trees')).toHaveValue('0.9');
  expect(problems).toEqual([]);
});

test('filter the presets, and the sea sliders sit with the water ones', async ({ page }) => {
  const problems = await open(page);
  const filter = page.getByTestId('preset-filter');
  await filter.fill('swamp');
  await expect(page.locator('[data-preset="swamp"]')).toBeVisible();
  await expect(page.locator('[data-preset="plains"]')).toHaveCount(0);
  await filter.fill('no such landscape');
  await expect(page.locator('[data-preset]')).toHaveCount(0);
  await expect(page.getByTestId('presets')).toContainText('No preset matches');
  await filter.press('Escape');
  await expect(page.locator('[data-preset]')).toHaveCount(11);

  const water = page.locator('[data-group="Water"]');
  await expect(water.getByTestId('slider-sea')).toHaveValue('0');
  await expect(water.getByTestId('slider-islands')).toHaveValue('0.5');
  await expect(page.locator('[data-group="More"]')).toHaveCount(0);
  expect(problems).toEqual([]);
});

test('the biome mix stays at 100 percent', async ({ page }) => {
  await open(page);
  await page.locator('[data-preset="plains"]').click();
  await page.locator('[data-biome="forest"] input').fill('40');
  await expect(page.locator('[data-biome="forest"] .pct')).toHaveText('40%');
  await expect(page.locator('[data-biome="plains"] .pct')).toHaveText('60%');
  await page.locator('[data-biome="taiga"] input').fill('20');
  await expect(page.locator('[data-biome="plains"] .pct')).toHaveText('48%');
  await expect(page.locator('[data-biome="forest"] .pct')).toHaveText('32%');
  // The last biome can't be removed.
  await page.locator('[data-preset="plains"]').click();
  await page.locator('[data-biome="plains"] input').fill('0');
  await expect(page.locator('[data-biome="plains"] .pct')).toHaveText('100%');
});

test('place, edit, move and delete a hill on the map', async ({ page }) => {
  const problems = await open(page);
  await page.getByRole('button', { name: 'Map', exact: true }).click();
  await page.locator('[data-tool="hill"]').click();
  const a = await mapPoint(page, 300, 400);
  const b = await mapPoint(page, 380, 400);
  await page.mouse.move(a.x, a.y);
  await page.mouse.down();
  await page.mouse.move(b.x, b.y, { steps: 5 });
  await page.mouse.up();

  const edits = page.getByTestId('edits');
  await expect(edits.locator('li')).toHaveCount(1);
  await expect(edits).toContainText('Hill');
  const inspector = page.getByTestId('inspector');
  await expect(inspector).toBeVisible();
  const radius = await field(page, 'radius');
  expect(radius).toBeGreaterThan(70);
  expect(radius).toBeLessThan(90);

  await typeField(page, 'height', 40);
  await expect(edits).toContainText('+40 high');

  // Drag it 100 blocks east.
  const c = await mapPoint(page, 400, 400);
  await page.mouse.move(a.x, a.y);
  await page.mouse.down();
  await page.mouse.move(c.x, c.y, { steps: 5 });
  await page.mouse.up();
  const x = await field(page, 'x');
  expect(Math.abs(x - 400)).toBeLessThan(4);

  await page.getByTestId('view-map').locator('canvas').focus();
  await page.keyboard.press('Delete');
  await expect(page.getByTestId('edits')).toHaveCount(0);
  await page.getByTestId('undo').click();
  await expect(page.getByTestId('edits').locator('li')).toHaveCount(1);
  expect(problems).toEqual([]);
});

test('a tall world raises the height limits, and going back lowers what is too high', async ({ page }) => {
  const problems = await open(page);
  await expect(page.locator('[data-height="standard"]')).toHaveClass(/on/);
  await expect(page.getByTestId('sea-level')).toHaveAttribute('max', '160');
  await page.locator('[data-height="tall"]').click();
  await expect(page.locator('[data-height="tall"]')).toHaveClass(/on/);
  await expect(page.getByTestId('height-note')).toContainText('experimental settings');
  await expect(page.getByTestId('sea-level')).toHaveAttribute('max', '300');

  await page.locator('[data-tool="mountain"]').click();
  const p = await mapPoint(page, 512, 512);
  await page.mouse.click(p.x, p.y);
  await typeField(page, 'height', 1500);
  await expect(page.getByTestId('edits')).toContainText('1500 high');

  await page.getByTestId('save-settings').click();
  const saved = (await hooks(page)).files?.['C:\\Users\\you\\Documents\\Vistaforge World.json'];
  expect(saved?.world_height).toBe('tall');

  await page.locator('[data-height="standard"]').click();
  await expect(page.getByTestId('edits')).toContainText('320 high');
  await expect(page.getByTestId('height-note')).toHaveCount(0);
  await page.getByTestId('undo').click();
  await expect(page.locator('[data-height="tall"]')).toHaveClass(/on/);
  await expect(page.getByTestId('edits')).toContainText('1500 high');
  expect(problems).toEqual([]);
});

test('a preset made for a tall world switches the height; others keep it', async ({ page }) => {
  const problems = await open(page);
  const peaks = page.locator('[data-preset="epic_peaks"]');
  await expect(peaks.locator('.tall')).toHaveText('Tall');
  await expect(page.locator('[data-preset="plains"] .tall')).toHaveCount(0);
  await peaks.click();
  await expect(page.locator('[data-height="tall"]')).toHaveClass(/on/);
  await page.locator('[data-preset="plains"]').click();
  await expect(page.locator('[data-height="tall"]')).toHaveClass(/on/);
  await peaks.click();
  await page.locator('[data-height="standard"]').click();
  await expect(page.getByText('modified')).toBeVisible();
  await page.getByTestId('undo').click();
  await expect(page.getByText('modified')).toHaveCount(0);
  expect(problems).toEqual([]);
});

test('draw a river with clicks and a double-click', async ({ page }) => {
  await open(page);
  await page.getByRole('button', { name: 'Map', exact: true }).click();
  await page.locator('[data-tool="river"]').click();
  for (const [x, z] of [
    [100, 100],
    [400, 300],
  ]) {
    const p = await mapPoint(page, x, z);
    await page.mouse.click(p.x, p.y);
  }
  const end = await mapPoint(page, 800, 900);
  await page.mouse.dblclick(end.x, end.y);
  await expect(page.getByTestId('edits')).toContainText('River');
  await expect(page.getByTestId('edits')).toContainText('3 points');
  await expect(page.locator('[data-tool="select"]')).toHaveClass(/on/);
});

test('draw a road in one stroke, then widen it', async ({ page }) => {
  const problems = await open(page);
  await page.getByRole('button', { name: 'Map', exact: true }).click();
  await page.locator('[data-tool="road"]').click();
  const start = await mapPoint(page, 100, 500);
  await page.mouse.move(start.x, start.y);
  await page.mouse.down();
  // A gentle curve, drawn with many small moves.
  for (let k = 1; k <= 40; k++) {
    const p = await mapPoint(page, 100 + k * 20, 500 + 120 * Math.sin(k / 8));
    await page.mouse.move(p.x, p.y);
  }
  await page.mouse.up();
  const edits = page.getByTestId('edits');
  await expect(edits).toContainText('Road');
  await expect(edits).toContainText('5 wide');
  // The stroke keeps the points that shape it, not one for every move.
  const points = Number((await edits.textContent())!.match(/(\d+) points/)![1]);
  expect(points).toBeGreaterThan(3);
  expect(points).toBeLessThan(40);
  await expect(page.locator('[data-tool="select"]')).toHaveClass(/on/);
  await typeField(page, 'width', 60);
  // Up to 20 blocks either side of the middle.
  expect(await field(page, 'width')).toBe(40);
  await expect(edits).toContainText('40 wide');
  expect(problems).toEqual([]);
});

test('place a waterfall by dragging the way it flows, then turn it on the map', async ({ page }) => {
  const problems = await open(page);
  await page.getByRole('button', { name: 'Map', exact: true }).click();
  await page.locator('[data-tool="waterfall"]').click();
  const lip = await mapPoint(page, 512, 512);
  const east = await mapPoint(page, 560, 512);
  await page.mouse.move(lip.x, lip.y);
  await page.mouse.down();
  await page.mouse.move(east.x, east.y, { steps: 5 });
  await page.mouse.up();
  const edits = page.getByTestId('edits');
  await expect(edits).toContainText('Waterfall');
  expect(await field(page, 'angle')).toBe(0);
  expect(await field(page, 'radius')).toBe(3);
  await typeField(page, 'drop', 30);
  await expect(edits).toContainText('30 drop, r 3');

  // The round handle sits downstream; dragging it round to the south turns the flow south.
  const box = (await page.getByTestId('view-map').locator('canvas').boundingBox())!;
  const scale = (Math.min(box.width, box.height) * 0.92) / 1024;
  const pool = Math.min(3 + 2.5 + 0.12 * 30, 3 + 14);
  const reach = Math.max(2 * pool + 4, 32 / scale);
  const handle = await mapPoint(page, 512 + reach, 512);
  const south = await mapPoint(page, 512, 512 + reach);
  await page.mouse.move(handle.x, handle.y);
  await page.mouse.down();
  await page.mouse.move(south.x, south.y, { steps: 8 });
  await page.mouse.up();
  expect(await field(page, 'angle')).toBe(90);
  await page.getByTestId('undo').click();
  expect(await field(page, 'angle')).toBe(0);
  expect(problems).toEqual([]);
});

test('a click with the mountain tool places a mountain sized to the map', async ({ page }) => {
  await open(page);
  await page.locator('[data-tool="mountain"]').click();
  const p = await mapPoint(page, 512, 512);
  await page.mouse.click(p.x, p.y);
  await expect(page.getByTestId('edits')).toContainText('Mountain');
  await expect(page.getByTestId('inspector').locator('[data-field="radius"]')).toHaveAttribute('aria-valuenow', '82');
});

test('drag a field with the mouse to change a landform', async ({ page }) => {
  const problems = await open(page);
  await page.locator('[data-tool="mountain"]').click();
  const p = await mapPoint(page, 512, 512);
  await page.mouse.click(p.x, p.y);
  const edits = page.getByTestId('edits');
  await expect(edits).toContainText('74 high, r 82');

  /** Drags across a field by `dx` pixels, holding Shift if `fine`. */
  async function drag(key: string, dx: number, fine = false) {
    const box = (await page.getByTestId('inspector').locator(`[data-field="${key}"]`).boundingBox())!;
    const y = box.y + box.height / 2;
    await page.mouse.move(box.x + box.width / 2, y);
    await page.mouse.down();
    if (fine) await page.keyboard.down('Shift');
    await page.mouse.move(box.x + box.width / 2 + dx, y, { steps: 10 });
    if (fine) await page.keyboard.up('Shift');
    await page.mouse.up();
  }

  // Height steps evenly: the whole 10 to 320 range over 400 pixels.
  await drag('height', 100);
  const high = await field(page, 'height');
  expect(Math.abs(high - 151)).toBeLessThanOrEqual(2);
  await expect(edits).toContainText(`${high} high`);
  // Shift makes the steps ten times finer.
  await drag('height', 100, true);
  expect(Math.abs((await field(page, 'height')) - high - 8)).toBeLessThanOrEqual(2);
  // Radius steps in ratio: a hundred pixels left divides it by 4 (the 8 to 2048 range over 400 pixels).
  await drag('radius', -100);
  expect(Math.abs((await field(page, 'radius')) - 20.5)).toBeLessThanOrEqual(1);
  // Past the end it stops at the limit.
  await drag('height', 1000);
  expect(await field(page, 'height')).toBe(320);

  // A click opens it for typing; arrow keys nudge it; Delete there leaves the landform alone.
  await typeField(page, 'height', 250);
  await expect(edits).toContainText('250 high');
  const height = page.getByTestId('inspector').locator('[data-field="height"]');
  await height.press('ArrowRight');
  await height.press('Shift+ArrowLeft');
  expect(await field(page, 'height')).toBe(241);
  await height.press('Delete');
  await expect(edits.locator('li')).toHaveCount(1);

  // Every change is one undo step.
  await page.getByTestId('undo').click();
  await page.getByTestId('undo').click();
  await page.getByTestId('undo').click();
  expect(await field(page, 'height')).toBe(320);
  expect(problems).toEqual([]);
});

test('export with progress, then replace the world', async ({ page }) => {
  const problems = await open(page, { exportMs: 700 });
  await page.getByTestId('world-name').fill('Test Valley');
  await page.getByTestId('export').click();
  const dialog = page.getByTestId('export-dialog');
  await expect(dialog).toHaveAttribute('data-phase', 'running');
  await expect(page.getByTestId('export-progress')).toContainText('of 4,096 chunks');
  await expect(dialog).toHaveAttribute('data-phase', 'done');
  await expect(dialog).toContainText('World ready');
  await expect(dialog).toContainText('Test Valley');
  await page.getByTestId('close-export').click();
  await expect(dialog).toHaveCount(0);

  // The world exists now, so a second export asks first.
  await page.getByTestId('export').click();
  await expect(dialog).toHaveAttribute('data-phase', 'confirm');
  await expect(dialog).toContainText('Replace “Test Valley”?');
  await page.getByTestId('confirm-replace').click();
  await expect(dialog).toHaveAttribute('data-phase', 'done');
  expect((await hooks(page)).calls?.exportWorld).toBe(2);
  expect(problems).toEqual([]);
});

test('cancel an export', async ({ page }) => {
  await open(page, { exportMs: 30_000 });
  await page.getByTestId('export').click();
  await expect(page.getByTestId('export-dialog')).toHaveAttribute('data-phase', 'running');
  await page.getByTestId('cancel-export').click();
  await expect(page.getByTestId('export-dialog')).toHaveAttribute('data-phase', 'cancelled');
  await page.getByTestId('close-export').click();
  await expect(page.getByTestId('export')).toBeEnabled();
});

test('a failed export says why', async ({ page }) => {
  await open(page, { exportMs: 1000, exportError: 'disk full' });
  await page.getByTestId('export').click();
  await expect(page.getByTestId('export-error')).toHaveText('Disk full');
});

test('world names Windows refuses block export', async ({ page }) => {
  await open(page);
  await page.getByTestId('world-name').fill('My/World');
  await expect(page.getByTestId('name-problem')).toHaveText("The name can't contain /");
  await expect(page.getByTestId('export')).toBeDisabled();
  await page.getByTestId('world-name').fill('CON');
  await expect(page.getByTestId('name-problem')).toContainText("doesn't allow");
  await page.getByTestId('world-name').fill('Conquest Valley');
  await expect(page.getByTestId('export')).toBeEnabled();
});

test('save and open settings', async ({ page }) => {
  await open(page);
  await page.getByTestId('seed').fill('12345');
  await page.getByTestId('seed').press('Enter');
  await page.getByTestId('save-settings').click();
  await expect(page.getByTestId('toast')).toContainText('Saved Vistaforge World.json');
  const saved = (await hooks(page)).files?.['C:\\Users\\you\\Documents\\Vistaforge World.json'];
  expect(saved?.seed).toBe(12345);

  await page.getByTestId('seed').fill('hello world');
  await page.getByTestId('seed').press('Enter');
  await expect(page.getByTestId('seed')).not.toHaveValue('12345');
  await page.keyboard.press('Control+o');
  await expect(page.getByTestId('toast')).toContainText('Opened Vistaforge World.json');
  await expect(page.getByTestId('seed')).toHaveValue('12345');
});

test('first run without a catalog, then a wrong file, then the right one', async ({ page }) => {
  await page.addInitScript(() => localStorage.clear());
  const problems: string[] = [];
  page.on('pageerror', (e) => problems.push(e.message));
  await page.addInitScript(() => {
    (window as unknown as { __vistaforgeMock: MockHooks }).__vistaforgeMock = { instances: [] };
  });
  await page.goto('/');
  await expect(page.getByText('Choose the block catalog from your Conquest Reforged game')).toBeVisible();
  await expect(page.getByTestId('export-why')).toHaveText('Choose the block catalog.');

  await page.evaluate(() => {
    (window as unknown as { __vistaforgeMock: MockHooks }).__vistaforgeMock.pick = 'C:\\wrong\\catalog.json';
  });
  await page.getByTestId('catalog').getByRole('button', { name: 'Choose…' }).click();
  await expect(page.getByTestId('catalog-error')).toContainText('lacks blocks');

  await page.getByTestId('catalog').getByRole('button', { name: 'Choose…' }).click();
  await expect(page.getByTestId('catalog')).toContainText('19,168 blocks');
  await expect(page.getByTestId('preview-status')).toContainText('Preview 256×256');
  await expect(page.getByTestId('export-why')).toHaveText("Choose the game's saves folder.");
  await page.getByTestId('saves').getByRole('button', { name: 'Choose…' }).click();
  await expect(page.getByTestId('export')).toBeEnabled();
  expect(problems).toEqual([]);
});

test('remembers the session', async ({ page }) => {
  await open(page);
  await page.getByTestId('world-name').fill('Remembered');
  await page.locator('[data-preset="swamp"]').click();
  await page.getByRole('button', { name: '3D', exact: true }).click();
  await page.waitForTimeout(900);
  await page.reload();
  await expect(page.getByTestId('world-name')).toHaveValue('Remembered');
  await expect(page.locator('[data-preset="swamp"]')).toHaveClass(/on/);
  await expect(page.getByTestId('view-map')).toBeHidden();
});
