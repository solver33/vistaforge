// A small made-up scene in the bridge's layout, for the mock backend and tests: rolling grass
// hills, a lake, a few trees and flowers, with simple painted textures.

import type { WorldSettings } from '../types';
import type { SceneRequest } from './backend';
import { encodeScene, FULL, CUTOUT, QUAD_FLOATS, STATE_WORDS, WATER, WATER_TOP } from './scene';

const TILE = 16;
const SEA = 62;

/** Tiles of the atlas, in order. */
const TILES = ['grass_top', 'grass_side', 'dirt', 'stone', 'sand', 'log', 'leaves', 'flower', 'water'] as const;
type TileName = (typeof TILES)[number];

const tileIndex = (t: TileName) => TILES.indexOf(t);

function hash(x: number, y: number, seed: number): number {
  let h = Math.imul(x, 0x27d4eb2d) ^ Math.imul(y, 0x165667b1) ^ Math.imul(seed, 0x9e3779b1);
  h = Math.imul(h ^ (h >>> 15), 0x85ebca77);
  h ^= h >>> 13;
  return (h >>> 0) / 4294967296;
}

function paint(name: TileName): Uint8Array {
  const px = new Uint8Array(TILE * TILE * 4);
  for (let y = 0; y < TILE; y++) {
    for (let x = 0; x < TILE; x++) {
      const n = hash(x, y, name.length * 31 + name.charCodeAt(0));
      const shade = 0.85 + 0.3 * n;
      let rgb: [number, number, number] = [128, 128, 128];
      let a = 255;
      switch (name) {
        case 'grass_top':
          rgb = [150, 150, 150];
          break;
        case 'grass_side':
          rgb = y < 3 + (n > 0.5 ? 1 : 0) ? [96, 150, 60] : [134, 96, 67];
          break;
        case 'dirt':
          rgb = [134, 96, 67];
          break;
        case 'stone':
          rgb = [125, 125, 125];
          break;
        case 'sand':
          rgb = [219, 207, 163];
          break;
        case 'log':
          rgb = x % 4 === 0 ? [80, 60, 36] : [109, 85, 50];
          break;
        case 'leaves':
          rgb = [140, 140, 140];
          a = n < 0.62 ? 255 : 0;
          break;
        case 'flower': {
          const stem = x === 8 && y > 7;
          const petal = Math.hypot(x - 8, y - 5) < 3;
          rgb = petal ? [210, 40, 40] : [60, 130, 40];
          a = stem || petal ? 255 : 0;
          break;
        }
        case 'water':
          rgb = [60, 90, 200];
          break;
      }
      const i = (y * TILE + x) * 4;
      px.set([Math.min(255, rgb[0] * shade), Math.min(255, rgb[1] * shade), Math.min(255, rgb[2] * shade), a], i);
    }
  }
  return px;
}

type V3 = [number, number, number];
interface Face {
  o: V3;
  u: V3;
  v: V3;
  tile: TileName;
  tint: number;
}

/** A cube's faces in the order the tracer expects: down, up, north, south, west, east. */
function cube(tiles: [TileName, TileName, TileName], tint: [number, number, number] = [0, 0, 0]): Face[] {
  const [bottom, top, side] = tiles;
  const [tb, tt, ts] = tint;
  return [
    { o: [0, 0, 1], u: [1, 0, 0], v: [0, 0, -1], tile: bottom, tint: tb },
    { o: [0, 1, 0], u: [1, 0, 0], v: [0, 0, 1], tile: top, tint: tt },
    { o: [1, 1, 0], u: [-1, 0, 0], v: [0, -1, 0], tile: side, tint: ts },
    { o: [0, 1, 1], u: [1, 0, 0], v: [0, -1, 0], tile: side, tint: ts },
    { o: [0, 1, 0], u: [0, 0, 1], v: [0, -1, 0], tile: side, tint: ts },
    { o: [1, 1, 1], u: [0, 0, -1], v: [0, -1, 0], tile: side, tint: ts },
  ];
}

function cross(tile: TileName): Face[] {
  return [
    { o: [0, 1, 0], u: [1, 0, 1], v: [0, -1, 0], tile, tint: 0 },
    { o: [1, 1, 1], u: [-1, 0, -1], v: [0, -1, 0], tile, tint: 0 },
    { o: [0, 1, 1], u: [1, 0, -1], v: [0, -1, 0], tile, tint: 0 },
    { o: [1, 1, 0], u: [-1, 0, 1], v: [0, -1, 0], tile, tint: 0 },
  ];
}

interface Block {
  flags: number;
  color: number;
  faces: Face[];
}

const AIR = 0;
const GRASS = 1;
const DIRT = 2;
const STONE = 3;
const SAND = 4;
const WATER_ID = 5;
const WATER_TOP_ID = 6;
const LOG = 7;
const LEAVES = 8;
const FLOWER = 9;

const BLOCKS: Block[] = [
  { flags: 0, color: 0, faces: [] },
  { flags: FULL, color: 0x6a8f3a, faces: cube(['dirt', 'grass_top', 'grass_side'], [0, 1, 0]) },
  { flags: FULL, color: 0x86603f, faces: cube(['dirt', 'dirt', 'dirt']) },
  { flags: FULL, color: 0x7d7d7d, faces: cube(['stone', 'stone', 'stone']) },
  { flags: FULL, color: 0xdbcfa3, faces: cube(['sand', 'sand', 'sand']) },
  { flags: WATER, color: 0x3f76e4, faces: [] },
  { flags: WATER | WATER_TOP, color: 0x3f76e4, faces: [] },
  { flags: FULL, color: 0x6d5532, faces: cube(['log', 'log', 'log']) },
  { flags: CUTOUT, color: 0x4a8a2a, faces: cube(['leaves', 'leaves', 'leaves'], [2, 2, 2]) },
  { flags: CUTOUT, color: 0xd22828, faces: cross('flower') },
];

/** The scene of `request`'s area of a made-up map the size of `settings`. */
export function mockScene(settings: WorldSettings, request: SceneRequest): Uint8Array {
  const started = performance.now();
  const world = settings.size;
  const side = Math.min(128, 2 ** Math.ceil(Math.log2(Math.max(16, Math.min(request.size, 2048)))));
  const x0 = Math.max(0, Math.min(request.x0, world - 1));
  const z0 = Math.max(0, Math.min(request.z0, world - 1));
  const width = Math.min(side, world - x0);
  const depth = Math.min(side, world - z0);
  const seed = settings.seed | 0;

  const ground = new Int32Array(side * side);
  for (let z = 0; z < depth; z++) {
    for (let x = 0; x < width; x++) {
      const wx = x0 + x;
      const wz = z0 + z;
      const h = 64 + 6 * Math.sin(wx / 17 + seed) + 5 * Math.cos(wz / 13) + 3 * Math.sin((wx + wz) / 7);
      ground[z * side + x] = Math.round(h);
    }
  }
  // Extra blocks above the ground: trees and flowers.
  const extra = new Map<number, Map<number, number>>();
  const put = (x: number, y: number, z: number, id: number) => {
    if (x < 0 || z < 0 || x >= width || z >= depth) return;
    const k = z * side + x;
    if (y <= ground[k]) return;
    let col = extra.get(k);
    if (!col) extra.set(k, (col = new Map()));
    if (id === LEAVES && col.get(y) === LOG) return;
    col.set(y, id);
  };
  for (let z = 2; z < depth - 2; z++) {
    for (let x = 2; x < width - 2; x++) {
      const g = ground[z * side + x];
      if (g <= SEA + 1) continue;
      const r = hash(x0 + x, z0 + z, seed);
      if (r < 0.006) {
        for (let y = g + 1; y <= g + 5; y++) put(x, y, z, LOG);
        for (let dy = 3; dy <= 6; dy++) {
          const rad = dy >= 6 ? 1 : 2;
          for (let dz = -rad; dz <= rad; dz++) {
            for (let dx = -rad; dx <= rad; dx++) {
              if (Math.abs(dx) === rad && Math.abs(dz) === rad && hash(dx, dz + dy, seed) < 0.5) continue;
              put(x + dx, g + dy, z + dz, LEAVES);
            }
          }
        }
      } else if (r < 0.04) {
        put(x, g + 1, z, FLOWER);
      }
    }
  }

  const columns = new Uint32Array(4 * side * side);
  const water = new Uint8Array(4 * side * side);
  const voxels: number[] = [];
  let minY = Infinity;
  let maxY = -Infinity;
  const at = (x: number, z: number) => (x >= 0 && z >= 0 && x < width && z < depth ? ground[z * side + x] : Infinity);
  for (let z = 0; z < depth; z++) {
    for (let x = 0; x < width; x++) {
      const k = z * side + x;
      const g = ground[k];
      const bottom = Math.min(g, at(x - 1, z), at(x + 1, z), at(x, z - 1), at(x, z + 1));
      const col = extra.get(k);
      let top = Math.max(g, SEA);
      if (col) for (const y of col.keys()) top = Math.max(top, y);
      const cells: number[] = [];
      for (let y = bottom; y <= top; y++) {
        let id = AIR;
        if (y <= g) {
          const beach = g <= SEA + 1;
          if (y === g) id = beach ? SAND : GRASS;
          else if (y >= g - 3) id = beach ? SAND : DIRT;
          else id = STONE;
        } else if (y <= SEA) {
          id = y === SEA ? WATER_TOP_ID : WATER_ID;
        } else {
          id = col?.get(y) ?? AIR;
        }
        cells.push(id);
      }
      // No air on top.
      while (cells.length > 0 && cells[cells.length - 1] === AIR) cells.pop();
      columns[4 * k] = voxels.length;
      columns[4 * k + 1] = (bottom + 64) | (cells.length << 16);
      const tint = Math.round(20 * Math.sin((x0 + x) / 40));
      columns[4 * k + 2] = ((0x79 + tint) << 16) | (0xc0 << 8) | 0x5a;
      columns[4 * k + 3] = (0x59 << 16) | ((0xae + tint) << 8) | 0x30;
      water.set([0x3f, 0x76, 0xe4, 0], 4 * k);
      voxels.push(...cells);
      minY = Math.min(minY, bottom);
      maxY = Math.max(maxY, bottom + cells.length);
    }
  }

  const states = new Uint32Array(STATE_WORDS * BLOCKS.length);
  const quads: number[] = [];
  BLOCKS.forEach((b, i) => {
    const s = i * STATE_WORDS;
    const wet = (b.flags & WATER) !== 0;
    states.set([b.flags, 1, b.color, wet ? tileIndex('water') + 1 : 0, quads.length / QUAD_FLOATS, b.faces.length], s);
    for (const f of b.faces) {
      const cut = (b.flags & CUTOUT) !== 0 ? 1 : 0;
      const kind = f.tint === 1 ? 1 : f.tint === 2 ? 2 : 0;
      quads.push(...f.o, tileIndex(f.tile), ...f.u, kind, ...f.v, 0, 0, 0, 1, 0, 0, 1, cut, 0);
    }
  });

  const atlas = new Uint8Array(TILES.length * TILE * TILE * 4);
  const aw = TILES.length * TILE;
  TILES.forEach((t, i) => {
    const px = paint(t);
    for (let y = 0; y < TILE; y++) atlas.set(px.subarray(y * TILE * 4, (y + 1) * TILE * 4), (y * aw + i * TILE) * 4);
  });

  return encodeScene({
    header: {
      x0,
      z0,
      side,
      width,
      depth,
      min_y: minY,
      max_y: maxY,
      sea_level: SEA,
      tile: TILE,
      atlas_width: aw,
      atlas_height: TILE,
      tiles: TILES.length,
      source: {
        game_dir: null,
        minecraft_jar: false,
        mods: 0,
        packs: 0,
        models: 0,
        fallback: BLOCKS.length - 1,
        textures: TILES.length,
        missing_textures: 0,
        missing: [],
      },
      seconds: (performance.now() - started) / 1000,
    },
    columns,
    water,
    voxels: Uint16Array.from(voxels),
    states,
    quads: Float32Array.from(quads),
    atlas,
  });
}
