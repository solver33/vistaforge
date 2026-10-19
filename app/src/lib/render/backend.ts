// What render mode asks of the app around it: the scene of an area, a game folder to read
// textures from, and somewhere to save a picture. Kept apart from the main Backend so that
// render mode is one self-contained feature.

import type { WorldSettings } from '../types';
import { decodeScene, type Scene } from './scene';

export interface SceneRequest {
  /** North-west corner of the area, in blocks from the map's corner. */
  x0: number;
  z0: number;
  size: number;
  game_dir: string | null;
  catalog_path: string | null;
}

export interface RenderBackend {
  buildScene(settings: WorldSettings, request: SceneRequest, onProgress: (done: number, total: number) => void): Promise<Scene>;
  pickGameDir(current: string | null): Promise<string | null>;
  pickImagePath(suggestedName: string): Promise<string | null>;
  saveImage(path: string, png: Uint8Array): Promise<void>;
}

const errors = (e: unknown) => (e instanceof Error ? e : new Error(String(e)));

/** The file name as hex of its UTF-8 bytes, for a request header. */
export function hexPath(path: string): string {
  return Array.from(new TextEncoder().encode(path), (b) => b.toString(16).padStart(2, '0')).join('');
}

export async function renderBackend(kind: 'tauri' | 'http' | 'mock'): Promise<RenderBackend> {
  if (kind === 'tauri') {
    const { Channel, invoke } = await import('@tauri-apps/api/core');
    const { open, save } = await import('@tauri-apps/plugin-dialog');
    return {
      async buildScene(settings, request, onProgress) {
        const channel = new Channel<[number, number]>();
        channel.onmessage = ([done, total]) => onProgress(done, total);
        try {
          return decodeScene(await invoke<ArrayBuffer>('render_scene', { settings, request, on_progress: channel }));
        } catch (e) {
          throw errors(e);
        }
      },
      pickGameDir: (current) =>
        open({
          title: 'Choose the game folder of your modpack (the one with the mods folder)',
          multiple: false,
          directory: true,
          defaultPath: current ?? undefined,
        }),
      pickImagePath: (name) =>
        save({ title: 'Save the picture', defaultPath: `${name}.png`, filters: [{ name: 'PNG image', extensions: ['png'] }] }),
      async saveImage(path, png) {
        try {
          await invoke('save_image', png, { headers: { path: hexPath(path) } });
        } catch (e) {
          throw errors(e);
        }
      },
    };
  }
  if (kind === 'http') {
    return {
      async buildScene(settings, request) {
        const response = await fetch('/api/render_scene', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ settings, request }),
        });
        if (!response.ok) throw new Error(await response.text());
        return decodeScene(await response.arrayBuffer());
      },
      pickGameDir: async (current) => prompt('Game folder (the one with the mods folder)', current ?? ''),
      pickImagePath: async (name) => `${name}.png`,
      // A browser can only download it.
      async saveImage(path, png) {
        const url = URL.createObjectURL(new Blob([png as BlobPart], { type: 'image/png' }));
        const a = document.createElement('a');
        a.href = url;
        a.download = path.split(/[\\/]/).pop() ?? 'render.png';
        a.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      },
    };
  }
  const count = (name: string) => {
    const calls = window.__vistaforgeMock?.calls;
    if (calls) calls[name] = (calls[name] ?? 0) + 1;
  };
  return {
    async buildScene(settings, request, onProgress) {
      count('renderScene');
      for (let i = 1; i <= 4; i++) {
        await new Promise((r) => setTimeout(r, 30));
        onProgress(i, 4);
      }
      const { mockScene } = await import('./mockScene');
      return decodeScene(mockScene(settings, request));
    },
    async pickGameDir() {
      count('pickGameDir');
      const pick = window.__vistaforgeMock?.pick;
      return pick === undefined ? 'C:\\Games\\Conquest' : pick;
    },
    async pickImagePath(name) {
      count('pickImagePath');
      return `C:\\Users\\you\\Pictures\\${name}.png`;
    },
    async saveImage() {
      count('saveImage');
    },
  };
}
