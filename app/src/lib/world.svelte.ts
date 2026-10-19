// An opened Minecraft world: its map, the square chosen on it, and that square in the 3D
// preview and in render mode. The world is only read, never changed. Coordinates here are the
// game's own; the preview and render mode see the square from its north-west corner.

import { app } from './app.svelte';
import { errorText } from './format';
import { render, type SceneSource } from './render/state.svelte';
import type { CatalogInfo, WorldInfo, WorldMapData } from './types';

/** Sides of the square that can be chosen, in blocks (render mode's largest area). */
export const WORLD_AREAS = [256, 512, 1024, 2048];

export const DIMENSION_NAMES: Record<string, string> = {
  overworld: 'Overworld',
  the_nether: 'The Nether',
  the_end: 'The End',
};

const KEY = 'vistaforge.world.v1';

export interface Square {
  x0: number;
  z0: number;
  size: number;
}

/** What is under the mouse on the world map. */
export interface WorldCursor {
  x: number;
  z: number;
  /** The ground there, when it is inside the area read for the preview. */
  y: number | null;
  water: number | null;
  biome: string | null;
}

function rememberedSize(): number {
  try {
    const size = Number(JSON.parse(localStorage.getItem(KEY) ?? '{}').size);
    return WORLD_AREAS.includes(size) ? size : 512;
  } catch {
    return 512;
  }
}

/** Squares start on chunk borders, so each chunk is read once. */
const snap = (v: number) => Math.round(v / 16) * 16;

class WorldView {
  info = $state.raw<WorldInfo | null>(null);
  map = $state.raw<WorldMapData | null>(null);
  opening = $state(false);
  mapBusy = $state(false);
  mapProgress = $state(0);
  mapError = $state<string | null>(null);
  /** The square shown in the 3D view and rendered. */
  area = $state<Square>({ x0: -256, z0: -256, size: rememberedSize() });
  /** The square the 3D preview shows now. */
  loaded = $state.raw<Square | null>(null);
  areaBusy = $state(false);
  areaError = $state<string | null>(null);
  cursor = $state.raw<WorldCursor | null>(null);
  /** The catalog the world was opened with: loading a catalog closes the world in the bridge. */
  openedWith = $state.raw<CatalogInfo | null>(null);

  /** Changes whenever the chosen square, the world or its dimension does. */
  key = $derived(
    this.info ? `${this.info.path}|${this.info.dimension}|${this.area.x0},${this.area.z0},${this.area.size}` : '',
  );

  private generation = 0;
  private areaTimer: ReturnType<typeof setTimeout> | undefined;
  private loading = false;
  private queued = false;

  /** Render mode's scenes while a world is open: the chosen square, from the world. */
  private readonly source: SceneSource = {
    get size() {
      return world.area.size;
    },
    get key() {
      return world.key;
    },
    build(request, onProgress) {
      const backend = app.backend;
      if (!backend) return Promise.reject(new Error('Vistaforge is still starting.'));
      const { x0, z0 } = world.area;
      return backend.worldScene({ ...request, x0: x0 + request.x0, z0: z0 + request.z0 }, onProgress);
    },
  };

  async pick(): Promise<void> {
    const path = await app.backend?.pickWorld(app.savesDir);
    if (path) await this.open(path);
  }

  async open(path: string, dimension: string | null = null): Promise<void> {
    const backend = app.backend;
    if (!backend || this.opening) return;
    if (!app.catalog) {
      app.notify('Choose the block catalog first: it tells Vistaforge how every block looks.', true);
      return;
    }
    this.opening = true;
    try {
      const info = await backend.openWorld(path, dimension);
      const same = this.info?.path === info.path && this.info.dimension === info.dimension;
      this.generation++;
      this.info = info;
      this.openedWith = app.catalog;
      this.map = null;
      this.mapError = null;
      this.loaded = null;
      this.areaError = null;
      if (!same) {
        const at = info.player ?? info.spawn;
        this.centerOn(at[0], at[2]);
      }
      app.holdPreview(true);
      render.source = this.source;
      render.scene = null;
      render.error = null;
      render.center = null;
      render.size = Math.min(this.area.size, 2048);
      // The map is where the square is chosen.
      if (app.ui.view === '3d') app.ui.view = 'split';
      void this.loadMap();
      this.scheduleArea(0);
    } catch (e) {
      app.notify(errorText(e), true);
    } finally {
      this.opening = false;
    }
  }

  close(): void {
    if (!this.info) return;
    this.generation++;
    clearTimeout(this.areaTimer);
    void app.backend?.closeWorld().catch(() => undefined);
    this.info = null;
    this.openedWith = null;
    this.map = null;
    this.loaded = null;
    this.cursor = null;
    this.areaBusy = false;
    this.mapBusy = false;
    render.source = null;
    render.scene = null;
    render.error = null;
    app.holdPreview(false);
  }

  async setDimension(dimension: string): Promise<void> {
    if (this.info && dimension !== this.info.dimension) await this.open(this.info.path, dimension);
  }

  /** Puts the square's centre at (x, z). */
  centerOn(x: number, z: number): void {
    const half = this.area.size / 2;
    this.area.x0 = snap(x - half);
    this.area.z0 = snap(z - half);
  }

  /** Moves the square's north-west corner, on chunk borders. */
  moveTo(x0: number, z0: number): void {
    this.area.x0 = snap(x0);
    this.area.z0 = snap(z0);
  }

  setSize(size: number): void {
    const { x0, z0, size: old } = this.area;
    this.area.size = size;
    this.centerOn(x0 + old / 2, z0 + old / 2);
    render.size = Math.min(size, 2048);
    render.center = null;
    try {
      localStorage.setItem(KEY, JSON.stringify({ size }));
    } catch {
      // Only a convenience.
    }
  }

  private async loadMap(): Promise<void> {
    const backend = app.backend;
    if (!backend) return;
    const generation = this.generation;
    this.mapBusy = true;
    this.mapProgress = 0;
    try {
      const map = await backend.worldMap((done, total) => {
        if (generation === this.generation) this.mapProgress = total > 0 ? done / total : 0;
      });
      if (generation === this.generation) this.map = map;
    } catch (e) {
      if (generation === this.generation) this.mapError = errorText(e);
    } finally {
      if (generation === this.generation) this.mapBusy = false;
    }
  }

  /** Reads the square for the 3D preview once it has stopped moving for a moment. */
  scheduleArea(delay = 250): void {
    clearTimeout(this.areaTimer);
    if (!this.info) return;
    this.areaTimer = setTimeout(() => void this.loadArea(), delay);
  }

  private async loadArea(): Promise<void> {
    const backend = app.backend;
    if (!backend || !this.info) return;
    if (this.loading) {
      this.queued = true;
      return;
    }
    this.loading = true;
    this.areaBusy = true;
    const generation = this.generation;
    const square = { ...this.area };
    const started = performance.now();
    try {
      const preview = await backend.worldPreview(square.x0, square.z0, square.size, app.ui.res);
      if (generation === this.generation) {
        app.preview = preview;
        app.previewMs = performance.now() - started;
        this.loaded = square;
        this.areaError = null;
      }
    } catch (e) {
      if (generation === this.generation) this.areaError = errorText(e);
    } finally {
      this.loading = false;
      if (this.queued) {
        this.queued = false;
        void this.loadArea();
      } else {
        this.areaBusy = false;
      }
    }
  }
}

export const world = new WorldView();
