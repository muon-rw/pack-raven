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

  function toggle(id: string) {
    if (expanded.has(id)) expanded.delete(id)
    else expanded.add(id)
  }

  function expandAll() {
    if (!nbt) return
    const queue: [string, NbtTag][] = [['', nbt.root]]
    while (queue.length && expanded.size < MAX_EXPANDED) {
      const [id, tag] = queue.shift()!
      const kids = children(tag)
      if (!kids?.length) continue
      expanded.add(id)
      kids.forEach((child, i) => queue.push([`${id}/${i}`, child.tag]))
    }
  }

  function collapseAll() {
    expanded.clear()
    expanded.add('')
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
</script>

{#snippet node(name: string, tag: NbtTag, id: string, depth: number)}
  {@const kids = children(tag)}
  {@const open = expanded.has(id)}
  <div class="row" style="--depth: {depth}" role="treeitem" aria-level={depth + 1} aria-expanded={kids?.length ? open : undefined} aria-selected="false">
    {#if kids?.length}
      <button class="twisty" aria-label="{open ? 'Collapse' : 'Expand'} {name}" onclick={() => toggle(id)}>
        <svg viewBox="0 0 12 12" class:open aria-hidden="true"><path d="M4.5 3l3 3-3 3" /></svg>
      </button>
    {:else}
      <span class="twisty" aria-hidden="true"></span>
    {/if}
    <span class="type">{labels[tag.type]}</span>
    {#if name}<span class="name">{name}</span>{/if}
    <span class="value" class:string={tag.type === 'string'} class:container={container(tag)} title={tag.type === 'string' ? tag.value : undefined}>{summary(tag)}</span>
  </div>
  {#if kids && open}
    {#each kids as child, i (i)}
      {@render node(child.name, child.tag, `${id}/${i}`, depth + 1)}
    {/each}
    {#if hidden(tag) > 0}
      <p class="more muted" style="--depth: {depth + 1}">…and {hidden(tag).toLocaleString()} more, which we don't show</p>
    {/if}
  {/if}
{/snippet}

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
    <div class="tree" role="tree" aria-label="NBT tags">
      {@render node(nbt.rootName, nbt.root, '', 0)}
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

  .ghost {
    padding: 2px 8px;
    font-weight: 400;
    font-size: var(--step--1);
    background: transparent;
    border-color: transparent;
  }

  .ghost:hover:not(:disabled) {
    background: var(--control);
  }

  .tree {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 6px 0;
    font-family: var(--font-log);
    font-size: var(--step--1);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 1px 12px 1px calc(8px + var(--depth) * 18px);
    white-space: nowrap;
    content-visibility: auto;
    contain-intrinsic-size: auto 22px;
  }

  .row:hover {
    background: var(--surface);
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

  .more {
    padding: 2px 12px 2px calc(30px + var(--depth) * 18px);
    font-size: var(--step--2);
  }

  @media (prefers-reduced-motion: reduce) {
    .twisty svg {
      transition: none;
    }
  }
</style>
