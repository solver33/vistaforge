// The light of the sky on the CPU: how much sunlight gets through the air, and how bright the
// sky is overall. Same model and numbers as SKY_FS in shaders.ts.

export type Vec3 = [number, number, number];

const R_PLANET = 6371e3;
const R_ATMOS = 6471e3;
const K_RLH: Vec3 = [5.5e-6, 13.0e-6, 22.4e-6];
const K_MIE = 21e-6;
const SH_RLH = 8e3;
const SH_MIE = 1.2e3;
const G = 0.758;
/** Sunlight above the air, in the units the picture is exposed for. */
export const SUN_POWER = 22;
/** Single scattering leaves the sky too dark beside the sun (real skies give about a fifth of
 * the light on the ground at noon): the sky is lifted and the direct sun lowered to match. */
export const SKY_GAIN = 1.6;
export const SUN_DIRECT = 0.35;
/** The real sun's angular radius. */
export const SUN_RADIUS = (0.2665 * Math.PI) / 180;

const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

function sphere(r0: Vec3, rd: Vec3, sr: number): [number, number] {
  const a = dot(rd, rd);
  const b = 2 * dot(rd, r0);
  const c = dot(r0, r0) - sr * sr;
  const d = b * b - 4 * a * c;
  if (d < 0) return [1e5, -1e5];
  return [(-b - Math.sqrt(d)) / (2 * a), (-b + Math.sqrt(d)) / (2 * a)];
}

const ORIGIN: Vec3 = [0, R_PLANET + 100, 0];

/** Sky radiance in direction r (above the horizon) with the sun towards s. */
export function skyRadiance(r: Vec3, s: Vec3, power = SUN_POWER): Vec3 {
  const p = sphere(ORIGIN, r, R_ATMOS);
  if (p[0] > p[1]) return [0, 0, 0];
  const ground = sphere(ORIGIN, r, R_PLANET)[0];
  const end = ground > 0 ? Math.min(p[1], ground) : p[1];
  const steps = 16;
  const iStep = (end - Math.max(p[0], 0)) / steps;
  let iTime = Math.max(p[0], 0);
  const rlh: Vec3 = [0, 0, 0];
  const mie: Vec3 = [0, 0, 0];
  let iOdRlh = 0;
  let iOdMie = 0;
  const mu = dot(r, s);
  const gg = G * G;
  const pRlh = (3 / (16 * Math.PI)) * (1 + mu * mu);
  const pMie = ((3 / (8 * Math.PI)) * ((1 - gg) * (mu * mu + 1))) / (Math.pow(1 + gg - 2 * mu * G, 1.5) * (2 + gg));
  for (let i = 0; i < steps; i++) {
    const pos: Vec3 = [r[0] * (iTime + iStep / 2), ORIGIN[1] + r[1] * (iTime + iStep / 2), r[2] * (iTime + iStep / 2)];
    const h = Math.hypot(pos[0], pos[1], pos[2]) - R_PLANET;
    const odRlh = Math.exp(-h / SH_RLH) * iStep;
    const odMie = Math.exp(-h / SH_MIE) * iStep;
    iOdRlh += odRlh;
    iOdMie += odMie;
    const [jOdRlh, jOdMie] = opticalDepth(pos, s, 8);
    for (let c = 0; c < 3; c++) {
      const attn = Math.exp(-(K_MIE * (iOdMie + jOdMie) + K_RLH[c] * (iOdRlh + jOdRlh)));
      rlh[c] += odRlh * attn;
      mie[c] += odMie * attn;
    }
    iTime += iStep;
  }
  return [0, 1, 2].map((c) => power * (pRlh * K_RLH[c] * rlh[c] + pMie * K_MIE * mie[c])) as Vec3;
}

function opticalDepth(from: Vec3, dir: Vec3, steps: number): [number, number] {
  const jStep = sphere(from, dir, R_ATMOS)[1] / steps;
  let t = 0;
  let rlh = 0;
  let mie = 0;
  for (let j = 0; j < steps; j++) {
    const m = t + jStep / 2;
    const h = Math.hypot(from[0] + dir[0] * m, from[1] + dir[1] * m, from[2] + dir[2] * m) - R_PLANET;
    rlh += Math.exp(-h / SH_RLH) * jStep;
    mie += Math.exp(-h / SH_MIE) * jStep;
    t += jStep;
  }
  return [rlh, mie];
}

/** Sunlight reaching the ground from direction s: nothing once the sun has set. */
export function sunlight(s: Vec3, power = SUN_POWER): Vec3 {
  const ground = sphere(ORIGIN, s, R_PLANET);
  if (ground[0] > 0) return [0, 0, 0];
  const [rlh, mie] = opticalDepth(ORIGIN, s, 32);
  // Soften the last moments before sunset instead of cutting the light off at the horizon.
  const fade = Math.min(1, Math.max(0, (s[1] + 0.02) / 0.04));
  return K_RLH.map((k) => power * fade * Math.exp(-(K_MIE * mie + k * rlh))) as Vec3;
}

export interface SkyLight {
  /** Mean radiance over the whole sky dome (the ground below gives less). */
  ambient: Vec3;
  /** Light from the sky on flat ground. */
  irradiance: Vec3;
}

/** The sky's light summed over a coarse grid of directions. */
export function skyLight(s: Vec3, power = SUN_POWER): SkyLight {
  const ambient: Vec3 = [0, 0, 0];
  const irradiance: Vec3 = [0, 0, 0];
  const rings = 6;
  const around = 12;
  let wSum = 0;
  for (let i = 0; i < rings; i++) {
    // Equal-area rings: cos θ evenly spaced.
    const cosT = (i + 0.5) / rings;
    const sinT = Math.sqrt(1 - cosT * cosT);
    for (let j = 0; j < around; j++) {
      const phi = ((j + 0.5) / around) * 2 * Math.PI;
      const d: Vec3 = [sinT * Math.cos(phi), Math.max(cosT, 0.002), sinT * Math.sin(phi)];
      const l = skyRadiance(d, s, power);
      for (let c = 0; c < 3; c++) {
        ambient[c] += l[c];
        irradiance[c] += l[c] * cosT;
      }
      wSum++;
    }
  }
  // Equal-area samples over the hemisphere's 2π steradians.
  for (let c = 0; c < 3; c++) {
    irradiance[c] *= (2 * Math.PI) / wSum;
    ambient[c] *= 0.6 / wSum;
  }
  return { ambient, irradiance };
}

export const luminance = (c: Vec3) => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
