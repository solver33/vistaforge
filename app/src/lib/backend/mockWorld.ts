// A made-up Minecraft world for the mock backend: rolling hills, a sea, snowy tops and an
// explored patch with ragged edges, in the game's own coordinates (negative ones included).

import { encodePreview, decodePreview } from '../decode';
import type { PreviewData, WorldInfo, WorldMapData } from '../types';

export const MOCK_WORLD_BOUNDS: [number, number, number, number] = [-1024, -768, 1280, 1024];
const SEA = 62;

function noise(x: number, z: number): number {
  return Math.sin(x * 12.9898 + z * 78.233) * 43758.5453 - Math.floor(Math.sin(x * 12.9898 + z * 78.233) * 43758.5453);
}

/** Saved chunks: a blob around the middle with a ragged edge, inside the bounds. */
export function explored(x: number, z: number): boolean {
  const [x0, z0, x1, z1] = MOCK_WORLD_BOUNDS;
  if (x < x0 || z < z0 || x >= x1 || z >= z1) return false;
  const cx = Math.floor(x / 16);
  const cz = Math.floor(z / 16);
  const dx = (cx * 16 - 128) / 1150;
  const dz = (cz * 16 - 128) / 900;
  return Math.hypot(dx, dz) < 0.92 + 0.12 * noise(cx, cz);
}

export function height(x: number, z: number): number {
  return Math.round(
    64 + 18 * Math.sin(x / 140) * Math.cos(z / 170) + 9 * Math.sin((x - z) / 53) + 4 * Math.cos(x / 23 + z / 31),
  );
}

function color(x: number, z: number, h: number): [number, number, number] {
  if (h < SEA) {
    const t = Math.min(0.85, 0.5 + (SEA - h) * 0.06);
    return [Math.round(150 * (1 - t) + 40 * t), Math.round(140 * (1 - t) + 90 * t), Math.round(100 * (1 - t) + 180 * t)];
  }
  if (h < SEA + 2) return [214, 200, 150];
  if (h > 86) return [235, 238, 242];
  if (h > 79) return [128, 124, 116];
  const forest = Math.sin(x / 60) + Math.cos(z / 45) > 0.8;
  return forest ? [52, 92, 40] : [96, 140, 62];
}

export function mockWorldInfo(path: string): WorldInfo {
  let chunks = 0;
  const [x0, z0, x1, z1] = MOCK_WORLD_BOUNDS;
  for (let z = z0; z < z1; z += 16) for (let x = x0; x < x1; x += 16) if (explored(x, z)) chunks++;
  const name = path.split(/[\\/]/).filter(Boolean).pop() ?? 'World';
  return {
    path,
    name,
    version: '1.20.1',
    data_version: 3465,
    dimension: 'overworld',
    dimensions: ['overworld', 'the_nether'],
    spawn: [96, 70, -40],
    player: [412, 80, 260],
    regions: 30,
    chunks,
    bounds: MOCK_WORLD_BOUNDS,
    seconds: 0.05,
  };
}

export function mockWorldMap(): WorldMapData {
  const [bx0, bz0, bx1, bz1] = MOCK_WORLD_BOUNDS;
  const scale = 2;
  const width = (bx1 - bx0) / scale;
  const heightPx = (bz1 - bz0) / scale;
  const rgba = new Uint8ClampedArray(width * heightPx * 4);
  const hs = new Float32Array(width * heightPx).fill(NaN);
  let chunks = 0;
  for (let j = 0; j < heightPx; j++) {
    for (let i = 0; i < width; i++) {
      const x = bx0 + i * scale + 1;
      const z = bz0 + j * scale + 1;
      if (!explored(x, z)) continue;
      const h = height(x, z);
      const k = j * width + i;
      hs[k] = Math.max(h, SEA);
      const c = color(x, z, h);
      rgba.set([c[0], c[1], c[2], 255], 4 * k);
    }
  }
  for (let k = 0; k < hs.length; k++) {
    const up = k >= width ? hs[k - width] : NaN;
    if (Number.isNaN(hs[k]) || Number.isNaN(up)) continue;
    const f = 1 + Math.max(-0.3, Math.min(0.28, (hs[k] - up) * 0.09));
    for (let c = 0; c < 3; c++) rgba[4 * k + c] = rgba[4 * k + c] * f;
  }
  for (let z = bz0; z < bz1; z += 16) for (let x = bx0; x < bx1; x += 16) if (explored(x, z)) chunks++;
  return {
    x0: bx0,
    z0: bz0,
    scale,
    width,
    height: heightPx,
    chunks,
    old_chunks: 0,
    unfinished: 12,
    failed: 0,
    seconds: 0.3,
    rgba,
  };
}

export function mockWorldPreview(x0: number, z0: number, size: number, res: number): PreviewData {
  res = Math.max(16, Math.min(res, size));
  const step = size / res;
  const n = res * res;
  const heights = new Float32Array(n);
  const water = new Float32Array(n).fill(NaN);
  const colors = new Uint8Array(4 * n);
  const biomes = new Uint8Array(n);
  let floor = Infinity;
  for (let j = 0; j < res; j++) {
    for (let i = 0; i < res; i++) {
      const x = x0 + Math.floor((i + 0.5) * step);
      const z = z0 + Math.floor((j + 0.5) * step);
      const k = j * res + i;
      if (!explored(x, z)) {
        heights[k] = NaN;
        colors.set([34, 39, 44, 255], 4 * k);
        continue;
      }
      const h = height(x, z);
      heights[k] = h + 1;
      floor = Math.min(floor, h + 1);
      if (h < SEA) water[k] = SEA + 1;
      const c = color(x, z, h);
      colors.set([c[0], c[1], c[2], 255], 4 * k);
      biomes[k] = h < SEA ? 2 : 1;
    }
  }
  for (let k = 0; k < n; k++) if (Number.isNaN(heights[k])) heights[k] = Number.isFinite(floor) ? floor : 0;
  const p: PreviewData = {
    size,
    res,
    step,
    heights,
    water,
    colors,
    biomes,
    biomeIds: ['unexplored', 'plains', 'ocean'],
    trees: new Float32Array(0),
    treeCount: 0,
    totalTrees: 0,
    species: [],
  };
  return decodePreview(encodePreview(p));
}
