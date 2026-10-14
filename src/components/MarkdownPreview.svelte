<script lang="ts">
  import { onDestroy, tick } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { renderDocument, type Block } from '../lib/markdown'
  import { mediaUrl, outerArchive } from '../lib/viewers'

  let {
    source,
    path,
    onopenpath,
    onscrollline,
  }: {
    source: string
    path: string
    onopenpath: (path: string) => void
    onscrollline?: (line: number) => void
  } = $props()

  const images = new Map<string, { url: string; release: () => void } | 'loading' | 'failed'>()
  let imageVersion = $state(0)
  let article: HTMLElement
  let programmaticUntil = 0

  let blocks = $derived.by((): Block[] => {
    void imageVersion
    return renderDocument(source, path, (image) => {
      const known = images.get(image)
      return typeof known === 'object' ? known.url : null
    })
  })

  // Relative images load asynchronously, from disk or out of a jar, so we render first and
  // fill them in as they arrive.
  $effect(() => {
    void blocks
    tick().then(() => {
      for (const image of article?.querySelectorAll<HTMLImageElement>('img[data-path]:not([src])') ?? []) load(image.dataset.path!)
    })
  })

  async function load(image: string) {
    if (images.has(image)) return
    images.set(image, 'loading')
    try {
      images.set(image, await mediaUrl(image, outerArchive(image) !== image))
    } catch {
      images.set(image, 'failed')
    }
    imageVersion++
  }

  onDestroy(() => {
    for (const image of images.values()) if (typeof image === 'object') image.release()
  })

  function onClick(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest('a')
    if (!link) return
    event.preventDefault()
    const { external, anchor, path: target } = link.dataset
    if (external) openUrl(external)
    else if (anchor) article.querySelector(`#${CSS.escape(anchor)}`)?.scrollIntoView({ block: 'start' })
    else if (target) onopenpath(target)
  }

  function blockElements() {
    return [...article.querySelectorAll<HTMLElement>(':scope > [data-line]')]
  }

  /** Scrolls so the source line, which may be fractional, sits at the top. */
  export function scrollToLine(line: number) {
    if (!article) return
    const elements = blockElements()
    let index = elements.findLastIndex((el) => Number(el.dataset.line) <= line)
    if (index < 0) index = 0
    const current = elements[index]
    if (!current) return
    const next = elements[index + 1]
    const start = Number(current.dataset.line)
    const span = next ? Number(next.dataset.line) - start : 1
    const bottom = next ? next.offsetTop : current.offsetTop + current.offsetHeight
    const fraction = Math.min(1, Math.max(0, (line - start) / span))
    programmaticUntil = performance.now() + 120
    article.scrollTop = current.offsetTop + fraction * (bottom - current.offsetTop) - 16
  }

  function onScroll() {
    if (!onscrollline || performance.now() < programmaticUntil) return
    const top = article.scrollTop + 16
    const elements = blockElements()
    const index = Math.max(0, elements.findLastIndex((el) => el.offsetTop <= top))
    const current = elements[index]
    if (!current) return
    const next = elements[index + 1]
    const start = Number(current.dataset.line)
    const bottom = next ? next.offsetTop : current.offsetTop + current.offsetHeight
    const fraction = bottom > current.offsetTop ? Math.min(1, (top - current.offsetTop) / (bottom - current.offsetTop)) : 0
    onscrollline(start + fraction * ((next ? Number(next.dataset.line) : start + 1) - start))
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions (links inside are focusable and keyboard-activated) -->
<article class="markdown" bind:this={article} onclick={onClick} onscroll={onScroll} aria-label="Preview">
  {#each blocks as block, i (i)}
    <div class="block" data-line={block.line}>{@html block.html}</div>
  {:else}
    <p class="muted">Nothing to preview yet.</p>
  {/each}
</article>

<style>
  .markdown {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 16px 24px 40px;
    line-height: 1.6;
    overflow-wrap: break-word;
  }

  .block > :global(:first-child) {
    margin-top: 0;
  }

  .markdown :global(h1),
  .markdown :global(h2) {
    margin: 24px 0 10px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--line);
  }

  .markdown :global(h1) {
    font-size: var(--step-2);
  }

  .markdown :global(h2) {
    font-size: var(--step-1);
  }

  .markdown :global(h3),
  .markdown :global(h4),
  .markdown :global(h5),
  .markdown :global(h6) {
    margin: 20px 0 8px;
    font-size: var(--step-0);
  }

  .markdown :global(h5),
  .markdown :global(h6) {
    color: var(--muted);
  }

  .markdown :global(p),
  .markdown :global(ul),
  .markdown :global(ol),
  .markdown :global(dl),
  .markdown :global(table),
  .markdown :global(pre),
  .markdown :global(blockquote),
  .markdown :global(details) {
    margin: 0 0 12px;
  }

  .markdown :global(ul),
  .markdown :global(ol) {
    padding-left: 24px;
  }

  .markdown :global(li > ul),
  .markdown :global(li > ol) {
    margin-bottom: 0;
  }

  .markdown :global(li:has(> input[type='checkbox'])) {
    list-style: none;
    margin-left: -20px;
  }

  .markdown :global(input[type='checkbox']) {
    margin: 0 6px 0 0;
    vertical-align: middle;
    accent-color: var(--forest);
  }

  .markdown :global(a) {
    color: var(--forest);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .markdown :global(a[data-path]) {
    text-decoration-style: dotted;
  }

  .markdown :global(code) {
    padding: 1px 5px;
    font-family: var(--font-log);
    font-size: 0.88em;
    background: var(--surface);
    border-radius: 4px;
  }

  .markdown :global(pre) {
    padding: 10px 14px;
    overflow-x: auto;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
  }

  .markdown :global(pre code) {
    padding: 0;
    background: none;
  }

  .markdown :global(blockquote) {
    padding: 2px 14px;
    color: var(--muted);
    border-left: 3px solid var(--line-strong);
  }

  .markdown :global(hr) {
    margin: 20px 0;
    border: 0;
    border-top: 1px solid var(--line);
  }

  .markdown :global(table) {
    display: block;
    max-width: 100%;
    overflow-x: auto;
    border-collapse: collapse;
  }

  .markdown :global(th),
  .markdown :global(td) {
    padding: 5px 12px;
    border: 1px solid var(--line);
  }

  .markdown :global(th) {
    background: var(--surface);
  }

  .markdown :global(img) {
    max-width: 100%;
    vertical-align: middle;
  }

  .markdown :global(img[data-path]:not([src])) {
    display: inline-block;
    min-width: 24px;
    min-height: 24px;
    background: var(--surface);
  }

  .markdown :global(summary) {
    cursor: pointer;
    font-weight: 700;
  }

  .markdown :global(kbd) {
    padding: 0 5px;
    font-family: var(--font-log);
    font-size: 0.85em;
    border: 1px solid var(--line-strong);
    border-bottom-width: 2px;
    border-radius: 4px;
  }
</style>
