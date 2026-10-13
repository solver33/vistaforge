<script lang="ts">
  import type { Snippet } from 'svelte';
  import { app } from '../lib/app.svelte';

  let { title, aside, children }: { title: string; aside?: Snippet; children: Snippet } = $props();

  const open = $derived(!app.ui.collapsed[title]);

  function toggle() {
    app.ui.collapsed[title] = open;
  }
</script>

<section class="section" class:closed={!open}>
  <header>
    <button class="toggle" onclick={toggle} aria-expanded={open}>
      <span class="chev" aria-hidden="true">▾</span>
      {title}
    </button>
    {#if aside && open}
      <div class="aside">{@render aside()}</div>
    {/if}
  </header>
  {#if open}
    <div class="content">{@render children()}</div>
  {/if}
</section>

<style>
  .section {
    border-bottom: 1px solid var(--line-soft);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 10px 0 6px;
    height: 36px;
  }

  .toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    border: 0;
    background: none;
    padding: 4px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .toggle:hover {
    color: var(--text);
  }

  .chev {
    display: inline-block;
    width: 12px;
    font-size: 10px;
    transition: transform 0.12s;
  }

  .closed .chev {
    transform: rotate(-90deg);
  }

  .aside {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .content {
    padding: 0 14px 14px;
  }
</style>
