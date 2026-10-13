// The desktop app: commands handled by app/src-tauri/src/main.rs.

import { Channel, invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { decodePreview } from '../decode';
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
import type { Backend } from './index';

/** Command errors arrive as plain strings; make them Errors. */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw e instanceof Error ? e : new Error(String(e));
  }
}

const SETTINGS_FILTER = [{ name: 'Vistaforge settings', extensions: ['json'] }];

export class TauriBackend implements Backend {
  readonly kind = 'tauri' as const;

  appInfo(): Promise<AppInfo> {
    return call('app_info');
  }

  detectInstances(): Promise<Instance[]> {
    return call('detect_instances');
  }

  loadCatalog(path: string): Promise<CatalogInfo> {
    return call('load_catalog', { path });
  }

  async preview(settings: WorldSettings, res: number): Promise<PreviewData> {
    return decodePreview(await call<ArrayBuffer>('preview', { settings, res }));
  }

  worldExists(savesDir: string, name: string): Promise<boolean> {
    return call('world_exists', { saves_dir: savesDir, name });
  }

  exportWorld(
    settings: WorldSettings,
    savesDir: string,
    overwrite: boolean,
    onProgress: (p: Progress) => void,
  ): Promise<ExportOutcome> {
    const channel = new Channel<Progress>();
    channel.onmessage = onProgress;
    return call('export_world', { settings, saves_dir: savesDir, overwrite, on_progress: channel });
  }

  cancelExport(): Promise<void> {
    return call('cancel_export');
  }

  readSettings(path: string): Promise<WorldSettings> {
    return call('read_settings', { path });
  }

  writeSettings(path: string, settings: WorldSettings): Promise<void> {
    return call('write_settings', { path, settings });
  }

  loadConfig(): Promise<AppConfig> {
    return call('load_config');
  }

  saveConfig(config: AppConfig): Promise<void> {
    return call('save_config', { config });
  }

  reveal(path: string): Promise<void> {
    return call('reveal', { path });
  }

  async pickCatalog(current: string | null): Promise<string | null> {
    return open({
      title: 'Choose the block catalog (vistaforge/catalog.json in your game folder)',
      multiple: false,
      directory: false,
      defaultPath: current ?? undefined,
      filters: [{ name: 'Vistaforge catalog', extensions: ['json'] }],
    });
  }

  async pickSavesDir(current: string | null): Promise<string | null> {
    return open({
      title: "Choose the game's saves folder",
      multiple: false,
      directory: true,
      defaultPath: current ?? undefined,
    });
  }

  async pickSettingsToOpen(): Promise<string | null> {
    return open({ title: 'Open map settings', multiple: false, directory: false, filters: SETTINGS_FILTER });
  }

  async pickSettingsToSave(suggestedName: string): Promise<string | null> {
    return save({ title: 'Save map settings', defaultPath: `${suggestedName}.json`, filters: SETTINGS_FILTER });
  }
}
