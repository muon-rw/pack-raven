<script lang="ts">
  import type { Snippet } from 'svelte'

  // Use aria-disabled rather than disabled on buttons inside a Tip, so keyboard and screen-reader
  // users can still focus them and hear the reason.
  let { tip, children }: { tip: string | null | undefined; children: Snippet<[string | undefined]> } = $props()

  const id = `tip-${Math.random().toString(36).slice(2, 9)}`
</script>

<span class="tip" title={tip ?? undefined}>
  {@render children(tip ? id : undefined)}
  {#if tip}<span class="visually-hidden" {id}>{tip}</span>{/if}
</span>

<style>
  .tip {
    display: inline-flex;
  }
</style>
