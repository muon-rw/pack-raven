<script lang="ts">
  import { onMount } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import Notice from './Notice.svelte'
  import { api, errorText, type AddonKind, type ModPreview, type ModRequest, type SearchResult, type SearchSort } from '../lib/api'
  import { browse } from '../lib/modUpdates.svelte'

  let { installedIds, blocked, game, install }: { installedIds: Set<number>; blocked: boolean; game: string; install: (requests: ModRequest[]) => Promise<void> } = $props()

  // We wait for a pause in typing, so each keystroke doesn't spend a CurseForge request.
  const SEARCH_DELAY_MS = 350

  let results: SearchResult[] = $state([])
  let nextIndex: number | null = $state(null)
  let searching = $state(false)
  let searchError: string | null = $state(null)
  let categories: { id: number; name: string }[] = $state([])
  let resolving: number | null = $state(null)
  let confirming: { result: SearchResult; preview: ModPreview } | null = $state(null)
  let previewError: { projectId: number; text: string } | null = $state(null)

  let searchField: HTMLInputElement
  let resultsRegion: HTMLElement

  export function focusSearch() {
    searchField.focus()
    searchField.select()
  }

  let latestSearch = 0
  let timer: ReturnType<typeof setTimeout> | undefined

  async function run(append: boolean) {
    const id = ++latestSearch
    searching = true
    try {
      const page = await api.searchMods({ text: browse.text, kind: browse.kind, categoryId: browse.categoryId, sort: browse.sort, index: append ? (nextIndex ?? 0) : 0 })
      if (id !== latestSearch) return
      // Popularity can shift between requests, so a project can turn up on two pages.
      const seen = new Set(results.map((r) => r.projectId))
      results = append ? [...results, ...page.results.filter((r) => !seen.has(r.projectId))] : page.results
      nextIndex = page.nextIndex
      searchError = null
    } catch (error) {
      if (id === latestSearch) searchError = `Couldn't search CurseForge: ${errorText(error)}`
    } finally {
      if (id === latestSearch) searching = false
    }
  }

  function searchSoon() {
    clearTimeout(timer)
    timer = setTimeout(() => run(false), SEARCH_DELAY_MS)
  }

  async function loadCategories() {
    categories = await api.modCategories(browse.kind).catch(() => [])
  }

  function setKind(kind: AddonKind) {
    browse.kind = kind
    browse.categoryId = null
    loadCategories()
    run(false)
  }

  onMount(() => {
    loadCategories()
    run(false)
    return () => clearTimeout(timer)
  })

  async function startInstall(result: SearchResult) {
    const requests = [{ fileId: result.file.fileId, replaces: null }]
    resolving = result.projectId
    previewError = null
    try {
      const preview = await api.previewMods(requests)
      if (preview.dependencies.length || preview.unavailable.length) confirming = { result, preview }
      else {
        await install(requests)
        resultsRegion.focus()
      }
    } catch (error) {
      previewError = { projectId: result.projectId, text: errorText(error) }
    } finally {
      resolving = null
    }
  }

  async function confirmInstall() {
    if (!confirming) return
    const fileId = confirming.result.file.fileId
    confirming = null
    await install([{ fileId, replaces: null }])
    resultsRegion.focus()
  }

  function list(names: string[]) {
    return names.length < 3 ? names.join(' and ') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
  }

  const compact = new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 })

  function date(iso: string) {
    return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
  }

  const kindLabels: Record<AddonKind, string> = { mods: 'Mods', resourcepacks: 'Resource packs', shaderpacks: 'Shader packs', datapacks: 'Data packs' }
  const typeLabels = { release: 'Release', beta: 'Beta', alpha: 'Alpha' }
</script>

<div class="browser">
  <div class="tools">
    <input
      type="search"
      bind:this={searchField}
      bind:value={browse.text}
      oninput={searchSoon}
      placeholder="Search CurseForge"
      aria-label="Search CurseForge"
    />
    <label>
      <span class="visually-hidden">Type</span>
      <select value={browse.kind} onchange={(e) => setKind(e.currentTarget.value as AddonKind)}>
        {#each Object.entries(kindLabels) as [kind, label] (kind)}<option value={kind}>{label}</option>{/each}
      </select>
    </label>
    <label>
      <span class="visually-hidden">Category</span>
      <select bind:value={browse.categoryId} onchange={() => run(false)}>
        <option value={null}>All categories</option>
        {#each categories as category (category.id)}<option value={category.id}>{category.name}</option>{/each}
      </select>
    </label>
    <label class="sort">
      Sort by
      <select bind:value={browse.sort} onchange={() => run(false)}>
        {#each [['relevance', 'Relevance'], ['popularity', 'Popularity'], ['downloads', 'Downloads'], ['newest', 'Newest'], ['updated', 'Recently updated']] as [sort, label] (sort)}
          <option value={sort as SearchSort}>{label}</option>
        {/each}
      </select>
    </label>
  </div>

  {#if searchError}
    <Notice tone="error">
      <p>{searchError}</p>
      <p><button onclick={() => run(false)} disabled={searching}>Try again</button></p>
    </Notice>
  {/if}

  <p class="visually-hidden" role="status">
    {#if searching}Searching CurseForge.{:else if !searchError}{results.length} {results.length === 1 ? 'result' : 'results'}.{/if}
  </p>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="results" role="region" aria-label="Search results" aria-busy={searching} tabindex="0" bind:this={resultsRegion}>
    {#if !results.length && searching}
      <p class="muted empty">Searching CurseForge.</p>
    {:else if !results.length && !searchError}
      <p class="muted empty">
        {browse.text.trim() ? `CurseForge has nothing matching "${browse.text.trim()}" for ${game}.` : `CurseForge has nothing in this category for ${game}.`}
      </p>
    {/if}
    <ul class:stale={searching && results.length}>
      {#each results as result (result.projectId)}
        {@const installed = installedIds.has(result.projectId)}
        <li>
          {#if result.logo}<img src={result.logo} alt="" width="44" height="44" loading="lazy" />{:else}<span class="logo" aria-hidden="true"></span>{/if}
          <div class="about">
            <div class="title">
              {#if result.url}<a href={result.url} onclick={(e) => { e.preventDefault(); openUrl(result.url!) }}>{result.name}</a>{:else}<strong>{result.name}</strong>{/if}
              {#if result.authors.length}<span class="muted">by {result.authors.join(', ')}</span>{/if}
            </div>
            <p class="summary">{result.summary}</p>
            <p class="meta muted">{compact.format(result.downloads)} downloads · Updated {date(result.updated)}</p>
            {#if confirming?.result.projectId === result.projectId}
              <div class="confirm" role="group" aria-label="Install {result.name}" {@attach (node) => {
                node.scrollIntoView({ block: 'nearest' })
                node.querySelector<HTMLButtonElement>('button.primary')?.focus()
              }}>
                {#if confirming.preview.dependencies.length}
                  <p>{result.name} requires {list(confirming.preview.dependencies)}, so packraven installs {confirming.preview.dependencies.length === 1 ? 'it' : 'them'} too.</p>
                {/if}
                {#if confirming.preview.unavailable.length}
                  <p class="failed">It also requires {list(confirming.preview.unavailable)}, which CurseForge has no file of for {game}.</p>
                {/if}
                <p class="confirm-actions">
                  <button class="primary" onclick={confirmInstall} disabled={blocked}>
                    {confirming.preview.dependencies.length ? `Install all ${confirming.preview.dependencies.length + 1}` : 'Install anyway'}
                  </button>
                  <button class="quiet" onclick={() => { confirming = null; resultsRegion.focus() }}>Cancel</button>
                </p>
              </div>
            {/if}
            {#if previewError?.projectId === result.projectId}<p class="failed">Couldn't install {result.name}: {previewError.text}</p>{/if}
          </div>
          <div class="action">
            {#if installed}
              <span class="chip ok">Installed</span>
            {:else}
              {#if result.file.releaseType !== 'release'}<span class="chip pending">{typeLabels[result.file.releaseType]}</span>{/if}
              <button
                class:busy={resolving === result.projectId}
                disabled={blocked || resolving !== null || confirming?.result.projectId === result.projectId}
                onclick={() => startInstall(result)}
                aria-label="Install {result.name}"
              >
                {resolving === result.projectId ? 'Checking' : 'Install'}
              </button>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
    {#if nextIndex !== null}
      <p class="more">
        <button onclick={() => run(true)} disabled={searching} class:busy={searching}>Show more</button>
      </p>
    {/if}
  </div>
</div>

<style>
  .browser {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-height: 0;
  }

  .tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px 16px;
  }

  .tools input {
    flex: 1 1 240px;
    max-width: 360px;
  }

  .tools label {
    max-width: 100%;
  }

  .tools select {
    max-width: 100%;
  }

  .sort {
    display: flex;
    align-items: center;
    gap: 8px;
    white-space: nowrap;
  }

  .results {
    container-type: inline-size;
    flex: 1;
    min-height: 240px;
    overflow: auto;
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    display: grid;
    grid-template-columns: 44px 1fr auto;
    gap: 14px;
    align-items: start;
    padding: 14px 16px;
  }

  @media (max-height: 700px) {
    .browser {
      flex: none;
    }

    .results {
      flex: none;
      min-height: 0;
      overflow: visible;
    }
  }

  .stale {
    opacity: 0.55;
    transition: opacity 120ms ease-out;
  }

  /* At high zoom the results pane gets narrow, so the action drops under the text instead of
     squeezing it. */
  @container (max-width: 440px) {
    li {
      grid-template-columns: 32px 1fr;
      gap: 10px 12px;
    }

    li img,
    li .logo {
      width: 32px;
      height: 32px;
    }

    .action {
      grid-column: 2;
    }
  }

  li + li {
    border-top: 1px solid var(--line);
  }

  img,
  .logo {
    width: 44px;
    height: 44px;
    border-radius: var(--radius-small);
    object-fit: cover;
  }

  .logo {
    display: block;
    background: var(--line);
  }

  .about {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .title {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 8px;
  }

  .title a {
    color: var(--text);
    font-weight: 700;
  }

  .about p {
    margin: 0;
  }

  .summary {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    max-width: 72ch;
  }

  .meta {
    font-size: var(--step--1);
    font-variant-numeric: tabular-nums;
  }

  .action {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .confirm {
    display: grid;
    gap: 8px;
    margin-top: 6px;
    padding: 10px 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-control);
    background: var(--paper);
  }

  .confirm-actions {
    display: flex;
    gap: 8px;
  }

  .empty {
    padding: 14px 16px;
    margin: 0;
  }

  .more {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    margin: 0;
    border-top: 1px solid var(--line);
  }
</style>
