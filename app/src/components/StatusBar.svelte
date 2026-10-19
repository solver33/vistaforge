<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { formatInt } from '../lib/format';
  import { DIMENSION_NAMES, world } from '../lib/world.svelte';

  const c = $derived(app.cursor);
  const w = $derived(world.cursor);
</script>

<footer class="status">
  {#if world.info}
    <span class="left" data-testid="preview-status">
      {#if world.areaBusy}
        <span class="spinner"></span> Reading this part of the world…
      {:else if world.areaError}
        <span class="error-text">{world.areaError}</span>
      {:else if world.loaded && app.preview}
        Area {world.loaded.size}×{world.loaded.size} from X {world.loaded.x0}, Z {world.loaded.z0} · read in {Math.round(app.previewMs)} ms
      {:else}
        No preview yet
      {/if}
    </span>
    <span class="cursor mono" data-testid="cursor">
      {#if w}
        X {w.x} · Z {w.z}{w.y !== null ? ` · Y ${Math.round(w.y)}` : ''}{w.water !== null ? ` · water ${w.water.toFixed(0)}` : ''}{w.biome
          ? ` · ${app.biomeName(w.biome.replace(/^minecraft:/, ''))}`
          : ''}
      {/if}
    </span>
    <span class="right faint">
      {#if app.backend && app.backend.kind !== 'tauri'}
        <span class="mode">{app.backend.kind === 'mock' ? 'demo data' : 'dev server'}</span>
      {/if}
      <span data-testid="status-dimension">{DIMENSION_NAMES[world.info.dimension] ?? world.info.dimension}</span>
      · {formatInt(world.info.chunks)} chunks
    </span>
  {:else}
    <span
      class="left"
      data-testid="preview-status"
      title={app.preview && app.preview.totalTrees > app.preview.treeCount
        ? `The preview draws ${formatInt(app.preview.treeCount)} of them, spread evenly`
        : undefined}
    >
      {#if app.previewBusy}
        <span class="spinner"></span> Updating preview…
      {:else if app.previewError}
        <span class="error-text">{app.previewError}</span>
      {:else if app.preview}
        Preview {app.preview.res}×{app.preview.res} in {Math.round(app.previewMs)} ms · {formatInt(app.preview.totalTrees)} trees
      {:else}
        No preview yet
      {/if}
    </span>
    <span class="cursor mono" data-testid="cursor">
      {#if c}
        X {c.x} · Z {c.z} · Y {c.height.toFixed(1)}{c.water !== null ? ` · water ${c.water.toFixed(0)}` : ''} · {app.biomeName(c.biome)}
      {/if}
    </span>
    <span class="right faint">
      {#if app.backend && app.backend.kind !== 'tauri'}
        <span class="mode">{app.backend.kind === 'mock' ? 'demo data' : 'dev server'}</span>
      {/if}
      {app.settings.size}×{app.settings.size} · seed {app.settings.seed}
    </span>
  {/if}
</footer>

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 16px;
    height: 26px;
    padding: 0 12px;
    font-size: 12px;
    background: var(--panel);
    border-top: 1px solid var(--line);
    color: var(--muted);
  }

  .left {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 280px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .cursor {
    flex: 1;
    text-align: center;
    color: var(--text);
  }

  .right {
    display: flex;
    gap: 10px;
  }

  .mode {
    color: var(--accent);
  }
</style>
