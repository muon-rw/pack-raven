<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity'
  import { api, errorText, type NbtDocument, type NbtTag } from '../lib/api'

  let { path, onfail }: { path: string; onfail: (message: string) => void } = $props()

  const MAX_EXPANDED = 5000

  type Child = { name: string; tag: NbtTag }

  let nbt = $state<NbtDocument | null>(null)
  const expanded = new SvelteSet<string>()

  $effect(() => {
    const current = path
    nbt = null
    expanded.clear()
    expanded.add('')
    focused = 0
    api.readNbt(current).then(
      (loaded) => {
        if (path === current) nbt = loaded
      },
      (e) => path === current && onfail(errorText(e)),
    )
  })

  const labels: Record<NbtTag['type'], string> = {
    byte: 'byte',
    short: 'short',
    int: 'int',
    long: 'long',
    float: 'float',
    double: 'double',
    string: 'string',
    byteArray: 'byte[]',
    intArray: 'int[]',
    longArray: 'long[]',
    list: 'list',
    compound: 'compound',
  }

  const arrayItemType = { byteArray: 'byte', intArray: 'int', longArray: 'long' } as const

  function children(tag: NbtTag): Child[] | null {
    switch (tag.type) {
      case 'compound':
        return tag.value.map(([name, child]) => ({ name, tag: child }))
      case 'list':
        return tag.value.items.map((child, i) => ({ name: `[${i}]`, tag: child }))
      case 'byteArray':
      case 'intArray':
      case 'longArray':
        return tag.value.items.map((item, i) => ({ name: `[${i}]`, tag: { type: arrayItemType[tag.type], value: item } as NbtTag }))
      default:
        return null
    }
  }

  function plural(count: number, noun: string) {
    return `${count.toLocaleString()} ${noun}${count === 1 ? '' : 's'}`
  }

  // We use SNBT's suffixes, which pack devs know from commands, so a byte reads as 1b.
  function summary(tag: NbtTag) {
    switch (tag.type) {
      case 'byte':
        return `${tag.value}b`
      case 'short':
        return `${tag.value}s`
      case 'int':
        return `${tag.value}`
      case 'long':
        return `${tag.value}L`
      case 'float':
        return `${tag.value}f`
      case 'double':
        return `${tag.value}d`
      case 'string':
        return JSON.stringify(tag.value)
      case 'compound':
        return `${tag.value.length.toLocaleString()} ${tag.value.length === 1 ? 'entry' : 'entries'}`
      case 'list':
        return tag.value.items.length ? `${plural(tag.value.items.length, 'item')} of ${labels[tag.value.itemType as NbtTag['type']] ?? tag.value.itemType}` : 'empty'
      default:
        return plural(tag.value.length, 'item')
    }
  }

  function hidden(tag: NbtTag) {
    return tag.type === 'byteArray' || tag.type === 'intArray' || tag.type === 'longArray' ? tag.value.length - tag.value.items.length : 0
  }

  function container(tag: NbtTag) {
    return children(tag) !== null
  }

  const ROW = 22
  const OVERSCAN = 20

  type Row = { kind: 'tag'; id: string; name: string; tag: NbtTag; depth: number; kids: number; parent: number } | { kind: 'more'; id: string; depth: number; count: number }

  function toggle(id: string) {
    if (expanded.has(id)) expanded.delete(id)
    else expanded.add(id)
  }

  // We walk the queue by index instead of shift(), so a 30k-entry list stays linear.
  function expandAll() {
    if (!nbt) return
    const queue: [string, NbtTag][] = [['', nbt.root]]
    for (let at = 0; at < queue.length && expanded.size < MAX_EXPANDED; at++) {
      const [id, tag] = queue[at]
      const kids = children(tag)
      if (!kids?.length) continue
      expanded.add(id)
      kids.forEach((child, i) => queue.push([`${id}/${i}`, child.tag]))
    }
  }

  function collapseAll() {
    expanded.clear()
    expanded.add('')
    focused = 0
  }

  let tagCount = $derived.by(() => {
    let count = 0
    // An array is one tag however many numbers it holds.
    const walk = (tag: NbtTag) => {
      count++
      if (tag.type === 'compound' || tag.type === 'list') for (const child of children(tag)!) walk(child.tag)
    }
    if (nbt) walk(nbt.root)
    return count
  })

  // We render only the rows in view, so expanding a huge structure file stays cheap.
  let rows = $derived.by(() => {
    const list: Row[] = []
    if (!nbt) return list
    const visit = (name: string, tag: NbtTag, id: string, depth: number, parent: number) => {
      const kids = children(tag)
      const index = list.length
      list.push({ kind: 'tag', id, name, tag, depth, kids: kids?.length ?? 0, parent })
      if (!kids || !expanded.has(id)) return
      kids.forEach((child, i) => visit(child.name, child.tag, `${id}/${i}`, depth + 1, index))
      const more = hidden(tag)
      if (more > 0) list.push({ kind: 'more', id: `${id}/more`, depth: depth + 1, count: more })
    }
    visit(nbt.rootName, nbt.root, '', 0, -1)
    return list
  })

  let viewport = $state<HTMLDivElement>()
  let scrollTop = $state(0)
  let height = $state(0)
  let focused = $state(0)
  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN))
  let last = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN))

  function moveTo(index: number) {
    focused = Math.max(0, Math.min(rows.length - 1, index))
    if (!viewport) return
    const top = focused * ROW
    if (top < viewport.scrollTop) viewport.scrollTop = top
    else if (top + ROW > viewport.scrollTop + viewport.clientHeight) viewport.scrollTop = top + ROW - viewport.clientHeight
  }

  function onKey(event: KeyboardEvent) {
    const row = rows[focused]
    if (!row) return
    const page = Math.max(1, Math.floor((viewport?.clientHeight ?? ROW) / ROW) - 1)
    const tagRow = row.kind === 'tag' ? row : null
    const open = tagRow ? expanded.has(tagRow.id) : false
    let handled = true
    if (event.key === 'ArrowDown') moveTo(focused + 1)
    else if (event.key === 'ArrowUp') moveTo(focused - 1)
    else if (event.key === 'PageDown') moveTo(focused + page)
    else if (event.key === 'PageUp') moveTo(focused - page)
    else if (event.key === 'Home') moveTo(0)
    else if (event.key === 'End') moveTo(rows.length - 1)
    else if (event.key === 'ArrowRight' && tagRow?.kids) open ? moveTo(focused + 1) : expanded.add(tagRow.id)
    else if (event.key === 'ArrowLeft' && tagRow?.kids && open) expanded.delete(tagRow.id)
    else if (event.key === 'ArrowLeft' && tagRow && tagRow.parent >= 0) moveTo(tagRow.parent)
    else if ((event.key === 'Enter' || event.key === ' ') && tagRow?.kids) toggle(tagRow.id)
    else handled = false
    if (handled) event.preventDefault()
  }

  $effect(() => {
    if (focused >= rows.length) focused = Math.max(0, rows.length - 1)
  })
</script>

<div class="viewer">
  <div class="bar">
    <span class="muted">
      {#if nbt}
        {plural(tagCount, 'tag')}, {nbt.compression === 'none' ? 'uncompressed' : `${nbt.compression}-compressed`}. Read-only.
      {:else}
        Reading the file…
      {/if}
    </span>
    <span class="controls">
      <button class="ghost" disabled={!nbt} onclick={expandAll}>Expand all</button>
      <button class="ghost" disabled={!nbt} onclick={collapseAll}>Collapse all</button>
    </span>
  </div>
  {#if nbt}
    <div
      class="tree"
      role="tree"
      aria-label="NBT tags"
      tabindex="0"
      aria-activedescendant="nbt-row-{focused}"
      bind:this={viewport}
      bind:clientHeight={height}
      onscroll={() => (scrollTop = viewport!.scrollTop)}
      onkeydown={onKey}
    >
      <div class="space" style:height="{rows.length * ROW}px">
        {#each rows.slice(first, last) as row, k (row.id)}
          {@const index = first + k}
          {#if row.kind === 'tag'}
            {@const open = expanded.has(row.id)}
            <!-- svelte-ignore a11y_click_events_have_key_events (the tree handles keys for the focused row) -->
            <div
              id="nbt-row-{index}"
              class="row"
              class:focused={index === focused}
              style:top="{index * ROW}px"
              style="--depth: {row.depth}"
              role="treeitem"
              tabindex="-1"
              aria-level={row.depth + 1}
              aria-expanded={row.kids ? open : undefined}
              aria-selected={index === focused}
              onclick={() => (focused = index)}
            >
              {#if row.kids}
                <button class="twisty" tabindex="-1" aria-hidden="true" onclick={() => toggle(row.id)}>
                  <svg viewBox="0 0 12 12" class:open aria-hidden="true"><path d="M4.5 3l3 3-3 3" /></svg>
                </button>
              {:else}
                <span class="twisty" aria-hidden="true"></span>
              {/if}
              <span class="type">{labels[row.tag.type]}</span>
              {#if row.name}<span class="name">{row.name}</span>{/if}
              <span class="value" class:string={row.tag.type === 'string'} class:container={container(row.tag)} title={row.tag.type === 'string' ? row.tag.value : undefined}>{summary(row.tag)}</span>
            </div>
          {:else}
            <p id="nbt-row-{index}" class="row more muted" style:top="{index * ROW}px" style="--depth: {row.depth}">…and {row.count.toLocaleString()} more, which we don't show</p>
          {/if}
        {/each}
      </div>
    </div>
  {/if}
</div>

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
    padding: 6px 10px 6px 14px;
    font-size: var(--step--1);
    border-bottom: 1px solid var(--line);
  }

  .controls {
    display: flex;
    gap: 2px;
  }

  .tree {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 6px 0;
    font-family: var(--font-log);
    font-size: var(--step--1);
  }

  .tree:focus-visible {
    outline-offset: -2px;
  }

  .space {
    position: relative;
    min-width: max-content;
  }

  .row {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    margin: 0;
    padding: 0 12px 0 calc(8px + var(--depth) * 18px);
    white-space: nowrap;
  }

  .row:hover {
    background: var(--surface);
  }

  .tree:focus-visible .row.focused {
    background: var(--highlight);
    box-shadow: inset 2px 0 0 var(--forest);
  }

  .twisty {
    flex: none;
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    padding: 0;
    background: transparent;
    border: 0;
    cursor: pointer;
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

  .type {
    flex: none;
    min-width: 8ch;
    font-size: var(--step--2);
    color: var(--muted);
  }

  .name {
    flex: none;
    font-weight: 700;
  }

  .name::after {
    content: ':';
    font-weight: 400;
    color: var(--muted);
  }

  .value {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--warn);
  }

  .value.string {
    color: var(--ochre-ink);
  }

  .value.container {
    color: var(--muted);
  }

  .row.more {
    max-width: none;
    padding-left: calc(30px + var(--depth) * 18px);
    font-size: var(--step--2);
  }

  @media (prefers-reduced-motion: reduce) {
    .twisty svg {
      transition: none;
    }
  }
</style>
