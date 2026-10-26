<script lang="ts">
  import type { Snippet } from 'svelte'
  import { providePane, titles, type View } from '../lib/panes.svelte'

  let {
    view,
    id,
    mode,
    side,
    focused,
    onfocus,
    onclose,
    onmenu,
    top,
    children,
  }: {
    view: View
    id: string
    // In 'tabs' mode the window is too narrow for two panes, so App draws a tab strip and only the
    // focused pane shows.
    mode: 'single' | 'split' | 'tabs'
    side: 'left' | 'right' | null
    focused: boolean
    onfocus: () => void
    onclose: () => void
    onmenu: (event: MouseEvent) => void
    top?: Snippet
    children: Snippet
  } = $props()

  providePane(() => focused)

  let section = $state<HTMLElement>()
  let scroller = $state<HTMLElement>()

  // Each pane has one scroller shared by every view it shows, so we reset it on each view change.
  $effect(() => {
    void view
    if (scroller) scroller.scrollTop = 0
  })

  // Shortcuts follow the focused pane, so when a shortcut or the rail moves focus to this pane,
  // we move keyboard focus with it. Otherwise Ctrl+S would save in one pane while you type in the other.
  $effect(() => {
    if (mode === 'split' && focused && section && !section.contains(document.activeElement)) section.focus({ preventScroll: true })
  })
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex (it's -1, so we can move focus here without adding a tab stop) -->
<section
  class="pane"
  class:split={mode === 'split'}
  class:focused
  {id}
  role={mode === 'tabs' ? 'tabpanel' : undefined}
  aria-labelledby={mode === 'tabs' ? `${id}-tab` : undefined}
  aria-label={mode === 'split' ? `${titles[view]}, ${side} pane` : undefined}
  hidden={mode === 'tabs' && !focused}
  tabindex="-1"
  bind:this={section}
  onpointerdowncapture={onfocus}
  onfocusin={onfocus}
>
  {#if mode === 'split'}
    <!-- svelte-ignore a11y_no_static_element_interactions (the menu only repeats Close and the rail's Swap sides) -->
    <header class="bar" oncontextmenu={onmenu}>
      <span class="title">{titles[view]}</span>
      <button class="icon quiet" aria-label="Close {titles[view]}" title="Close {titles[view]}" onclick={onclose}>
        <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" /></svg>
      </button>
    </header>
  {/if}
  <div class="view" bind:this={scroller}>
    {@render top?.()}
    {@render children()}
  </div>
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .pane[hidden] {
    display: none;
  }

  /* The focused bar's wash marks the pane with keyboard focus, so the section itself draws no ring. */
  .pane:focus {
    outline: none;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    padding: 0 6px 0 var(--view-inset);
    color: var(--muted);
    border-bottom: 1px solid var(--line);
  }

  .focused .bar {
    color: var(--ochre-ink);
    background: var(--here);
    border-bottom-color: var(--here-border);
  }

  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--step--1);
    font-weight: 700;
  }

  .bar button {
    display: grid;
    place-items: center;
    padding: 4px;
    color: inherit;
  }

  .bar button:hover:not(:disabled) {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .bar svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  /* We use a flex column so full-height views (Run, Files, Git) fill the space below any banners.
     The pane is a container so views lay out by the pane's width, not the window's. */
  .view {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: auto;
    padding: 28px var(--view-inset);
    container: pane / inline-size;
  }

  /* The bar already names the view, so a split pane spends less height above its header. */
  .split .view {
    padding-top: 20px;
  }
</style>
