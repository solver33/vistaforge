<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { formatInt, shortPath } from '../lib/format';
  import Section from './Section.svelte';

  const crVersion = $derived(app.catalog?.mods['conquest'] ?? null);
  /** Found game folders that aren't already the ones in use. */
  const suggestions = $derived(
    app.instances.filter((i) =>
      i.catalog ? i.catalog !== app.catalogPath || (i.saves && i.saves !== app.savesDir) : !app.savesDir && i.saves,
    ),
  );
  const anyCatalog = $derived(app.instances.some((i) => i.catalog));
</script>

<Section title="Game">
  <div class="field" data-testid="catalog">
    <div class="row">
      <span class="name">Block catalog</span>
      <button class="btn small" onclick={() => app.pickCatalog()}>Choose…</button>
    </div>
    {#if app.catalogBusy}
      <div class="status"><span class="spinner"></span> Reading the catalog…</div>
    {:else if app.catalog}
      <div class="status ok">
        ✓ {crVersion ? `Conquest Reforged ${crVersion}` : 'Catalog'} · {formatInt(app.catalog.blocks)} blocks
      </div>
    {:else if app.catalogError}
      <div class="status error-text" data-testid="catalog-error">{app.catalogError}</div>
    {:else}
      <div class="status muted">Not chosen yet.</div>
    {/if}
    {#if app.catalogPath}
      <div class="path mono" title={app.catalogPath}>{shortPath(app.catalogPath)}</div>
    {/if}
  </div>

  <div class="field" data-testid="saves">
    <div class="row">
      <span class="name">Saves folder</span>
      <button class="btn small" onclick={() => app.pickSavesDir()}>Choose…</button>
    </div>
    {#if app.savesDir}
      <div class="path mono" title={app.savesDir}>{shortPath(app.savesDir)}</div>
    {:else}
      <div class="status muted">Where Minecraft keeps your worlds.</div>
    {/if}
  </div>

  {#if suggestions.length > 0}
    <div class="found">
      <div class="found-title muted">Found on this computer</div>
      {#each suggestions.slice(0, 4) as inst (inst.game_dir)}
        <div class="instance">
          <div class="inst-text">
            <div class="inst-name" title={inst.game_dir}>{inst.name}</div>
            <div class="faint">{inst.launcher}{inst.catalog ? ' · has a catalog' : ''}</div>
          </div>
          <button class="btn small" onclick={() => app.useInstance(inst)}>Use</button>
        </div>
      {/each}
    </div>
  {/if}

  {#if !app.catalog && !app.catalogBusy && !anyCatalog}
    <p class="hint muted">
      The catalog lists the blocks in your game. Put the Vistaforge exporter mod in your Conquest Reforged
      game's <span class="mono">mods</span> folder and start the game once: it writes
      <span class="mono">vistaforge/catalog.json</span> in the game folder.
      <button class="link" onclick={() => app.rescan()}>Look again</button>
    </p>
  {/if}
</Section>

<style>
  .field + .field {
    margin-top: 12px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .name {
    font-weight: 600;
  }

  .status {
    margin-top: 4px;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .status.ok {
    color: var(--ok);
  }

  .path {
    margin-top: 3px;
    color: var(--faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .found {
    margin-top: 14px;
    padding: 8px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel-2);
  }

  .found-title {
    font-size: 11px;
    margin-bottom: 4px;
  }

  .instance {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 0;
  }

  .inst-text {
    flex: 1;
    min-width: 0;
  }

  .inst-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint {
    margin: 12px 0 0;
    line-height: 1.5;
  }

  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--accent);
    text-decoration: underline;
  }
</style>
