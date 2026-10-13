<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { formatInt } from '../lib/format';
  import { parseSeed } from '../lib/seed';
  import Range from './Range.svelte';
  import Section from './Section.svelte';

  const SIZES = [256, 512, 768, 1024, 1536, 2048, 3072, 4096];

  const sizes = $derived(
    [...new Set([...SIZES, app.settings.size])]
      .filter((s) => s >= (app.info?.min_size ?? 256) && s <= (app.info?.max_size ?? 4096))
      .sort((a, b) => a - b),
  );
  const chunks = $derived((app.settings.size / 16) ** 2);
  const regions = $derived(Math.ceil(app.settings.size / 512) ** 2);

  let seedText = $state('');
  $effect(() => {
    seedText = String(app.settings.seed);
  });

  function setSeed() {
    app.settings.seed = parseSeed(seedText);
    seedText = String(app.settings.seed);
    app.commit();
  }
</script>

<Section title="World">
  <label class="field">
    <span class="label">Name</span>
    <input
      type="text"
      data-testid="world-name"
      class:invalid={app.nameProblem !== null}
      bind:value={app.settings.name}
      onchange={() => app.commit()}
      spellcheck="false"
    />
  </label>
  {#if app.nameProblem}
    <div class="problem error-text" data-testid="name-problem">{app.nameProblem}</div>
  {/if}

  <div class="two">
    <label class="field">
      <span class="label">Size</span>
      <select
        data-testid="world-size"
        value={app.settings.size}
        onchange={(e) => {
          app.settings.size = Number(e.currentTarget.value);
          app.commit();
        }}
      >
        {#each sizes as s (s)}
          <option value={s}>{s} × {s}</option>
        {/each}
      </select>
    </label>
    <label class="field">
      <span class="label">Seed</span>
      <span class="seed">
        <input
          type="text"
          data-testid="seed"
          bind:value={seedText}
          onchange={setSeed}
          onkeydown={(e) => e.key === 'Enter' && setSeed()}
          spellcheck="false"
          title="A number, or any text"
        />
        <button class="btn icon" title="New random seed" data-testid="new-seed" onclick={() => app.newSeed()}>⚄</button>
      </span>
    </label>
  </div>
  <div class="faint note">{formatInt(chunks)} chunks · {regions} region {regions === 1 ? 'file' : 'files'}</div>

  <Range
    label="Sea level"
    help="Y of the sea surface, and the default level for lakes."
    value={app.settings.sea_level}
    normal={app.currentPreset?.sea_level ?? null}
    min={0}
    max={160}
    step={1}
    format={(v) => `Y ${v}`}
    testid="sea-level"
    oninput={(v) => (app.settings.sea_level = v)}
    onchange={() => app.commit()}
  />
</Section>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-bottom: 8px;
    min-width: 0;
  }

  .label {
    color: var(--muted);
    font-size: 12px;
  }

  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .seed {
    display: flex;
    gap: 4px;
  }

  .seed input {
    flex: 1;
    width: 0;
  }

  .problem {
    margin: -4px 0 8px;
    font-size: 12px;
  }

  .note {
    margin: -4px 0 8px;
    font-size: 12px;
  }
</style>
