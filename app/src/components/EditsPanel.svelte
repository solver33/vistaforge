<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { describe, kindInfo } from '../lib/edits';
  import Section from './Section.svelte';

  function select(i: number) {
    app.selected = i;
    if (app.ui.view === '3d') app.ui.view = 'split';
  }
</script>

<Section title="Landforms">
  {#snippet aside()}
    {#if app.settings.edits.length > 0}
      <button class="btn small ghost" data-testid="clear-edits" onclick={() => app.clearEdits()}>Clear all</button>
    {/if}
  {/snippet}
  {#if app.settings.edits.length === 0}
    <p class="muted empty">
      Place hills, mountains, plateaus, lakes, rivers, ridges, roads, waterfalls and biome zones with the tools above the map.
    </p>
  {:else}
    <ul class="list" data-testid="edits">
      {#each app.settings.edits as e, i (i)}
        {@const k = kindInfo(e.kind)}
        <li class:on={app.selected === i}>
          <button class="item" onclick={() => select(i)}>
            <span class="dot" style:background={k.color}></span>
            <span class="kind">{k.label}</span>
            <span class="desc faint">{describe(e, (id) => app.biomeName(id))}</span>
          </button>
          <button class="btn icon small ghost" title="Remove" aria-label={`Remove ${k.label}`} onclick={() => app.removeEdit(i)}>✕</button>
        </li>
      {/each}
    </ul>
  {/if}
</Section>

<style>
  .empty {
    margin: 0;
    line-height: 1.5;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    display: flex;
    align-items: center;
    border-radius: var(--radius);
  }

  li.on {
    background: var(--accent-soft);
  }

  .item {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    border: 0;
    background: none;
    text-align: left;
    padding: 5px 6px;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex: none;
  }

  .kind {
    font-weight: 600;
  }

  .desc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
