<script lang="ts">
  import type { LogLine } from '../lib/jobs.svelte'

  let { lines, label, empty = 'Nothing logged yet.' }: { lines: LogLine[]; label: string; empty?: string } = $props()

  let viewport: HTMLDivElement
  let pinned = $state(true)
  let dragging = false
  let lastInput = 0

  const nearBottom = () => viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < 24
  const userInput = () => (lastInput = performance.now())

  function stick() {
    if (pinned && viewport) viewport.scrollTop = viewport.scrollHeight
  }

  function onScroll() {
    if (nearBottom()) pinned = true
    else if (dragging || performance.now() - lastInput < 300) pinned = false
  }

  function jumpToLatest() {
    pinned = true
    stick()
  }

  $effect(() => {
    void lines.length
    void lines[lines.length - 1]?.id
    if (!lines.length) pinned = true
    stick()
  })

  $effect(() => {
    const observer = new ResizeObserver(stick)
    observer.observe(viewport)
    return () => observer.disconnect()
  })
</script>

<svelte:window onpointerup={() => (dragging = false)} />

<div class="frame">
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="log"
    bind:this={viewport}
    onscroll={onScroll}
    onwheel={userInput}
    onkeydown={userInput}
    onpointerdown={() => (dragging = true)}
    role="log"
    aria-live="off"
    aria-label={label}
    tabindex="0"
  >
    {#each lines as line (line.id)}
      <div class="line {line.stream}">{line.text}</div>
    {:else}
      <div class="empty">{empty}</div>
    {/each}
  </div>
  {#if !pinned}
    <button class="jump" onclick={jumpToLatest}>Jump to latest</button>
  {/if}
</div>

<style>
  .frame {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 160px;
  }

  .log {
    flex: 1;
    min-height: 160px;
    overflow: auto;
    padding: 10px 12px;
    background: var(--paper);
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
    font-family: var(--font-log);
    font-size: var(--step--2);
    line-height: 1.55;
    overflow-anchor: none;
  }

  .line {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: color-mix(in srgb, var(--text) 85%, var(--muted));
  }

  .line.info {
    color: var(--forest);
  }

  .line.err {
    color: var(--fail);
  }

  .empty {
    color: var(--muted);
    font-family: var(--font-ui);
  }

  .jump {
    position: absolute;
    right: 14px;
    bottom: 12px;
    padding: 5px 12px;
    font-size: var(--step--1);
    box-shadow: 0 6px 18px var(--shadow);
  }
</style>
