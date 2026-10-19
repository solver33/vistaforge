// A pretend backend for working on the UI in a browser and for its tests. Tests can steer it
// through `window.__vistaforgeMock` (see MockHooks) before the page loads.

import { decodePreview, encodePreview } from '../decode';
import type {
  AppConfig,
  AppInfo,
  BiomeInfo,
  CatalogInfo,
  ExportOutcome,
  Instance,
  Preset,
  PreviewData,
  Progress,
  Sliders,
  WorldInfo,
  WorldMapData,
  WorldSettings,
} from '../types';
import type { SceneRequest } from '../render/backend';
import { decodeScene, type Scene } from '../render/scene';
import { decodeWorldMap, encodeWorldMap } from '../worldmap';
import type { Backend } from './index';
import { mockPreview } from './mockTerrain';
import { mockWorldInfo, mockWorldMap, mockWorldPreview } from './mockWorld';

export interface MockHooks {
  /** How long a pretend export takes. */
  exportMs?: number;
  /** Extra delay before a preview answers. */
  previewMs?: number;
  /** Game folders "found" on this computer. */
  instances?: Instance[];
  /** What the next file or folder picker returns (undefined: a sensible default). */
  pick?: string | null;
  /** Counts of backend calls, for tests to read. */
  calls?: Record<string, number>;
  /** Settings files "on disk". */
  files?: Record<string, WorldSettings>;
  /** World folders that already exist, as `<saves>/<name>`. */
  worlds?: string[];
  /** Make the next export fail with this message. */
  exportError?: string;
  /** Extra delay before an opened world's map or area answers. */
  worldMs?: number;
}

declare global {
  interface Window {
    __vistaforgeMock?: MockHooks;
  }
}

const GAME = 'C:\\Users\\you\\curseforge\\minecraft\\Instances\\CR-Modpack-fabric-1.20.1-1.7.0';

const BIOMES: BiomeInfo[] = [
  ['plains', 'Plains'],
  ['meadow', 'Meadow'],
  ['forest', 'Forest'],
  ['birch_forest', 'Birch forest'],
  ['taiga', 'Taiga'],
  ['pine_forest', 'Pine forest'],
  ['swamp', 'Swamp'],
  ['marsh', 'Marsh'],
  ['highlands', 'Highlands'],
  ['mountains', 'Mountains'],
  ['alpine_meadow', 'Alpine meadow'],
  ['snowy_peaks', 'Snowy peaks'],
  ['beach', 'Beach'],
  ['river', 'River'],
  ['lake', 'Lake'],
].map(([id, name]) => ({ id, name, feature: ['beach', 'river', 'lake'].includes(id) }));

const NORMAL: Sliders = {
  relief: 0.5,
  roughness: 0.5,
  mountains: 0.5,
  erosion: 0.5,
  water: 0.5,
  rivers: 0.5,
  trees: 0.5,
  vegetation: 0.5,
  detail: 0.5,
  biome_blend: 0.5,
  biome_scale: 0.5,
  sea: 0,
  islands: 0.5,
  plateaus: 0,
  volcanoes: 0,
};

function preset(
  id: string,
  name: string,
  description: string,
  biomes: [string, number][],
  sliders: Sliders = {},
  sea_level = 63,
): Preset {
  return {
    id,
    name,
    description,
    biomes: biomes.map(([biome, share]) => ({ biome, share })),
    sliders: { ...NORMAL, ...sliders },
    sea_level,
  };
}

const PRESETS: Preset[] = [
  preset('plains', 'Plains', 'Gentle grassland with scattered oaks and ponds.', [['plains', 1]], { relief: 0.3, mountains: 0.1 }),
  preset('meadow', 'Flower meadows', 'Rolling meadows full of flowers.', [['meadow', 3], ['plains', 1]], { relief: 0.4, vegetation: 0.8 }),
  preset('forest', 'Old forest', 'Dense broadleaf forest with clearings.', [['forest', 4], ['birch_forest', 1], ['meadow', 1]], { trees: 0.75 }),
  preset('taiga', 'Taiga', 'Spruce forest on rolling hills.', [['taiga', 3], ['pine_forest', 1]], { relief: 0.6 }),
  preset('swamp', 'Swamp', 'Wet woodland with pools and willows.', [['swamp', 3], ['marsh', 2]], { relief: 0.15, water: 0.85 }, 63),
  preset('highlands', 'Highlands', 'Heath and rock outcrops on windswept uplands.', [['highlands', 3], ['pine_forest', 1]], { relief: 0.7, trees: 0.25 }),
  preset('mountains', 'Realistic mountains', 'Eroded ranges with snow peaks and alpine meadows.', [['mountains', 3], ['alpine_meadow', 1], ['snowy_peaks', 1], ['taiga', 2]], { relief: 0.85, mountains: 0.85, erosion: 0.75 }),
  preset('river_valley', 'River valley', 'A broad valley with a winding river between forested hills.', [['plains', 2], ['forest', 2], ['meadow', 1]], { rivers: 0.85 }),
  preset('lake_district', 'Lake district', 'Hills and many lakes.', [['forest', 2], ['highlands', 1], ['meadow', 1]], { water: 0.9, relief: 0.6 }),
  preset('mixed', 'Mixed lands', 'A bit of everything.', [['plains', 2], ['forest', 2], ['taiga', 1], ['swamp', 1], ['mountains', 1], ['meadow', 1]]),
  {
    ...preset('epic_peaks', 'Epic peaks', 'Tall world: rock and snow far above forested valleys.', [['mountains', 2], ['snowy_peaks', 2], ['taiga', 1]], { relief: 0.65, mountains: 1 }),
    world_height: 'tall',
  },
];

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function hooks(): MockHooks {
  window.__vistaforgeMock ??= {};
  return window.__vistaforgeMock;
}

function count(name: string) {
  const h = hooks();
  h.calls ??= {};
  h.calls[name] = (h.calls[name] ?? 0) + 1;
}

/** The next picked path: the test's choice if it set one, else `fallback`. */
function pick(fallback: string | null): string | null {
  const h = hooks();
  if (h.pick === undefined) return fallback;
  const chosen = h.pick;
  delete h.pick;
  return chosen;
}

const CONFIG_KEY = 'vistaforge-mock-config';

export class MockBackend implements Backend {
  readonly kind = 'mock' as const;
  private cancelled = false;

  async appInfo(): Promise<AppInfo> {
    count('appInfo');
    return {
      version: '0.1.0-mock',
      presets: structuredClone(PRESETS),
      biomes: BIOMES,
      min_size: 256,
      max_size: 4096,
      defaults: {
        name: 'Vistaforge World',
        size: 1024,
        seed: 1,
        preset: 'plains',
        biomes: [{ biome: 'plains', share: 1 }],
        sliders: { ...NORMAL },
        sea_level: 63,
        edits: [],
        world_height: 'standard',
      },
    };
  }

  async detectInstances(): Promise<Instance[]> {
    count('detectInstances');
    return (
      hooks().instances ?? [
        {
          launcher: 'CurseForge',
          name: 'CR-Modpack-fabric-1.20.1-1.7.0',
          game_dir: GAME,
          saves: `${GAME}\\saves`,
          catalog: `${GAME}\\vistaforge\\catalog.json`,
        },
      ]
    );
  }

  async loadCatalog(path: string): Promise<CatalogInfo> {
    count('loadCatalog');
    await sleep(150);
    if (/broken|wrong/i.test(path)) {
      throw new Error(
        'this catalog lacks blocks Vistaforge needs; export it from a Conquest Reforged 1.20.1 game with the Vistaforge exporter mod: missing block conquest:oak_log_pillar',
      );
    }
    return {
      path,
      blocks: 19168,
      mods: { conquest: '1.7.0', 'fabric-api': '0.92.2+1.20.1', minecraft: '1.20.1' },
      seconds: 0.4,
    };
  }

  async preview(settings: WorldSettings, res: number): Promise<PreviewData> {
    count('preview');
    const delay = hooks().previewMs ?? 0;
    if (delay > 0) await sleep(delay);
    // Through the real byte layout, so the decoder is exercised too.
    return decodePreview(encodePreview(mockPreview(settings, res, BIOMES.map((b) => b.id))));
  }

  async worldExists(savesDir: string, name: string): Promise<boolean> {
    return (hooks().worlds ?? []).includes(`${savesDir}\\${name}`);
  }

  async exportWorld(
    settings: WorldSettings,
    savesDir: string,
    overwrite: boolean,
    onProgress: (p: Progress) => void,
  ): Promise<ExportOutcome> {
    count('exportWorld');
    const h = hooks();
    const world = `${savesDir}\\${settings.name}`;
    if ((h.worlds ?? []).includes(world) && !overwrite) {
      throw new Error(`a world named "${settings.name}" already exists in ${savesDir}`);
    }
    this.cancelled = false;
    const total = (settings.size / 16) ** 2;
    const ms = h.exportMs ?? 2500;
    const started = performance.now();
    for (;;) {
      await sleep(40);
      if (this.cancelled) return { status: 'cancelled' };
      const t = Math.min(1, (performance.now() - started) / ms);
      onProgress({ chunks_done: Math.round(total * t), chunks_total: total });
      if (h.exportError && t > 0.3) {
        const message = h.exportError;
        delete h.exportError;
        throw new Error(message);
      }
      if (t >= 1) break;
    }
    h.worlds = [...(h.worlds ?? []).filter((w) => w !== world), world];
    return {
      status: 'done',
      report: {
        world_dir: world,
        chunks: total,
        regions: Math.ceil(settings.size / 512) ** 2,
        block_states: 812,
        trees: Math.round(settings.size * settings.size * 0.0004),
        seconds: ms / 1000,
        spawn: [settings.size / 2, 72, settings.size / 2],
      },
    };
  }

  async cancelExport(): Promise<void> {
    this.cancelled = true;
  }

  async readSettings(path: string): Promise<WorldSettings> {
    const file = hooks().files?.[path];
    if (!file) throw new Error(`reading ${path}: file not found`);
    return structuredClone(file);
  }

  async writeSettings(path: string, settings: WorldSettings): Promise<void> {
    const h = hooks();
    h.files = { ...(h.files ?? {}), [path]: structuredClone(settings) };
  }

  async loadConfig(): Promise<AppConfig> {
    try {
      const text = localStorage.getItem(CONFIG_KEY);
      if (text) return JSON.parse(text) as AppConfig;
    } catch {
      // No storage: start fresh.
    }
    return { catalog: null, saves_dir: null, settings: null, ui: null };
  }

  async saveConfig(config: AppConfig): Promise<void> {
    try {
      localStorage.setItem(CONFIG_KEY, JSON.stringify(config));
    } catch {
      // No storage: nothing to remember.
    }
  }

  async reveal(path: string): Promise<void> {
    count('reveal');
    console.info('reveal', path);
  }

  async pickCatalog(): Promise<string | null> {
    return pick(`${GAME}\\vistaforge\\catalog.json`);
  }

  async pickSavesDir(): Promise<string | null> {
    return pick(`${GAME}\\saves`);
  }

  async pickSettingsToOpen(): Promise<string | null> {
    return pick(Object.keys(hooks().files ?? {})[0] ?? null);
  }

  async pickSettingsToSave(suggestedName: string): Promise<string | null> {
    return pick(`C:\\Users\\you\\Documents\\${suggestedName}.json`);
  }

  async pickWorld(savesDir: string | null): Promise<string | null> {
    count('pickWorld');
    return pick(`${savesDir ?? `${GAME}\\saves`}\\Kingdom of Ashford`);
  }

  async openWorld(path: string, dimension: string | null): Promise<WorldInfo> {
    count('openWorld');
    await sleep(80);
    if (/empty|nothing/i.test(path)) {
      throw new Error(
        `no saved chunks in ${path}. Choose a world's own folder: the one inside the saves folder that holds level.dat and a region folder.`,
      );
    }
    const info = mockWorldInfo(path);
    if (dimension && info.dimensions.includes(dimension)) info.dimension = dimension;
    return info;
  }

  async closeWorld(): Promise<void> {
    count('closeWorld');
  }

  async worldMap(onProgress: (done: number, total: number) => void): Promise<WorldMapData> {
    count('worldMap');
    const delay = hooks().worldMs ?? 0;
    for (let i = 1; i <= 5; i++) {
      await sleep(20 + delay / 5);
      onProgress(i, 5);
    }
    return decodeWorldMap(encodeWorldMap(mockWorldMap()));
  }

  async worldPreview(x0: number, z0: number, size: number, res: number): Promise<PreviewData> {
    count('worldPreview');
    const delay = hooks().worldMs ?? 0;
    if (delay > 0) await sleep(delay);
    return mockWorldPreview(x0, z0, size, res);
  }

  async worldScene(request: SceneRequest, onProgress: (done: number, total: number) => void): Promise<Scene> {
    count('worldScene');
    for (let i = 1; i <= 4; i++) {
      await sleep(30);
      onProgress(i, 4);
    }
    const { mockScene } = await import('../render/mockScene');
    // The made-up scene starts at the map's corner; move the world's square onto it.
    const settings = { size: 65536, seed: 3 } as WorldSettings;
    return decodeScene(mockScene(settings, { ...request, x0: request.x0 + 32768, z0: request.z0 + 32768 }));
  }
}
