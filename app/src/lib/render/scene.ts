// The scene render mode traces: one square area of the map as columns of blocks, the look of
// each block (quads with textures) and a texture atlas. Built by the bridge
// (app/bridge/src/render/mod.rs), whose doc comment gives the binary layout this reads.

export const MAGIC = 'VFR1';

/** State flags, as the bridge writes them. */
export const FULL = 1;
export const WATER = 2;
export const WATERLOGGED = 4;
export const WATER_TOP = 8;
export const CUTOUT = 16;

/** u32s per state, f32s per quad. */
export const STATE_WORDS = 12;
export const QUAD_FLOATS = 20;

export interface SceneSource {
  /** The game folder the models and textures came from, if any. */
  game_dir: string | null;
  minecraft_jar: boolean;
  mods: number;
  packs: number;
  /** Blocks drawn from the game's own models, and from the catalog's colours instead. */
  models: number;
  fallback: number;
  textures: number;
  missing_textures: number;
  missing: string[];
}

export interface SceneHeader {
  x0: number;
  z0: number;
  /** Columns per side of the column table (a power of two). */
  side: number;
  /** The part of it on the map. */
  width: number;
  depth: number;
  min_y: number;
  max_y: number;
  sea_level: number;
  voxels: number;
  states: number;
  quads: number;
  tile: number;
  atlas_width: number;
  atlas_height: number;
  tiles: number;
  source: SceneSource;
  seconds: number;
}

export interface Scene {
  header: SceneHeader;
  /** Per column (z, then x): voxel offset, (bottom + 64) | count << 16, grass, foliage. */
  columns: Uint32Array;
  /** Per column: water colour R, G, B, 0. */
  water: Uint8Array;
  /** Render state per cell, bottom up per column; 0 is air. */
  voxels: Uint16Array;
  states: Uint32Array;
  quads: Float32Array;
  /** RGBA, `atlas_width` × `atlas_height`. */
  atlas: Uint8Array;
}

/** Reads a scene, sharing the bytes where they are aligned. */
export function decodeScene(input: ArrayBuffer | Uint8Array): Scene {
  let bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  if (bytes.byteOffset % 4 !== 0) bytes = bytes.slice();
  const { buffer, byteOffset: base, byteLength } = bytes;
  const magic = String.fromCharCode(...bytes.subarray(0, 4));
  if (magic !== MAGIC) throw new Error('This is not a render scene from this version of Vistaforge.');
  const view = new DataView(buffer, base, byteLength);
  const headLen = view.getUint32(4, true);
  const header = JSON.parse(new TextDecoder().decode(bytes.subarray(8, 8 + headLen))) as SceneHeader;
  const n = header.side * header.side;
  let at = 8 + headLen;
  const take = (len: number) => {
    if (at + len > byteLength) throw new Error('The render scene is cut short.');
    const start = at;
    at += len;
    return base + start;
  };
  const columns = new Uint32Array(buffer, take(16 * n), 4 * n);
  const water = new Uint8Array(buffer, take(4 * n), 4 * n);
  const voxels = new Uint16Array(buffer, take(2 * header.voxels), header.voxels);
  at += (4 - (at % 4)) % 4;
  const states = new Uint32Array(buffer, take(4 * STATE_WORDS * header.states), STATE_WORDS * header.states);
  const quads = new Float32Array(buffer, take(4 * QUAD_FLOATS * header.quads), QUAD_FLOATS * header.quads);
  const atlas = new Uint8Array(buffer, take(4 * header.atlas_width * header.atlas_height), 4 * header.atlas_width * header.atlas_height);
  return { header, columns, water, voxels, states, quads, atlas };
}

/** The highest cell of each column plus one, +64 (0 where empty), and its 2×2 maxima up to
 * one texel: the tracer skips the air above everything in a square at once. */
export function heightPyramid(scene: Scene): Uint16Array[] {
  const side = scene.header.side;
  const level0 = new Uint16Array(side * side);
  const c = scene.columns;
  for (let i = 0; i < side * side; i++) {
    const packed = c[4 * i + 1];
    const count = packed >>> 16;
    level0[i] = count > 0 ? (packed & 0xffff) + count : 0;
  }
  const levels = [level0];
  for (let s = side >> 1; s >= 1; s >>= 1) {
    const prev = levels[levels.length - 1];
    const ps = s * 2;
    const next = new Uint16Array(s * s);
    for (let z = 0; z < s; z++) {
      for (let x = 0; x < s; x++) {
        const i = 2 * z * ps + 2 * x;
        next[z * s + x] = Math.max(prev[i], prev[i + 1], prev[i + ps], prev[i + ps + 1]);
      }
    }
    levels.push(next);
  }
  return levels;
}

/** Where the ground of the area lies: the median top, for aiming the camera. */
export function medianTop(scene: Scene): number {
  const { side, width, depth } = scene.header;
  const tops: number[] = [];
  const step = Math.max(1, Math.floor(Math.max(width, depth) / 64));
  for (let z = 0; z < depth; z += step) {
    for (let x = 0; x < width; x += step) {
      const packed = scene.columns[4 * (z * side + x) + 1];
      const count = packed >>> 16;
      if (count > 0) tops.push((packed & 0xffff) - 64 + count);
    }
  }
  if (tops.length === 0) return scene.header.sea_level;
  tops.sort((a, b) => a - b);
  return tops[tops.length >> 1];
}

/** A scene in the same layout, for tests and the mock backend. */
export function encodeScene(scene: Omit<Scene, 'header'> & { header: Omit<SceneHeader, 'voxels' | 'states' | 'quads'> }): Uint8Array {
  const header: SceneHeader = {
    ...scene.header,
    voxels: scene.voxels.length,
    states: scene.states.length / STATE_WORDS,
    quads: scene.quads.length / QUAD_FLOATS,
  };
  let head = new TextEncoder().encode(JSON.stringify(header));
  const padded = Math.ceil(head.length / 4) * 4;
  if (padded !== head.length) {
    const h = new Uint8Array(padded).fill(0x20);
    h.set(head);
    head = h;
  }
  const voxelBytes = Math.ceil((scene.voxels.length * 2) / 4) * 4;
  const total =
    8 + head.length + scene.columns.byteLength + scene.water.byteLength + voxelBytes + scene.states.byteLength + scene.quads.byteLength + scene.atlas.byteLength;
  const out = new Uint8Array(total);
  out.set([...MAGIC].map((ch) => ch.charCodeAt(0)));
  new DataView(out.buffer).setUint32(4, head.length, true);
  let at = 8;
  const put = (b: ArrayBufferView, len = b.byteLength) => {
    out.set(new Uint8Array(b.buffer, b.byteOffset, b.byteLength), at);
    at += len;
  };
  out.set(head, at);
  at += head.length;
  put(scene.columns);
  put(scene.water);
  put(scene.voxels, voxelBytes);
  put(scene.states);
  put(scene.quads);
  put(scene.atlas);
  return out;
}
