<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { api, errorText } from '../lib/api'
  import { fs } from '../lib/workspace.svelte'

  let {
    query,
    active = $bindable(0),
    onopen,
    onshowfolder,
  }: {
    query: string
    active?: number
    onopen: (path: string, pinned: boolean) => void
    onshowfolder: (path: string) => void
  } = $props()

  const MAX_SHOWN = 200

  type Result = { path: string; name: string; dir: string; isDir: boolean; rank: number }

  let index = $state<{ paths: string[]; truncated: boolean } | null>(null)
  let loadError = $state<string | null>(null)
  let list: HTMLUListElement

  async function load() {
    try {
      index = await api.indexFiles()
      loadError = null
    } catch (e) {
      loadError = errorText(e)
    }
  }

  onMount(load)

  $effect(() => {
    if (fs.seq) load()
  })

  let tokens = $derived(query.trim().toLowerCase().split(/\s+/).filter(Boolean))

  // Every word you type has to appear somewhere in the path. We rank a match in the name above
  // one in the folders, a name that starts with the word above one that only contains it, and
  // shorter paths first among equals.
  let matches = $derived.by(() => {
    const out: Result[] = []
    for (const raw of index?.paths ?? []) {
      const isDir = raw.endsWith('/')
      const path = isDir ? raw.slice(0, -1) : raw
      const lower = path.toLowerCase()
      const cut = path.lastIndexOf('/') + 1
      const name = lower.slice(cut)
      let rank = 0
      for (const token of tokens) {
        if (!lower.includes(token)) {
          rank = -1
          break
        }
        rank += name === token ? 0 : name.startsWith(token) ? 1 : name.includes(token) ? 2 : 4
      }
      if (rank >= 0) out.push({ path, name: path.slice(cut), dir: path.slice(0, Math.max(cut - 1, 0)), isDir, rank: rank * 10_000 + path.length })
    }
    return out.sort((a, b) => a.rank - b.rank)
  })

  let shown = $derived(matches.slice(0, MAX_SHOWN))

  $effect(() => {
    void tokens
    active = 0
  })

  function segments(text: string) {
    const lower = text.toLowerCase()
    const hit = new Array<boolean>(text.length).fill(false)
    for (const token of tokens) {
      for (let at = lower.indexOf(token); at >= 0; at = lower.indexOf(token, at + 1)) hit.fill(true, at, at + token.length)
    }
    const out: { text: string; hit: boolean }[] = []
    for (let i = 0; i < text.length; i++) {
      if (out.length && out.at(-1)!.hit === hit[i]) out.at(-1)!.text += text[i]
      else out.push({ text: text[i], hit: hit[i] })
    }
    return out
  }

  export async function move(delta: number) {
    if (!shown.length) return
    active = (active + delta + shown.length) % shown.length
    await tick()
    list.querySelector(`#result-${active}`)?.scrollIntoView({ block: 'nearest' })
  }

  export function choose(pinned: boolean) {
    const result = shown[active]
    if (!result) return null
    pick(result, pinned)
    return result.path
  }

  function pick(result: Result, pinned: boolean) {
    if (result.isDir) onshowfolder(result.path)
    else onopen(result.path, pinned)
  }

  function onClick(index: number, result: Result) {
    active = index
    pick(result, false)
  }

  function plural(count: number) {
    return `${count.toLocaleString()} ${count === 1 ? 'match' : 'matches'}`
  }
</script>

{#snippet marked(text: string)}
  {#each segments(text) as part, i (i)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
{/snippet}

<div class="results">
  <p class="summary muted" role="status">
    {#if loadError}
      <span class="failed">{loadError}</span>
    {:else if !index}
      Indexing the pack…
    {:else if !matches.length}
      No file or folder names match.
    {:else if matches.length > shown.length}
      Showing the best {shown.length} of {plural(matches.length)}.
    {:else}
      {plural(matches.length)}
    {/if}
    {#if index?.truncated}<span> We only searched the first {index.paths.length.toLocaleString()} entries.</span>{/if}
  </p>
  <ul id="file-results" role="listbox" aria-label="Search results" bind:this={list}>
    {#each shown as result, i (result.path)}
      <!-- svelte-ignore a11y_click_events_have_key_events (the search field handles keys) -->
      <li
        id="result-{i}"
        role="option"
        aria-selected={i === active}
        class:active={i === active}
        title={result.path}
        onclick={() => onClick(i, result)}
        ondblclick={() => !result.isDir && onopen(result.path, true)}
      >
        <span class="name" class:dir={result.isDir}>{@render marked(result.name)}{#if result.isDir}<span class="muted">/</span>{/if}</span>
        {#if result.dir}<span class="where">{@render marked(result.dir)}</span>{/if}
      </li>
    {/each}
  </ul>
</div>

<style>
  .results {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .summary {
    padding: 6px 12px;
    font-size: var(--step--2);
    border-bottom: 1px solid var(--line);
  }

  ul {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 4px 0;
    list-style: none;
    font-size: var(--step--1);
  }

  li {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 3px 12px;
    white-space: nowrap;
    cursor: pointer;
    user-select: none;
  }

  li:hover {
    background: var(--surface);
  }

  li.active {
    background: var(--here);
  }

  .name {
    flex: none;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .name.dir {
    font-weight: 600;
  }

  .where {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--step--2);
    color: var(--muted);
  }

  mark {
    color: inherit;
    background: none;
    font-weight: 700;
    text-decoration: underline;
    text-decoration-color: var(--forest);
    text-decoration-thickness: 2px;
    text-underline-offset: 2px;
  }
</style>
