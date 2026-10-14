<script lang="ts">
  import { tick, untrack } from 'svelte'
  import { api, errorText } from '../lib/api'
  import { LogModel, type Level } from '../lib/logs'
  import { shortcut } from '../lib/platform'
  import ContextMenu from './ContextMenu.svelte'
  import type { MenuItem } from '../lib/menu'

  let { path }: { path: string } = $props()

  // Rows have a fixed height, so we can render only the ones on screen. A modded debug.log
  // has tens of thousands of lines.
  const ROW = 20
  const OVERSCAN = 30
  const POLL_MS = 1000
  const MAX_WIDTH_CH = 2000
  const FILTERS: { level: Level; label: string }[] = [
    { level: 'error', label: 'Errors' },
    { level: 'warn', label: 'Warnings' },
    { level: 'info', label: 'Info' },
    { level: 'debug', label: 'Debug' },
  ]

  // The model is plain data for speed, so we bump these counters to tell Svelte it changed.
  const model = new LogModel()
  let version = $state(0)
  // Rows are keyed by line number, so when a new log replaces the old one we change the keys
  // too, or Svelte would keep showing the old lines' text.
  let generation = $state(0)
  const collapsed = new Set<number>()
  let foldVersion = $state(0)

  let loadError = $state<string | null>(null)
  let loaded = $state(false)
  let truncated = $state(false)
  let followable = $state(false)
  let offset = 0
  let fetching = false
  let shown = $state<Record<Level, boolean>>({ error: true, warn: true, info: true, debug: true, none: true })
  let query = $state('')
  let scrollTop = $state(0)
  let viewportHeight = $state(0)
  let pinned = $state(true)
  let selected = $state<number | null>(null)
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null)
  let viewport: HTMLDivElement
  let filterField: HTMLInputElement

  export function reload() {
    offset = 0
    return fetchMore()
  }

  async function fetchMore() {
    if (fetching) return
    fetching = true
    const current = path
    try {
      const chunk = await api.readLog(current, offset)
      if (current !== path) return
      if (chunk.reset) {
        generation++
        model.clear()
        collapsed.clear()
        selected = null
        truncated = chunk.truncated
      }
      model.append(chunk.text)
      offset = chunk.offset
      followable = chunk.followable
      loadError = null
      loaded = true
      if (chunk.reset || chunk.text) {
        version++
        if (pinned) {
          await tick()
          scrollToEnd()
        }
      }
    } catch (e) {
      loadError = errorText(e)
    } finally {
      fetching = false
    }
  }

  // Minecraft keeps writing latest.log while the game runs, so we read whatever it added.
  $effect(() => {
    void path
    untrack(() => {
      offset = 0
      loaded = false
      pinned = true
      selected = null
      fetchMore()
    })
    const timer = setInterval(() => followable && !document.hidden && fetchMore(), POLL_MS)
    return () => clearInterval(timer)
  })

  let needle = $derived(query.trim().toLowerCase())

  // A filter keeps whole entries, so a matching error keeps its stack trace.
  let matching = $derived.by(() => {
    void version
    if (!needle) return null
    const entries = new Uint8Array(model.entries.length)
    for (let i = 0; i < model.lines.length; i++) {
      if (model.lines[i].toLowerCase().includes(needle)) entries[model.lineEntry[i]] = 1
    }
    return entries
  })

  let rows = $derived.by(() => {
    void version
    void foldVersion
    const out: number[] = []
    for (let i = 0; i < model.lines.length; i++) {
      const entry = model.lineEntry[i]
      if (!shown[model.entries[entry].level]) continue
      if (matching && !matching[entry]) continue
      if (i !== model.entries[entry].start && collapsed.has(entry)) continue
      out.push(i)
    }
    return out
  })

  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN))
  let last = $derived(Math.min(rows.length, Math.ceil((scrollTop + viewportHeight) / ROW) + OVERSCAN))
  let width = $derived.by(() => {
    void version
    return Math.min(model.longest + 12, MAX_WIDTH_CH)
  })
  let counts = $derived.by(() => {
    void version
    return { ...model.counts }
  })
  let matchCount = $derived(matching ? matching.reduce((sum, hit) => sum + hit, 0) : 0)

  function scrollToEnd() {
    if (viewport) viewport.scrollTop = viewport.scrollHeight
  }

  function onScroll() {
    scrollTop = viewport.scrollTop
    pinned = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < ROW * 2
  }

  // When a filter changes, we keep the selected line in view, or the end if you were following.
  $effect(() => {
    void shown.error
    void shown.warn
    void shown.info
    void shown.debug
    void needle
    void foldVersion
    untrack(async () => {
      await tick()
      const at = selected === null ? -1 : rows.indexOf(selected)
      if (at >= 0) reveal(at)
      else if (pinned) scrollToEnd()
    })
  })

  function reveal(rowIndex: number) {
    if (!viewport) return
    const top = rowIndex * ROW
    if (top < viewport.scrollTop || top + ROW > viewport.scrollTop + viewport.clientHeight) {
      viewport.scrollTop = Math.max(0, top - viewport.clientHeight / 3)
    }
  }

  function toggleLevel(level: Level) {
    shown[level] = !shown[level]
  }

  function folded(entry: number) {
    void foldVersion
    return collapsed.has(entry)
  }

  function toggleFold(entry: number) {
    if (collapsed.has(entry)) collapsed.delete(entry)
    else collapsed.add(entry)
    foldVersion++
  }

  function isStart(line: number) {
    return model.entries[model.lineEntry[line]].start === line
  }

  // Like IDEs, F8 and Shift+F8 step through errors, starting from the selected line, or else
  // from the edge of the view they move away from, so errors already on screen count.
  function stepError(direction: 1 | -1) {
    const edge = direction > 0 ? Math.floor(viewport.scrollTop / ROW) - 1 : Math.ceil((viewport.scrollTop + viewport.clientHeight) / ROW)
    const from = selected !== null && rows.includes(selected) ? rows.indexOf(selected) : edge
    for (let at = from + direction; at >= 0 && at < rows.length; at += direction) {
      const line = rows[at]
      if (isStart(line) && model.entries[model.lineEntry[line]].level === 'error') {
        selected = line
        pinned = false
        reveal(at)
        return
      }
    }
  }

  function jumpToLatest() {
    pinned = true
    scrollToEnd()
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === 'F8') {
      event.preventDefault()
      stepError(event.shiftKey ? -1 : 1)
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
      event.preventDefault()
      filterField.focus()
      filterField.select()
    } else if (event.key === 'Escape' && query) {
      query = ''
    }
  }

  function entryText(entry: number) {
    return model.lines.slice(model.entries[entry].start, model.entryEnd(entry)).join('\n')
  }

  function copy(text: string) {
    navigator.clipboard.writeText(text).catch((e) => (loadError = `Couldn't copy: ${errorText(e)}`))
  }

  function openMenu(event: MouseEvent, line: number) {
    event.preventDefault()
    event.stopPropagation()
    selected = line
    const entry = model.lineEntry[line]
    const logger = model.entries[entry].head?.logger
    const lines = model.entryEnd(entry) - model.entries[entry].start
    menu = {
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Copy line', run: () => copy(model.lines[line]) },
        ...(lines > 1 ? [{ label: `Copy entry (${lines} lines)`, run: () => copy(entryText(entry)) }] : []),
        'separator',
        ...(logger ? [{ label: `Filter to ${shortLogger(logger)}`, run: () => (query = logger) }] : []),
      ],
    }
  }

  function shortLogger(logger: string) {
    return logger.slice(logger.lastIndexOf('.') + 1)
  }

  function parts(text: string) {
    if (!needle) return [{ text, hit: false }]
    const out: { text: string; hit: boolean }[] = []
    const lower = text.toLowerCase()
    let at = 0
    for (let found = lower.indexOf(needle); found >= 0; found = lower.indexOf(needle, found + needle.length)) {
      if (found > at) out.push({ text: text.slice(at, found), hit: false })
      out.push({ text: text.slice(found, found + needle.length), hit: true })
      at = found + needle.length
    }
    if (at < text.length) out.push({ text: text.slice(at), hit: false })
    return out
  }
</script>

{#snippet marked(text: string)}
  {#each parts(text) as part, i (i)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
{/snippet}

<div class="viewer">
  <div class="bar">
    <span class="levels" role="group" aria-label="Show levels">
      {#each FILTERS as { level, label } (level)}
        <button class="chip {level}" aria-pressed={shown[level]} disabled={!counts[level] && loaded} onclick={() => toggleLevel(level)}>
          {label} <span class="count">{counts[level].toLocaleString()}</span>
        </button>
      {/each}
      <span class="step">
        <button class="ghost" aria-label="Previous error" title="Previous error (Shift+F8)" disabled={!counts.error} onclick={() => stepError(-1)}>↑</button>
        <button class="ghost" aria-label="Next error" title="Next error (F8)" disabled={!counts.error} onclick={() => stepError(1)}>↓</button>
      </span>
    </span>
    <span class="search">
      <input
        bind:this={filterField}
        bind:value={query}
        type="search"
        placeholder="Filter lines"
        aria-label="Filter log lines"
        title="Filter log lines ({shortcut('Ctrl+F')})"
        spellcheck="false"
      />
      {#if needle}<span class="muted small">{matchCount.toLocaleString()} {matchCount === 1 ? 'entry' : 'entries'}</span>{/if}
    </span>
  </div>
  {#if truncated}<p class="note muted small">This log is larger than 64 MB, so we show only its end.</p>{/if}
  {#if loadError}<p class="note failed small">{loadError}</p>{/if}

  <div class="frame">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (the log takes F8 and Ctrl+F) -->
    <div class="log" bind:this={viewport} bind:clientHeight={viewportHeight} onscroll={onScroll} onkeydown={onKey} role="log" aria-label="Log lines" tabindex="0">
      <div class="content" style:height="{rows.length * ROW}px" style:width="calc({width}ch + 80px)">
        {#each rows.slice(first, last) as line, k (`${generation}:${line}`)}
          {@const entryIndex = model.lineEntry[line]}
          {@const entry = model.entries[entryIndex]}
          {@const start = entry.start === line}
          {@const extra = start ? model.entryEnd(entryIndex) - line - 1 : 0}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (the log container handles keys) -->
          <div
            class="row {entry.level}"
            class:start
            class:selected={selected === line}
            style:top="{(first + k) * ROW}px"
            onclick={() => (selected = line)}
            oncontextmenu={(e) => openMenu(e, line)}
          >
            <span class="number">{line + 1}</span>
            {#if extra}
              <button class="fold" aria-label="{folded(entryIndex) ? 'Show' : 'Hide'} {extra} more lines" onclick={(e) => (e.stopPropagation(), toggleFold(entryIndex))}>
                <svg viewBox="0 0 12 12" class:open={!folded(entryIndex)} aria-hidden="true"><path d="M4.5 3l3 3-3 3" /></svg>
              </button>
            {:else}
              <span class="fold" aria-hidden="true"></span>
            {/if}
            {#if start && entry.head}
              <span class="time">{entry.head.time}</span>
              <span class="level">{entry.head.level}</span>
              <span class="thread" title={entry.head.thread}>{entry.head.thread}</span>
              {#if entry.head.logger}<span class="logger" title={[entry.head.logger, entry.head.marker].filter(Boolean).join(' / ')}>{shortLogger(entry.head.logger)}</span>{/if}
              <span class="message">{@render marked(entry.head.message)}</span>
              {#if extra && folded(entryIndex)}<span class="hidden-lines">{extra} more {extra === 1 ? 'line' : 'lines'}</span>{/if}
            {:else}
              <span class="message">{@render marked(model.lines[line])}</span>
            {/if}
          </div>
        {/each}
      </div>
      {#if loaded && !rows.length}
        <p class="empty muted">{model.lines.length ? 'No lines match these filters.' : 'This log is empty.'}</p>
      {:else if !loaded && !loadError}
        <p class="empty muted">Reading the log…</p>
      {/if}
    </div>
    {#if followable && !pinned}
      <button class="jump" onclick={jumpToLatest}>Jump to latest</button>
    {/if}
  </div>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} label="Log line actions" items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .viewer {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .bar {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 6px 12px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--line);
  }

  .levels,
  .search,
  .step {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .step {
    margin-left: 4px;
  }

  .chip {
    padding: 2px 8px;
    font-weight: 400;
    font-size: var(--step--1);
    color: var(--muted);
    background: transparent;
    border-color: var(--line);
  }

  .chip[aria-pressed='true'] {
    color: var(--text);
    background: var(--surface);
    border-color: var(--line-strong);
  }

  .chip.error[aria-pressed='true'] .count {
    color: var(--fail);
  }

  .chip.warn[aria-pressed='true'] .count {
    color: var(--warn);
  }

  .count {
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  .ghost {
    width: 26px;
    padding: 2px 0;
    font-weight: 400;
    background: transparent;
    border-color: transparent;
  }

  .ghost:hover:not(:disabled) {
    background: var(--control);
  }

  .search input {
    width: 220px;
    padding: 3px 8px;
    font-size: var(--step--1);
  }

  .small {
    font-size: var(--step--1);
  }

  .note {
    padding: 6px 12px;
    border-bottom: 1px solid var(--line);
  }

  .frame {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .log {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow: auto;
    font-family: var(--font-log);
    font-size: var(--step--2);
    outline: none;
  }

  .log:focus-visible {
    box-shadow: inset 0 0 0 2px var(--forest);
  }

  .content {
    position: relative;
    min-width: 100%;
  }

  .row {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 20px;
    padding-right: 12px;
    white-space: pre;
    tab-size: 4;
    color: var(--text);
  }

  .row:hover {
    background: var(--surface);
  }

  .row.selected {
    background: var(--here);
  }

  .row.error {
    color: var(--fail);
  }

  .row.error.start:not(.selected) {
    background: var(--fail-fill);
  }

  .row.warn .message {
    color: var(--warn);
  }

  .row.debug {
    color: var(--muted);
  }

  .number {
    flex: none;
    width: 6ch;
    padding-right: 4px;
    text-align: right;
    color: var(--muted);
    background: var(--paper);
    border-right: 1px solid var(--line);
    user-select: none;
    font-variant-numeric: tabular-nums;
  }

  .fold {
    flex: none;
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    padding: 0;
    background: transparent;
    border: 0;
  }

  .fold svg {
    width: 11px;
    height: 11px;
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.6;
    transition: transform 0.1s ease;
  }

  .fold svg.open {
    transform: rotate(90deg);
  }

  .time {
    flex: none;
    min-width: 8ch;
    color: var(--muted);
  }

  .level {
    flex: none;
    width: 5ch;
    font-weight: 700;
  }

  /* Fixed columns keep the messages lined up; the full names show on hover. */
  .thread,
  .logger {
    flex: none;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--muted);
  }

  .thread {
    width: 13ch;
  }

  .logger {
    width: 16ch;
    color: var(--forest);
  }

  .message {
    flex: none;
  }

  .hidden-lines {
    flex: none;
    padding: 0 6px;
    font-family: var(--font-ui);
    color: var(--muted);
    border: 1px solid var(--line);
    border-radius: 999px;
  }

  mark {
    color: inherit;
    background: color-mix(in srgb, var(--ochre) 55%, transparent);
    border-radius: 2px;
  }

  .empty {
    position: absolute;
    top: 16px;
    left: 0;
    right: 0;
    text-align: center;
    font-family: var(--font-ui);
  }

  .jump {
    position: absolute;
    right: 18px;
    bottom: 14px;
    padding: 5px 12px;
    font-size: var(--step--1);
    box-shadow: 0 6px 18px var(--shadow);
  }

  @media (prefers-reduced-motion: reduce) {
    .fold svg {
      transition: none;
    }
  }
</style>
