// Reads the packed preview written by app/bridge/src/encode.rs (layout documented there).

import type { PreviewData } from './types';

const MAGIC = 'VFP1';

interface Header {
  size: number;
  res: number;
  step: number;
  trees: number;
  biomes: string[];
  species: string[];
}

export function decodePreview(data: ArrayBuffer | Uint8Array): PreviewData {
  let buf: ArrayBuffer;
  let base = 0;
  if (data instanceof Uint8Array) {
    // Float32Array views need 4-byte alignment; copy a view that starts elsewhere.
    if (data.byteOffset % 4 === 0) {
      buf = data.buffer as ArrayBuffer;
      base = data.byteOffset;
    } else {
      buf = data.slice().buffer;
    }
  } else {
    buf = data;
  }
  const bytes = new Uint8Array(buf, base);
  const magic = String.fromCharCode(bytes[0], bytes[1], bytes[2], bytes[3]);
  if (magic !== MAGIC) throw new Error('not a Vistaforge preview');
  const view = new DataView(buf, base);
  const headerLen = view.getUint32(4, true);
  const header = JSON.parse(new TextDecoder().decode(bytes.subarray(8, 8 + headerLen))) as Header;
  const n = header.res * header.res;
  let at = base + 8 + headerLen;
  const take = <T>(make: (offset: number) => T, len: number): T => {
    const out = make(at);
    at += len;
    return out;
  };
  const heights = take((o) => new Float32Array(buf, o, n), 4 * n);
  const water = take((o) => new Float32Array(buf, o, n), 4 * n);
  const colors = take((o) => new Uint8Array(buf, o, 4 * n), 4 * n);
  const biomes = take((o) => new Uint8Array(buf, o, n), Math.ceil(n / 4) * 4);
  const trees = new Float32Array(buf, at, 6 * header.trees);
  return {
    size: header.size,
    res: header.res,
    step: header.step,
    heights,
    water,
    colors,
    biomes,
    biomeIds: header.biomes,
    trees,
    treeCount: header.trees,
    species: header.species,
  };
}

/** Packs a preview the way the bridge does; used by the mock backend and tests. */
export function encodePreview(p: PreviewData): ArrayBuffer {
  const header = JSON.stringify({
    size: p.size,
    res: p.res,
    step: p.step,
    trees: p.treeCount,
    biomes: p.biomeIds,
    species: p.species,
  });
  let headerBytes = new TextEncoder().encode(header);
  const padded = Math.ceil(headerBytes.length / 4) * 4;
  if (padded !== headerBytes.length) {
    const h = new Uint8Array(padded).fill(0x20);
    h.set(headerBytes);
    headerBytes = h;
  }
  const n = p.res * p.res;
  const biomeLen = Math.ceil(n / 4) * 4;
  const total = 8 + headerBytes.length + 12 * n + biomeLen + 24 * p.treeCount;
  const out = new ArrayBuffer(total);
  const bytes = new Uint8Array(out);
  bytes.set([...MAGIC].map((c) => c.charCodeAt(0)));
  new DataView(out).setUint32(4, headerBytes.length, true);
  bytes.set(headerBytes, 8);
  let at = 8 + headerBytes.length;
  new Float32Array(out, at, n).set(p.heights);
  at += 4 * n;
  new Float32Array(out, at, n).set(p.water);
  at += 4 * n;
  bytes.set(p.colors, at);
  at += 4 * n;
  bytes.set(p.biomes, at);
  at += biomeLen;
  new Float32Array(out, at, 6 * p.treeCount).set(p.trees.subarray(0, 6 * p.treeCount));
  return out;
}
