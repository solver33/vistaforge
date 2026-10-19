import { describe, expect, it } from 'vitest';
import { decodeWorldMap, encodeWorldMap } from './worldmap';

/** Packs a map as app/bridge/src/world/map.rs `pack_map` does. */
function packLikeRust(header: string, rgba: number[]): ArrayBuffer {
  let head = new TextEncoder().encode(header);
  const padded = Math.ceil(head.length / 4) * 4;
  const h = new Uint8Array(padded).fill(0x20);
  h.set(head);
  head = h;
  const out = new Uint8Array(8 + head.length + rgba.length);
  out.set([0x56, 0x46, 0x4d, 0x31]);
  new DataView(out.buffer).setUint32(4, head.length, true);
  out.set(head, 8);
  out.set(rgba, 8 + head.length);
  return out.buffer;
}

describe('decodeWorldMap', () => {
  it('reads what the Rust bridge writes', () => {
    const header =
      '{"x0":-48,"z0":32,"scale":2,"width":2,"height":1,"chunks":3,"old_chunks":1,"unfinished":2,"failed":0,"seconds":0.25}';
    const m = decodeWorldMap(packLikeRust(header, [10, 20, 30, 255, 0, 0, 0, 0]));
    expect(m.x0).toBe(-48);
    expect(m.z0).toBe(32);
    expect(m.scale).toBe(2);
    expect([m.width, m.height]).toEqual([2, 1]);
    expect([m.chunks, m.old_chunks, m.unfinished, m.failed]).toEqual([3, 1, 2, 0]);
    expect([...m.rgba]).toEqual([10, 20, 30, 255, 0, 0, 0, 0]);
  });

  it('round-trips the mock backend encoding', () => {
    const rgba = new Uint8ClampedArray(4 * 3 * 2).map((_, i) => i * 7);
    const map = {
      x0: 512,
      z0: -1024,
      scale: 4,
      width: 3,
      height: 2,
      chunks: 100,
      old_chunks: 0,
      unfinished: 0,
      failed: 1,
      seconds: 1.5,
      rgba,
    };
    const back = decodeWorldMap(encodeWorldMap(map));
    expect({ ...back, rgba: [...back.rgba] }).toEqual({ ...map, rgba: [...rgba] });
  });

  it('refuses something that is not a map', () => {
    expect(() => decodeWorldMap(new Uint8Array([1, 2, 3, 4, 0, 0, 0, 0]).buffer)).toThrow(/not a Vistaforge world map/);
  });
});
