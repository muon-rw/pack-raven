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

