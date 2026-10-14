<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { renderMarkdown } from '../lib/markdown'

  let { markdown, label }: { markdown: string; label: string } = $props()

  function openLink(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest('a')
    if (!link) return
    event.preventDefault()
    openUrl(link.href)
  }
</script>

<!-- We push headings down two levels so a changelog's own # and ## sit below the page's h2 and h3. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<article class="changelog" aria-label={label} onclick={openLink}>{@html renderMarkdown(markdown, { headingOffset: 2 })}</article>

<style>
  .changelog {
    max-height: var(--changelog-max-height, none);
    overflow: auto;
    padding: 18px 22px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
    line-height: 1.6;
  }

  .changelog :global(h3),
  .changelog :global(h4),
  .changelog :global(h5),
  .changelog :global(h6) {
    margin: 20px 0 6px;
    font-size: var(--step-0);
  }

  /* After the shift, # and ## arrive as h3 and h4. We size them like the Files preview's h1 and h2. */
  .changelog :global(h3) {
    font-size: var(--step-2);
  }

  .changelog :global(h4) {
    font-size: var(--step-1);
  }

  .changelog :global(h6) {
    color: var(--muted);
  }

  .changelog > :global(:first-child) {
    margin-top: 0;
  }

  .changelog :global(p),
  .changelog :global(ul),
  .changelog :global(ol) {
    margin: 0 0 10px;
  }

  .changelog :global(ul),
  .changelog :global(ol) {
    padding-left: 20px;
  }

  .changelog :global(code) {
    font-family: var(--font-log);
    font-size: 0.9em;
  }
</style>
