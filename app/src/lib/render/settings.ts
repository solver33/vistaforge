// What the render panel controls, remembered between runs in this browser profile.

export type CloudStyle = 'soft' | 'blocky';

export interface RenderSettings {
  /** Where the light is (the sun, or the moon at night): degrees clockwise from north. */
  sunAzimuth: number;
  /** Degrees above the horizon. */
  sunElevation: number;
  /** Moonlight and stars instead of the sun. */
  night: boolean;
  /** Times the real sun's width: bigger gives softer shadows. */
  sunSize: number;
  sunBrightness: number;
  skyBrightness: number;
  /** 0 is clear air. */
  fog: number;
  /** Y below which the fog is thickest. */
  fogLevel: number;
  /** Blocks over which it thins out above that. */
  fogFade: number;
  /** 0 is no clouds. */
  clouds: number;
  cloudStyle: CloudStyle;
  /** Y of the clouds' undersides. */
  cloudHeight: number;
  /** Blocks from the undersides of the soft clouds to their tops. */
  cloudThickness: number;
  /** Times the soft clouds' usual size across. */
  cloudSize: number;
  /** Blocks of water light gets through before it is mostly gone. */
  waterClarity: number;
  waves: number;
  /** Stops brighter (+) or darker (−). */
  exposure: number;
  /** Times light bounces off surfaces. */
  bounces: number;
  /** Samples per pixel before the picture is finished. */
  samples: number;
  denoise: boolean;
  /** Share of the screen's pixels traced. */
  scale: number;
  /** Vertical field of view, degrees. */
  fov: number;
  /** Depth of field: 0 is everything sharp. */
  aperture: number;
  /** How far bright light spreads into a glow, as in a real lens: 0 is none. */
  glow: number;
  /** How much darker the corners are: 0 is none. */
  vignette: number;
}

export const DEFAULT_SETTINGS: RenderSettings = {
  sunAzimuth: 225,
  sunElevation: 32,
  night: false,
  sunSize: 3,
  sunBrightness: 1,
  skyBrightness: 1,
  fog: 0,
  fogLevel: 80,
  fogFade: 24,
  clouds: 0,
  cloudStyle: 'soft',
  cloudHeight: 192,
  cloudThickness: 80,
  cloudSize: 1,
  waterClarity: 6,
  waves: 0.5,
  exposure: 0,
  bounces: 3,
  samples: 256,
  denoise: true,
  scale: 1,
  fov: 50,
  aperture: 0,
  glow: 0.4,
  vignette: 0.25,
};

export interface TimePreset {
  name: string;
  azimuth: number;
  elevation: number;
  night: boolean;
}

/** Times of day for one click, with the sun's path from east to west across the south. */
export const TIME_PRESETS: TimePreset[] = [
  { name: 'Dawn', azimuth: 80, elevation: 1.5, night: false },
  { name: 'Morning', azimuth: 110, elevation: 20, night: false },
  { name: 'Noon', azimuth: 180, elevation: 62, night: false },
  { name: 'Golden hour', azimuth: 250, elevation: 7, night: false },
  { name: 'Sunset', azimuth: 272, elevation: 0.8, night: false },
  { name: 'Dusk', azimuth: 285, elevation: -4, night: false },
  { name: 'Night', azimuth: 160, elevation: 38, night: true },
];

/** The light's lowest point: a few degrees below the horizon for dusk. */
export const MIN_ELEVATION = -8;

const KEY = 'vistaforge.render.v1';

export function loadSettings(): RenderSettings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    const saved = JSON.parse(raw) as Partial<RenderSettings>;
    const out = { ...DEFAULT_SETTINGS };
    for (const k of Object.keys(DEFAULT_SETTINGS) as (keyof RenderSettings)[]) {
      const v = saved[k];
      if (typeof v === typeof DEFAULT_SETTINGS[k]) (out as Record<string, unknown>)[k] = v;
    }
    if (out.cloudStyle !== 'soft' && out.cloudStyle !== 'blocky') out.cloudStyle = DEFAULT_SETTINGS.cloudStyle;
    return out;
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

export function saveSettings(s: RenderSettings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // Not remembered; nothing else depends on it.
  }
}

/** The sun's direction from the ground: north is −Z, east is +X. */
export function sunDirection(azimuthDeg: number, elevationDeg: number): [number, number, number] {
  const a = (azimuthDeg * Math.PI) / 180;
  const e = (elevationDeg * Math.PI) / 180;
  return [Math.cos(e) * Math.sin(a), Math.sin(e), -Math.cos(e) * Math.cos(a)];
}

/** Fog density per block for the 0–1 slider: gentle at first, thick at the end. */
export function fogDensity(f: number): number {
  return 0.012 * f * f;
}
