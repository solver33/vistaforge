// The real core served by `vistaforge-devserver` (app/bridge/src/bin/devserver.rs), for working
// on the UI in a browser. Vite forwards /api to it; start both with `npm run dev:real`.

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

async function post(command: string, args: Record<string, unknown> = {}): Promise<Response> {
  const response = await fetch(`/api/${command}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(args),
  });
  if (!response.ok) throw new Error(await response.text());
  return response;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return (await post(command, args)).json() as Promise<T>;
}

interface ExportStatus {
  running: boolean;
  progress: Progress | null;
  outcome: ExportOutcome | null;
  error: string | null;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export class HttpBackend implements Backend {
  readonly kind = 'http' as const;

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
    return decodePreview(await (await post('preview', { settings, res })).arrayBuffer());
  }

  worldExists(savesDir: string, name: string): Promise<boolean> {
    return call('world_exists', { saves_dir: savesDir, name });
  }

  async exportWorld(
    settings: WorldSettings,
    savesDir: string,
    overwrite: boolean,
    onProgress: (p: Progress) => void,
  ): Promise<ExportOutcome> {
    await call('export_world', { settings, saves_dir: savesDir, overwrite });
    for (;;) {
      await sleep(150);
      const status = await call<ExportStatus>('export_status');
      if (status.progress) onProgress(status.progress);
      if (status.running) continue;
      if (status.error) throw new Error(status.error);
      if (status.outcome) return status.outcome;
    }
  }

  async cancelExport(): Promise<void> {
    await call('cancel_export');
  }

  readSettings(path: string): Promise<WorldSettings> {
    return call('read_settings', { path });
  }

  async writeSettings(path: string, settings: WorldSettings): Promise<void> {
    await call('write_settings', { path, settings });
  }

  loadConfig(): Promise<AppConfig> {
    return call('load_config');
  }

  async saveConfig(config: AppConfig): Promise<void> {
    await call('save_config', { config });
  }

  async reveal(path: string): Promise<void> {
    await call('reveal', { path });
  }

  // No native pickers in a browser: type the path.
  async pickCatalog(current: string | null): Promise<string | null> {
    return prompt('Path to catalog.json', current ?? '');
  }

  async pickSavesDir(current: string | null): Promise<string | null> {
    return prompt('Path to the saves folder', current ?? '');
  }

  async pickSettingsToOpen(): Promise<string | null> {
    return prompt('Settings file to open');
  }

  async pickSettingsToSave(suggestedName: string): Promise<string | null> {
    return prompt('Save settings as', `${suggestedName}.json`);
  }
}
