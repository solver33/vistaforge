import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { decodePreview, encodePreview } from './decode';

// Written by the Rust encoder (bridge/src/encode.rs, `sample()`), which checks it byte for byte.
const SAMPLE_HEX = readFileSync(new URL('./preview-sample.hex', import.meta.url), 'utf8').trim();

function fromHex(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(2 * i, 2 * i + 2), 16);
  return out;
}

describe('decodePreview', () => {
  it('reads what the Rust bridge writes', () => {
    const p = decodePreview(fromHex(SAMPLE_HEX).buffer as ArrayBuffer);
    expect(p.size).toBe(256);
    expect(p.res).toBe(2);
    expect(p.step).toBe(128);
    expect([...p.heights]).toEqual([64, 65.5, 70.25, 80]);
    expect(p.water[0]).toBe(63);
    expect(Number.isNaN(p.water[1])).toBe(true);
    expect(Number.isNaN(p.water[2])).toBe(true);
    expect(p.water[3]).toBe(81);
    expect([...p.colors]).toEqual([10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255]);
    expect([...p.biomes].map((b) => p.biomeIds[b])).toEqual(['lake', 'plains', 'plains', 'forest']);
    expect(p.treeCount).toBe(1);
    expect(p.totalTrees).toBe(1);
    expect([...p.trees]).toEqual([100.5, 66, 30.5, 14, 4.5, 0]);
    expect(p.species).toEqual(['birch']);
  });

  it('copes with an unaligned view', () => {
    const bytes = fromHex(SAMPLE_HEX);
    const shifted = new Uint8Array(bytes.length + 1);
    shifted.set(bytes, 1);
    const p = decodePreview(shifted.subarray(1));
    expect([...p.heights]).toEqual([64, 65.5, 70.25, 80]);
  });

  it('round-trips through encodePreview', () => {
    const p = decodePreview(fromHex(SAMPLE_HEX).buffer as ArrayBuffer);
    const again = decodePreview(encodePreview(p));
    expect(again).toEqual(p);
  });

  it('keeps the full tree count of a thinned forest', () => {
    const p = decodePreview(fromHex(SAMPLE_HEX).buffer as ArrayBuffer);
    const again = decodePreview(encodePreview({ ...p, totalTrees: 350_000 }));
    expect(again.treeCount).toBe(1);
    expect(again.totalTrees).toBe(350_000);
  });

  it('rejects other data', () => {
    expect(() => decodePreview(new Uint8Array(16))).toThrow(/preview/);
  });
});
