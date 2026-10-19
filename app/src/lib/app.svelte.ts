// The app's state and actions. Components read and change `app`; App.svelte wires the effects
// (previews, undo steps, remembering the session) once the backend is ready.

import { pickBackend, type Backend } from './backend';
import { isLine, rounded } from './edits';
import { baseName, errorText } from './format';
import { clampHeights, heightLimits } from './heights';
import { History } from './history';
import { worldNameProblem } from './names';
import { randomSeed } from './seed';
import type {
  AppInfo,
  BiomeId,
  CatalogInfo,
  EditKind,
  ExportReport,
  Instance,
  Preset,
  PreviewData,
  ShapeEdit,
  WorldHeight,
  WorldSettings,
} from './types';

export type View = '3d' | 'split' | 'map';
export type Tool = 'select' | EditKind;

/** View preferences, remembered between runs. */
export interface UiPrefs {
  view: View;
  /** Preview samples per side. */
  res: number;
  /** Vertical scale of the 3D view. */
  exaggeration: number;
  trees3d: boolean;
  water3d: boolean;
  shadows: boolean;
  mapTrees: boolean;
  mapGrid: boolean;
  /** Collapsed sidebar sections, by title. */
  collapsed: Record<string, boolean>;
}

const DEFAULT_UI: UiPrefs = {
  view: 'split',
  res: 256,
  exaggeration: 1.5,
  trees3d: true,
  water3d: true,
  shadows: true,
  mapTrees: true,
  mapGrid: true,
  collapsed: {},
};

export const RESOLUTIONS = [
  { res: 128, label: 'Low' },
  { res: 256, label: 'Medium' },
  { res: 512, label: 'High' },
];

export type ExportState =
  | { phase: 'idle' }
  | { phase: 'confirm'; world: string }
  | { phase: 'running'; done: number; total: number; started: number; cancelling: boolean }
  | { phase: 'done'; report: ExportReport }
  | { phase: 'cancelled' }
  | { phase: 'error'; message: string };

/** What is under the mouse on the map. */
export interface Cursor {
  x: number;
  z: number;
  height: number;
  water: number | null;
  biome: BiomeId;
}

const BLANK: WorldSettings = {
  name: 'Vistaforge World',
  size: 1024,
  seed: 1,
  preset: '',
  biomes: [],
  sliders: {},
  sea_level: 63,
  edits: [],
  world_height: 'standard',
};

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

/** Why the settings can't be generated, or null. Mirrors `WorldSettings::validate`. */
export function settingsProblem(s: WorldSettings, info: AppInfo | null): string | null {
  const min = info?.min_size ?? 256;
  const max = info?.max_size ?? 4096;
  if (!Number.isInteger(s.size) || s.size % 16 !== 0 || s.size < min || s.size > max) {
    return `The size must be a multiple of 16 between ${min} and ${max}.`;
  }
  if (!Number.isFinite(s.sea_level) || s.sea_level < -60 || s.sea_level > 300) {
    return 'Sea level must be between -60 and 300.';
  }
  if (!s.biomes.some((b) => b.share > 0)) return 'Give at least one biome a share of the map.';
  return null;
}

class AppState {
  backend: Backend | null = null;
  info = $state.raw<AppInfo | null>(null);
  ready = $state(false);
  startError = $state<string | null>(null);

  settings = $state<WorldSettings>(clone(BLANK));
  catalogPath = $state<string | null>(null);
  catalog = $state.raw<CatalogInfo | null>(null);
  catalogBusy = $state(false);
  catalogError = $state<string | null>(null);
  savesDir = $state<string | null>(null);
  instances = $state.raw<Instance[]>([]);

  preview = $state.raw<PreviewData | null>(null);
  previewBusy = $state(false);
  previewError = $state<string | null>(null);
  previewMs = $state(0);

  ui = $state<UiPrefs>(clone(DEFAULT_UI));
  tool = $state<Tool>('select');
  zoneBiome = $state<BiomeId>('forest');
  selected = $state<number | null>(null);
  cursor = $state.raw<Cursor | null>(null);
  exportState = $state<ExportState>({ phase: 'idle' });
  toast = $state<{ text: string; error: boolean } | null>(null);
  canUndo = $state(false);
  canRedo = $state(false);

  nameProblem = $derived(worldNameProblem(this.settings.name));
  settingsProblem = $derived(settingsProblem(this.settings, this.info));
  /** Why export is not possible yet, in the order to fix them. */
  exportProblems = $derived.by(() => {
    const out: string[] = [];
    if (!this.catalog) out.push('Choose the block catalog.');
    if (!this.savesDir) out.push("Choose the game's saves folder.");
    if (this.nameProblem) out.push(this.nameProblem);
    if (this.settingsProblem) out.push(this.settingsProblem);
    return out;
  });
  /** Changes when anything that shapes the preview changes (not the world name). */
  previewKey = $derived.by(() => {
    const { name: _name, ...shape } = this.settings;
    return `${JSON.stringify(shape)}|${this.ui.res}|${this.catalog?.path ?? ''}`;
  });
  settingsJson = $derived(JSON.stringify(this.settings));
  configJson = $derived(
    JSON.stringify({ c: this.catalogPath, s: this.savesDir, w: this.settings, u: this.ui }),
  );
  currentPreset = $derived(this.info?.presets.find((p) => p.id === this.settings.preset) ?? null);
  /** Sea level and landform limits for the chosen world height. */
  limits = $derived(heightLimits(this.settings.world_height));
  /** True when the biomes, sliders or sea level differ from the preset they started from. */
  presetModified = $derived.by(() => {
    const p = this.currentPreset;
    if (!p) return false;
    const s = this.settings;
    // Compare percentages: the UI rescales shares, which doesn't change the mix.
    const shares = (list: { biome: string; share: number }[]) => {
      const total = list.reduce((t, b) => t + Math.max(0, b.share), 0) || 1;
      return JSON.stringify(
        list
          .filter((b) => b.share > 0)
          .map((b) => [b.biome, Math.round((100 * b.share) / total)])
          .sort(),
      );
    };
    return (
      shares(s.biomes) !== shares(p.biomes) ||
      s.sea_level !== p.sea_level ||
      (p.world_height !== undefined && s.world_height !== p.world_height) ||
      Object.entries(p.sliders).some(([k, v]) => Math.abs((s.sliders[k] ?? v) - v) > 1e-6)
    );
  });

  private history = new History('');
  /** Set while an opened world, not the generator, fills `preview`. */
  private previewHeld = false;
  private previewInFlight = false;
  private previewQueued = false;
  private previewTimer: ReturnType<typeof setTimeout> | undefined;
  private commitTimer: ReturnType<typeof setTimeout> | undefined;
  private holding = false;
  private configTimer: ReturnType<typeof setTimeout> | undefined;
  private toastTimer: ReturnType<typeof setTimeout> | undefined;

  // ---- Start-up -------------------------------------------------------------------------

  async start(): Promise<void> {
    try {
      const backend = await pickBackend();
      this.backend = backend;
      const info = await backend.appInfo();
      this.info = info;
      const config = await backend.loadConfig().catch(() => null);
      const ui = config?.ui as Partial<UiPrefs> | null | undefined;
      this.ui = { ...clone(DEFAULT_UI), ...(ui ?? {}) };
      this.settings = config?.settings
        ? this.normalise(config.settings)
        : this.fromPreset(info.presets[0], info.defaults);
      this.savesDir = config?.saves_dir ?? null;
      this.history.reset(this.snapshot());
      this.ready = true;
      const instances = await backend.detectInstances().catch(() => [] as Instance[]);
      this.instances = instances;
      if (config?.catalog) {
        await this.loadCatalog(config.catalog);
      } else {
        // First run: take the game folder that has a catalog, if there is exactly one best one.
        const found = instances.find((i) => i.catalog);
        if (found) await this.useInstance(found);
      }
    } catch (e) {
      this.startError = errorText(e);
    }
  }

  /** Fills gaps in settings from an older file with the core's defaults. */
  normalise(s: WorldSettings): WorldSettings {
    const d = this.info?.defaults ?? BLANK;
    const out: WorldSettings = {
      ...clone(d),
      ...clone(s),
      sliders: { ...d.sliders, ...(s.sliders ?? {}) },
      biomes: clone(s.biomes ?? d.biomes),
      edits: clone(s.edits ?? []),
    };
    return out;
  }

  fromPreset(p: Preset | undefined, base: WorldSettings): WorldSettings {
    const s = clone(base);
    if (p) {
      s.preset = p.id;
      s.biomes = clone(p.biomes);
      s.sliders = { ...base.sliders, ...p.sliders };
      s.sea_level = p.sea_level;
      if (p.world_height) s.world_height = p.world_height;
    }
    return s;
  }

  snapshot(): string {
    return JSON.stringify($state.snapshot(this.settings));
  }

  // ---- Notices --------------------------------------------------------------------------

  notify(text: string, error = false): void {
    this.toast = { text, error };
    clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => (this.toast = null), error ? 8000 : 4000);
  }

  // ---- Undo -----------------------------------------------------------------------------

  /** Records the current settings as an undo step. */
  commit(): void {
    clearTimeout(this.commitTimer);
    this.history.commit(this.snapshot());
    this.canUndo = this.history.canUndo;
    this.canRedo = this.history.canRedo;
  }

  /** Records an undo step once the settings stop changing for a moment. */
  commitSoon(): void {
    clearTimeout(this.commitTimer);
    // A pause in the middle of a drag is not a step: the drag ends with one.
    if (this.holding) return;
    this.commitTimer = setTimeout(() => this.commit(), 400);
  }

  /** A mouse button is down: what it drags becomes one undo step when it comes up. */
  holdCommits(): void {
    this.commit();
    this.holding = true;
  }

  releaseCommits(): void {
    if (!this.holding) return;
    this.holding = false;
    this.commit();
  }

  undo(): void {
    this.commit();
    const prev = this.history.undo();
    if (prev) this.restore(prev);
  }

  redo(): void {
    clearTimeout(this.commitTimer);
    const next = this.history.redo();
    if (next) this.restore(next);
  }

  private restore(state: string): void {
    this.settings = JSON.parse(state) as WorldSettings;
    if (this.selected !== null && this.selected >= this.settings.edits.length) this.selected = null;
    this.canUndo = this.history.canUndo;
    this.canRedo = this.history.canRedo;
  }

  // ---- Settings -------------------------------------------------------------------------

  biomeName(id: BiomeId): string {
    return this.info?.biomes.find((b) => b.id === id)?.name ?? id.replace(/_/g, ' ');
  }

  applyPreset(p: Preset): void {
    const s = this.settings;
    s.preset = p.id;
    s.biomes = clone(p.biomes);
    s.sliders = { ...(this.info?.defaults.sliders ?? {}), ...p.sliders };
    s.sea_level = p.sea_level;
    // A preset made for a tall world brings its height; the rest keep the one chosen.
    if (p.world_height) {
      s.world_height = p.world_height;
      clampHeights(s);
    }
    this.commit();
  }

  /** Each biome's share of the map in percent (shares are relative weights in the file). */
  percents = $derived.by(() => {
    const total = this.settings.biomes.reduce((t, b) => t + Math.max(0, b.share), 0);
    const out: Record<BiomeId, number> = {};
    for (const b of this.settings.biomes) out[b.biome] = total > 0 ? (100 * Math.max(0, b.share)) / total : 0;
    return out;
  });

  /**
   * Gives `biome` `pct` percent of the map and scales the other biomes to share the rest.
   * The last biome can't be taken out.
   */
  setPercent(biome: BiomeId, pct: number): void {
    pct = Math.max(0, Math.min(100, pct));
    const current = this.percents;
    const others = Object.entries(current).filter(([id, v]) => id !== biome && v > 0);
    const otherTotal = others.reduce((t, [, v]) => t + v, 0);
    if (otherTotal <= 0 && pct < 100) pct = 100;
    const next: Record<BiomeId, number> = { [biome]: pct };
    for (const [id, v] of others) next[id] = otherTotal > 0 ? (v * (100 - pct)) / otherTotal : 0;
    // Keep the core's biome order, so files stay tidy.
    const order = this.info?.biomes.map((b) => b.id) ?? Object.keys(next);
    this.settings.biomes = Object.entries(next)
      .filter(([, v]) => v >= 0.05)
      .sort(([a], [b]) => order.indexOf(a) - order.indexOf(b))
      .map(([id, v]) => ({ biome: id, share: Math.round(v * 10) / 10 }));
  }

  /** Splits the map evenly between the biomes in the mix. */
  evenMix(): void {
    const used = this.settings.biomes.filter((b) => b.share > 0);
    this.settings.biomes = used.map((b) => ({ biome: b.biome, share: Math.round(1000 / used.length) / 10 }));
    this.commit();
  }

  newSeed(): void {
    this.settings.seed = randomSeed();
    this.commit();
  }

  /** Changes how tall the world is; going back to standard lowers anything above its limits. */
  setWorldHeight(h: WorldHeight): void {
    if (this.settings.world_height === h) return;
    this.settings.world_height = h;
    clampHeights(this.settings);
    this.commit();
  }

  // ---- Landforms ------------------------------------------------------------------------

  addEdit(edit: ShapeEdit): void {
    this.settings.edits.push(rounded(edit));
    this.selected = this.settings.edits.length - 1;
    this.commit();
  }

  removeEdit(index: number): void {
    this.settings.edits.splice(index, 1);
    this.selected = null;
    this.commit();
  }

  clearEdits(): void {
    this.settings.edits = [];
    this.selected = null;
    this.commit();
  }

  /** Removes one point of a river or ridge, keeping at least two. */
  removePoint(index: number, point: number): void {
    const e = this.settings.edits[index];
    if (e && isLine(e) && e.points.length > 2) {
      e.points.splice(point, 1);
      this.commit();
    }
  }

  // ---- Game folders ---------------------------------------------------------------------

  async loadCatalog(path: string): Promise<void> {
    if (!this.backend) return;
    this.catalogPath = path;
    this.catalogBusy = true;
    this.catalogError = null;
    try {
      this.catalog = await this.backend.loadCatalog(path);
    } catch (e) {
      this.catalog = null;
      this.catalogError = errorText(e);
    } finally {
      this.catalogBusy = false;
    }
  }

  async pickCatalog(): Promise<void> {
    const path = await this.backend?.pickCatalog(this.catalogPath);
    if (path) await this.loadCatalog(path);
  }

  async pickSavesDir(): Promise<void> {
    const path = await this.backend?.pickSavesDir(this.savesDir);
    if (path) this.savesDir = path;
  }

  async useInstance(instance: Instance): Promise<void> {
    if (instance.saves) this.savesDir = instance.saves;
    if (instance.catalog) await this.loadCatalog(instance.catalog);
  }

  async rescan(): Promise<void> {
    if (this.backend) this.instances = await this.backend.detectInstances().catch(() => []);
  }

  // ---- Settings files -------------------------------------------------------------------

  async openSettings(): Promise<void> {
    if (!this.backend) return;
    const path = await this.backend.pickSettingsToOpen();
    if (!path) return;
    try {
      const loaded = this.normalise(await this.backend.readSettings(path));
      this.commit();
      this.settings = loaded;
      this.selected = null;
      this.commit();
      this.notify(`Opened ${baseName(path)}.`);
    } catch (e) {
      this.notify(errorText(e), true);
    }
  }

  async saveSettings(): Promise<void> {
    if (!this.backend) return;
    const path = await this.backend.pickSettingsToSave(this.settings.name || 'map');
    if (!path) return;
    try {
      await this.backend.writeSettings(path, $state.snapshot(this.settings) as WorldSettings);
      this.notify(`Saved ${baseName(path)}.`);
    } catch (e) {
      this.notify(errorText(e), true);
    }
  }

  /** Starts a fresh map from the current preset, keeping the name, size, height and folders. */
  newMap(): void {
    if (!this.info) return;
    const preset = this.currentPreset ?? this.info.presets[0];
    const keep = {
      name: this.settings.name,
      size: this.settings.size,
      world_height: preset?.world_height ?? this.settings.world_height,
    };
    this.commit();
    this.settings = { ...this.fromPreset(preset, this.info.defaults), ...keep, seed: randomSeed() };
    this.selected = null;
    this.commit();
  }

  // ---- Preview --------------------------------------------------------------------------

  schedulePreview(delay = 150): void {
    clearTimeout(this.previewTimer);
    if (this.previewHeld) return;
    this.previewTimer = setTimeout(() => void this.runPreview(), delay);
  }

  /**
   * Stops (or restarts) previews of the generated map while an opened world shows its own.
   * Restarting brings the generated map's preview back.
   */
  holdPreview(held: boolean): void {
    if (held === this.previewHeld) return;
    this.previewHeld = held;
    clearTimeout(this.previewTimer);
    this.preview = null;
    this.previewError = null;
    if (!held) this.schedulePreview(0);
  }

  async runPreview(): Promise<void> {
    if (!this.backend || !this.catalog || this.previewHeld) return;
    if (this.previewInFlight) {
      this.previewQueued = true;
      return;
    }
    const problem = this.settingsProblem;
    if (problem) {
      this.previewError = problem;
      return;
    }
    this.previewInFlight = true;
    this.previewBusy = true;
    const started = performance.now();
    try {
      const settings = $state.snapshot(this.settings) as WorldSettings;
      const preview = await this.backend.preview(settings, this.ui.res);
      // An opened world took over the view meanwhile.
      if (this.previewHeld) return;
      this.preview = preview;
      this.previewMs = performance.now() - started;
      this.previewError = null;
    } catch (e) {
      if (!this.previewHeld) this.previewError = errorText(e);
    } finally {
      this.previewInFlight = false;
      if (this.previewQueued && !this.previewHeld) {
        this.previewQueued = false;
        void this.runPreview();
      } else {
        this.previewQueued = false;
        this.previewBusy = false;
      }
    }
  }

  // ---- Remembering the session ----------------------------------------------------------

  saveConfigSoon(): void {
    clearTimeout(this.configTimer);
    this.configTimer = setTimeout(() => {
      void this.backend
        ?.saveConfig({
          catalog: this.catalogPath,
          saves_dir: this.savesDir,
          settings: $state.snapshot(this.settings) as WorldSettings,
          ui: $state.snapshot(this.ui) as unknown as Record<string, unknown>,
        })
        .catch(() => undefined);
    }, 600);
  }

  // ---- Export ---------------------------------------------------------------------------

  async startExport(): Promise<void> {
    if (!this.backend || this.exportProblems.length > 0 || !this.savesDir) return;
    if (await this.backend.worldExists(this.savesDir, this.settings.name)) {
      this.exportState = { phase: 'confirm', world: this.settings.name };
      return;
    }
    await this.runExport(false);
  }

  async runExport(overwrite: boolean): Promise<void> {
    if (!this.backend || !this.savesDir) return;
    const total = (this.settings.size / 16) ** 2;
    this.exportState = { phase: 'running', done: 0, total, started: performance.now(), cancelling: false };
    try {
      const outcome = await this.backend.exportWorld(
        $state.snapshot(this.settings) as WorldSettings,
        this.savesDir,
        overwrite,
        (p) => {
          const st = this.exportState;
          if (st.phase === 'running') {
            st.done = p.chunks_done;
            st.total = p.chunks_total;
          }
        },
      );
      this.exportState = outcome.status === 'done' ? { phase: 'done', report: outcome.report } : { phase: 'cancelled' };
    } catch (e) {
      this.exportState = { phase: 'error', message: errorText(e) };
    }
  }

  async cancelExport(): Promise<void> {
    const st = this.exportState;
    if (st.phase === 'running') {
      st.cancelling = true;
      await this.backend?.cancelExport();
    }
  }

  closeExport(): void {
    if (this.exportState.phase !== 'running') this.exportState = { phase: 'idle' };
  }

  async reveal(path: string): Promise<void> {
    try {
      await this.backend?.reveal(path);
    } catch (e) {
      this.notify(errorText(e), true);
    }
  }
}

export const app = new AppState();

/** Tools offered on the map, in toolbar order. */
export function isEditTool(tool: Tool): tool is EditKind {
  return tool !== 'select';
}
