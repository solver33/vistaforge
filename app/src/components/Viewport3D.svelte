<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '../lib/app.svelte';
  import { PreviewScene } from '../lib/scene3d';

  let canvas: HTMLCanvasElement;
  let scene = $state.raw<PreviewScene | null>(null);
  let failed = $state(false);

  const HEIGHTS = [1, 1.5, 2, 3];

  onMount(() => {
    try {
      const s = new PreviewScene(canvas, options());
      scene = s;
      (window as unknown as { __vistaforgeScene?: PreviewScene }).__vistaforgeScene = s;
      return () => s.dispose();
    } catch (e) {
      console.error(e);
      failed = true;
    }
  });

  function options() {
    return {
      exaggeration: app.ui.exaggeration,
      trees: app.ui.trees3d,
      water: app.ui.water3d,
      shadows: app.ui.shadows,
    };
  }

  $effect(() => {
    const p = app.preview;
    if (scene && p) scene.setPreview(p);
  });

  $effect(() => {
    const o = options();
    scene?.setOptions(o);
  });
</script>

<div class="viewport" data-testid="view-3d">
  <canvas bind:this={canvas}></canvas>

  <div class="toolbar">
    <button class="btn small" onclick={() => scene?.resetCamera()} title="Look at the whole map">Reset view</button>
    <span class="segmented" title="Vertical scale">
      {#each HEIGHTS as h (h)}
        <button class:on={app.ui.exaggeration === h} onclick={() => (app.ui.exaggeration = h)}>×{h}</button>
      {/each}
    </span>
    <label class="check"><input type="checkbox" bind:checked={app.ui.trees3d} /> Trees</label>
    <label class="check"><input type="checkbox" bind:checked={app.ui.water3d} /> Water</label>
    <label class="check"><input type="checkbox" bind:checked={app.ui.shadows} /> Shadows</label>
  </div>

  <div class="help">Drag to turn · right-drag to move · wheel to zoom</div>

  {#if failed}
    <div class="overlay">
      <div class="card">The 3D view could not start on this computer (WebGL is unavailable). The map view still works.</div>
    </div>
  {:else if !app.catalog}
    <div class="overlay">
      <div class="card">
        <p>Choose the block catalog from your Conquest Reforged game to see the map.</p>
        <button class="btn primary" onclick={() => app.pickCatalog()}>Choose catalog…</button>
      </div>
    </div>
  {:else if app.previewError && !app.preview}
    <div class="overlay"><div class="card error-text">{app.previewError}</div></div>
  {/if}
</div>

<style>
  .viewport {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #a9c6dc;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
    outline: none;
  }

  .toolbar {
    position: absolute;
    top: 10px;
    left: 10px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px;
    border-radius: var(--radius);
    background: rgba(18, 21, 24, 0.82);
    border: 1px solid var(--line);
  }

  .check {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
  }

  .help {
    position: absolute;
    left: 10px;
    bottom: 10px;
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 11px;
    color: var(--muted);
    background: rgba(18, 21, 24, 0.7);
    pointer-events: none;
  }

  .overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(18, 21, 24, 0.55);
  }

  .card {
    max-width: 360px;
    padding: 16px 18px;
    border-radius: 10px;
    background: var(--panel);
    border: 1px solid var(--line);
    text-align: center;
  }

  .card p {
    margin: 0 0 12px;
  }
</style>
