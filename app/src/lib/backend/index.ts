// What the UI asks of the generator. In the app this is the Tauri shell around the Rust core;
// in a browser it is either the development server (the real core over HTTP) or a mock.

import type {
  AppConfig,
  AppInfo,
  CatalogInfo,
  ExportOutcome,
  Instance,
  PreviewData,
  Progress,
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
