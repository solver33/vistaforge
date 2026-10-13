<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { biomeColor } from '../lib/palette';
  import Section from './Section.svelte';

  const choosable = $derived(app.info?.biomes.filter((b) => !b.feature) ?? []);
  const used = $derived(app.settings.biomes.filter((b) => b.share > 0).length);
</script>

<Section title="Biome mix">
  {#snippet aside()}
    <button class="btn small ghost" disabled={used < 2} onclick={() => app.evenMix()} title="Give every biome in the mix the same share">
      Even
    </button>
  {/snippet}
  <div class="list" data-testid="biomes">
    {#each choosable as b (b.id)}
      {@const pct = app.percents[b.id] ?? 0}
      <div class="row" class:off={pct === 0} data-biome={b.id}>
        <span class="swatch" style:background={biomeColor(b.id)}></span>
        <span class="name">{b.name}</span>
        <input
          type="range"
          min="0"
          max="100"
          step="1"
          value={pct}
          aria-label={`${b.name} share`}
          oninput={(e) => {
            app.setPercent(b.id, Number(e.currentTarget.value));
            // The mix may refuse (the last biome stays at 100%); keep the thumb honest.
            e.currentTarget.value = String(app.percents[b.id] ?? 0);
          }}
          onchange={() => app.commit()}
        />
        <span class="pct mono">{pct > 0 && pct < 1 ? '<1' : Math.round(pct)}%</span>
      </div>
    {/each}
  </div>
  <p class="faint note">Beaches, rivers and lakes come from the terrain itself.</p>
</Section>

<style>
  .row {
    display: grid;
    grid-template-columns: 10px 96px 1fr 38px;
    align-items: center;
    gap: 8px;
    height: 24px;
  }

  .row.off .name,
  .row.off .pct {
    color: var(--faint);
  }

  .row.off .swatch {
    opacity: 0.35;
  }

  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 3px;
  }

  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .pct {
    text-align: right;
    color: var(--muted);
  }

  .note {
    margin: 8px 0 0;
    font-size: 12px;
  }
</style>
