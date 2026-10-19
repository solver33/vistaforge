// Render mode's own state: whether it is showing, the area and settings, and the scene.

import { app } from '../app.svelte';
import type { WorldSettings } from '../types';
import { renderBackend, type RenderBackend, type SceneRequest } from './backend';
import type { Scene } from './scene';
import { loadSettings, saveSettings, type RenderSettings } from './settings';
import type { TracerStatus } from './tracer';

export type ViewMode = 'preview' | 'render';

/** Where scenes come from instead of the generator: an opened world supplies its own. */
export interface SceneSource {
  /** Side of the square that render areas are picked in, in blocks. */
  readonly size: number;
  /** Changes when what the source shows changes, so an older render reads as out of date. */
  readonly key: string;
  /** The scene of `request`, an area of the square from its north-west corner. */
  build(request: SceneRequest, onProgress: (done: number, total: number) => void): Promise<Scene>;
}

export const AREA_SIZES = [256, 512, 1024, 2048];

const KEY = 'vistaforge.render.view.v1';

interface Remembered {
  mode: ViewMode;
  size: number;
  gameDir: string | null;
}

function remembered(): Remembered {
  const fallback: Remembered = { mode: 'preview', size: 512, gameDir: null };
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? { ...fallback, ...(JSON.parse(raw) as Partial<Remembered>) } : fallback;
  } catch {
    return fallback;
  }
}

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

class RenderState {
  private readonly saved = remembered();
  mode = $state<ViewMode>(this.saved.mode);
  settings = $state<RenderSettings>(loadSettings());
  /** Side of the rendered square, in blocks. */
  size = $state(this.saved.size);
  /** Centre of the area, in blocks from the map's corner; null is the map's centre. */
  center = $state<{ x: number; z: number } | null>(null);
  gameDir = $state<string | null>(this.saved.gameDir);

  scene = $state.raw<Scene | null>(null);
  building = $state(false);
  progress = $state(0);
  error = $state<string | null>(null);
  status = $state.raw<TracerStatus | null>(null);
  /** What the map looked like when the scene was built. */
  builtKey = $state('');

  /** Set while an opened world, not the generator, provides the scenes. */
  source = $state.raw<SceneSource | null>(null);
  /** Side of the square render areas are picked in: the map, or an opened world's area. */
  mapSize = $derived(this.source?.size ?? app.settings.size);

  private backend: RenderBackend | null = null;

  /** The area to render, clamped to the map. */
  area = $derived.by(() => {
    const world = this.mapSize;
    const size = Math.min(this.size, AREA_SIZES.find((s) => s >= world) ?? 2048);
    const c = this.center ?? { x: world / 2, z: world / 2 };
    const clamp = (v: number) => Math.max(0, Math.min(Math.round(v - size / 2), Math.max(0, world - size)));
    return { x0: clamp(c.x), z0: clamp(c.z), size };
  });

  /** The map changed since the scene was built. */
  stale = $derived(this.scene !== null && this.builtKey !== this.mapKey());

  mapKey(): string {
    if (this.source) return this.source.key;
    const { name: _name, ...shape } = app.settings;
    return JSON.stringify(shape);
  }

  remember(): void {
    try {
      localStorage.setItem(KEY, JSON.stringify({ mode: this.mode, size: this.size, gameDir: this.gameDir }));
    } catch {
      // Only a convenience.
    }
    saveSettings($state.snapshot(this.settings) as RenderSettings);
  }

  private async getBackend(): Promise<RenderBackend | null> {
    if (!this.backend && app.backend) this.backend = await renderBackend(app.backend.kind);
    return this.backend;
  }

  async build(): Promise<void> {
    const backend = await this.getBackend();
    if (!backend || this.building) return;
    const source = this.source;
    if (app.settingsProblem && !source) {
      this.error = app.settingsProblem;
      return;
    }
    this.building = true;
    this.progress = 0;
    this.error = null;
    const key = this.mapKey();
    try {
      const settings = $state.snapshot(app.settings) as WorldSettings;
      const { x0, z0, size } = this.area;
      const request = { x0, z0, size, game_dir: this.gameDir, catalog_path: app.catalogPath };
      const onProgress = (done: number, total: number) => (this.progress = total > 0 ? done / total : 0);
      this.scene = source
        ? await source.build(request, onProgress)
        : await backend.buildScene(settings, request, onProgress);
      this.builtKey = key;
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.building = false;
    }
  }

  async pickGameDir(): Promise<void> {
    const backend = await this.getBackend();
    const dir = await backend?.pickGameDir(this.gameDir ?? this.scene?.header.source.game_dir ?? null);
    if (!dir) return;
    this.gameDir = dir;
    this.remember();
    if (this.scene) await this.build();
  }

  async saveImage(png: () => Promise<Blob>): Promise<void> {
    const backend = await this.getBackend();
    if (!backend) return;
    try {
      const path = await backend.pickImagePath(app.settings.name || 'render');
      if (!path) return;
      const bytes = new Uint8Array(await (await png()).arrayBuffer());
      await backend.saveImage(path, bytes);
      app.notify(`Saved ${path.split(/[\\/]/).pop()}.`);
    } catch (e) {
      app.notify(errorText(e), true);
    }
  }
}

export const render = new RenderState();
