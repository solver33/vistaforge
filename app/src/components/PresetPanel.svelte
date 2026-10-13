<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { biomeColor } from '../lib/palette';
  import Section from './Section.svelte';

  const presets = $derived(app.info?.presets ?? []);
  /** With more presets than this, the list scrolls and gets a filter box. */
  const MANY = 8;
  let filter = $state('');
  let grid = $state<HTMLDivElement>();

  const shown = $derived.by(() => {
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    if (words.length === 0) return presets;
    return presets.filter((p) => {
      const text = [p.name, p.description, ...p.biomes.map((b) => app.biomeName(b.biome))].join(' ').toLowerCase();
      return words.every((w) => text.includes(w));
    });
  });

  // Keep the chosen preset in sight in the scrolling list, without scrolling the side panel.
  $effect(() => {
    const id = app.settings.preset;
    const card = grid?.querySelector<HTMLElement>(`[data-preset="${CSS.escape(id)}"]`);
    if (!grid || !card) return;
    const top = card.offsetTop - grid.offsetTop;
    if (top < grid.scrollTop) grid.scrollTop = top;
    else if (top + card.offsetHeight > grid.scrollTop + grid.clientHeight) {
      grid.scrollTop = top + card.offsetHeight - grid.clientHeight;
    }
  });
</script>

<Section title="Preset">
  {#snippet aside()}
    {#if app.presetModified}
      <span class="badge" title="The biomes or sliders differ from the preset">modified</span>
    {/if}
  {/snippet}
  {#if presets.length > MANY}
    <input
      class="filter"
      type="search"
      placeholder={`Filter ${presets.length} presets`}
      aria-label="Filter presets"
      data-testid="preset-filter"
      bind:value={filter}
      onkeydown={(e) => {
        if (e.key === 'Escape') filter = '';
      }}
    />
  {/if}
  <div class="grid" class:many={presets.length > MANY} data-testid="presets" bind:this={grid}>
    {#each shown as p (p.id)}
      {@const total = p.biomes.reduce((t, b) => t + b.share, 0) || 1}
      <button
        class="preset"
        class:on={app.settings.preset === p.id}
        title={p.description}
        data-preset={p.id}
        onclick={() => app.applyPreset(p)}
      >
        <span class="strip" aria-hidden="true">
          {#each p.biomes as b (b.biome)}
            <span style:flex-grow={b.share / total} style:background={biomeColor(b.biome)}></span>
          {/each}
        </span>
        <span class="pname">{p.name}</span>
        <span class="pdesc">{p.description}</span>
      </button>
    {:else}
      <p class="none faint">No preset matches “{filter}”.</p>
    {/each}
  </div>
</Section>

<style>
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .grid.many {
    max-height: 316px;
    overflow-y: auto;
    padding-right: 4px;
    align-content: start;
  }

  .filter {
    width: 100%;
    margin-bottom: 8px;
  }

  .none {
    grid-column: 1 / -1;
    margin: 4px 0;
    font-size: 12px;
  }

  .preset {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    text-align: left;
    gap: 3px;
    padding: 0 0 7px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel-2);
    overflow: hidden;
  }

  .preset:hover {
    border-color: #3d454e;
    background: var(--panel-3);
  }

  .preset.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent) inset;
  }

  .strip {
    display: flex;
    height: 5px;
  }

  .strip span {
    flex-basis: 0;
  }

  .pname {
    padding: 2px 8px 0;
    font-weight: 600;
  }

  .pdesc {
    padding: 0 8px;
    font-size: 11px;
    color: var(--muted);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .badge {
    font-size: 11px;
    color: var(--accent);
    border: 1px solid var(--accent-soft);
    background: var(--accent-soft);
    border-radius: 9px;
    padding: 0 7px;
  }
</style>
