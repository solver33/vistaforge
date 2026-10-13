<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { biomeColor } from '../lib/palette';
  import Section from './Section.svelte';

  const presets = $derived(app.info?.presets ?? []);
</script>

<Section title="Preset">
  {#snippet aside()}
    {#if app.presetModified}
      <span class="badge" title="The biomes or sliders differ from the preset">modified</span>
    {/if}
  {/snippet}
  <div class="grid" data-testid="presets">
    {#each presets as p (p.id)}
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
    {/each}
  </div>
</Section>

<style>
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
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
