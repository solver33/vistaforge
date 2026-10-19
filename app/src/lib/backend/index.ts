// What the UI asks of the generator. In the app this is the Tauri shell around the Rust core;
// in a browser it is either the development server (the real core over HTTP) or a mock.

import type { Scene } from '../render/scene';
import type { SceneRequest } from '../render/backend';
import type {
  AppConfig,
  AppInfo,
  CatalogInfo,
  ExportOutcome,
  Instance,
  PreviewData,
  Progress,
  WorldInfo,
  WorldMapData,
  WorldSettings,
} from '../types';

export interface Backend {
  readonly kind: 'tauri' | 'http' | 'mock';
  appInfo(): Promise<AppInfo>;
  detectInstances(): Promise<Instance[]>;
  loadCatalog(path: string): Promise<CatalogInfo>;
  preview(settings: WorldSettings, res: number): Promise<PreviewData>;
  worldExists(savesDir: string, name: string): Promise<boolean>;
  exportWorld(
    settings: WorldSettings,
    savesDir: string,
    overwrite: boolean,
    onProgress: (p: Progress) => void,
  ): Promise<ExportOutcome>;
  cancelExport(): Promise<void>;
  readSettings(path: string): Promise<WorldSettings>;
  writeSettings(path: string, settings: WorldSettings): Promise<void>;
  loadConfig(): Promise<AppConfig>;
  saveConfig(config: AppConfig): Promise<void>;
  /** Shows a folder in the system file manager. */
  reveal(path: string): Promise<void>;
  pickCatalog(current: string | null): Promise<string | null>;
  pickSavesDir(current: string | null): Promise<string | null>;
  pickSettingsToOpen(): Promise<string | null>;
  pickSettingsToSave(suggestedName: string): Promise<string | null>;

  // Opened worlds: read only, in the game's own coordinates.
  pickWorld(savesDir: string | null): Promise<string | null>;
  openWorld(path: string, dimension: string | null): Promise<WorldInfo>;
  closeWorld(): Promise<void>;
  /** The picture of every saved chunk; `onProgress` gets (regions done, regions in all). */
  worldMap(onProgress: (done: number, total: number) => void): Promise<WorldMapData>;
  /** A square of the world, as the 3D preview shows a generated map. */
  worldPreview(x0: number, z0: number, size: number, res: number): Promise<PreviewData>;
  /** Render mode's scene of a square of the world. */
  worldScene(request: SceneRequest, onProgress: (done: number, total: number) => void): Promise<Scene>;
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

/** The Tauri app, the dev server with `?backend=http`, and the mock otherwise. */
export async function pickBackend(): Promise<Backend> {
  if (window.__TAURI_INTERNALS__) {
    const { TauriBackend } = await import('./tauri');
    return new TauriBackend();
  }
  const wanted = new URLSearchParams(location.search).get('backend') ?? import.meta.env.VITE_BACKEND;
  if (wanted === 'http') {
    const { HttpBackend } = await import('./http');
    return new HttpBackend();
  }
  const { MockBackend } = await import('./mock');
  return new MockBackend();
}
