<script lang="ts">
  import { onMount } from 'svelte';
  import { app, type Tool } from '../lib/app.svelte';
  import {
    KINDS,
    defaultRadius,
    hitTest,
    isLine,
    kindInfo,
    moved,
    newCircle,
    newLine,
    rounded,
    type Hit,
  } from '../lib/edits';
  import { biomeColor } from '../lib/palette';
  import { shadeMap } from '../lib/shade';
  import type { EditKind, Point, ShapeEdit } from '../lib/types';
  import Inspector from './Inspector.svelte';

  let wrap: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  /** The shaded preview at preview resolution, scaled up when drawn. */
  let image: HTMLCanvasElement | null = null;

  // View: the world point at the canvas centre and pixels per block.
  let cx = 512;
  let cz = 512;
  let scale = 0.5;
  let width = 0;
  let height = 0;
  let fittedSize = 0;
  /** Panned or zoomed by hand since the last fit; otherwise resizing re-fits the map. */
  let userMoved = false;

  type Drag =
    | { type: 'pan'; sx: number; sy: number; cx: number; cz: number }
    | { type: 'move'; index: number; x: number; z: number; original: ShapeEdit }
    | { type: 'radius'; index: number }
    | { type: 'vertex'; index: number; vertex: number }
    | { type: 'create'; kind: EditKind; x: number; z: number; r: number };
  let drag: Drag | null = null;
  /** River or ridge points placed so far. */
  let draft: { kind: 'river' | 'ridge'; points: Point[] } | null = null;
  let hover: Hit | null = null;
  let mouse: Point | null = null;
  let spaceDown = false;
  let cursorStyle = $state('crosshair');
  let drafting = $state(false);
  let frame = 0;

  const tools: { tool: Tool; label: string; key: string }[] = [
    { tool: 'select', label: 'Select', key: 'V' },
    ...KINDS.map((k) => ({ tool: k.kind as Tool, label: k.label, key: k.key })),
  ];
  const zoneBiomes = $derived(app.info?.biomes.filter((b) => !b.feature) ?? []);
  const toolHelp = $derived(
    app.tool === 'select'
      ? 'Click a landform to edit it, drag to move it. Drag empty ground to pan, wheel to zoom.'
      : kindInfo(app.tool).help,
  );

  // ---- Coordinates ----------------------------------------------------------------------

  const toScreen = (x: number, z: number): Point => [(x - cx) * scale + width / 2, (z - cz) * scale + height / 2];
  const toWorld = (sx: number, sy: number): Point => [(sx - width / 2) / scale + cx, (sy - height / 2) / scale + cz];

  function local(e: MouseEvent): Point {
    const r = canvas.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  function fit() {
    const size = app.settings.size;
    scale = Math.max(0.01, (Math.min(width, height) * 0.92) / size);
    cx = size / 2;
    cz = size / 2;
    fittedSize = size;
    userMoved = false;
    redraw();
  }

  function minScale() {
    return (Math.min(width, height) * 0.25) / app.settings.size;
  }

  // ---- Drawing --------------------------------------------------------------------------

  function redraw() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      draw();
    });
  }

  function rgba(hex: string, a: number): string {
    const n = parseInt(hex.slice(1), 16);
    return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
  }

  function drawEdit(c: CanvasRenderingContext2D, e: ShapeEdit, selected: boolean, hovered: boolean) {
    const k = kindInfo(e.kind);
    const color = e.kind === 'biome_zone' ? biomeColor(e.biome) : k.color;
    c.save();
    if (isLine(e)) {
      const pts = e.points.map(([x, z]) => toScreen(x, z));
      const path = () => {
        c.beginPath();
        pts.forEach(([x, y], i) => (i === 0 ? c.moveTo(x, y) : c.lineTo(x, y)));
      };
      c.lineJoin = 'round';
      c.lineCap = 'round';
      path();
      c.strokeStyle = rgba(k.color, e.kind === 'river' ? 0.55 : 0.3);
      c.lineWidth = Math.max(e.kind === 'river' ? 3 : 6, e.width * scale);
      c.stroke();
      path();
      c.strokeStyle = selected || hovered ? '#ffffff' : rgba(k.color, 1);
      c.lineWidth = selected ? 2.5 : 1.5;
      if (e.kind === 'ridge') c.setLineDash([6, 4]);
      c.stroke();
      c.setLineDash([]);
      // Flow direction at the river's mouth.
      if (e.kind === 'river' && pts.length >= 2) {
        const [x1, y1] = pts[pts.length - 2];
        const [x2, y2] = pts[pts.length - 1];
        const a = Math.atan2(y2 - y1, x2 - x1);
        c.fillStyle = selected || hovered ? '#ffffff' : k.color;
        c.beginPath();
        c.moveTo(x2 + 9 * Math.cos(a), y2 + 9 * Math.sin(a));
        c.lineTo(x2 + 9 * Math.cos(a + 2.5), y2 + 9 * Math.sin(a + 2.5));
        c.lineTo(x2 + 9 * Math.cos(a - 2.5), y2 + 9 * Math.sin(a - 2.5));
        c.fill();
      }
      if (selected) {
        for (const [x, y] of pts) handle(c, x, y, 'square');
      }
    } else {
      const [x, y] = toScreen(e.x, e.z);
      const r = Math.max(2, e.radius * scale);
      c.beginPath();
      c.arc(x, y, r, 0, Math.PI * 2);
      c.fillStyle = rgba(color.startsWith('#') ? color : '#c58fd6', selected ? 0.24 : 0.14);
      c.fill();
      c.strokeStyle = selected || hovered ? '#ffffff' : color;
      c.lineWidth = selected ? 2.5 : 1.5;
      if (e.kind === 'biome_zone' || e.kind === 'plateau') c.setLineDash([7, 5]);
      c.stroke();
      c.setLineDash([]);
      if (r > 26) {
        c.font = '600 12px "Segoe UI", system-ui, sans-serif';
        c.textAlign = 'center';
        c.textBaseline = 'middle';
        const label =
          e.kind === 'biome_zone'
            ? app.biomeName(e.biome)
            : e.kind === 'plateau'
              ? `Plateau Y${e.level}`
              : e.kind === 'lake'
                ? `Lake −${e.depth}`
                : `${k.label} ${e.height >= 0 ? '+' : ''}${e.height}`;
        // Below the centre handle.
        const ly = y + (selected ? 16 : 0);
        c.lineWidth = 3;
        c.strokeStyle = 'rgba(0,0,0,0.55)';
        c.strokeText(label, x, ly);
        c.fillStyle = '#ffffff';
        c.fillText(label, x, ly);
      }
      if (selected) {
        handle(c, x, y, 'square');
        handle(c, x + r, y, 'round');
      }
    }
    c.restore();
  }

  function handle(c: CanvasRenderingContext2D, x: number, y: number, shape: 'square' | 'round') {
    c.fillStyle = '#ffffff';
    c.strokeStyle = '#1a1e22';
    c.lineWidth = 1.5;
    c.beginPath();
    if (shape === 'square') c.rect(x - 4.5, y - 4.5, 9, 9);
    else c.arc(x, y, 5.5, 0, Math.PI * 2);
    c.fill();
    c.stroke();
  }

  function draw() {
    if (!ctx) return;
    const c = ctx;
    const dpr = window.devicePixelRatio || 1;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    c.fillStyle = '#0f1214';
    c.fillRect(0, 0, width, height);
    const size = app.settings.size;
    const [x0, y0] = toScreen(0, 0);
    const side = size * scale;

    if (image && app.preview && app.preview.size === size) {
      // Smooth when zoomed out, crisp samples when zoomed in.
      c.imageSmoothingEnabled = scale * app.preview.step < 3;
      c.imageSmoothingQuality = 'high';
      c.drawImage(image, x0, y0, side, side);
    } else {
      c.fillStyle = '#1d2226';
      c.fillRect(x0, y0, side, side);
    }

    if (app.ui.mapGrid) {
      c.lineWidth = 1;
      // Region files every 512 blocks; chunks every 16 when there's room to see them.
      for (const [every, alpha] of [
        [16, 0.12],
        [512, 0.32],
      ] as const) {
        if (every * scale < 9) continue;
        c.strokeStyle = `rgba(255,255,255,${alpha})`;
        c.beginPath();
        for (let v = every; v < size; v += every) {
          const [sx] = toScreen(v, 0);
          const [, sy] = toScreen(0, v);
          c.moveTo(Math.round(sx) + 0.5, y0);
          c.lineTo(Math.round(sx) + 0.5, y0 + side);
          c.moveTo(x0, Math.round(sy) + 0.5);
          c.lineTo(x0 + side, Math.round(sy) + 0.5);
        }
        c.stroke();
      }
    }
    c.strokeStyle = 'rgba(255,255,255,0.55)';
    c.lineWidth = 1;
    c.strokeRect(Math.round(x0) + 0.5, Math.round(y0) + 0.5, Math.round(side), Math.round(side));

    const edits = app.settings.edits;
    edits.forEach((e, i) => drawEdit(c, e, i === app.selected, hover?.index === i));

    // What is being drawn right now.
    if (drag?.type === 'create') {
      const r = Math.max(drag.r, 1);
      const preview = newCircle(drag.kind as Exclude<EditKind, 'river' | 'ridge'>, drag.x, drag.z, r, placeContext(drag.x, drag.z));
      drawEdit(c, preview, true, false);
    }
    if (draft) {
      const pts = mouse ? [...draft.points, mouse] : draft.points;
      if (pts.length >= 2) drawEdit(c, newLine(draft.kind, pts, size), true, false);
      else if (pts.length === 1) {
        const [x, y] = toScreen(pts[0][0], pts[0][1]);
        handle(c, x, y, 'square');
      }
    }

    // Compass and scale.
    c.fillStyle = 'rgba(255,255,255,0.7)';
    c.font = '600 11px "Segoe UI", system-ui, sans-serif';
    c.textAlign = 'center';
    c.fillText('N', width - 22, 22);
    c.beginPath();
    c.moveTo(width - 22, 28);
    c.lineTo(width - 26, 38);
    c.lineTo(width - 18, 38);
    c.fill();
    const nice = [16, 32, 64, 128, 256, 512, 1024, 2048].find((b) => b * scale >= 70) ?? 4096;
    const bar = nice * scale;
    c.fillRect(width - 16 - bar, height - 18, bar, 2);
    c.textAlign = 'right';
    c.fillText(`${nice} blocks`, width - 16, height - 24);
  }

  // ---- Interaction ----------------------------------------------------------------------

  function placeContext(x: number, z: number) {
    const p = app.preview;
    let ground: number | null = null;
    if (p) {
      const i = Math.min(p.res - 1, Math.max(0, Math.floor(x / p.step)));
      const j = Math.min(p.res - 1, Math.max(0, Math.floor(z / p.step)));
      ground = p.heights[j * p.res + i];
    }
    return { size: app.settings.size, ground, seaLevel: app.settings.sea_level, biome: app.zoneBiome };
  }

  function updateCursor(x: number, z: number) {
    const p = app.preview;
    const size = app.settings.size;
    if (!p || x < 0 || z < 0 || x >= size || z >= size || p.size !== size) {
      app.cursor = null;
      return;
    }
    const i = Math.min(p.res - 1, Math.floor(x / p.step));
    const j = Math.min(p.res - 1, Math.floor(z / p.step));
    const k = j * p.res + i;
    const w = p.water[k];
    app.cursor = {
      x: Math.floor(x),
      z: Math.floor(z),
      height: p.heights[k],
      water: Number.isNaN(w) ? null : w,
      biome: p.biomeIds[p.biomes[k]] ?? '',
    };
  }

  function setCursorStyle() {
    if (drag?.type === 'pan') cursorStyle = 'grabbing';
    else if (spaceDown) cursorStyle = 'grab';
    else if (app.tool !== 'select') cursorStyle = 'crosshair';
    else if (hover?.part.kind === 'radius') cursorStyle = 'ew-resize';
    else if (hover) cursorStyle = 'move';
    else cursorStyle = 'default';
  }

  function onPointerDown(e: PointerEvent) {
    canvas.focus();
    const [sx, sy] = local(e);
    const [x, z] = toWorld(sx, sy);
    canvas.setPointerCapture(e.pointerId);
    if (e.button === 1 || e.button === 2 || spaceDown) {
      drag = { type: 'pan', sx, sy, cx, cz };
    } else if (e.button !== 0) {
      return;
    } else if (app.tool === 'select') {
      const hit = hitTest(app.settings.edits, x, z, 8 / scale, app.selected);
      if (!hit) {
        app.selected = null;
        drag = { type: 'pan', sx, sy, cx, cz };
      } else {
        app.selected = hit.index;
        const edit = app.settings.edits[hit.index];
        if (hit.part.kind === 'vertex') {
          if (e.altKey) app.removePoint(hit.index, hit.part.index);
          else drag = { type: 'vertex', index: hit.index, vertex: hit.part.index };
        } else if (hit.part.kind === 'radius') {
          drag = { type: 'radius', index: hit.index };
        } else {
          drag = { type: 'move', index: hit.index, x, z, original: $state.snapshot(edit) as ShapeEdit };
        }
      }
    } else if (app.tool === 'river' || app.tool === 'ridge') {
      if (!draft || draft.kind !== app.tool) draft = { kind: app.tool, points: [] };
      const last = draft.points[draft.points.length - 1];
      // The second click of a double-click lands on the same spot; don't add it twice.
      if (!last || Math.hypot(last[0] - x, last[1] - z) * scale > 4) draft.points.push([x, z]);
      drafting = true;
    } else {
      drag = { type: 'create', kind: app.tool, x, z, r: 0 };
    }
    setCursorStyle();
    redraw();
  }

  function onPointerMove(e: PointerEvent) {
    const [sx, sy] = local(e);
    const [x, z] = toWorld(sx, sy);
    mouse = [x, z];
    updateCursor(x, z);
    const edits = app.settings.edits;
    switch (drag?.type) {
      case 'pan':
        cx = drag.cx - (sx - drag.sx) / scale;
        cz = drag.cz - (sy - drag.sy) / scale;
        userMoved = true;
        break;
      case 'move':
        edits[drag.index] = moved(drag.original, x - drag.x, z - drag.z);
        break;
      case 'radius': {
        const edit = edits[drag.index];
        if (!isLine(edit)) edit.radius = Math.max(4, Math.hypot(x - edit.x, z - edit.z));
        break;
      }
      case 'vertex': {
        const edit = edits[drag.index];
        if (isLine(edit)) edit.points[drag.vertex] = [x, z];
        break;
      }
      case 'create':
        drag.r = Math.hypot(x - drag.x, z - drag.z);
        break;
      default:
        hover = app.tool === 'select' ? hitTest(edits, x, z, 8 / scale, app.selected) : null;
    }
    setCursorStyle();
    redraw();
  }

  function onPointerUp(e: PointerEvent) {
    if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
    const d = drag;
    drag = null;
    if (!d) return;
    if (d.type === 'create') {
      const kind = d.kind as Exclude<EditKind, 'river' | 'ridge'>;
      // A plain click places a landform of a sensible size.
      const r = d.r * scale < 6 ? defaultRadius(kind, app.settings.size) : d.r;
      app.addEdit(newCircle(kind, d.x, d.z, r, placeContext(d.x, d.z)));
      if (!e.shiftKey) app.tool = 'select';
    } else if (d.type !== 'pan') {
      const edits = app.settings.edits;
      edits[d.index] = rounded($state.snapshot(edits[d.index]) as ShapeEdit);
      app.commit();
    }
    setCursorStyle();
    redraw();
  }

  function finishDraft() {
    if (draft && draft.points.length >= 2) {
      app.addEdit(newLine(draft.kind, draft.points, app.settings.size));
      app.tool = 'select';
    }
    draft = null;
    drafting = false;
    redraw();
  }

  function cancelDraft() {
    draft = null;
    drafting = false;
    redraw();
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const [sx, sy] = local(e);
    const [wx, wz] = toWorld(sx, sy);
    scale = Math.min(24, Math.max(minScale(), scale * Math.exp(-e.deltaY * 0.0015)));
    // Keep the point under the mouse in place.
    cx = wx - (sx - width / 2) / scale;
    cz = wz - (sy - height / 2) / scale;
    userMoved = true;
    redraw();
  }

  function typing(e: KeyboardEvent): boolean {
    const t = e.target as HTMLElement | null;
    return !!t && (t.tagName === 'INPUT' || t.tagName === 'SELECT' || t.tagName === 'TEXTAREA' || t.isContentEditable);
  }

  function onKeyDown(e: KeyboardEvent) {
    if (typing(e) || e.ctrlKey || e.metaKey || e.altKey) return;
    if (app.ui.view === '3d') return;
    if (e.key === ' ') {
      spaceDown = true;
      setCursorStyle();
      e.preventDefault();
      return;
    }
    if (e.key === 'Escape') {
      if (draft) cancelDraft();
      else if (app.tool !== 'select') app.tool = 'select';
      else app.selected = null;
      return;
    }
    if (e.key === 'Enter' && draft) {
      finishDraft();
      return;
    }
    if (e.key === 'Backspace' && draft) {
      draft.points.pop();
      if (draft.points.length === 0) cancelDraft();
      redraw();
      return;
    }
    if ((e.key === 'Delete' || e.key === 'Backspace') && app.selected !== null) {
      app.removeEdit(app.selected);
      return;
    }
    if (e.key === 'f' || e.key === 'F') {
      fit();
      return;
    }
    const t = tools.find((t) => t.key.toLowerCase() === e.key.toLowerCase());
    if (t) {
      cancelDraft();
      app.tool = t.tool;
    }
  }

  function onKeyUp(e: KeyboardEvent) {
    if (e.key === ' ') {
      spaceDown = false;
      setCursorStyle();
    }
  }

  function chooseTool(tool: Tool) {
    cancelDraft();
    app.tool = tool;
  }

  onMount(() => {
    ctx = canvas.getContext('2d');
    const observer = new ResizeObserver(() => {
      const dpr = window.devicePixelRatio || 1;
      width = wrap.clientWidth;
      height = wrap.clientHeight;
      canvas.width = Math.max(1, Math.round(width * dpr));
      canvas.height = Math.max(1, Math.round(height * dpr));
      if (width > 0 && (!userMoved || fittedSize !== app.settings.size)) fit();
      else redraw();
    });
    observer.observe(wrap);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  });

  // New preview: rebuild the picture.
  $effect(() => {
    const p = app.preview;
    const trees = app.ui.mapTrees;
    if (!p) {
      image = null;
    } else {
      const img = shadeMap(p, trees);
      const c = document.createElement('canvas');
      c.width = p.res;
      c.height = p.res;
      c.getContext('2d')!.putImageData(img, 0, 0);
      image = c;
    }
    redraw();
  });

  // A different map size: show all of it.
  $effect(() => {
    const size = app.settings.size;
    if (width > 0 && size !== fittedSize) fit();
  });

  // Anything else drawn on the map.
  $effect(() => {
    void JSON.stringify(app.settings.edits);
    void app.selected;
    void app.ui.mapGrid;
    void app.tool;
    redraw();
  });
</script>

<svelte:window onkeydown={onKeyDown} onkeyup={onKeyUp} />

<div class="editor" data-testid="view-map">
  <div class="tools">
    {#each tools as t (t.tool)}
      <button
        class="tool"
        class:on={app.tool === t.tool}
        data-tool={t.tool}
        title={`${t.label} (${t.key})`}
        onclick={() => chooseTool(t.tool)}
      >
        {t.label}
      </button>
    {/each}
    {#if app.tool === 'biome_zone'}
      <select bind:value={app.zoneBiome} data-testid="zone-biome" title="Biome for new zones">
        {#each zoneBiomes as b (b.id)}
          <option value={b.id}>{b.name}</option>
        {/each}
      </select>
    {/if}
    <span class="spacer"></span>
    <label class="check"><input type="checkbox" bind:checked={app.ui.mapTrees} /> Trees</label>
    <label class="check"><input type="checkbox" bind:checked={app.ui.mapGrid} /> Grid</label>
    <button class="btn small" onclick={fit} title="Show the whole map (F)">Fit</button>
  </div>

  <div class="stage" bind:this={wrap}>
    <canvas
      bind:this={canvas}
      tabindex="0"
      style:cursor={cursorStyle}
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={onPointerUp}
      onpointerleave={() => {
        mouse = null;
        app.cursor = null;
        if (draft) redraw();
      }}
      ondblclick={() => draft && finishDraft()}
      onwheel={onWheel}
      oncontextmenu={(e) => e.preventDefault()}
    ></canvas>

    {#if app.selected !== null && app.settings.edits[app.selected]}
      <div class="inspector-slot">
        <Inspector index={app.selected} />
      </div>
    {/if}

    <div class="hint">
      {#if drafting}
        Click to add points · double-click or Enter to finish · Backspace removes the last point · Esc cancels
      {:else}
        {toolHelp}
      {/if}
    </div>
  </div>
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    background: #0f1214;
  }

  .tools {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
    flex-wrap: wrap;
  }

  .tool {
    height: 26px;
    padding: 0 9px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
  }

  .tool:hover {
    background: var(--panel-2);
    color: var(--text);
  }

  .tool.on {
    background: var(--accent-soft);
    border-color: rgba(210, 166, 76, 0.4);
    color: var(--accent);
  }

  .tools select {
    height: 26px;
  }

  .spacer {
    flex: 1;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    margin-right: 6px;
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

  .inspector-slot {
    position: absolute;
    left: 10px;
    bottom: 34px;
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
