<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { app } from '../lib/app.svelte';
  import { WORLD_AREAS, world, type Square } from '../lib/world.svelte';

  let wrap: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  /** The map of saved chunks at one pixel per map sample, scaled when drawn. */
  let picture: HTMLCanvasElement | null = null;

  // View: the world point at the canvas centre and pixels per block.
  let cx = 0;
  let cz = 0;
  let scale = 0.25;
  let width = 0;
  let height = 0;
  /** Panned or zoomed by hand since the last fit; otherwise the map arriving re-fits it. */
  let userMoved = false;
  /** The world and dimension the view was last fitted to. */
  let fittedTo = '';
  let frame = 0;
  let cursorStyle = $state('crosshair');

  const MIN_SCALE = 1 / 256;
  const MAX_SCALE = 16;
  const SQUARE = '#e2b85c';

  type Drag =
    | { type: 'square'; dx: number; dz: number }
    /** Panning, or a click that moves the square once it ends without moving. */
    | { type: 'pan'; sx: number; sy: number; cx: number; cz: number; moved: boolean; click: boolean };
  let drag: Drag | null = null;

  const toScreen = (x: number, z: number): [number, number] => [
    (x - cx) * scale + width / 2,
    (z - cz) * scale + height / 2,
  ];
  const toWorld = (sx: number, sy: number): [number, number] => [
    (sx - width / 2) / scale + cx,
    (sy - height / 2) / scale + cz,
  ];

  function local(e: MouseEvent): [number, number] {
    const r = canvas.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  const inside = (x: number, z: number, a: Square) => x >= a.x0 && z >= a.z0 && x < a.x0 + a.size && z < a.z0 + a.size;

  /** Saved chunks and the square together. */
  function extent(): [number, number, number, number] {
    const a = world.area;
    let [x0, z0, x1, z1] = [a.x0, a.z0, a.x0 + a.size, a.z0 + a.size];
    const m = world.map;
    const b: [number, number, number, number] | null = m
      ? [m.x0, m.z0, m.x0 + m.width * m.scale, m.z0 + m.height * m.scale]
      : (world.info?.bounds ?? null);
    if (b) {
      x0 = Math.min(x0, b[0]);
      z0 = Math.min(z0, b[1]);
      x1 = Math.max(x1, b[2]);
      z1 = Math.max(z1, b[3]);
    }
    return [x0, z0, x1, z1];
  }

  /** Shows every saved chunk. */
  function fit() {
    if (width <= 0) return;
    const [x0, z0, x1, z1] = extent();
    const w = Math.max(64, x1 - x0);
    const h = Math.max(64, z1 - z0);
    scale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, Math.min(width / w, height / h) * 0.92));
    cx = (x0 + x1) / 2;
    cz = (z0 + z1) / 2;
    userMoved = false;
    redraw();
  }

  /** Brings the square into view when it was moved somewhere off screen. */
  function follow() {
    if (width <= 0 || drag) return;
    const a = world.area;
    const [sx0, sy0] = toScreen(a.x0, a.z0);
    const [sx1, sy1] = toScreen(a.x0 + a.size, a.z0 + a.size);
    if (sx1 < 0 || sy1 < 0 || sx0 > width || sy0 > height) {
      cx = a.x0 + a.size / 2;
      cz = a.z0 + a.size / 2;
      if (a.size * scale > Math.min(width, height)) scale = (Math.min(width, height) * 0.6) / a.size;
    }
  }

  // ---- Drawing --------------------------------------------------------------------------

  function redraw() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      draw();
    });
  }

  function marker(c: CanvasRenderingContext2D, x: number, z: number, label: string, shape: 'dot' | 'diamond') {
    const [sx, sy] = toScreen(x + 0.5, z + 0.5);
    if (sx < -40 || sy < -40 || sx > width + 40 || sy > height + 40) return;
    c.beginPath();
    if (shape === 'dot') c.arc(sx, sy, 5, 0, Math.PI * 2);
    else {
      c.moveTo(sx, sy - 7);
      c.lineTo(sx + 6, sy);
      c.lineTo(sx, sy + 7);
      c.lineTo(sx - 6, sy);
      c.closePath();
    }
    c.fillStyle = shape === 'dot' ? '#ffffff' : '#7fd0ff';
    c.strokeStyle = 'rgba(0, 0, 0, 0.7)';
    c.lineWidth = 2;
    c.stroke();
    c.fill();
    c.font = '600 11px "Segoe UI", system-ui, sans-serif';
    c.textAlign = 'left';
    c.textBaseline = 'middle';
    c.lineWidth = 3;
    c.strokeText(label, sx + 9, sy);
    c.fillStyle = '#ffffff';
    c.fillText(label, sx + 9, sy);
  }

  function draw() {
    if (!ctx) return;
    const c = ctx;
    const dpr = window.devicePixelRatio || 1;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    c.fillStyle = '#0f1214';
    c.fillRect(0, 0, width, height);

    const m = world.map;
    if (picture && m) {
      const [x0, y0] = toScreen(m.x0, m.z0);
      const px = m.scale * scale;
      // Smooth when zoomed out, crisp samples when zoomed in.
      c.imageSmoothingEnabled = px < 3;
      c.imageSmoothingQuality = 'high';
      c.drawImage(picture, x0, y0, m.width * px, m.height * px);
    }

    if (app.ui.mapGrid) {
      // Region files every 512 blocks; chunks every 16 when there's room to see them.
      const [wx0, wz0] = toWorld(0, 0);
      const [wx1, wz1] = toWorld(width, height);
      c.lineWidth = 1;
      for (const [every, alpha] of [
        [16, 0.1],
        [512, 0.28],
      ] as const) {
        if (every * scale < 10) continue;
        c.strokeStyle = `rgba(255,255,255,${alpha})`;
        c.beginPath();
        for (let v = Math.ceil(wx0 / every) * every; v <= wx1; v += every) {
          const x = Math.round(toScreen(v, 0)[0]) + 0.5;
          c.moveTo(x, 0);
          c.lineTo(x, height);
        }
        for (let v = Math.ceil(wz0 / every) * every; v <= wz1; v += every) {
          const y = Math.round(toScreen(0, v)[1]) + 0.5;
          c.moveTo(0, y);
          c.lineTo(width, y);
        }
        c.stroke();
      }
    }

    // The square: the rest of the map a little darker, so it stands out.
    const a = world.area;
    const [sx, sy] = toScreen(a.x0, a.z0);
    const side = a.size * scale;
    c.fillStyle = 'rgba(8, 10, 12, 0.32)';
    c.beginPath();
    c.rect(0, 0, width, height);
    c.rect(sx, sy, side, side);
    c.fill('evenodd');
    c.lineWidth = 3;
    c.strokeStyle = 'rgba(0, 0, 0, 0.55)';
    c.strokeRect(sx, sy, side, side);
    c.lineWidth = 1.5;
    c.strokeStyle = SQUARE;
    if (world.areaBusy) c.setLineDash([6, 4]);
    c.strokeRect(sx, sy, side, side);
    c.setLineDash([]);
    if (side < 14) {
      // Too small to see: a cross where it is.
      const [mx, my] = [sx + side / 2, sy + side / 2];
      c.beginPath();
      c.moveTo(mx - 10, my);
      c.lineTo(mx + 10, my);
      c.moveTo(mx, my - 10);
      c.lineTo(mx, my + 10);
      c.stroke();
    }
    c.font = '600 11px "Segoe UI", system-ui, sans-serif';
    c.textAlign = 'left';
    c.textBaseline = 'bottom';
    const label = `${a.size} × ${a.size}`;
    c.lineWidth = 3;
    c.strokeStyle = 'rgba(0, 0, 0, 0.7)';
    c.strokeText(label, sx, sy - 4);
    c.fillStyle = SQUARE;
    c.fillText(label, sx, sy - 4);

    const info = world.info;
    if (info) {
      marker(c, info.spawn[0], info.spawn[2], 'Spawn', 'diamond');
      if (info.player) marker(c, info.player[0], info.player[2], 'Player', 'dot');
    }

    // Compass and scale.
    c.fillStyle = 'rgba(255,255,255,0.7)';
    c.font = '600 11px "Segoe UI", system-ui, sans-serif';
    c.textAlign = 'center';
    c.textBaseline = 'alphabetic';
    c.fillText('N', width - 22, 22);
    c.beginPath();
    c.moveTo(width - 22, 28);
    c.lineTo(width - 26, 38);
    c.lineTo(width - 18, 38);
    c.fill();
    const nice = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384].find((b) => b * scale >= 70) ?? 32768;
    const bar = nice * scale;
    c.fillRect(width - 16 - bar, height - 18, bar, 2);
    c.textAlign = 'right';
    c.fillText(`${nice} blocks`, width - 16, height - 24);
  }

  // ---- Mouse ----------------------------------------------------------------------------

  /** Ground under the mouse, from the 3D preview's samples when the mouse is over its square. */
  function updateCursor(x: number, z: number) {
    const bx = Math.floor(x);
    const bz = Math.floor(z);
    const p = app.preview;
    const s = world.loaded;
    let y: number | null = null;
    let water: number | null = null;
    let biome: string | null = null;
    if (p && s && inside(bx, bz, s)) {
      const i = Math.min(p.res - 1, Math.floor((bx - s.x0) / p.step));
      const j = Math.min(p.res - 1, Math.floor((bz - s.z0) / p.step));
      const k = j * p.res + i;
      const id = p.biomeIds[p.biomes[k]] ?? null;
      if (id !== 'unexplored') {
        y = p.heights[k];
        water = Number.isNaN(p.water[k]) ? null : p.water[k];
        biome = id;
      }
    }
    world.cursor = { x: bx, z: bz, y, water, biome };
  }

  function onPointerDown(e: PointerEvent) {
    canvas.focus();
    const [sx, sy] = local(e);
    const [wx, wz] = toWorld(sx, sy);
    canvas.setPointerCapture(e.pointerId);
    const a = world.area;
    if (e.button === 0 && inside(wx, wz, a) && a.size * scale >= 14) {
      drag = { type: 'square', dx: wx - a.x0, dz: wz - a.z0 };
    } else {
      drag = { type: 'pan', sx, sy, cx, cz, moved: false, click: e.button === 0 };
    }
    cursorStyle = 'grabbing';
  }

  function onPointerMove(e: PointerEvent) {
    const [sx, sy] = local(e);
    const [wx, wz] = toWorld(sx, sy);
    updateCursor(wx, wz);
    if (!drag) {
      cursorStyle = inside(wx, wz, world.area) && world.area.size * scale >= 14 ? 'move' : 'crosshair';
      return;
    }
    if (drag.type === 'square') {
      world.moveTo(wx - drag.dx, wz - drag.dz);
      redraw();
      return;
    }
    if (!drag.moved && Math.hypot(sx - drag.sx, sy - drag.sy) < 4) return;
    drag.moved = true;
    cx = drag.cx - (sx - drag.sx) / scale;
    cz = drag.cz - (sy - drag.sy) / scale;
    userMoved = true;
    redraw();
  }

  function onPointerUp(e: PointerEvent) {
    const d = drag;
    drag = null;
    if (!d) return;
    const [wx, wz] = toWorld(...local(e));
    if (d.type === 'pan' && d.click && !d.moved) world.centerOn(wx, wz);
    cursorStyle = inside(wx, wz, world.area) ? 'move' : 'crosshair';
    redraw();
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const [sx, sy] = local(e);
    const [wx, wz] = toWorld(sx, sy);
    scale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale * Math.exp(-e.deltaY * 0.0015)));
    // Keep the point under the mouse in place.
    cx = wx - (sx - width / 2) / scale;
    cz = wz - (sy - height / 2) / scale;
    userMoved = true;
    redraw();
  }

  function goTo(at: [number, number, number] | null) {
    if (!at) return;
    world.centerOn(at[0], at[2]);
  }

  onMount(() => {
    ctx = canvas.getContext('2d');
    const observer = new ResizeObserver(() => {
      const dpr = window.devicePixelRatio || 1;
      width = wrap.clientWidth;
      height = wrap.clientHeight;
      canvas.width = Math.max(1, Math.round(width * dpr));
      canvas.height = Math.max(1, Math.round(height * dpr));
      if (width > 0 && !userMoved) fit();
      else redraw();
    });
    observer.observe(wrap);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
      world.cursor = null;
    };
  });

  // Another world or dimension: show all of it.
  $effect(() => {
    const info = world.info;
    const key = info ? `${info.path}|${info.dimension}` : '';
    if (key !== fittedTo) {
      fittedTo = key;
      userMoved = false;
      untrack(fit);
    }
  });

  // The map arrived: draw it, and fit the view to it unless it was moved by hand.
  $effect(() => {
    const m = world.map;
    if (!m) {
      picture = null;
    } else {
      const c = document.createElement('canvas');
      c.width = m.width;
      c.height = m.height;
      c.getContext('2d')!.putImageData(new ImageData(new Uint8ClampedArray(m.rgba), m.width, m.height), 0, 0);
      picture = c;
    }
    if (m && !userMoved) untrack(fit);
    else redraw();
  });

  // The square moved or anything else drawn changed.
  $effect(() => {
    void world.area.x0;
    void world.area.z0;
    void world.area.size;
    void world.areaBusy;
    void app.ui.mapGrid;
    follow();
    redraw();
  });
</script>

<div class="worldmap" data-testid="view-world-map">
  <div class="tools">
    <span class="label muted">Area</span>
    <span class="segmented" data-testid="world-area-sizes" title="Side of the square the 3D view and render mode show, in blocks">
      {#each WORLD_AREAS as s (s)}
        <button class:on={world.area.size === s} onclick={() => world.setSize(s)}>{s}</button>
      {/each}
    </span>
    <span class="spacer"></span>
    <button
      class="btn small"
      disabled={!world.info?.player}
      onclick={() => goTo(world.info?.player ?? null)}
      title={world.info?.player ? 'Move the square to where the player was' : 'The player is not in this dimension'}
      >Player</button
    >
    <button class="btn small" onclick={() => goTo(world.info?.spawn ?? null)} title="Move the square to the world spawn">Spawn</button>
    <label class="check"><input type="checkbox" bind:checked={app.ui.mapGrid} /> Grid</label>
    <button class="btn small" onclick={fit} title="Show every saved chunk">Fit</button>
  </div>

  <div class="stage" bind:this={wrap}>
    <canvas
      bind:this={canvas}
      tabindex="0"
      data-testid="world-map-canvas"
      style:cursor={cursorStyle}
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={onPointerUp}
      onpointerleave={() => (world.cursor = null)}
      onwheel={onWheel}
      oncontextmenu={(e) => e.preventDefault()}
    ></canvas>

    {#if world.mapBusy}
      <div class="overlay">
        <div class="card" data-testid="world-map-busy">
          <div><span class="spinner"></span> Drawing the map of the saved chunks…</div>
          <div class="bar"><span style:width="{Math.round(world.mapProgress * 100)}%"></span></div>
        </div>
      </div>
    {:else if world.mapError}
      <div class="overlay"><div class="card error-text">{world.mapError}</div></div>
    {/if}

    <div class="hint">
      Drag the square to choose what the 3D view shows · click to move it there · drag the map to pan · wheel to zoom
    </div>
  </div>
</div>

<style>
  .worldmap {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    background: #0f1214;
  }

  .tools {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
    flex-wrap: wrap;
  }

  .label {
    margin: 0 2px 0 4px;
  }

  .spacer {
    flex: 1;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    margin: 0 4px;
  }

  .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
    outline: none;
    touch-action: none;
  }

  .overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    pointer-events: none;
  }

  .card {
    min-width: 260px;
    max-width: 380px;
    padding: 14px 16px;
    border-radius: 10px;
    background: rgba(18, 21, 24, 0.9);
    border: 1px solid var(--line);
  }

  .card .spinner {
    vertical-align: -2px;
    margin-right: 4px;
  }

  .bar {
    height: 4px;
    margin-top: 10px;
    border-radius: 2px;
    background: var(--line);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.2s;
  }

  .hint {
    position: absolute;
    left: 10px;
    right: 150px;
    bottom: 8px;
    font-size: 11px;
    color: var(--muted);
    pointer-events: none;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
