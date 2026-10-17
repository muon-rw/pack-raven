<script lang="ts">
  import { clamp } from '../lib/storedSplit.svelte'

  let {
    value,
    min,
    max,
    total,
    label,
    controls,
    onresize,
    onreset,
  }: {
    value: number
    min: number
    max: number
    total: number
    label: string
    controls: string
    onresize: (value: number) => void
    onreset: () => void
  } = $props()

  const STEP = 16
  const BIG_STEP = 64

  let drag = $state<{ x: number; value: number } | null>(null)
  let percent = (px: number) => (total > 0 ? Math.round((100 * px) / total) : 50)

  function start(event: PointerEvent) {
    if (event.button !== 0) return
    // Keeps the drag from selecting text in the panes.
    event.preventDefault()
    ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
    drag = { x: event.clientX, value }
  }

  function move(event: PointerEvent) {
    if (drag) onresize(clamp(drag.value + event.clientX - drag.x, min, max))
  }

  function key(event: KeyboardEvent) {
    const step = event.shiftKey ? BIG_STEP : STEP
    const next = { ArrowLeft: value - step, ArrowRight: value + step, Home: min, End: max }[event.key]
    if (next !== undefined) onresize(clamp(next, min, max))
    else if (event.key === 'Enter') onreset()
    else return
    event.preventDefault()
  }
</script>

<!-- A focusable separator is interactive under the ARIA window splitter pattern, which Svelte's check doesn't know. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="splitter"
  class:dragging={!!drag}
  role="separator"
  tabindex="0"
  aria-orientation="vertical"
  aria-label={label}
  aria-controls={controls}
  aria-valuenow={percent(value)}
  aria-valuemin={percent(min)}
  aria-valuemax={percent(max)}
  title="Drag to resize. Double-click to reset."
  onpointerdown={start}
  onpointermove={move}
  onpointerup={() => (drag = null)}
  onpointercancel={() => (drag = null)}
  ondblclick={onreset}
  onkeydown={key}
></div>

<style>
  /* The handle fills the gap between the panes, so the whole gap is the hit area. */
  .splitter {
    grid-column: 2;
    grid-row: 1 / -1;
    position: relative;
    cursor: col-resize;
    touch-action: none;
  }

  .splitter::before {
    content: '';
    position: absolute;
    inset: 0 auto;
    left: 50%;
    width: 1px;
    translate: -50% 0;
    border-radius: 1px;
    background: transparent;
    transition:
      background-color 0.12s ease,
      width 0.12s ease;
  }

  .splitter:hover::before {
    background: var(--edge);
  }

  .splitter:focus-visible {
    outline: none;
  }

  .splitter.dragging::before,
  .splitter:focus-visible::before {
    width: 2px;
    background: var(--forest);
  }

  @media (prefers-reduced-motion: reduce) {
    .splitter::before {
      transition: none;
    }
  }
</style>
