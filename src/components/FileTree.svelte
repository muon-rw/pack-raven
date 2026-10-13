<script lang="ts">
  import { SvelteMap, SvelteSet } from 'svelte/reactivity'
  import { api, errorText, type DirEntry } from '../lib/api'
  import { fs } from '../lib/workspace.svelte'
  import { untrack } from 'svelte'

  let {
    selected,
    changes,
    onopen,
  }: { selected: string | null; changes: Map<string, string>; onopen: (path: string) => void } = $props()

  const children = new SvelteMap<string, DirEntry[]>()
  const expanded = new SvelteSet<string>()
  const loading = new SvelteSet<string>()
  let focused = $state<string | null>(null)
  let error = $state<string | null>(null)
  let tree: HTMLDivElement

  type Row = { entry: DirEntry; depth: number; parent: string }

  async function load(dir: string) {
    loading.add(dir)
    try {
      children.set(dir, await api.listDir(dir))
      error = null
    } catch (e) {
      error = errorText(e)
    } finally {
      loading.delete(dir)
    }
  }

  export function refresh() {
    for (const dir of ['', ...expanded]) load(dir)
  }

  $effect(() => {
    if (!children.has('')) load('')
  })

  $effect(() => {
    if (!fs.seq) return
    for (const dir of fs.dirs) {
      if (untrack(() => children.has(dir))) load(dir)
    }
  })

  let rows = $derived.by(() => {
    const out: Row[] = []
    const walk = (dir: string, depth: number) => {
      for (const entry of children.get(dir) ?? []) {
        out.push({ entry, depth, parent: dir })
        if (entry.isDir && expanded.has(entry.path)) walk(entry.path, depth + 1)
      }
    }
    walk('', 0)
    return out
  })

  let changedDirs = $derived.by(() => {
    const dirs = new Set<string>()
    for (const path of changes.keys()) {
      let index = path.lastIndexOf('/')
      while (index > 0) {
        dirs.add(path.slice(0, index))
        index = path.lastIndexOf('/', index - 1)
      }
    }
    return dirs
  })

  function toggle(entry: DirEntry, open = !expanded.has(entry.path)) {
    if (!entry.isDir) return
    if (open) {
      expanded.add(entry.path)
      if (!children.has(entry.path)) load(entry.path)
    } else {
      expanded.delete(entry.path)
    }
  }

  function activate(row: Row) {
    focused = row.entry.path
    if (row.entry.isDir) toggle(row.entry)
    else onopen(row.entry.path)
  }

  function focusRow(path: string) {
    focused = path
    tree.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)?.focus()
  }

  function onKey(event: KeyboardEvent) {
    const index = rows.findIndex((r) => r.entry.path === focused)
    const row = rows[index]
    if (!row) return
    const move = (to: number) => rows[to] && focusRow(rows[to].entry.path)
    switch (event.key) {
      case 'ArrowDown':
        move(index + 1)
        break
      case 'ArrowUp':
        move(index - 1)
        break
      case 'Home':
        move(0)
        break
      case 'End':
        move(rows.length - 1)
        break
      case 'ArrowRight':
        if (row.entry.isDir && !expanded.has(row.entry.path)) toggle(row.entry, true)
        else if (row.entry.isDir) move(index + 1)
        break
      case 'ArrowLeft':
        if (row.entry.isDir && expanded.has(row.entry.path)) toggle(row.entry, false)
        else if (row.parent) focusRow(row.parent)
        break
      case 'Enter':
      case ' ':
        activate(row)
        break
      default:
        return
    }
    event.preventDefault()
  }

  function badge(path: string) {
    const status = changes.get(path)
    return status === '?' ? 'N' : status
  }
</script>

<div class="tree" role="tree" aria-label="Pack files" bind:this={tree} tabindex="-1" onkeydown={onKey}>
  {#if error}<p class="failed small">{error}</p>{/if}
  {#each rows as row (row.entry.path)}
    {@const status = row.entry.isDir ? null : badge(row.entry.path)}
    <!-- svelte-ignore a11y_click_events_have_key_events (keys are handled on the tree container) -->
    <div
      role="treeitem"
      aria-level={row.depth + 1}
      aria-expanded={row.entry.isDir ? expanded.has(row.entry.path) : undefined}
      aria-selected={row.entry.path === selected}
      tabindex={row.entry.path === (focused ?? rows[0]?.entry.path) ? 0 : -1}
      data-path={row.entry.path}
      class="row"
      class:selected={row.entry.path === selected}
      class:ignored={row.entry.ignored}
      style="--depth: {row.depth}"
      onclick={() => activate(row)}
      onfocus={() => (focused = row.entry.path)}
    >
      <span class="twisty" aria-hidden="true">
        {#if row.entry.isDir}
          <svg viewBox="0 0 12 12" class:open={expanded.has(row.entry.path)}><path d="M4.5 3l3 3-3 3" /></svg>
        {/if}
      </span>
      <span class="name" class:dir={row.entry.isDir}>{row.entry.name}</span>
      {#if loading.has(row.entry.path)}<span class="small muted">loading</span>{/if}
      {#if status}
        <span class="status s{status}" title="Changed in git">{status}</span>
      {:else if row.entry.isDir && changedDirs.has(row.entry.path)}
        <span class="dot" title="Contains changes"></span>
      {/if}
    </div>
  {/each}
</div>

<style>
  .tree {
    min-height: 0;
    overflow: auto;
    padding: 4px 0;
    font-size: var(--step--1);
    outline: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 10px 3px calc(8px + var(--depth) * 16px);
    white-space: nowrap;
    cursor: pointer;
    user-select: none;
  }

  .row:hover {
    background: var(--surface);
  }

  .row:focus-visible {
    outline: 2px solid var(--forest);
    outline-offset: -2px;
  }

  .row.selected {
    background: var(--here);
  }

  .row.ignored .name {
    color: var(--muted);
    opacity: 0.75;
  }

  .twisty {
    flex: none;
    width: 14px;
    height: 14px;
    display: grid;
    place-items: center;
  }

  .twisty svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.6;
    transition: transform 0.1s ease;
  }

  .twisty svg.open {
    transform: rotate(90deg);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .name.dir {
    font-weight: 600;
  }

  .status {
    flex: none;
    font-family: var(--font-log);
    font-size: var(--step--2);
    font-weight: 700;
    color: var(--warn);
  }

  .status.sA,
  .status.sN {
    color: var(--forest);
  }

  .status.sD {
    color: var(--fail);
  }

  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--warn);
  }

  .small {
    font-size: var(--step--2);
    padding: 0 10px;
  }
</style>
