<script lang="ts">
  import { tick, untrack } from 'svelte'
  import { errorText } from '../lib/api'
  import type { Level, LogModel } from '../lib/logs'
  import { shortcut } from '../lib/platform'
  import ContextMenu from './ContextMenu.svelte'
  import type { MenuItem } from '../lib/menu'

  // The model is plain data for speed, so its owner bumps `version` when lines arrive and
  // `generation` when a new log replaces the old one.
  let {
    model,
    version,
    generation,
    live = false,
    framed = false,
    loading = false,
    label,
    empty = 'Nothing logged yet.',
  }: {
    model: LogModel
    version: number
    generation: number
    live?: boolean
    framed?: boolean
    loading?: boolean
    label: string
    empty?: string
  } = $props()

  // Rows have a fixed height, so we can render only the ones on screen. A modded debug.log
  // has tens of thousands of lines.
  const ROW = 20
  const OVERSCAN = 30
  const MAX_WIDTH_CH = 2000
  const FILTERS: { level: Level; label: string }[] = [
    { level: 'error', label: 'Errors' },
    { level: 'warn', label: 'Warnings' },
    { level: 'info', label: 'Info' },
    { level: 'debug', label: 'Debug' },
  ]

  const collapsed = new Set<number>()
  let foldVersion = $state(0)
  let note = $state<string | null>(null)
  let shown = $state<Record<Level, boolean>>({ error: true, warn: true, info: true, debug: true, none: true })
  let query = $state('')
  let scrollTop = $state(0)
  let viewportHeight = $state(0)
  let pinned = $state(true)
  let selected = $state<number | null>(null)
  let spoken = $state('')
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null)
  let viewport: HTMLDivElement
  let filterField: HTMLInputElement
  let dragging = false
  let lastInput = 0

  // A new log starts at its end, with nothing folded or selected.
  $effect(() => {
    void generation
    untrack(() => {
      collapsed.clear()
      foldVersion++
      selected = null
      pinned = true
    })
  })

  // Filtering scans every line, so we wait for a pause in typing before applying it.
  let needle = $state('')
  $effect(() => {
    const next = query.trim().toLowerCase()
    const timer = setTimeout(() => (needle = next), next ? 150 : 0)
    return () => clearTimeout(timer)
  })

  // Lines only ever get appended, and clear() swaps in a new array, so we lowercase each line once.
  let lowered: string[] = []
  let loweredFrom: string[] | null = null

  function lowerLines() {
    if (loweredFrom !== model.lines) {
      loweredFrom = model.lines
      lowered = []
    }
    for (let i = lowered.length; i < model.lines.length; i++) lowered.push(model.lines[i].toLowerCase())
    return lowered
  }

  // A filter keeps whole entries, so a matching error keeps its stack trace.
  let matching = $derived.by(() => {
    void version
    void generation
    if (!needle) return null
    const lines = lowerLines()
    const entries = new Uint8Array(model.entries.length)
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].includes(needle)) entries[model.lineEntry[i]] = 1
    }
    return entries
  })

  let rows = $derived.by(() => {
    void version
    void generation
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
    void generation
    return Math.min(model.longest + 12, MAX_WIDTH_CH)
  })
  let counts = $derived.by(() => {
    void version
    void generation
    return { ...model.counts }
  })
  let matchCount = $derived(matching ? matching.reduce((sum, hit) => sum + hit, 0) : 0)

  // Scroll events arrive a frame later, so when we scroll we record the position ourselves and
  // render the right rows straight away.
  function scrollTo(top: number) {
    if (!viewport) return
    viewport.scrollTop = top
    scrollTop = viewport.scrollTop
  }

  function scrollToEnd() {
    if (viewport) scrollTo(viewport.scrollHeight)
  }

  // Following stops only when you scroll up yourself. Content arriving or the pane resizing
  // also fire scroll events, and those shouldn't let go of the end.
  function onScroll() {
    scrollTop = viewport.scrollTop
    const nearBottom = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < ROW * 2
    if (nearBottom) pinned = true
    else if (dragging || performance.now() - lastInput < 300) pinned = false
  }

  function userInput() {
    lastInput = performance.now()
  }

  $effect(() => {
    void version
    void generation
    untrack(async () => {
      if (!pinned) return
      await tick()
      scrollToEnd()
    })
  })

  $effect(() => {
    const observer = new ResizeObserver(() => pinned && scrollToEnd())
    observer.observe(viewport)
    return () => observer.disconnect()
  })

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
      scrollTo(Math.max(0, top - viewport.clientHeight / 3))
    }
  }

  // Picking a line in a live log stops following, so new lines don't scroll it away.
  function select(line: number) {
    selected = line
    if (live) pinned = false
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
    selected = null
    scrollToEnd()
  }

  // You can move the selected line with the arrow keys and open its actions with the menu key or
  // Shift+F10, so copying and filtering work without a mouse.
  function moveSelection(by: number) {
    if (!rows.length) return
    const at = selected !== null ? rows.indexOf(selected) : -1
    const from = at >= 0 ? at : Math.floor(viewport.scrollTop / ROW) - (by > 0 ? 1 : 0)
    const next = Math.max(0, Math.min(rows.length - 1, from + by))
    select(rows[next])
    pinned = false
    reveal(next)
    announce(rows[next])
  }

  function announce(line: number) {
    spoken = `Line ${line + 1}: ${model.lines[line]}`
  }

  function onKey(event: KeyboardEvent) {
    userInput()
    const command = event.ctrlKey || event.metaKey
    const page = Math.max(1, Math.floor(viewport.clientHeight / ROW) - 1)
    if (event.key === 'F8') {
      event.preventDefault()
      stepError(event.shiftKey ? -1 : 1)
      if (selected !== null) announce(selected)
    } else if (command && event.key.toLowerCase() === 'f') {
      event.preventDefault()
      filterField.focus()
      filterField.select()
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      moveSelection(event.key === 'ArrowDown' ? 1 : -1)
    } else if (event.key === 'PageDown' || event.key === 'PageUp') {
      event.preventDefault()
      moveSelection(event.key === 'PageDown' ? page : -page)
    } else if (selected !== null && (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10'))) {
      event.preventDefault()
      const row = viewport.querySelector<HTMLElement>('.row.selected')?.getBoundingClientRect()
      const box = viewport.getBoundingClientRect()
      openMenu(row ? row.left + 80 : box.left + 40, row ? row.bottom : box.top + 40, selected)
    } else if (selected !== null && command && event.key.toLowerCase() === 'c' && window.getSelection()?.isCollapsed !== false) {
      event.preventDefault()
      copy(model.lines[selected])
    } else if (event.key === 'Escape' && query) {
      query = ''
    }
  }

  function entryText(entry: number) {
    return model.lines.slice(model.entries[entry].start, model.entryEnd(entry)).join('\n')
  }

  function copy(text: string) {
    navigator.clipboard.writeText(text).catch((e) => (note = `Couldn't copy: ${errorText(e)}`))
  }

  function onRowMenu(event: MouseEvent, line: number) {
    event.preventDefault()
    event.stopPropagation()
    openMenu(event.clientX, event.clientY, line)
  }

  function openMenu(x: number, y: number, line: number) {
    select(line)
    const entry = model.lineEntry[line]
    const logger = model.entries[entry].head?.logger
    const lines = model.entryEnd(entry) - model.entries[entry].start
    menu = {
      x,
      y,
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

<svelte:window onpointerup={() => (dragging = false)} />

{#snippet marked(text: string)}
  {#each parts(text) as part, i (i)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
{/snippet}

<div class="panel" class:framed>
  <div class="bar">
    <span class="levels" role="group" aria-label="Show levels">
      {#each FILTERS as { level, label: filterLabel } (level)}
        <button class="filter {level}" aria-pressed={shown[level]} disabled={!counts[level] && !loading} onclick={() => toggleLevel(level)}>
          {filterLabel} <span class="count">{counts[level].toLocaleString()}</span>
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
        aria-label="Filter {label.toLowerCase()}"
        title="Filter lines ({shortcut('Ctrl+F')})"
        spellcheck="false"
      />
      {#if needle}<span class="muted small">{matchCount.toLocaleString()} {matchCount === 1 ? 'entry' : 'entries'}</span>{/if}
    </span>
  </div>
  {#if note}<p class="note failed small">{note}</p>{/if}

  <div class="frame">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (the log takes arrows, F8, Ctrl+F and the menu key) -->
    <div
      class="log"
      bind:this={viewport}
      bind:clientHeight={viewportHeight}
      onscroll={onScroll}
      onwheel={userInput}
      onkeydown={onKey}
      onpointerdown={() => (dragging = true)}
      role="log"
      aria-live="off"
      aria-label={label}
      tabindex="0"
    >
      <div class="content" style:height="{rows.length * ROW}px" style:width="calc({width}ch + 80px)">
        {#each rows.slice(first, last) as line, k (`${generation}:${line}`)}
          {@const entryIndex = model.lineEntry[line]}
          {@const entry = model.entries[entryIndex]}
          {@const start = entry.start === line}
          {@const extra = start ? model.entryEnd(entryIndex) - line - 1 : 0}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (the log container moves the selection with arrow keys) -->
          <div
            class="row {entry.level}"
            class:app={entry.stream === 'info'}
            class:start
            class:selected={selected === line}
            style:top="{(first + k) * ROW}px"
            onclick={() => select(line)}
            oncontextmenu={(e) => onRowMenu(e, line)}
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
      {#if loading}
        <p class="empty muted">Reading the log…</p>
      {:else if !rows.length}
        <p class="empty muted">{model.lines.length ? 'No lines match these filters.' : empty}</p>
      {/if}
    </div>
    {#if live && !pinned}
      <button class="jump" onclick={jumpToLatest}>Jump to latest</button>
    {/if}
    <span class="visually-hidden" aria-live="polite">{spoken}</span>
  </div>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} label="Log line actions" items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    container-type: inline-size;
  }

  .panel.framed {
    min-height: 240px;
    overflow: hidden;
    background: var(--field);
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
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

  .levels {
    flex-wrap: wrap;
  }

  .step {
    margin-left: 4px;
  }

  .filter {
    padding: 2px 8px;
    font-weight: 400;
    font-size: var(--step--1);
    white-space: nowrap;
    color: var(--muted);
    background: transparent;
    border-color: var(--line);
    border-radius: 999px;
  }

  .filter[aria-pressed='true']:not(:disabled) {
    color: var(--text);
    background: var(--surface);
    border-color: var(--line-strong);
  }

  .filter.error[aria-pressed='true']:not(:disabled) .count {
    color: var(--fail);
  }

  .filter.warn[aria-pressed='true']:not(:disabled) .count {
    color: var(--warn);
  }

  .count {
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  .ghost {
    width: 26px;
    padding: 2px 0;
    font-size: inherit;
  }

  .search input {
    width: 220px;
    max-width: 100%;
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
    overflow-anchor: none;
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

  /* packraven's own messages, like "Building the server pack", stand apart from the game's. */
  .row.app .message {
    color: var(--forest);
    font-weight: 700;
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

  /* A narrow pane, like the Test view's side-by-side logs, keeps its width for the message. */
  @container (max-width: 760px) {
    .thread,
    .logger {
      display: none;
    }

    .search input {
      width: 140px;
    }
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
    color: var(--hit-ink);
    background: var(--hit);
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
