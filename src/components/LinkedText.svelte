<script lang="ts">
  import { nav } from '../lib/nav.svelte'

  let { text }: { text: string } = $props()

  const PAGES = { 'App settings': 'settings', 'Pack settings': 'pack', Changes: 'changes', Git: 'git', Mods: 'mods', Releases: 'releases' } as const
  type PageName = keyof typeof PAGES

  let parts = $derived(text.split(/\b(App settings|Pack settings|Changes|Git|Mods|Releases)\b/))

  function isPage(part: string): part is PageName {
    return part in PAGES
  }
</script>

{#each parts as part, index (index)}{#if isPage(part)}<button class="link" onclick={() => (nav.view = PAGES[part])}>{part}</button>{:else}{part}{/if}{/each}
