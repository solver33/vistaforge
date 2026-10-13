<script lang="ts">
  import { app } from '../lib/app.svelte';
  import { sliderGroups } from '../lib/sliders';
  import Range from './Range.svelte';
  import Section from './Section.svelte';

  const groups = $derived(sliderGroups(Object.keys(app.settings.sliders)));
</script>

<Section title="Terrain & life">
  <div data-testid="sliders">
    {#each groups as g (g.title)}
      <div class="group">
        <div class="gtitle faint">{g.title}</div>
        {#each g.sliders as s (s.key)}
          <Range
            label={s.label}
            help={s.help}
            value={app.settings.sliders[s.key] ?? 0.5}
            normal={app.currentPreset?.sliders[s.key] ?? 0.5}
            testid={`slider-${s.key}`}
            oninput={(v) => (app.settings.sliders[s.key] = v)}
            onchange={() => app.commit()}
          />
        {/each}
      </div>
    {/each}
  </div>
</Section>

<style>
  .group + .group {
    margin-top: 10px;
  }

  .gtitle {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: 2px;
  }
</style>
