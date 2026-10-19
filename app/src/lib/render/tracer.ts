// The path tracer behind render mode, in WebGL2. Each frame adds one sample per pixel to
// running averages (in bands when a whole frame would take too long), and after each sample
// the denoiser and the tone mapper make the picture. Moving the camera starts over at half
// resolution until it stops.

import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { heightPyramid, medianTop, type Scene } from './scene';
import { fogDensity, sunDirection, type RenderSettings } from './settings';
import { ATROUS_FS, COMPOSITE_FS, FULLSCREEN_VS, PREPARE_FS, SKY_FS, TRACE_FS } from './shaders';
import { luminance, skyLight, sunlight, SKY_GAIN, SUN_DIRECT, SUN_POWER, SUN_RADIUS, type Vec3 } from './sky';

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
/** Frame times that make the next sample split into more bands, or fewer. */
const SLOW_MS = 70;
const FAST_MS = 28;
const MAX_BANDS = 32;
const START_BANDS = 4;
/** How long the camera must be still before full resolution comes back. */
const SETTLE_MS = 250;

type Uniforms = Map<string, WebGLUniformLocation | null>;

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
  private readonly floatBlend: boolean;
  private readonly maxTexture: number;
  private readonly vao: WebGLVertexArrayObject;
  private readonly programs: Record<'trace' | 'sky' | 'prepare' | 'atrous' | 'composite', Program>;
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
  private baseExposure = 1;

  private sky: Target;
  private accum: Target | null = null;
  private prep: Target | null = null;
  private pingA: Target | null = null;
  private pingB: Target | null = null;
  private out: Target | null = null;
  private width = 0;
  private height = 0;

  private samples = 0;
  private seed = 0;
  private band = 0;
  private bands = 1;
  private bandsNext = 1;
  private started = performance.now();
  private finishedAt = 0;
  private skyDirty = true;
  private pictureDirty = false;
  private blitDirty = false;
  private moving = false;
  private lastMove = 0;
  private lastFrame = 0;
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
    if (!gl) throw new Error('WebGL 2 is unavailable.');
    if (!gl.getExtension('EXT_color_buffer_float')) throw new Error('Floating-point images are unavailable.');
    this.gl = gl;
    this.floatBlend = !!gl.getExtension('EXT_float_blend');
    this.parallel = gl.getExtension('KHR_parallel_shader_compile') as { COMPLETION_STATUS_KHR: number } | null;
    this.maxTexture = gl.getParameter(gl.MAX_TEXTURE_SIZE) as number;
    this.vao = gl.createVertexArray()!;
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.pixelStorei(gl.PACK_ALIGNMENT, 1);

    this.programs = {
      trace: this.program(TRACE_FS),
      sky: this.program(SKY_FS),
      prepare: this.program(PREPARE_FS),
      atrous: this.program(ATROUS_FS),
      composite: this.program(COMPOSITE_FS),
    };
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
      this.blitDirty = true;
    });
    this.resize.observe(canvas);
    this.updateLight();
    this.raf = requestAnimationFrame(this.frame);
  }

  dispose(): void {
    cancelAnimationFrame(this.raf);
    this.resize.disconnect();
    this.controls.dispose();
    this.canvas.removeEventListener('webglcontextlost', this.onLost);
    const gl = this.gl;
    this.dropScene();
    for (const t of [this.sky, this.accum, this.prep, this.pingA, this.pingB, this.out]) this.dropTarget(t);
    for (const p of Object.values(this.programs)) gl.deleteProgram(p.program);
    gl.deleteVertexArray(this.vao);
    gl.getExtension('WEBGL_lose_context')?.loseContext();
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
    const light = ['sunAzimuth', 'sunElevation', 'sunBrightness', 'skyBrightness'];
    if (changed.some((k) => light.includes(k))) this.updateLight();
    if (changed.some((k) => k === 'fov')) {
      this.camera.fov = settings.fov;
      this.camera.updateProjectionMatrix();
    }
    const display = ['exposure', 'denoise'];
    if (changed.every((k) => display.includes(k))) {
      this.pictureDirty = true;
    } else if (changed.every((k) => k === 'samples')) {
      if (this.samples < settings.samples) this.finishedAt = 0;
      this.pictureDirty = true;
    } else {
      this.restart();
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
    const dt = now - this.lastFrame;
    this.lastFrame = now;
    if (this.moving && now - this.lastMove > SETTLE_MS) {
      this.moving = false;
      this.restart();
    }
    this.fitSize();
    if (this.skyDirty) this.drawSky();
    const gl = this.gl;
    if (!this.scene) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.viewport(0, 0, this.canvas.width, this.canvas.height);
      gl.clearColor(0.66, 0.78, 0.86, 1);
      gl.clear(gl.COLOR_BUFFER_BIT);
      return;
    }
    if (this.samples < this.settings.samples) {
      if (this.band === 0) {
        // Split slow frames into more bands, and join fast ones.
        if (dt > SLOW_MS && this.bands < MAX_BANDS && !this.moving) this.bandsNext = this.bands * 2;
        else if (dt < FAST_MS && this.bands > 1) this.bandsNext = this.bands / 2;
        this.bands = this.bandsNext;
        this.seed++;
      }
      this.trace(this.band, this.bands);
      this.band++;
      if (this.band >= this.bands) {
        this.band = 0;
        this.samples++;
        this.pictureDirty = true;
        if (this.samples >= this.settings.samples) this.finishedAt = now;
      }
    }
    if (this.pictureDirty && this.samples > 0) {
      this.pictureDirty = false;
      this.develop();
      this.blitDirty = true;
    }
    if (this.blitDirty && this.out) {
      this.blitDirty = false;
      this.blit();
    }
    this.status(now, false);
  };

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
    this.band = 0;
    // Start a still picture in a few bands, so that no single draw runs long enough for the
    // system to reset the graphics card; fast cards join them up within a few samples.
    this.bands = this.moving ? 1 : START_BANDS;
    this.bandsNext = this.bands;
    this.finishedAt = 0;
    this.started = performance.now();
  }

  private readonly onLost = (e: Event) => {
    e.preventDefault();
    this.lost = true;
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
    const sunE = sunlight(this.sun).map((c) => c * SUN_DIRECT) as Vec3;
    this.sunE = sunE.map((c) => c * s.sunBrightness) as Vec3;
    const sky = skyLight(this.sun, SUN_POWER * SKY_GAIN);
    this.ambient = sky.ambient.map((c) => c * s.skyBrightness) as Vec3;
    // Expose for the light on flat ground, as a camera would, but only partly: sunsets stay
    // darker than noon. The brightness sliders are left to change the picture.
    const ground = luminance(sunE) * Math.max(this.sun[1], 0) + luminance(sky.irradiance);
    this.baseExposure = Math.min(12, 1.3 * Math.pow(6 / Math.max(ground, 0.02), 0.6));
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
    gl.uniform1f(this.u(p, 'uSunPower'), SUN_POWER * SKY_GAIN * this.settings.skyBrightness);
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
      this.blitDirty = true;
    }
    const share = this.settings.scale * (this.moving ? 0.5 : 1);
    const w = Math.max(1, Math.round(cw * share));
    const h = Math.max(1, Math.round(ch * share));
    if (w === this.width && h === this.height && this.accum) return;
    this.width = w;
    this.height = h;
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    const gl = this.gl;
    for (const t of [this.accum, this.prep, this.pingA, this.pingB, this.out]) this.dropTarget(t);
    const acc = this.floatBlend ? gl.RGBA32F : gl.RGBA16F;
    this.accum = this.target(w, h, [acc, acc, acc, acc], gl.NEAREST);
    this.prep = this.target(w, h, [gl.RGBA16F, gl.RGBA16F, gl.RGBA16F], gl.NEAREST);
    this.pingA = this.target(w, h, [gl.RGBA16F, gl.RGBA16F], gl.NEAREST);
    // The second half of the ping-pong writes into the prepare pass's own images.
    this.pingB = this.attach([this.prep.textures[0], this.prep.textures[1]]);
    this.out = this.target(w, h, [gl.RGBA8], gl.LINEAR);
    this.restart();
  }

  private trace(band: number, bands: number): void {
    const gl = this.gl;
    const s = this.scene!;
    const h = s.header;
    const st = this.settings;
    const p = this.programs.trace;
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.accum!.fbo);
    gl.viewport(0, 0, this.width, this.height);
    const y0 = Math.floor((band * this.height) / bands);
    const y1 = Math.floor(((band + 1) * this.height) / bands);
    gl.enable(gl.SCISSOR_TEST);
    gl.scissor(0, y0, this.width, y1 - y0);
    // The running mean: new = old + (sample − old) / n.
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.CONSTANT_ALPHA, gl.ONE_MINUS_CONSTANT_ALPHA);
    gl.blendColor(0, 0, 0, 1 / (this.samples + 1));
    gl.useProgram(p.program);

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
    gl.uniform2f(this.u(p, 'uRes'), this.width, this.height);
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
    i('uBounces', Math.round(st.bounces));
    f('uCloudY', st.cloudHeight);
    f('uCloudCover', st.clouds);

    gl.drawBuffers([gl.COLOR_ATTACHMENT0, gl.COLOR_ATTACHMENT1, gl.COLOR_ATTACHMENT2, gl.COLOR_ATTACHMENT3]);
    this.draw();
    gl.disable(gl.BLEND);
    gl.disable(gl.SCISSOR_TEST);
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
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.out!.fbo);
    gl.drawBuffers([gl.COLOR_ATTACHMENT0]);
    gl.useProgram(cp.program);
    this.bind(cp, 'uE', 0, e);
    this.bind(cp, 'uLaF', 1, a);
    this.bind(cp, 'uLm', 2, lm);
    this.bind(cp, 'uM', 3, m);
    this.bind(cp, 'uLa', 4, la);
    gl.uniform1i(this.u(cp, 'uDenoise'), this.settings.denoise ? 1 : 0);
    gl.uniform1f(this.u(cp, 'uExposure'), this.baseExposure * Math.pow(2, this.settings.exposure));
    this.draw();
    // Leave no target bound for reading as a texture next time round.
    for (let u = 0; u < 5; u++) {
      gl.activeTexture(gl.TEXTURE0 + u);
      gl.bindTexture(gl.TEXTURE_2D, null);
    }
  }

  private blit(): void {
    const gl = this.gl;
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, this.out!.fbo);
    gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
    gl.blitFramebuffer(0, 0, this.width, this.height, 0, 0, this.canvas.width, this.canvas.height, gl.COLOR_BUFFER_BIT, gl.LINEAR);
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
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
