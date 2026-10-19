<script lang="ts">
  import { app, RESOLUTIONS, type View } from '../lib/app.svelte';
  import { world } from '../lib/world.svelte';

  const VIEWS: { view: View; label: string }[] = [
    { view: '3d', label: '3D' },
    { view: 'split', label: 'Split' },
    { view: 'map', label: 'Map' },
  ];
</script>

<header class="top">
  <div class="brand">
    <img src="/icon.png" alt="" width="22" height="22" />
    <span>Vistaforge</span>
  </div>

  <nav class="files">
    {#if world.info}
      <span class="world" title={world.info.path}>
        <span class="world-name" data-testid="world-title">{world.info.name}</span>
        <span class="badge" title="Vistaforge only reads this world and never changes it">read only</span>
      </span>
      <button class="btn ghost" data-testid="close-world" onclick={() => world.close()} title="Back to making a new map"
        >Close world</button
      >
    {:else}
      <button class="btn ghost" onclick={() => app.newMap()} title="Start again from the current preset with a new seed">New</button>
      <button class="btn ghost" onclick={() => app.openSettings()} title="Open map settings (Ctrl+O)">Open…</button>
      <button class="btn ghost" data-testid="save-settings" onclick={() => app.saveSettings()} title="Save map settings (Ctrl+S)">Save…</button>
      <span class="sep"></span>
      <button class="btn ghost icon" data-testid="undo" disabled={!app.canUndo} onclick={() => app.undo()} title="Undo (Ctrl+Z)" aria-label="Undo">↶</button>
      <button class="btn ghost icon" data-testid="redo" disabled={!app.canRedo} onclick={() => app.redo()} title="Redo (Ctrl+Y)" aria-label="Redo">↷</button>
      <span class="sep"></span>
      <button
        class="btn ghost"
        data-testid="open-world"
        disabled={!app.catalog || world.opening}
        onclick={() => world.pick()}
        title={app.catalog ? 'Look at a world you played: its map, in 3D and in render mode' : 'Choose the block catalog first'}
      >
        {world.opening ? 'Opening…' : 'Open world…'}
      </button>
    {/if}
  </nav>

  <div class="middle">
    <span class="segmented" data-testid="views">
      {#each VIEWS as v (v.view)}
        <button class:on={app.ui.view === v.view} onclick={() => (app.ui.view = v.view)}>{v.label}</button>
      {/each}
    </span>
    <label class="quality" title="How detailed the preview is. Higher is slower.">
      Preview
      <select bind:value={app.ui.res} data-testid="resolution">
        {#each RESOLUTIONS as r (r.res)}
          <option value={r.res}>{r.label}</option>
        {/each}
      </select>
    </label>
  </div>

  <div class="export">
    {#if world.info}
      <button
        class="btn"
        data-testid="open-other-world"
        disabled={world.opening}
        onclick={() => world.pick()}
        title="Open another world from the saves folder">Open another world…</button
      >
    {:else}
      {#if app.exportProblems.length > 0}
        <span class="why muted" data-testid="export-why">{app.exportProblems[0]}</span>
      {/if}
      <button
        class="btn primary"
        data-testid="export"
        disabled={app.exportProblems.length > 0 || app.exportState.phase === 'running'}
        title={app.exportProblems.join('\n') || 'Write the world into the saves folder'}
        onclick={() => app.startExport()}
      >
        Generate world
      </button>
    {/if}
  </div>
</header>

<style>
  .top {
    display: flex;
    align-items: center;
    gap: 14px;
    height: 48px;
    padding: 0 12px;
    background: var(--panel);
    border-bottom: 1px solid var(--line);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 316px;
    font-size: 15px;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .files {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .sep {
    width: 1px;
    height: 20px;
    margin: 0 6px;
    background: var(--line);
  }

  .middle {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 14px;
  }

  .quality {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
  }

  .quality select {
    height: 26px;
  }

  .export {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .world {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    max-width: 320px;
    margin-right: 6px;
  }

  .world-name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge {
    flex: none;
    padding: 1px 7px;
    border-radius: 9px;
    font-size: 11px;
    color: var(--accent);
    background: var(--accent-soft);
  }

  .why {
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
</style>
