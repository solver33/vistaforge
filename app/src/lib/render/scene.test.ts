import { describe, expect, it } from 'vitest';
import type { WorldSettings } from '../types';
import { hexPath } from './backend';
import { mockScene } from './mockScene';
import { decodeScene, heightPyramid, medianTop, QUAD_FLOATS, STATE_WORDS } from './scene';
import { sunDirection } from './settings';
import { luminance, skyLight, sunlight } from './sky';

const settings = { size: 1000, seed: 7 } as WorldSettings;
const request = { x0: 950, z0: 10, size: 100, game_dir: null, catalog_path: null };

describe('render scene', () => {
  it('decodes what was encoded', () => {
    const bytes = mockScene(settings, request);
    const s = decodeScene(bytes);
    expect(s.header.side).toBe(128);
    // The side rounds up to a power of two, clipped to the map: 50 columns before the east edge.
    expect(s.header.width).toBe(50);
    expect(s.header.depth).toBe(128);
    expect(s.columns.length).toBe(4 * 128 * 128);
    expect(s.water.length).toBe(4 * 128 * 128);
    expect(s.states.length).toBe(STATE_WORDS * s.header.states);
    expect(s.quads.length).toBe(QUAD_FLOATS * s.header.quads);
    expect(s.atlas.length).toBe(4 * s.header.atlas_width * s.header.atlas_height);
    // Every column's cells lie inside the voxel list.
    let total = 0;
    for (let i = 0; i < 128 * 128; i++) total += s.columns[4 * i + 1] >>> 16;
    expect(total).toBe(s.header.voxels);
    // Every cell is a known state.
    expect(Math.max(...s.voxels)).toBeLessThan(s.header.states);
    // Works from bytes that don't start on a 4-byte boundary too.
    const shifted = new Uint8Array(bytes.length + 1);
    shifted.set(bytes, 1);
    expect(decodeScene(shifted.subarray(1)).header.voxels).toBe(s.header.voxels);
  });

  it('refuses something else', () => {
    expect(() => decodeScene(new TextEncoder().encode('PNG!....'))).toThrow();
  });

  it('builds the height pyramid up to one texel', () => {
    const s = decodeScene(mockScene(settings, request));
    const levels = heightPyramid(s);
    expect(levels.length).toBe(8);
    expect(levels[7].length).toBe(1);
    const top = Math.max(...levels[0]);
    expect(levels[7][0]).toBe(top);
    expect(top - 64).toBe(s.header.max_y);
    const y = medianTop(s);
    expect(y).toBeGreaterThan(50);
    expect(y).toBeLessThan(90);
  });
});

describe('sky', () => {
  it('puts the sun where the compass says', () => {
    const [x, y, z] = sunDirection(90, 0);
    expect(x).toBeCloseTo(1);
    expect(y).toBeCloseTo(0);
    expect(z).toBeCloseTo(0);
    expect(sunDirection(0, 0)[2]).toBeCloseTo(-1);
    expect(sunDirection(0, 90)[1]).toBeCloseTo(1);
  });

  it('dims and reddens the sun near the horizon', () => {
    const noon = sunlight(sunDirection(180, 60));
    const evening = sunlight(sunDirection(270, 3));
    expect(luminance(noon)).toBeGreaterThan(10);
    expect(luminance(evening)).toBeLessThan(luminance(noon) / 2);
    expect(evening[0] / evening[2]).toBeGreaterThan(noon[0] / noon[2]);
    expect(luminance(sunlight(sunDirection(0, -10)))).toBe(0);
  });

  it('lights the ground from the sky less than from the sun at noon', () => {
    const s = sunDirection(180, 50);
    const sky = skyLight(s);
    const sun = luminance(sunlight(s)) * s[1];
    expect(luminance(sky.irradiance)).toBeGreaterThan(0.5);
    expect(luminance(sky.irradiance)).toBeLessThan(sun);
    // Bluer than the sunlight.
    expect(sky.irradiance[2]).toBeGreaterThan(sky.irradiance[0]);
  });
});

describe('save path header', () => {
  it('is the hex of the UTF-8 bytes', () => {
    expect(hexPath('a/б')).toBe('612fd0b1');
  });
});
