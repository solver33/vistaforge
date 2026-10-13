// The 3D preview: the map as a lit diorama with water and simple trees, orbited with the mouse.
// It renders only when something changes.

import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { speciesStyle } from './palette';
import type { PreviewData } from './types';

export interface SceneOptions {
  exaggeration: number;
  trees: boolean;
  water: boolean;
  shadows: boolean;
}

const SKY = new THREE.Color('#a9c6dc');
const WATER = new THREE.Color('#2e6a8e');
/** Most trees drawn; beyond this, an even sample is shown. The bridge sends no more. */
const MAX_TREES = 120_000;
/** Above this many trees, crowns use fewer triangles: they are a few pixels wide anyway. */
const DENSE_TREES = 30_000;

/** sRGB byte → linear, as three.js expects vertex colours. */
const LINEAR = Float32Array.from({ length: 256 }, (_, v) => {
  const c = v / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
});

export class PreviewScene {
  readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(45, 1, 1, 100_000);
  private readonly controls: OrbitControls;
  private readonly sun = new THREE.DirectionalLight('#fff3df', 2.4);
  private readonly world = new THREE.Group();
  private readonly resize: ResizeObserver;
  private preview: PreviewData | null = null;
  private options: SceneOptions;
  private mapSize = 0;
  private dirty = true;
  private frameCount = 0;
  /** The user has turned or zoomed the view, so a resize keeps their camera. */
  private userMoved = false;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    options: SceneOptions,
  ) {
    this.options = { ...options };
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = 1.05;
    this.renderer.shadowMap.enabled = true;
    this.renderer.shadowMap.type = THREE.PCFShadowMap;

    this.scene.background = SKY;
    this.scene.add(new THREE.HemisphereLight('#dbe9f7', '#5a5040', 1.1));
    this.sun.castShadow = true;
    this.scene.add(this.sun, this.sun.target, this.world);

    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.09;
    this.controls.screenSpacePanning = false;
    this.controls.maxPolarAngle = Math.PI * 0.49;
    this.controls.zoomToCursor = true;
    this.controls.addEventListener('start', () => (this.userMoved = true));

    this.resize = new ResizeObserver(() => this.fit());
    this.resize.observe(canvas.parentElement ?? canvas);
    this.fit();
    this.renderer.setAnimationLoop(() => this.frame());
  }

  /** Frames drawn so far; tests use it to see the view is alive. */
  get frames(): number {
    return this.frameCount;
  }

  dispose(): void {
    this.renderer.setAnimationLoop(null);
    this.resize.disconnect();
    this.controls.dispose();
    this.clear();
    this.renderer.dispose();
  }

  private fit(): void {
    const el = this.canvas.parentElement ?? this.canvas;
    const w = Math.max(1, el.clientWidth);
    const h = Math.max(1, el.clientHeight);
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    // Switching between 3D and Split changes the shape of the view: keep the map in frame.
    if (!this.userMoved) this.resetCamera();
    this.dirty = true;
  }

  private frame(): void {
    const moved = this.controls.update();
    if (moved || this.dirty) {
      this.renderer.render(this.scene, this.camera);
      this.dirty = false;
      this.frameCount++;
    }
  }

  setOptions(options: SceneOptions): void {
    const rebuild = options.exaggeration !== this.options.exaggeration;
    this.options = { ...options };
    if (rebuild && this.preview) {
      this.build(this.preview, false);
    } else {
      this.applyVisibility();
    }
  }

  setPreview(preview: PreviewData): void {
    const first = this.preview === null || preview.size !== this.mapSize;
    this.build(preview, first);
  }

  /** Looks at the whole map from the south-east, as close as fits the view. */
  resetCamera(): void {
    const p = this.preview;
    if (!p) return;
    const size = p.size;
    const half = size / 2;
    let top = 0;
    for (const h of p.heights) top = Math.max(top, h);
    const tall = (top - this.base(p)) * this.options.exaggeration;
    const dir = new THREE.Vector3(0.45, 0.62, 0.65).normalize();
    this.controls.target.set(0, tall / 2, 0);
    this.camera.near = Math.max(0.5, size / 2000);
    this.camera.far = size * 20;

    // The nearest distance at which every corner of the block (peaks included) lands inside the
    // view with a small margin. Perspective makes near corners loom, so it is searched for.
    const corners = [-half, half].flatMap((x) =>
      [-half, half].flatMap((z) => [0, tall].map((y) => new THREE.Vector3(x, y, z))),
    );
    const fits = (distance: number) => {
      this.camera.position.copy(dir).multiplyScalar(distance).add(this.controls.target);
      this.camera.lookAt(this.controls.target);
      this.camera.updateMatrixWorld();
      this.camera.updateProjectionMatrix();
      const v = new THREE.Vector3();
      return corners.every((c) => {
        v.copy(c).project(this.camera);
        return Math.abs(v.x) <= 0.92 && Math.abs(v.y) <= 0.9 && v.z < 1;
      });
    };
    let near = size * 0.3;
    let far = size * 12;
    for (let k = 0; k < 24; k++) {
      const mid = (near + far) / 2;
      if (fits(mid)) far = mid;
      else near = mid;
    }
    const distance = far;
    this.camera.position.copy(dir).multiplyScalar(distance).add(this.controls.target);
    this.camera.far = distance + size * 8;
    this.camera.updateProjectionMatrix();
    this.controls.minDistance = 8;
    this.controls.maxDistance = Math.max(distance * 1.5, size * 3);
    this.controls.update();
    this.userMoved = false;
    this.dirty = true;
  }

  private base(p: PreviewData): number {
    let min = Infinity;
    for (const h of p.heights) min = Math.min(min, h);
    return Math.floor(min) - 2;
  }

  private clear(): void {
    for (const child of [...this.world.children]) {
      this.world.remove(child);
      child.traverse((o) => {
        const mesh = o as THREE.Mesh;
        mesh.geometry?.dispose();
        const mats = Array.isArray(mesh.material) ? mesh.material : mesh.material ? [mesh.material] : [];
        mats.forEach((m) => m.dispose());
      });
    }
  }

  private applyVisibility(): void {
    for (const child of this.world.children) {
      if (child.name === 'water') child.visible = this.options.water;
      if (child.name.startsWith('trees')) child.visible = this.options.trees;
    }
    this.sun.castShadow = this.options.shadows;
    this.renderer.shadowMap.needsUpdate = true;
    this.world.traverse((o) => {
      const m = o as THREE.Mesh;
      if (m.material) (Array.isArray(m.material) ? m.material : [m.material]).forEach((x) => (x.needsUpdate = true));
    });
    this.dirty = true;
  }

  private build(p: PreviewData, resetView: boolean): void {
    this.preview = p;
    this.mapSize = p.size;
    this.clear();
    const ex = this.options.exaggeration;
    const base = this.base(p);
    const half = p.size / 2;
    const n = p.res;
    const xAt = (i: number) => (i + 0.5) * p.step - half;

    // Ground.
    const positions = new Float32Array(n * n * 3);
    const colors = new Float32Array(n * n * 3);
    for (let j = 0; j < n; j++) {
      for (let i = 0; i < n; i++) {
        const k = j * n + i;
        positions.set([xAt(i), (p.heights[k] - base) * ex, xAt(j)], 3 * k);
        colors.set([LINEAR[p.colors[4 * k]], LINEAR[p.colors[4 * k + 1]], LINEAR[p.colors[4 * k + 2]]], 3 * k);
      }
    }
    const index = new Uint32Array((n - 1) * (n - 1) * 6);
    let t = 0;
    for (let j = 0; j < n - 1; j++) {
      for (let i = 0; i < n - 1; i++) {
        const a = j * n + i;
        const b = a + 1;
        const c = a + n;
        const d = c + 1;
        index.set([a, c, b, b, c, d], t);
        t += 6;
      }
    }
    const ground = new THREE.BufferGeometry();
    ground.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    ground.setAttribute('color', new THREE.BufferAttribute(colors, 3));
    ground.setIndex(new THREE.BufferAttribute(index, 1));
    ground.computeVertexNormals();
    const terrain = new THREE.Mesh(
      ground,
      new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.95, metalness: 0 }),
    );
    terrain.name = 'terrain';
    terrain.castShadow = true;
    terrain.receiveShadow = true;
    this.world.add(terrain);
    this.world.add(this.skirt(p, base, ex));

    // Water: each wet sample's level, carried one sample past the shore so the ground clips it.
    const level = new Float32Array(n * n).fill(NaN);
    for (let j = 0; j < n; j++) {
      for (let i = 0; i < n; i++) {
        const k = j * n + i;
        let w = p.water[k];
        if (Number.isNaN(w)) {
          for (let dj = -1; dj <= 1; dj++) {
            for (let di = -1; di <= 1; di++) {
              const ii = i + di;
              const jj = j + dj;
              if (ii < 0 || jj < 0 || ii >= n || jj >= n) continue;
              const v = p.water[jj * n + ii];
              if (!Number.isNaN(v) && !(v <= w)) w = v;
            }
          }
        }
        level[k] = w;
      }
    }
    const wpos = new Float32Array(n * n * 3);
    const windex: number[] = [];
    for (let j = 0; j < n; j++) {
      for (let i = 0; i < n; i++) {
        const k = j * n + i;
        wpos.set([xAt(i), ((Number.isNaN(level[k]) ? base : level[k]) - base) * ex - 0.15, xAt(j)], 3 * k);
      }
    }
    const wet = (k: number) => !Number.isNaN(level[k]);
    for (let j = 0; j < n - 1; j++) {
      for (let i = 0; i < n - 1; i++) {
        const a = j * n + i;
        const b = a + 1;
        const c = a + n;
        const d = c + 1;
        if (wet(a) && wet(b) && wet(c) && wet(d)) windex.push(a, c, b, b, c, d);
      }
    }
    if (windex.length > 0) {
      const wg = new THREE.BufferGeometry();
      wg.setAttribute('position', new THREE.BufferAttribute(wpos, 3));
      wg.setIndex(windex);
      wg.computeVertexNormals();
      const water = new THREE.Mesh(
        wg,
        new THREE.MeshStandardMaterial({
          color: WATER,
          roughness: 0.18,
          metalness: 0.05,
          transparent: true,
          opacity: 0.84,
        }),
      );
      water.name = 'water';
      water.receiveShadow = true;
      this.world.add(water);
    }

    this.addTrees(p, base, ex);

    // Sun from the north-west, its shadow box covering the map.
    const reach = p.size * 0.8;
    this.sun.position.set(-p.size * 0.55, p.size * 0.9, -p.size * 0.35);
    this.sun.target.position.set(0, 0, 0);
    const cam = this.sun.shadow.camera;
    cam.left = -reach;
    cam.right = reach;
    cam.top = reach;
    cam.bottom = -reach;
    cam.near = 1;
    cam.far = p.size * 3;
    cam.updateProjectionMatrix();
    const mapSize = Math.min(4096, this.renderer.capabilities.maxTextureSize);
    if (this.sun.shadow.mapSize.x !== mapSize) {
      this.sun.shadow.mapSize.set(mapSize, mapSize);
      this.sun.shadow.map?.dispose();
      this.sun.shadow.map = null;
    }
    this.sun.shadow.bias = -0.0004;
    this.sun.shadow.normalBias = p.step * 0.6;

    this.scene.fog = new THREE.Fog(SKY, p.size * 3, p.size * 9);
    if (resetView) this.resetCamera();
    this.applyVisibility();
  }

  /** Walls down from the map edge, so the map reads as a solid block of land. */
  private skirt(p: PreviewData, base: number, ex: number): THREE.Mesh {
    const n = p.res;
    const half = p.size / 2;
    const bottom = -6 * ex - 4;
    const edge: [number, number][] = [];
    for (let i = 0; i < n; i++) edge.push([i, 0]);
    for (let j = 0; j < n; j++) edge.push([n - 1, j]);
    for (let i = n - 1; i >= 0; i--) edge.push([i, n - 1]);
    for (let j = n - 1; j >= 0; j--) edge.push([0, j]);
    const pos: number[] = [];
    const col: number[] = [];
    const top = new THREE.Color('#7a6248');
    const low = new THREE.Color('#57524a');
    for (const [i, j] of edge) {
      // Push the wall's corners out to the map border.
      const x = i === 0 ? -half : i === n - 1 ? half : (i + 0.5) * p.step - half;
      const z = j === 0 ? -half : j === n - 1 ? half : (j + 0.5) * p.step - half;
      const y = (p.heights[j * n + i] - base) * ex;
      pos.push(x, y, z, x, bottom, z);
      col.push(top.r, top.g, top.b, low.r, low.g, low.b);
    }
    const index: number[] = [];
    for (let e = 0; e < edge.length - 1; e++) {
      const a = 2 * e;
      index.push(a, a + 1, a + 2, a + 2, a + 1, a + 3);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
    g.setIndex(index);
    g.computeVertexNormals();
    const mesh = new THREE.Mesh(
      g,
      new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 1, side: THREE.DoubleSide }),
    );
    mesh.name = 'skirt';
    return mesh;
  }

  private addTrees(p: PreviewData, base: number, ex: number): void {
    if (p.treeCount === 0) return;
    const drawn = Math.min(p.treeCount, MAX_TREES);
    const pick = (k: number) => Math.floor((k * p.treeCount) / drawn);
    const dense = drawn > DENSE_TREES;
    const half = p.size / 2;
    const trunkGeo = new THREE.CylinderGeometry(0.35, 0.5, 1, dense ? 4 : 6, 1, dense).translate(0, 0.5, 0);
    const roundGeo = new THREE.IcosahedronGeometry(1, dense ? 0 : 1);
    const coneGeo = new THREE.ConeGeometry(1, 1, dense ? 5 : 7, 1, dense).translate(0, 0.5, 0);
    const count = { trunk: 0, round: 0, cone: 0 };
    for (let k = 0; k < drawn; k++) {
      const t = pick(k);
      const style = speciesStyle(p.species[p.trees[6 * t + 5]] ?? '');
      if (style.crown !== 'bush') count.trunk++;
      if (style.crown === 'cone') count.cone++;
      else if (style.crown !== 'none') count.round++;
    }
    const mat = () => new THREE.MeshStandardMaterial({ roughness: 0.9, metalness: 0 });
    const trunks = new THREE.InstancedMesh(trunkGeo, mat(), Math.max(1, count.trunk));
    const rounds = new THREE.InstancedMesh(roundGeo, mat(), Math.max(1, count.round));
    const cones = new THREE.InstancedMesh(coneGeo, mat(), Math.max(1, count.cone));
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const pos = new THREE.Vector3();
    const scale = new THREE.Vector3();
    const c = new THREE.Color();
    const at = { trunk: 0, round: 0, cone: 0 };
    for (let k = 0; k < drawn; k++) {
      const o = 6 * pick(k);
      const x = p.trees[o] - half;
      const y = (p.trees[o + 1] - base) * ex;
      const z = p.trees[o + 2] - half;
      const h = Math.max(1.5, p.trees[o + 3]);
      const r = Math.max(0.8, p.trees[o + 4]);
      const style = speciesStyle(p.species[p.trees[o + 5]] ?? '');
      const trunkR = Math.max(0.45, h * 0.035);
      const trunkTop =
        style.crown === 'cone' ? h * 0.35 : style.crown === 'none' ? h : style.crown === 'droop' ? h * 0.6 : h * 0.62;
      if (style.crown !== 'bush') {
        m.compose(pos.set(x, y, z), q, scale.set(trunkR, trunkTop, trunkR));
        trunks.setMatrixAt(at.trunk, m);
        trunks.setColorAt(at.trunk++, c.set(style.bark));
      }
      if (style.crown === 'cone') {
        m.compose(pos.set(x, y + h * 0.22, z), q, scale.set(r, h * 0.8, r));
        cones.setMatrixAt(at.cone, m);
        cones.setColorAt(at.cone++, c.set(style.leaves));
      } else if (style.crown !== 'none') {
        const [sy, cy] =
          style.crown === 'bush' ? [0.7, r * 0.55] : style.crown === 'droop' ? [0.75, h - r * 0.55] : [0.82, h - r * 0.8];
        m.compose(pos.set(x, y + cy, z), q, scale.set(r, r * sy, r));
        rounds.setMatrixAt(at.round, m);
        rounds.setColorAt(at.round++, c.set(style.leaves));
      }
    }
    for (const [mesh, used, name] of [
      [trunks, at.trunk, 'trees-trunks'],
      [rounds, at.round, 'trees-crowns'],
      [cones, at.cone, 'trees-cones'],
    ] as const) {
      mesh.count = used;
      mesh.name = name;
      mesh.castShadow = true;
      mesh.receiveShadow = true;
      mesh.instanceMatrix.needsUpdate = true;
      if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
      mesh.computeBoundingSphere();
      this.world.add(mesh);
    }
  }
}
