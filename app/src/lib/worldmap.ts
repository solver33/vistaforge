// Reads the map of an opened world written by app/bridge/src/world/map.rs (layout there).

import type { WorldMapData } from './types';

const MAGIC = 'VFM1';

type Header = Omit<WorldMapData, 'rgba'>;

export function decodeWorldMap(data: ArrayBuffer): WorldMapData {
  const bytes = new Uint8Array(data);
  const magic = String.fromCharCode(bytes[0], bytes[1], bytes[2], bytes[3]);
  if (magic !== MAGIC) throw new Error('not a Vistaforge world map');
  const len = new DataView(data).getUint32(4, true);
  const header = JSON.parse(new TextDecoder().decode(bytes.subarray(8, 8 + len))) as Header;
  const n = 4 * header.width * header.height;
  return { ...header, rgba: new Uint8ClampedArray(data, 8 + len, n) };
}

/** Packs a map the way the bridge does; used by the mock backend and tests. */
export function encodeWorldMap(map: WorldMapData): ArrayBuffer {
  const { rgba, ...header } = map;
  let head = new TextEncoder().encode(JSON.stringify(header));
  const padded = Math.ceil(head.length / 4) * 4;
  if (padded !== head.length) {
    const h = new Uint8Array(padded).fill(0x20);
    h.set(head);
    head = h;
  }
  const out = new ArrayBuffer(8 + head.length + rgba.length);
  const bytes = new Uint8Array(out);
  bytes.set([...MAGIC].map((c) => c.charCodeAt(0)));
  new DataView(out).setUint32(4, head.length, true);
  bytes.set(head, 8);
  bytes.set(rgba, 8 + head.length);
  return out;
}
