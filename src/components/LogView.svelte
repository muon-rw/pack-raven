<script lang="ts">
  import type { LogLine } from '../lib/jobs.svelte'

  let { lines, label, empty = 'Nothing logged yet.' }: { lines: LogLine[]; label: string; empty?: string } = $props()

  let viewport: HTMLDivElement
  let pinned = $state(true)
  let lastTop = 0

  function nearBottom() {
    return viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < 24
  }

  function onScroll() {
    const top = viewport.scrollTop
    if (top < lastTop - 1) pinned = nearBottom()
    else if (nearBottom()) pinned = true
    lastTop = top
  }

  function jumpToLatest() {
    pinned = true
    viewport.scrollTop = viewport.scrollHeight
  }

  $effect(() => {
    void lines.length
    void lines[lines.length - 1]?.id
    if (!lines.length) pinned = true
    if (pinned && viewport) {
      viewport.scrollTop = viewport.scrollHeight
      lastTop = viewport.scrollTop
    }
  })
</script>

<div class="frame">
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="log" bind:this={viewport} onscroll={onScroll} role="log" aria-live="off" aria-label={label} tabindex="0">
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
    background: var(--ink);
    border: 1px solid var(--quill);
    border-radius: var(--radius-panel);
    font-family: var(--font-log);
    font-size: var(--step--2);
    line-height: 1.55;
  }

  .line {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: color-mix(in srgb, var(--bone) 85%, var(--ash));
    content-visibility: auto;
    contain-intrinsic-size: auto 1.55em;
  }

  .line.info {
    color: var(--teal);
  }

  .line.err {
    color: var(--rose);
  }

  .empty {
    color: var(--ash);
    font-family: var(--font-ui);
  }

  .jump {
    position: absolute;
    right: 14px;
    bottom: 12px;
    padding: 5px 12px;
    font-size: var(--step--1);
    box-shadow: 0 6px 18px rgb(5 8 16 / 0.5);
  }
</style>
