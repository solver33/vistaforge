<script lang="ts">
  import { onMount } from 'svelte';
  import BiomePanel from './components/BiomePanel.svelte';
  import EditsPanel from './components/EditsPanel.svelte';
  import ExportDialog from './components/ExportDialog.svelte';
  import GamePanel from './components/GamePanel.svelte';
  import MapEditor from './components/MapEditor.svelte';
  import PresetPanel from './components/PresetPanel.svelte';
  import ShapePanel from './components/ShapePanel.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import TopBar from './components/TopBar.svelte';
  import Viewport3D from './components/Viewport3D.svelte';
  import WorldPanel from './components/WorldPanel.svelte';
  import { app } from './lib/app.svelte';

  onMount(() => {
    void app.start();
  });

  // A new preview whenever something that shapes it changes.
  $effect(() => {
    void app.previewKey;
    if (app.ready && app.catalog) app.schedulePreview();
  });

  // Every change becomes an undo step once it settles, and a drag once the mouse button comes up.
  $effect(() => {
    void app.settingsJson;
    if (app.ready) app.commitSoon();
  });

  // Remember folders, the map and the view for next time.
  $effect(() => {
    void app.configJson;
    if (app.ready) app.saveConfigSoon();
  });

  function onKeyDown(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    const t = e.target as HTMLElement | null;
    const inText = !!t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA') && (t as HTMLInputElement).type !== 'range';
    const key = e.key.toLowerCase();
    if (key === 'z' && !inText) {
      e.preventDefault();
      if (e.shiftKey) app.redo();
      else app.undo();
    } else if (key === 'y' && !inText) {
      e.preventDefault();
      app.redo();
    } else if (key === 's') {
      e.preventDefault();
      void app.saveSettings();
    } else if (key === 'o') {
      e.preventDefault();
      void app.openSettings();
    }
  }
</script>

<svelte:window
  onkeydown={onKeyDown}
  onpointerdown={() => app.holdCommits()}
  onpointerup={() => app.releaseCommits()}
  onpointercancel={() => app.releaseCommits()}
  onblur={() => app.releaseCommits()}
/>

{#if app.startError}
  <div class="splash">
    <div class="card">
      <h1>Vistaforge could not start</h1>
      <p class="error-text">{app.startError}</p>
    </div>
  </div>
{:else if !app.ready}
  <div class="splash"><span class="spinner"></span></div>
{:else}
  <div class="app">
    <TopBar />
    <div class="body">
      <aside class="sidebar">
        <GamePanel />
        <PresetPanel />
        <WorldPanel />
        <BiomePanel />
        <ShapePanel />
        <EditsPanel />
      </aside>
      <main class="views" class:v3d={app.ui.view === '3d'} class:vmap={app.ui.view === 'map'}>
        <section class="pane pane-3d"><Viewport3D /></section>
        <section class="pane pane-map"><MapEditor /></section>
      </main>
    </div>
    <StatusBar />
  </div>
  <ExportDialog />
  {#if app.toast}
    <div class="toast" class:error={app.toast.error} role="status" data-testid="toast">{app.toast.text}</div>
  {/if}
{/if}

<style>
  .app {
    display: grid;
    grid-template-rows: 48px 1fr 26px;
    height: 100vh;
  }

  .body {
    display: grid;
    grid-template-columns: 340px 1fr;
    min-height: 0;
  }

  .sidebar {
    overflow-y: auto;
    background: var(--panel);
    border-right: 1px solid var(--line);
  }

  .views {
    display: grid;
    grid-template-columns: 1fr 1fr;
    min-width: 0;
    min-height: 0;
  }

  .views.v3d,
  .views.vmap {
    grid-template-columns: 1fr;
  }

  .views.v3d .pane-map,
  .views.vmap .pane-3d {
    display: none;
  }

  .pane {
    position: relative;
    min-width: 0;
    min-height: 0;
  }

  .pane-3d {
    border-right: 1px solid var(--line);
  }

  .splash {
    display: grid;
    place-items: center;
    height: 100vh;
  }

  .splash .card {
    max-width: 480px;
    padding: 20px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--panel);
  }

  .splash h1 {
    margin: 0 0 8px;
    font-size: 18px;
  }

  .toast {
    position: fixed;
    left: 50%;
    bottom: 40px;
    transform: translateX(-50%);
    padding: 8px 14px;
    border-radius: 8px;
    background: var(--panel-3);
    border: 1px solid var(--line);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 30;
  }

  .toast.error {
    border-color: var(--danger);
    color: var(--danger);
  }
</style>
