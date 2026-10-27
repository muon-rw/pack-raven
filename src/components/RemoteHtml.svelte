<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener'

  // `html` must already have been through sanitizeRemoteHtml, which marks the links we may open.
  // `topHeading` is the level the author's largest heading becomes, one below whatever labels this
  // HTML on the page.
  let { html, topHeading }: { html: string; topHeading: number } = $props()

  // Authors pick heading levels for their size and often skip some, so we renumber the levels they
  // used in order, which keeps the page outline unbroken for screen readers.
  function nestHeadings(source: string) {
    const used = [...new Set([...source.matchAll(/<h([1-6])\b/g)].map((m) => Number(m[1])))].sort()
    return source.replace(/<(\/?)h([1-6])\b/g, (_, slash, level) => `<${slash}h${Math.min(6, topHeading + used.indexOf(Number(level)))}`)
  }

  function openLink(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest('a')
    if (!link) return
    event.preventDefault()
    const external = link.getAttribute('data-external')
    if (external) openUrl(external)
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (links inside are real links; this only routes their clicks) -->
<div class="remote" onclick={openLink}>{@html nestHeadings(html)}</div>

<style>
  .remote {
    display: flow-root;
    overflow-wrap: anywhere;
    line-height: 1.55;
  }

  .remote > :global(:first-child) {
    margin-top: 0;
  }

  .remote > :global(:last-child) {
    margin-bottom: 0;
  }

  .remote :global(img) {
    max-width: 100%;
    height: auto;
  }

  .remote :global(:is(h1, h2, h3, h4)) {
    font-size: var(--step-1);
    margin: 16px 0 6px;
  }

  .remote :global(:is(h5, h6)) {
    font-size: var(--step-0);
    margin: 12px 0 4px;
  }

  .remote :global(p),
  .remote :global(ul),
  .remote :global(ol) {
    margin: 0 0 8px;
  }

  .remote :global(ul),
  .remote :global(ol) {
    padding-left: 20px;
  }

  .remote :global(li + li) {
    margin-top: 2px;
  }

  .remote :global(code) {
    font-family: var(--font-log);
    font-size: 0.9em;
  }

  .remote :global(a) {
    color: var(--forest);
  }

  .remote :global(.center),
  .remote :global(.aligncenter) {
    text-align: center;
  }

  .remote :global(.alignleft) {
    float: left;
    margin-right: 12px;
  }

  .remote :global(.alignright) {
    float: right;
    margin-left: 12px;
  }
</style>
