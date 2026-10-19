// The path tracer behind render mode, in WebGL2. Each frame adds one sample per pixel to
// running averages (a band of rows at a time when a whole frame would take too long), and the
// denoiser and the tone mapper make the picture from them. While the camera or the light moves, a
// quick view without noise is drawn instead, at whatever resolution keeps it smooth; once it
// stops, the render starts over and fades in over the quick view.

import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { heightPyramid, medianTop, type Scene } from './scene';
import { fogDensity, sunDirection, type RenderSettings } from './settings';
import {
  ATROUS_FS,
  BLOOM_DOWN_FS,
  BLOOM_UP_FS,
  COMPOSITE_FS,
  FINISH_FS,
  FULLSCREEN_VS,
  NOISE_FS,
  PREPARE_FS,
  PRESENT_FS,
  QUICK_FS,
  SKY_FS,
  TRACE_FS,
} from './shaders';
import { Pacer } from './pacer';
import {
  luminance,
  skyLight,
  sunlight,
  MOON_GAIN,
  MOON_TINT,
  SKY_GAIN,
  SUN_DIRECT,
  SUN_POWER,
  SUN_RADIUS,
  type Vec3,
} from './sky';

export interface TracerStatus {
  samples: number;
  target: number;
  /** Seconds since this picture started. */
  seconds: number;
  done: boolean;
  /** Shaders still compiling. */
  compiling: boolean;
  error: string | null;
}

const SKY_W = 256;
const SKY_H = 128;
const ATROUS_PASSES = 5;
/** Texture widths for the long data tables. */
const TABLE_W = 4096;
/** What one batch of the render should take on the graphics card. Short batches keep the
 * window responsive; Windows resets a card that is busy with one for two seconds. */
const TRACE_MS = 30;
/** What one quick view should take while moving: under a frame, so turning stays smooth. */
const QUICK_MS = 12;
/** The sharper quick view once still may take a few times longer, once. */
const SETTLE_QUICK = 3;
/** Most pixels traced at the full Resolution setting: about a 1920 × 1080 screen. Bigger
 * screens get the picture scaled up; the images it needs would not fit some cards. */
const MAX_PIXELS = 2_100_000;
/** How often the picture is remade from the samples, once it has a few. */
const DEVELOP_MS = 120;
/** How long the camera and the light must be still before the render starts. */
const SETTLE_MS = 250;
/** The coarsest quick view, as a share of the picture's width. */
const QUICK_MIN = 0.15;
/** Samples at which the render starts to show over the quick view, and covers it (with the
 * denoiser; without it the picture needs more before it looks better than the quick view). */
const FADE_FROM = 1;
const FADE_FULL = 6;
const FADE_FULL_RAW = 16;
/** The shortest fade from the quick view to the render. */
const FADE_MS = 300;
/** Side of the cloud noise, in texels. */
const NOISE_N = 64;
/** Halvings of the picture for the glow, from a quarter of its size. */
const BLOOM_LEVELS = 6;
/** Share of the light spread into the glow at the Glow setting's top. */
const GLOW_MAX = 0.12;
/** The soft clouds: blocks across one tile of their noise at Size 1, and extinction per block
 * where they are thickest. */
const CLOUD_TILE = 500;
const CLOUD_SIGMA = 0.35;
/** The noise texture's unit, clear of the 2D ones. */
const NOISE_UNIT = 8;

/** Times the graphics card has dropped the render in this window: each halves the pixels. */
let losses = 0;

type Uniforms = Map<string, WebGLUniformLocation | null>;

/** A batch of work handed to the graphics card: what it was, and how to tell it is done. */
interface Job {
  kind: 'quick' | 'trace' | 'develop' | 'noise';
  units: number;
  at: number;
  sync: WebGLSync | null;
  query: WebGLQuery | null;
}

interface TimerQuery {
  TIME_ELAPSED_EXT: number;
  GPU_DISJOINT_EXT: number;
}

interface Program {
  program: WebGLProgram;
  uniforms: Uniforms;
}

interface Target {
  fbo: WebGLFramebuffer;
  textures: WebGLTexture[];
}

/** Why render mode can't run here, or null. */
export function unsupported(): string | null {
  try {
    const gl = document.createElement('canvas').getContext('webgl2');
    if (!gl) return 'This computer’s graphics don’t support WebGL 2, which render mode needs.';
    const ok = !!gl.getExtension('EXT_color_buffer_float');
    gl.getExtension('WEBGL_lose_context')?.loseContext();
    return ok ? null : 'This computer’s graphics can’t draw into floating-point images, which render mode needs.';
  } catch {
    return 'WebGL 2 is unavailable.';
  }
}

export class PathTracer {
  readonly camera = new THREE.PerspectiveCamera(50, 1, 0.1, 100_000);
  readonly controls: OrbitControls;
  private readonly gl: WebGL2RenderingContext;
  private readonly parallel: { COMPLETION_STATUS_KHR: number } | null;
  private readonly timer: TimerQuery | null;
  private readonly floatBlend: boolean;
  private readonly maxTexture: number;
  private readonly vao: WebGLVertexArrayObject;
  private readonly programs: Record<
    | 'trace'
    | 'quick'
    | 'sky'
    | 'prepare'
    | 'atrous'
    | 'composite'
    | 'present'
    | 'noise'
    | 'bloomDown'
    | 'bloomUp'
    | 'finish',
    Program
  >;
  /** Tiling 3D noise for the soft clouds and the Milky Way, made on the card when first needed. */
  private readonly noise: WebGLTexture;
  private noiseReady = false;
  private ready = false;

  private scene: Scene | null = null;
  private sceneTextures: WebGLTexture[] = [];
  private tex: Record<string, WebGLTexture> = {};
  private levels = 1;
  private voxW = 1;
  private sceneKey = '';

  private settings: RenderSettings;
  private sun: Vec3 = [0, 1, 0];
  private sunE: Vec3 = [0, 0, 0];
  private ambient: Vec3 = [0, 0, 0];
  private skyUp: Vec3 = [0, 0, 0];
  private bounce: Vec3 = [0, 0, 0];
  /** The sky shader's lights: the sun's power, or the moon's (tinted), the other zero. */
  private skySun = 0;
  private skyMoon: Vec3 = [0, 0, 0];
  private stars = 0;
  private baseExposure = 1;

  private sky: Target;
  private accum: Target | null = null;
  private prep: Target | null = null;
  private pingA: Target | null = null;
  private pingB: Target | null = null;
  private out: Target | null = null;
  private quick: Target | null = null;
  /** The light before tone mapping, and the glow made from it, halving in size. */
  private hdr: Target | null = null;
  private bloom: Target[] = [];
  private bloomSize: [number, number][] = [];
  private width = 0;
  private height = 0;

  private samples = 0;
  private seed = 0;
  /** The next row of this sample to trace. */
  private row = 0;
  private lastDevelop = 0;
  /** Rows of the render, and pixels of the quick view, per batch. */
  private readonly tracePacer = new Pacer({ targetMs: TRACE_MS, start: 24, min: 4 });
  private readonly quickPacer = new Pacer({ targetMs: QUICK_MS, start: 120_000, min: 4096 });
  private job: Job | null = null;
  private frameMs = 16.7;
  private started = performance.now();
  private finishedAt = 0;
  private skyDirty = true;
  private pictureDirty = false;
  private presentDirty = false;
  private moving = false;
  private lastMove = 0;
  private lastFrame = 0;

  private quickDirty = true;
  /** Size of the quick view now in its image, from the corner. */
  private quickW = 1;
  private quickH = 1;
  /** How much of the render shows over the quick view. */
  private mix = 0;
  /** Quick views drawn so far (for tests). */
  quickFrames = 0;
  private lastStatus = 0;
  private raf = 0;
  private lost = false;
  /** Moving the camera from code, not the mouse. */
  private placing = false;
  private readonly resize: ResizeObserver;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    settings: RenderSettings,
    private readonly onStatus: (s: TracerStatus) => void,
  ) {
    this.settings = { ...settings };
    const gl = canvas.getContext('webgl2', {
      antialias: false,
      alpha: false,
      depth: false,
      stencil: false,
      preserveDrawingBuffer: false,
      powerPreference: 'high-performance',
    });
    if (!gl) {
      throw new Error(
        losses > 0
          ? 'The graphics card isn’t available to Vistaforge after it was reset. Close Vistaforge and open it again.'
          : 'WebGL 2 is unavailable.',
      );
    }
    if (!gl.getExtension('EXT_color_buffer_float')) throw new Error('Floating-point images are unavailable.');
    this.gl = gl;
    this.floatBlend = !!gl.getExtension('EXT_float_blend');
    this.parallel = gl.getExtension('KHR_parallel_shader_compile') as { COMPLETION_STATUS_KHR: number } | null;
    this.timer = gl.getExtension('EXT_disjoint_timer_query_webgl2') as TimerQuery | null;
    this.maxTexture = gl.getParameter(gl.MAX_TEXTURE_SIZE) as number;
    this.vao = gl.createVertexArray()!;
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.pixelStorei(gl.PACK_ALIGNMENT, 1);

    this.programs = {
      trace: this.program(TRACE_FS),
      quick: this.program(QUICK_FS),
      sky: this.program(SKY_FS),
      prepare: this.program(PREPARE_FS),
      atrous: this.program(ATROUS_FS),
      composite: this.program(COMPOSITE_FS),
      present: this.program(PRESENT_FS),
      noise: this.program(NOISE_FS),
      bloomDown: this.program(BLOOM_DOWN_FS),
      bloomUp: this.program(BLOOM_UP_FS),
      finish: this.program(FINISH_FS),
    };
    this.noise = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_3D, this.noise);
    gl.texStorage3D(gl.TEXTURE_3D, 1, gl.RGBA8, NOISE_N, NOISE_N, NOISE_N);
    gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    for (const w of [gl.TEXTURE_WRAP_S, gl.TEXTURE_WRAP_T, gl.TEXTURE_WRAP_R])
      gl.texParameteri(gl.TEXTURE_3D, w, gl.REPEAT);
    gl.bindTexture(gl.TEXTURE_3D, null);
    this.sky = this.target(SKY_W, SKY_H, [gl.RGBA16F], gl.LINEAR);
    gl.bindTexture(gl.TEXTURE_2D, this.sky.textures[0]);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);

    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.screenSpacePanning = true;
    this.controls.maxPolarAngle = Math.PI * 0.495;
    this.controls.addEventListener('change', () => {
      if (!this.placing) this.moved();
    });

    canvas.addEventListener('webglcontextlost', this.onLost);
    this.resize = new ResizeObserver(() => {
      this.presentDirty = true;
    });
    this.resize.observe(canvas);
    this.updateLight();
    this.raf = requestAnimationFrame(this.frame);
  }

  /** Stops and frees the tracer. With `keepContext`, the canvas's context stays usable for a
   * new tracer (after the graphics card was reset and the context came back). */
  dispose(keepContext = false): void {
    cancelAnimationFrame(this.raf);
    this.resize.disconnect();
    this.controls.dispose();
    this.canvas.removeEventListener('webglcontextlost', this.onLost);
    // Whatever was made before a reset is gone with it.
    if (this.lost) return;
    const gl = this.gl;
    this.dropScene();
    for (const t of [
      this.sky,
      this.accum,
      this.prep,
      this.pingA,
      this.pingB,
      this.out,
      this.quick,
      this.hdr,
      ...this.bloom,
    ]) {
      this.dropTarget(t);
    }
    gl.deleteTexture(this.noise);
    for (const p of Object.values(this.programs)) gl.deleteProgram(p.program);
    gl.deleteVertexArray(this.vao);
    if (!keepContext) gl.getExtension('WEBGL_lose_context')?.loseContext();
  }

  // ---- What to draw ---------------------------------------------------------------------

  setScene(scene: Scene): void {
    const gl = this.gl;
    const h = scene.header;
    const voxW = Math.min(this.maxTexture, 8192);
    if (Math.ceil(Math.max(h.voxels, 1) / voxW) > this.maxTexture) {
      throw new Error('This area holds more blocks than this graphics card can take. Choose a smaller area.');
    }
    if (h.atlas_height > this.maxTexture) throw new Error('The block textures don’t fit this graphics card.');
    this.dropScene();
    this.scene = scene;
    const side = h.side;
    this.levels = Math.log2(side) + 1;
    this.voxW = voxW;

    this.tex.columns = this.dataTexture(gl.RGBA32UI, gl.RGBA_INTEGER, gl.UNSIGNED_INT, side, side, scene.columns);
    this.tex.water = this.dataTexture(gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, side, side, scene.water);
    this.tex.voxels = this.table(gl.R16UI, gl.RED_INTEGER, gl.UNSIGNED_SHORT, 1, voxW, scene.voxels, Uint16Array);
    this.tex.states = this.table(gl.RGBA32UI, gl.RGBA_INTEGER, gl.UNSIGNED_INT, 4, TABLE_W, scene.states, Uint32Array);
    this.tex.quads = this.table(gl.RGBA32F, gl.RGBA, gl.FLOAT, 4, TABLE_W, scene.quads, Float32Array);
    this.tex.atlas = this.dataTexture(
      gl.SRGB8_ALPHA8,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      Math.max(h.atlas_width, 1),
      Math.max(h.atlas_height, 1),
      h.atlas_width > 0 && h.atlas_height > 0 ? scene.atlas : new Uint8Array(4),
    );

    const tops = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, tops);
    gl.texStorage2D(gl.TEXTURE_2D, this.levels, gl.R16UI, side, side);
    heightPyramid(scene).forEach((level, i) => {
      const s = side >> i;
      gl.texSubImage2D(gl.TEXTURE_2D, i, 0, 0, s, s, gl.RED_INTEGER, gl.UNSIGNED_SHORT, level);
    });
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST_MIPMAP_NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAX_LEVEL, this.levels - 1);
    this.tex.tops = tops;
    this.sceneTextures = Object.values(this.tex);

    // Keep the view when the same area comes back (after a change to the map).
    const key = `${h.x0},${h.z0},${h.width},${h.depth}`;
    if (key !== this.sceneKey) this.resetCamera();
    this.sceneKey = key;
    this.restart();
  }

  setSettings(settings: RenderSettings): void {
    const before = this.settings;
    this.settings = { ...settings };
    const changed = (Object.keys(settings) as (keyof RenderSettings)[]).filter((k) => before[k] !== settings[k]);
    if (changed.length === 0) return;
    const light = ['sunAzimuth', 'sunElevation', 'night', 'sunBrightness', 'skyBrightness'];
    if (changed.some((k) => light.includes(k))) this.updateLight();
    if (changed.some((k) => k === 'fov')) {
      this.camera.fov = settings.fov;
      this.camera.updateProjectionMatrix();
    }
    const display = ['exposure', 'denoise', 'glow', 'vignette'];
    if (changed.every((k) => display.includes(k))) {
      this.pictureDirty = true;
      // The quick view has the exposure in it.
      if (this.mix < 1) this.quickDirty = true;
    } else if (changed.every((k) => k === 'samples')) {
      if (this.samples < settings.samples) this.finishedAt = 0;
      this.pictureDirty = true;
    } else {
      // Like moving the camera: quick views while a slider is dragged, the render after.
      this.moved();
    }
  }

  resetCamera(): void {
    const s = this.scene;
    if (!s) return;
    const { width, depth } = s.header;
    const y = medianTop(s);
    const size = Math.max(width, depth);
    this.controls.target.set(width / 2, y, depth / 2);
    // From the south-east, looking down at about 30°.
    const dir = new THREE.Vector3(0.45, 0.55, 0.7).normalize();
    this.camera.position.copy(this.controls.target).addScaledVector(dir, size * 0.85);
    this.camera.fov = this.settings.fov;
    this.camera.updateProjectionMatrix();
    this.controls.maxDistance = size * 4;
    this.controls.minDistance = 2;
    this.placing = true;
    this.controls.update();
    this.placing = false;
    this.restart();
  }

  /** The finished picture as a PNG, at the size it is traced. */
  async picture(): Promise<Blob> {
    const gl = this.gl;
    if (!this.out) throw new Error('Nothing has been rendered yet.');
    const { width: w, height: h } = this;
    const px = new Uint8Array(w * h * 4);
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, this.out.fbo);
    gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px);
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
    const c = document.createElement('canvas');
    c.width = w;
    c.height = h;
    const ctx = c.getContext('2d')!;
    const img = ctx.createImageData(w, h);
    // WebGL rows go bottom up.
    for (let y = 0; y < h; y++) img.data.set(px.subarray((h - 1 - y) * w * 4, (h - y) * w * 4), y * w * 4);
    ctx.putImageData(img, 0, 0);
    return new Promise((resolve, reject) =>
      c.toBlob((b) => (b ? resolve(b) : reject(new Error('The picture could not be encoded.'))), 'image/png'),
    );
  }

  // ---- The loop -------------------------------------------------------------------------

  private readonly frame = (now: number) => {
    this.raf = requestAnimationFrame(this.frame);
    if (this.lost) return;
    if (!this.ready) {
      this.ready = this.compiled();
      if (!this.ready) {
        this.status(now, true);
        return;
      }
      if (!this.check()) return;
    }
    const dt = Math.min(Math.max(now - this.lastFrame, 4), 100);
    this.lastFrame = now;
    this.frameMs = this.frameMs * 0.9 + dt * 0.1;
    const gl = this.gl;
    if (!this.scene) {
      this.fitSize();
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.viewport(0, 0, this.canvas.width, this.canvas.height);
      gl.clearColor(0.66, 0.78, 0.86, 1);
      gl.clear(gl.COLOR_BUFFER_BIT);
      return;
    }
    if (this.moving && now - this.lastMove > SETTLE_MS) {
      this.moving = false;
      // A sharper quick view to judge the shot by while the render gets going.
      if (this.quickW < this.width) this.quickDirty = true;
    }

    // One batch of work at a time: nothing more is handed to the graphics card until it has
    // finished the last one, so work never piles up behind the window's own drawing.
    if (this.job && this.finished(now)) this.job = null;
    if (!this.job) {
      this.fitSize();
      if (this.skyDirty) this.drawSky();
      this.nextJob(now);
    }

    // The render shows through the quick view as its samples add up, never faster than FADE_MS.
    const goal = this.fadeGoal();
    if (goal > this.mix) {
      this.mix = Math.min(goal, this.mix + dt / FADE_MS);
      this.presentDirty = true;
    }
    if (this.presentDirty && this.out && this.quick) {
      this.presentDirty = false;
      this.present();
    }
    this.status(now, false);
  };

  /** Hands the graphics card the next piece of work, if there is any. */
  private nextJob(now: number): void {
    const all = this.width * this.height;
    if (!this.noiseReady && this.needsNoise()) {
      this.begin('noise', 1, now);
      this.makeNoise();
      this.end();
      // Clouds and stars that were left out until now.
      this.restart();
      return;
    }
    if (this.quickDirty) {
      // While moving, as much of the quick view as fits in QUICK_MS; once still, a few times that.
      const pixels = this.moving
        ? this.quickPacer.next(all)
        : Math.min(all, this.quickPacer.next(all) * SETTLE_QUICK);
      const share = Math.max(QUICK_MIN, Math.min(1, Math.sqrt(pixels / all)));
      this.begin('quick', Math.round(share * this.width) * Math.round(share * this.height), now);
      this.drawQuick(share);
      this.end();
      this.presentDirty = true;
      return;
    }
    if (this.moving) return;
    const due = this.samples <= FADE_FULL_RAW || this.samples >= this.settings.samples || now - this.lastDevelop > DEVELOP_MS;
    if (this.pictureDirty && this.samples > 0 && due) {
      this.pictureDirty = false;
      this.lastDevelop = now;
      this.begin('develop', 1, now);
      this.develop();
      this.end();
      this.presentDirty = true;
      return;
    }
    if (this.samples >= this.settings.samples) return;
    if (this.row === 0) this.seed++;
    const rows = Math.min(Math.round(this.tracePacer.next(this.height)), this.height - this.row);
    this.begin('trace', rows, now);
    this.trace(this.row, rows);
    this.end();
    this.row += rows;
    if (this.row >= this.height) {
      this.row = 0;
      this.samples++;
      this.pictureDirty = true;
      if (this.samples >= this.settings.samples) this.finishedAt = now;
    }
  }

  private begin(kind: Job['kind'], units: number, now: number): void {
    const gl = this.gl;
    let query: WebGLQuery | null = null;
    if (this.timer) {
      query = gl.createQuery();
      if (query) gl.beginQuery(this.timer.TIME_ELAPSED_EXT, query);
    }
    this.job = { kind, units, at: now, sync: null, query };
  }

  private end(): void {
    const gl = this.gl;
    const job = this.job!;
    if (job.query) gl.endQuery(this.timer!.TIME_ELAPSED_EXT);
    job.sync = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
    gl.flush();
  }

  /** Whether the graphics card has done the last batch; if so, its time sizes the next one. */
  private finished(now: number): boolean {
    const gl = this.gl;
    const job = this.job!;
    if (job.sync && gl.getSyncParameter(job.sync, gl.SYNC_STATUS) !== gl.SIGNALED) {
      // Never wait forever on a fence the card has dropped.
      if (now - job.at < 10_000) return false;
    }
    let ms = now - job.at;
    let quantum = this.frameMs;
    if (job.query) {
      const ready = gl.getQueryParameter(job.query, gl.QUERY_RESULT_AVAILABLE) as boolean;
      const disjoint = gl.getParameter(this.timer!.GPU_DISJOINT_EXT) as boolean;
      if (ready && !disjoint) {
        ms = (gl.getQueryParameter(job.query, gl.QUERY_RESULT) as number) / 1e6;
        quantum = 0;
      }
      gl.deleteQuery(job.query);
    }
    if (job.sync) gl.deleteSync(job.sync);
    if (job.kind === 'trace') this.tracePacer.took(job.units, ms, quantum, this.height);
    else if (job.kind === 'quick' && this.moving) this.quickPacer.took(job.units, ms, quantum, this.width * this.height);
    return true;
  }

  private fadeGoal(): number {
    if (this.moving || this.samples === 0) return 0;
    const st = this.settings;
    const full = Math.max(FADE_FROM + 1, Math.min(st.samples, st.denoise ? FADE_FULL : FADE_FULL_RAW));
    return Math.min(1, Math.max(0, (this.samples - FADE_FROM) / (full - FADE_FROM)));
  }

  private status(now: number, compiling: boolean): void {
    const done = this.scene !== null && this.samples >= this.settings.samples;
    if (now - this.lastStatus < 200 && !(done && this.finishedAt === now)) return;
    this.lastStatus = now;
    this.onStatus({
      samples: this.samples,
      target: this.settings.samples,
      seconds: ((done && this.finishedAt ? this.finishedAt : now) - this.started) / 1000,
      done,
      compiling,
      error: null,
    });
  }

  private moved(): void {
    this.moving = true;
    this.lastMove = performance.now();
    this.restart();
  }

  private restart(): void {
    this.samples = 0;
    this.row = 0;
    this.finishedAt = 0;
    this.started = performance.now();
    // The old picture is of something else now: back to the quick view.
    this.mix = 0;
    this.quickDirty = true;
    this.presentDirty = true;
  }

  private readonly onLost = (e: Event) => {
    // Asks for the context back, so a new tracer can use this canvas again.
    e.preventDefault();
    this.lost = true;
    this.job = null;
    losses++;
    this.onStatus({
      samples: 0,
      target: this.settings.samples,
      seconds: 0,
      done: false,
      compiling: false,
      error: 'The graphics card stopped the render (it took too long or ran out of memory). Try a smaller area or a lower resolution.',
    });
  };

  // ---- Light ----------------------------------------------------------------------------

  private updateLight(): void {
    const s = this.settings;
    this.sun = sunDirection(s.sunAzimuth, s.sunElevation);
    // At night the light is the moon, where the sun would be, dimmer and bluer.
    const tint: Vec3 = s.night ? MOON_TINT : [1, 1, 1];
    const gain = s.night ? MOON_GAIN : 1;
    const sunE = sunlight(this.sun).map((c, i) => c * SUN_DIRECT * gain * tint[i]) as Vec3;
    this.sunE = sunE.map((c) => c * s.sunBrightness) as Vec3;
    const raw = skyLight(this.sun, SUN_POWER * SKY_GAIN * gain);
    const sky = {
      ambient: raw.ambient.map((c, i) => c * tint[i]) as Vec3,
      irradiance: raw.irradiance.map((c, i) => c * tint[i]) as Vec3,
    };
    this.skySun = s.night ? 0 : SUN_POWER * SKY_GAIN * s.skyBrightness;
    // The night sky drawn darker than the moon's light on the land, as in a night photo, so the
    // stars stand out.
    this.skyMoon = s.night
      ? (tint.map((c) => c * SUN_POWER * SKY_GAIN * gain * s.skyBrightness * 0.5) as Vec3)
      : [0, 0, 0];
    // Stars at night, and the first few at dusk.
    this.stars = s.night ? 1 : Math.min(1, Math.max(0, (-s.sunElevation - 2) / 6)) * 0.4;
    this.ambient = sky.ambient.map((c) => c * s.skyBrightness) as Vec3;
    // Expose for the light on flat ground, as a camera would, but only partly: sunsets stay
    // darker than noon. The brightness sliders are left to change the picture.
    const ground = luminance(sunE) * Math.max(this.sun[1], 0) + luminance(sky.irradiance);
    this.baseExposure = Math.min(12, 1.3 * Math.pow(6 / Math.max(ground, 0.02), 0.6)) * (s.night ? 0.6 : 1);
    // For the quick view: the sky's light on flat ground, and what ground of a typical
    // brightness sends back up onto the undersides.
    this.skyUp = sky.irradiance.map((c) => (c * s.skyBrightness) / Math.PI) as Vec3;
    this.bounce = this.skyUp.map((c, i) => 0.3 * (c + (this.sunE[i] * Math.max(this.sun[1], 0)) / Math.PI)) as Vec3;
    this.skyDirty = true;
  }

  private drawSky(): void {
    const gl = this.gl;
    const p = this.programs.sky;
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.sky.fbo);
    gl.viewport(0, 0, SKY_W, SKY_H);
    gl.disable(gl.BLEND);
    gl.useProgram(p.program);
    gl.uniform3fv(this.u(p, 'uSunDir'), this.sun);
    gl.uniform2f(this.u(p, 'uSize'), SKY_W, SKY_H);
    gl.uniform1f(this.u(p, 'uSunPower'), this.skySun);
    gl.uniform3fv(this.u(p, 'uMoonDir'), this.sun);
    gl.uniform3fv(this.u(p, 'uMoonPower'), this.skyMoon);
    this.draw();
    this.skyDirty = false;
  }

  // ---- Passes ---------------------------------------------------------------------------

  /** Matches the traced size to the canvas, at the chosen share of its pixels. */
  private fitSize(): void {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const cw = Math.max(1, Math.round(this.canvas.clientWidth * dpr));
    const ch = Math.max(1, Math.round(this.canvas.clientHeight * dpr));
    if (this.canvas.width !== cw || this.canvas.height !== ch) {
      this.canvas.width = cw;
      this.canvas.height = ch;
      this.presentDirty = true;
    }
    // A new size waits until things stop moving: dragging the Resolution slider would otherwise
    // remake these large images on every step.
    if (this.moving && this.accum) return;
    const budget = MAX_PIXELS / 2 ** Math.min(losses, 2);
    const fit = Math.min(1, Math.sqrt(budget / (cw * ch))) * this.settings.scale;
    const w = Math.max(1, Math.round(cw * fit));
    const h = Math.max(1, Math.round(ch * fit));
    if (w === this.width && h === this.height && this.accum) return;
    this.width = w;
    this.height = h;
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    const gl = this.gl;
    for (const t of [this.accum, this.prep, this.pingA, this.pingB, this.out, this.quick, this.hdr, ...this.bloom]) {
      this.dropTarget(t);
    }
    const acc = this.floatBlend ? gl.RGBA32F : gl.RGBA16F;
    this.accum = this.target(w, h, [acc, acc, acc, acc], gl.NEAREST);
    this.prep = this.target(w, h, [gl.RGBA16F, gl.RGBA16F, gl.RGBA16F], gl.NEAREST);
    this.pingA = this.target(w, h, [gl.RGBA16F, gl.RGBA16F], gl.NEAREST);
    // The second half of the ping-pong writes into the prepare pass's own images.
    this.pingB = this.attach([this.prep.textures[0], this.prep.textures[1]]);
    this.out = this.target(w, h, [gl.RGBA8], gl.LINEAR);
    this.quick = this.target(w, h, [gl.RGBA8], gl.LINEAR);
    this.hdr = this.target(w, h, [gl.RGBA16F], gl.LINEAR);
    this.bloom = [];
    this.bloomSize = [];
    let bw = Math.ceil(w / 4);
    let bh = Math.ceil(h / 4);
    for (let k = 0; k < BLOOM_LEVELS && bw >= 2 && bh >= 2; k++) {
      this.bloom.push(this.target(bw, bh, [gl.RGBA16F], gl.LINEAR));
      this.bloomSize.push([bw, bh]);
      bw = Math.ceil(bw / 2);
      bh = Math.ceil(bh / 2);
    }
    this.quickW = 1;
    this.quickH = 1;
    this.restart();
  }

  /** One sample for `rows` rows from row `y0`, into the running averages. */
  private trace(y0: number, rows: number): void {
    const gl = this.gl;
    const p = this.programs.trace;
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.accum!.fbo);
    gl.viewport(0, 0, this.width, this.height);
    gl.enable(gl.SCISSOR_TEST);
    gl.scissor(0, y0, this.width, rows);
    // The running mean: new = old + (sample − old) / n.
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.CONSTANT_ALPHA, gl.ONE_MINUS_CONSTANT_ALPHA);
    gl.blendColor(0, 0, 0, 1 / (this.samples + 1));
    gl.useProgram(p.program);
    this.sceneUniforms(p, this.width, this.height);
    gl.uniform1i(this.u(p, 'uBounces'), Math.round(this.settings.bounces));
    gl.drawBuffers([gl.COLOR_ATTACHMENT0, gl.COLOR_ATTACHMENT1, gl.COLOR_ATTACHMENT2, gl.COLOR_ATTACHMENT3]);
    this.draw();
    gl.disable(gl.BLEND);
    gl.disable(gl.SCISSOR_TEST);
  }

  /** The quick view into the corner of its image, at `share` of the picture's size. */
  private drawQuick(share: number): void {
    const gl = this.gl;
    const p = this.programs.quick;
    const w = Math.max(1, Math.round(this.width * share));
    const h = Math.max(1, Math.round(this.height * share));
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.hdr!.fbo);
    gl.viewport(0, 0, w, h);
    gl.disable(gl.BLEND);
    gl.useProgram(p.program);
    this.sceneUniforms(p, w, h);
    gl.uniform1f(this.u(p, 'uExposure'), this.exposure());
    gl.uniform3fv(this.u(p, 'uSkyUp'), this.skyUp);
    gl.uniform3fv(this.u(p, 'uBounce'), this.bounce);
    gl.drawBuffers([gl.COLOR_ATTACHMENT0]);
    this.draw();
    this.finish(this.quick!, w, h);
    this.quickW = w;
    this.quickH = h;
    this.quickDirty = false;
    this.quickFrames++;
  }

  /** The scene, camera and light, for a picture of resW × resH pixels. */
  private sceneUniforms(p: Program, resW: number, resH: number): void {
    const gl = this.gl;
    const h = this.scene!.header;
    const st = this.settings;
    const units: [string, WebGLTexture][] = [
      ['uColumns', this.tex.columns],
      ['uWaterCol', this.tex.water],
      ['uVoxels', this.tex.voxels],
      ['uTops', this.tex.tops],
      ['uStates', this.tex.states],
      ['uQuads', this.tex.quads],
      ['uAtlas', this.tex.atlas],
      ['uSky', this.sky.textures[0]],
    ];
    units.forEach(([name, t], i) => this.bind(p, name, i, t));
    gl.activeTexture(gl.TEXTURE0 + NOISE_UNIT);
    gl.bindTexture(gl.TEXTURE_3D, this.noise);
    gl.uniform1i(this.u(p, 'uNoise'), NOISE_UNIT);

    const i = (name: string, v: number) => gl.uniform1i(this.u(p, name), v);
    const f = (name: string, v: number) => gl.uniform1f(this.u(p, name), v);
    const v3 = (name: string, v: ArrayLike<number>) => gl.uniform3fv(this.u(p, name), Array.from(v));
    i('uSide', h.side);
    i('uLevels', this.levels);
    i('uVoxW', this.voxW);
    i('uStateW', TABLE_W);
    i('uQuadW', TABLE_W);
    i('uTile', h.tile);
    i('uTilesPerRow', Math.max(1, Math.floor(Math.max(h.atlas_width, 1) / h.tile)));
    v3('uBoxMin', [0, Math.min(h.min_y, h.sea_level) - 8, 0]);
    v3('uBoxMax', [h.width, h.max_y + 0.01, h.depth]);
    gl.uniform2f(this.u(p, 'uOrigin'), h.x0, h.z0);

    const cam = this.camera;
    cam.updateMatrixWorld();
    const e = cam.matrixWorld.elements;
    v3('uCamPos', [cam.position.x, cam.position.y, cam.position.z]);
    v3('uCamRight', [e[0], e[1], e[2]]);
    v3('uCamUp', [e[4], e[5], e[6]]);
    v3('uCamFwd', [-e[8], -e[9], -e[10]]);
    f('uTanHalfFov', Math.tan((cam.fov * Math.PI) / 360));
    f('uAspect', this.width / this.height);
    f('uAperture', st.aperture * 1.5);
    f('uFocus', Math.max(cam.position.distanceTo(this.controls.target), 0.5));
    gl.uniform2f(this.u(p, 'uRes'), resW, resH);
    f('uPixelAngle', (2 * Math.tan((cam.fov * Math.PI) / 360)) / resH);
    gl.uniform1ui(this.u(p, 'uFrame'), this.seed >>> 0);

    v3('uSunDir', this.sun);
    v3('uSunE', this.sunE);
    f('uSunCos', Math.cos(SUN_RADIUS * st.sunSize));
    v3('uAmbient', this.ambient);
    f('uFogDensity', fogDensity(st.fog));
    f('uFogBase', st.fogLevel);
    f('uFogHeight', Math.max(st.fogFade, 1));
    f('uFogFar', Math.max(h.width, h.depth) * 4);
    v3('uFogColor', [1, 1, 1]);
    f('uWaterClarity', Math.max(st.waterClarity, 0.5));
    f('uWaves', st.waves);
    f('uCloudY', st.cloudHeight);
    f('uCloudCover', st.clouds);
    const style = st.clouds <= 0 ? 0 : st.cloudStyle === 'blocky' ? 1 : this.noiseReady ? 2 : 0;
    i('uCloudStyle', style);
    f('uCloudTop', st.cloudHeight + Math.max(st.cloudThickness, 4));
    f('uCloudScale', 1 / (CLOUD_TILE * Math.max(st.cloudSize, 0.1)));
    f('uCloudSigma', CLOUD_SIGMA);
    f('uCloudFar', Math.max(4000, Math.max(h.width, h.depth) * 4));
    v3('uCloudAmbTop', this.skyUp);
    v3('uCloudAmbBot', this.bounce);
    f('uMoon', st.night ? 1 : 0);
    f('uStars', this.noiseReady ? this.stars : 0);
  }

  private needsNoise(): boolean {
    const st = this.settings;
    return (st.clouds > 0 && st.cloudStyle === 'soft') || this.stars > 0;
  }

  /** The cloud noise, one layer at a time. */
  private makeNoise(): void {
    const gl = this.gl;
    const p = this.programs.noise;
    gl.activeTexture(gl.TEXTURE0 + NOISE_UNIT);
    gl.bindTexture(gl.TEXTURE_3D, null);
    const fbo = gl.createFramebuffer()!;
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.viewport(0, 0, NOISE_N, NOISE_N);
    gl.disable(gl.BLEND);
    gl.useProgram(p.program);
    gl.uniform1i(this.u(p, 'uN'), NOISE_N);
    for (let z = 0; z < NOISE_N; z++) {
      gl.framebufferTextureLayer(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, this.noise, 0, z);
      gl.drawBuffers([gl.COLOR_ATTACHMENT0]);
      gl.uniform1i(this.u(p, 'uLayer'), z);
      this.draw();
    }
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.deleteFramebuffer(fbo);
    this.noiseReady = true;
  }

  /** Spreads the light in the corner of `hdr` (w × h pixels) through the glow images; false
   * when there is no glow. */
  private spread(w: number, h: number): boolean {
    if (this.settings.glow <= 0 || this.bloom.length === 0) return false;
    const gl = this.gl;
    const down = this.programs.bloomDown;
    gl.disable(gl.BLEND);
    gl.useProgram(down.program);
    let src = this.hdr!.textures[0];
    let sw = this.width;
    let sh = this.height;
    let scale = [w / this.width, h / this.height];
    let max = [(w - 0.5) / this.width, (h - 0.5) / this.height];
    this.bloom.forEach((t, k) => {
      const [bw, bh] = this.bloomSize[k];
      gl.bindFramebuffer(gl.FRAMEBUFFER, t.fbo);
      gl.viewport(0, 0, bw, bh);
      gl.drawBuffers([gl.COLOR_ATTACHMENT0]);
      this.bind(down, 'uSrc', 0, src);
      gl.uniform2f(this.u(down, 'uSrcSize'), sw, sh);
      gl.uniform2f(this.u(down, 'uDstSize'), bw, bh);
      gl.uniform2f(this.u(down, 'uUvScale'), scale[0], scale[1]);
      gl.uniform2f(this.u(down, 'uUvMax'), max[0], max[1]);
      gl.uniform1i(this.u(down, 'uFirst'), k === 0 ? 1 : 0);
      this.draw();
      src = t.textures[0];
      sw = bw;
      sh = bh;
      scale = [1, 1];
      max = [1 - 0.5 / bw, 1 - 0.5 / bh];
    });
    const up = this.programs.bloomUp;
    gl.useProgram(up.program);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE);
    for (let k = this.bloom.length - 2; k >= 0; k--) {
      const [bw, bh] = this.bloomSize[k];
      const [cw, ch] = this.bloomSize[k + 1];
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.bloom[k].fbo);
      gl.viewport(0, 0, bw, bh);
      this.bind(up, 'uSrc', 0, this.bloom[k + 1].textures[0]);
      gl.uniform2f(this.u(up, 'uSrcSize'), cw, ch);
      gl.uniform2f(this.u(up, 'uDstSize'), bw, bh);
      this.draw();
    }
    gl.disable(gl.BLEND);
    return true;
  }

  /** The light in the corner of `hdr` (w × h pixels), with its glow, tone mapped into `to`. */
  private finish(to: Target, w: number, h: number): void {
    const gl = this.gl;
    const glow = this.spread(w, h);
    const p = this.programs.finish;
    gl.bindFramebuffer(gl.FRAMEBUFFER, to.fbo);
    gl.viewport(0, 0, w, h);
    gl.disable(gl.BLEND);
    gl.drawBuffers([gl.COLOR_ATTACHMENT0]);
    gl.useProgram(p.program);
    this.bind(p, 'uHdr', 0, this.hdr!.textures[0]);
    this.bind(p, 'uBloom', 1, glow ? this.bloom[0].textures[0] : this.hdr!.textures[0]);
    gl.uniform2f(this.u(p, 'uOutSize'), w, h);
    gl.uniform1f(this.u(p, 'uGlow'), glow ? GLOW_MAX * this.settings.glow : 0);
    gl.uniform1f(this.u(p, 'uBloomNorm'), 1 / Math.max(this.bloom.length, 1));
    gl.uniform1f(this.u(p, 'uVignette'), this.settings.vignette);
    this.draw();
    for (let u = 0; u < 2; u++) {
      gl.activeTexture(gl.TEXTURE0 + u);
      gl.bindTexture(gl.TEXTURE_2D, null);
    }
  }

  private exposure(): number {
    return this.baseExposure * Math.pow(2, this.settings.exposure);
  }

  /** Denoise (if on) and tone map into the picture. */
  private develop(): void {
    const gl = this.gl;
    const [lm, m, la, guide] = this.accum!.textures;
    const size = (p: Program) => gl.uniform2i(this.u(p, 'uSize'), this.width, this.height);
    gl.viewport(0, 0, this.width, this.height);
    gl.disable(gl.BLEND);
    let e = this.prep!.textures[0];
    let a = this.prep!.textures[1];
    if (this.settings.denoise) {
      const pp = this.programs.prepare;
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.prep!.fbo);
      gl.drawBuffers([gl.COLOR_ATTACHMENT0, gl.COLOR_ATTACHMENT1, gl.COLOR_ATTACHMENT2]);
      gl.useProgram(pp.program);
      this.bind(pp, 'uLm', 0, lm);
      this.bind(pp, 'uM', 1, m);
      this.bind(pp, 'uLa', 2, la);
      this.bind(pp, 'uGuide', 3, guide);
      gl.uniform1f(this.u(pp, 'uN'), this.samples);
      size(pp);
      this.draw();

      const ap = this.programs.atrous;
      gl.useProgram(ap.program);
      size(ap);
      this.bind(ap, 'uG', 2, this.prep!.textures[2]);
      for (let k = 0; k < ATROUS_PASSES; k++) {
        const to = k % 2 === 0 ? this.pingA! : this.pingB!;
        gl.bindFramebuffer(gl.FRAMEBUFFER, to.fbo);
        gl.drawBuffers([gl.COLOR_ATTACHMENT0, gl.COLOR_ATTACHMENT1]);
        this.bind(ap, 'uE', 0, e);
        this.bind(ap, 'uLa', 1, a);
        gl.uniform1i(this.u(ap, 'uStep'), 1 << k);
        this.draw();
        [e, a] = to.textures;
      }
    }

    const cp = this.programs.composite;
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.hdr!.fbo);
    gl.drawBuffers([gl.COLOR_ATTACHMENT0]);
    gl.useProgram(cp.program);
    this.bind(cp, 'uE', 0, e);
    this.bind(cp, 'uLaF', 1, a);
    this.bind(cp, 'uLm', 2, lm);
    this.bind(cp, 'uM', 3, m);
    this.bind(cp, 'uLa', 4, la);
    gl.uniform1i(this.u(cp, 'uDenoise'), this.settings.denoise ? 1 : 0);
    gl.uniform1f(this.u(cp, 'uExposure'), this.exposure());
    this.draw();
    // Leave no target bound for reading as a texture next time round.
    for (let u = 0; u < 5; u++) {
      gl.activeTexture(gl.TEXTURE0 + u);
      gl.bindTexture(gl.TEXTURE_2D, null);
    }
    this.finish(this.out!, this.width, this.height);
  }

  /** The quick view and the render, faded by `mix`, onto the screen. */
  private present(): void {
    const gl = this.gl;
    const p = this.programs.present;
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.disable(gl.BLEND);
    gl.useProgram(p.program);
    this.bind(p, 'uQuick', 0, this.quick!.textures[0]);
    this.bind(p, 'uPicture', 1, this.out!.textures[0]);
    gl.uniform2f(this.u(p, 'uSize'), this.canvas.width, this.canvas.height);
    gl.uniform2f(this.u(p, 'uQuickScale'), this.quickW / this.width, this.quickH / this.height);
    gl.uniform2f(this.u(p, 'uQuickMax'), (this.quickW - 0.5) / this.width, (this.quickH - 0.5) / this.height);
    gl.uniform1f(this.u(p, 'uMix'), this.mix);
    this.draw();
    for (let u = 0; u < 2; u++) {
      gl.activeTexture(gl.TEXTURE0 + u);
      gl.bindTexture(gl.TEXTURE_2D, null);
    }
  }

  // ---- WebGL plumbing -------------------------------------------------------------------

  private program(fs: string): Program {
    const gl = this.gl;
    const compile = (type: number, src: string) => {
      const sh = gl.createShader(type)!;
      gl.shaderSource(sh, src);
      gl.compileShader(sh);
      return sh;
    };
    const program = gl.createProgram()!;
    const vs = compile(gl.VERTEX_SHADER, FULLSCREEN_VS);
    const f = compile(gl.FRAGMENT_SHADER, fs);
    gl.attachShader(program, vs);
    gl.attachShader(program, f);
    gl.linkProgram(program);
    (program as { shaders?: WebGLShader[] }).shaders = [vs, f];
    return { program, uniforms: new Map() };
  }

  /** Whether every program has finished compiling (at once without the extension). */
  private compiled(): boolean {
    const gl = this.gl;
    if (!this.parallel) return true;
    return Object.values(this.programs).every(
      (p) => gl.getProgramParameter(p.program, this.parallel!.COMPLETION_STATUS_KHR) as boolean,
    );
  }

  /** Whether every program linked; if not, says so with the compiler's words and stops. */
  private check(): boolean {
    const gl = this.gl;
    for (const [name, p] of Object.entries(this.programs)) {
      if (gl.getProgramParameter(p.program, gl.LINK_STATUS)) continue;
      const shaders = (p.program as { shaders?: WebGLShader[] }).shaders ?? [];
      const logs = shaders.map((s) => gl.getShaderInfoLog(s)).filter(Boolean);
      const text = `The ${name} shader failed: ${logs.join(' ') || gl.getProgramInfoLog(p.program)}`;
      this.lost = true;
      this.onStatus({ samples: 0, target: 0, seconds: 0, done: false, compiling: false, error: text });
      return false;
    }
    return true;
  }

  private u(p: Program, name: string): WebGLUniformLocation | null {
    let loc = p.uniforms.get(name);
    if (loc === undefined) {
      loc = this.gl.getUniformLocation(p.program, name);
      p.uniforms.set(name, loc);
    }
    return loc;
  }

  private bind(p: Program, name: string, unit: number, texture: WebGLTexture): void {
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + unit);
    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.uniform1i(this.u(p, name), unit);
  }

  private draw(): void {
    const gl = this.gl;
    gl.bindVertexArray(this.vao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  private dataTexture(
    internal: number,
    format: number,
    type: number,
    w: number,
    h: number,
    data: ArrayBufferView,
  ): WebGLTexture {
    const gl = this.gl;
    const t = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texStorage2D(gl.TEXTURE_2D, 1, internal, w, h);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, w, h, format, type, data);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    return t;
  }

  /** A long list as rows of `width` texels of `channels` values, padded at the end. */
  private table<T extends Uint16Array | Uint32Array | Float32Array>(
    internal: number,
    format: number,
    type: number,
    channels: number,
    width: number,
    data: T,
    Kind: { new (n: number): T },
  ): WebGLTexture {
    const texels = Math.max(1, Math.ceil(data.length / channels));
    const w = Math.min(width, texels);
    const h = Math.ceil(texels / w);
    const padded = new Kind(w * h * channels);
    padded.set(data);
    return this.dataTexture(internal, format, type, w, h, padded);
  }

  private target(w: number, h: number, formats: number[], filter: number): Target {
    const gl = this.gl;
    const textures = formats.map((f) => {
      const t = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.texStorage2D(gl.TEXTURE_2D, 1, f, w, h);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      return t;
    });
    const target = this.attach(textures);
    gl.bindFramebuffer(gl.FRAMEBUFFER, target.fbo);
    gl.drawBuffers(textures.map((_, i) => gl.COLOR_ATTACHMENT0 + i));
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return target;
  }

  private attach(textures: WebGLTexture[]): Target {
    const gl = this.gl;
    const fbo = gl.createFramebuffer()!;
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    textures.forEach((t, i) => gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0 + i, gl.TEXTURE_2D, t, 0));
    const status = gl.checkFramebufferStatus(gl.FRAMEBUFFER);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    if (status !== gl.FRAMEBUFFER_COMPLETE) throw new Error(`This graphics card can’t draw the render (${status}).`);
    return { fbo, textures };
  }

  /** Deletes a target; the second ping-pong target shares its images with the first. */
  private dropTarget(t: Target | null): void {
    if (!t) return;
    const gl = this.gl;
    gl.deleteFramebuffer(t.fbo);
    if (t === this.pingB) return;
    for (const tex of t.textures) gl.deleteTexture(tex);
  }

  private dropScene(): void {
    for (const t of this.sceneTextures) this.gl.deleteTexture(t);
    this.sceneTextures = [];
    this.tex = {};
    this.scene = null;
  }
}
