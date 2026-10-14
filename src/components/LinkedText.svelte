<script lang="ts">
  import { nav } from '../lib/nav.svelte'

  let { text }: { text: string } = $props()

  const PAGES = { 'App settings': 'settings', 'Pack settings': 'pack' } as const
  type PageName = keyof typeof PAGES

  let parts = $derived(text.split(/(App settings|Pack settings)/))

  function isPage(part: string): part is PageName {
    return part in PAGES
  }
</script>

{#each parts as part, index (index)}{#if isPage(part)}<button class="link" onclick={() => (nav.view = PAGES[part])}>{part}</button>{:else}{part}{/if}{/each}

<style>
  .link {
    display: inline;
    padding: 0;
    font: inherit;
    color: var(--forest);
    text-decoration: underline;
    text-underline-offset: 0.18em;
    background: transparent;
    border: 0;
  }

  .link:hover:not(:disabled) {
    color: var(--forest-hi);
    background: transparent;
  }
</style>
