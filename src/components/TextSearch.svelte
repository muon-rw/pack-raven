<script lang="ts">
  import { tick, untrack } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { api, errorText, type Replaced, type TextMatch, type TextQuery, type TextResults } from '../lib/api'
  import { fs, onDiskChange, save, workspace } from '../lib/workspace.svelte'
  import { textSearch } from '../lib/textSearch.svelte'
  import { commandKey, isMac, shortcut } from '../lib/platform'
  import type { MenuItem } from '../lib/menu'
  import ContextMenu from './ContextMenu.svelte'
  import Twisty from './Twisty.svelte'

  let {
    onopen,
    onclose,
  }: {
    onopen: (path: string, match: TextMatch | null, options: { pinned: boolean; focusEditor: boolean }) => void
    onclose: () => void
  } = $props()

  const DEBOUNCE_MS = 250
  const REFRESH_MS = 400
  const ROW_STEP = 1000

  type Group = { path: string; matches: TextMatch[] }
  type Row = { kind: 'file'; key: string; group: Group } | { kind: 'match'; key: string; group: Group; match: TextMatch }
  type Option = 'matchCase' | 'wholeWord' | 'regex'

  const OPTIONS: { id: Option; label: string; key: string; glyph: string }[] = [
    { id: 'matchCase', label: 'Match case', key: 'C', glyph: 'Aa' },
    { id: 'wholeWord', label: 'Whole word', key: 'W', glyph: 'ab' },
    { id: 'regex', label: 'Use regular expression', key: 'R', glyph: '.*' },
  ]

  let findField: HTMLInputElement
  let replaceField: HTMLInputElement | undefined = $state()
  let undoButton: HTMLButtonElement | undefined = $state()
  let list: HTMLUListElement | undefined = $state()
  let results = $state.raw<TextResults | null>(null)
  let resultsFor = $state<string | null>(null)
  let error = $state<string | null>(null)
  let replacing = $state(false)
  let notice = $state<{ text: string; tone: 'note' | 'error'; undo: string | null } | null>(null)
  let active = $state<string | null>(null)
  let opened = $state<string | null>(null)
  let limit = $state(ROW_STEP)
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null)
  const dismissed = new SvelteSet<string>()
  const collapsed = new SvelteSet<string>()

  let query = $derived<TextQuery>({
    pattern: textSearch.pattern,
    regex: textSearch.regex,
    matchCase: textSearch.matchCase,
    wholeWord: textSearch.wholeWord,
    include: textSearch.include,
    exclude: textSearch.exclude,
    includeIgnored: textSearch.includeIgnored,
    replace: textSearch.showReplace ? textSearch.replacement : null,
  })
  let queryKey = $derived(JSON.stringify([query, workspace.pack]))
  // Until the results for what you typed arrive, the ones on screen answer an older question,
  // so we hold off acting on them.
  let stale = $derived(!!textSearch.pattern && resultsFor !== queryKey)

  // Changing the replacement text keeps the matches you hid, since they're the same matches.
  let sought = $derived(JSON.stringify([query.pattern, query.regex, query.matchCase, query.wholeWord, query.include, query.exclude, query.includeIgnored, workspace.pack]))

  $effect(() => {
    void sought
    untrack(() => {
      dismissed.clear()
      collapsed.clear()
      notice = null
      active = null
      opened = null
      limit = ROW_STEP
    })
  })

  let seq = 0
  let searchedOnce = false

  async function run(q: TextQuery = query, key: string = queryKey) {
    const mine = ++seq
    if (!q.pattern) {
      results = null
      error = null
      resultsFor = key
      return
    }
    try {
      const found = await api.searchText(q)
      if (mine !== seq || !found) return
      results = found
      error = null
    } catch (e) {
      if (mine !== seq) return
      results = null
      error = errorText(e)
    }
    resultsFor = key
  }

  $effect(() => {
    const q = query
    const key = queryKey
    const timer = setTimeout(() => run(q, key), searchedOnce ? DEBOUNCE_MS : 0)
    searchedOnce = true
    return () => clearTimeout(timer)
  })

  // Saving a file or changing one outside packraven can add or remove matches, so we search
  // again once the changes settle.
  let seenSeq = fs.seq
  $effect(() => {
    const now = fs.seq
    if (now === seenSeq) return
    seenSeq = now
    const timer = setTimeout(() => untrack(() => run()), REFRESH_MS)
    return () => clearTimeout(timer)
  })

  function fileKey(path: string) {
    return `f:${path}`
  }

  function matchKey(path: string, match: TextMatch) {
    return `m:${match.line}:${match.column}:${path}`
  }

  let groups = $derived.by((): Group[] =>
    (results?.files ?? [])
      .filter((file) => !dismissed.has(fileKey(file.path)))
      .map((file) => ({ path: file.path, matches: file.matches.filter((m) => !dismissed.has(matchKey(file.path, m))) }))
      .filter((group) => group.matches.length),
  )

  let matchCount = $derived(groups.reduce((sum, group) => sum + group.matches.length, 0))
  let hiddenCount = $derived((results?.total ?? 0) - matchCount)

  let rows = $derived.by(() => {
    const out: Row[] = []
    for (const group of groups) {
      out.push({ kind: 'file', key: fileKey(group.path), group })
      if (collapsed.has(group.path)) continue
      for (const match of group.matches) out.push({ kind: 'match', key: matchKey(group.path, match), group, match })
    }
    return out
  })

  // Rendering every row of a broad search would stall the window, so we show them in batches.
  let shown = $derived(rows.slice(0, limit))
  let activeIndex = $derived(Math.max(0, shown.findIndex((row) => row.key === active)))
  let allCollapsed = $derived(groups.length > 0 && groups.every((group) => collapsed.has(group.path)))
  let removing = $derived(!textSearch.replacement)

  let filterSummary = $derived(
    [textSearch.include.trim() && `only ${textSearch.include.trim()}`, textSearch.exclude.trim() && `not ${textSearch.exclude.trim()}`, textSearch.includeIgnored && 'ignored files too']
      .filter(Boolean)
      .join('; '),
  )

  export async function focus(replace: boolean) {
    await tick()
    const field = replace && textSearch.pattern && replaceField ? replaceField : findField
    field.focus()
    field.select()
  }

  /** Opens the next or previous match after the one you last opened, for F4 and Shift+F4. */
  export async function step(delta: number) {
    const all = groups.flatMap((group) => group.matches.map((match) => ({ group, match, key: matchKey(group.path, match) })))
    if (!all.length) return
    const at = all.findIndex((entry) => entry.key === (opened ?? active))
    const next = all[at < 0 ? (delta > 0 ? 0 : all.length - 1) : (at + delta + all.length) % all.length]
    collapsed.delete(next.group.path)
    const index = rows.findIndex((row) => row.key === next.key)
    if (index >= limit) limit = index + 1
    active = next.key
    opened = next.key
    onopen(next.group.path, next.match, { pinned: false, focusEditor: true })
    await tick()
    list?.querySelector(`[data-key="${CSS.escape(next.key)}"]`)?.scrollIntoView({ block: 'nearest' })
  }

  function name(path: string) {
    return path.slice(path.lastIndexOf('/') + 1)
  }

  function folder(path: string) {
    return path.slice(0, Math.max(path.lastIndexOf('/'), 0))
  }

  function plural(count: number, one: string, many = `${one}s`) {
    return `${count.toLocaleString()} ${count === 1 ? one : many}`
  }

  function altKey(letter: string) {
    return isMac ? `⌥${letter}` : `Alt+${letter}`
  }

  // The start of the preview truncates right to left, which drops a trailing space, so we keep
  // it as a no-break space and the preview still quotes the file exactly.
  function before(match: TextMatch) {
    return match.preview.slice(0, match.start).replace(/ +$/, (spaces) => ' '.repeat(spaces.length))
  }

  function toggleOption(option: Option) {
    textSearch[option] = !textSearch[option]
  }

  function onPanelKey(event: KeyboardEvent) {
    if (event.key === 'Enter' && commandKey(event) && event.altKey) {
      event.preventDefault()
      replace(groups)
    }
  }

  function onFieldKey(event: KeyboardEvent, field: 'find' | 'replace' | 'filter') {
    if (event.altKey && !event.ctrlKey && !event.metaKey) {
      const option = OPTIONS.find((o) => event.code === `Key${o.key}`)
      if (option) {
        event.preventDefault()
        toggleOption(option.id)
        return
      }
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      onclose()
    } else if (event.key === 'Enter' && !event.altKey) {
      event.preventDefault()
      run()
    } else if (event.key === 'ArrowDown' && field !== 'filter' && shown.length) {
      event.preventDefault()
      focusRow(0)
    }
  }

  async function focusRow(index: number) {
    const row = shown[index]
    if (!row) return
    active = row.key
    await tick()
    list?.querySelector<HTMLElement>(`[data-key="${CSS.escape(row.key)}"]`)?.focus()
  }

  function open(row: Row, pinned: boolean) {
    active = row.key
    if (row.kind === 'match') opened = row.key
    onopen(row.group.path, row.kind === 'match' ? row.match : null, { pinned, focusEditor: pinned })
  }

  function toggle(group: Group) {
    if (collapsed.has(group.path)) collapsed.delete(group.path)
    else collapsed.add(group.path)
  }

  function toggleAll() {
    if (allCollapsed) collapsed.clear()
    else for (const group of groups) collapsed.add(group.path)
  }

  async function hide(row: Row) {
    const index = shown.findIndex((r) => r.key === row.key)
    const hadFocus = list?.contains(document.activeElement) ?? false
    dismissed.add(row.key)
    if (!shown.length) {
      if (hadFocus) findField.focus()
      return
    }
    active = shown[Math.min(index, shown.length - 1)].key
    if (hadFocus) await focusRow(Math.min(index, shown.length - 1))
  }

  function onListKey(event: KeyboardEvent) {
    const index = activeIndex
    const row = shown[index]
    if (event.key === 'Escape') {
      onclose()
    } else if (!row) {
      return
    } else if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      const rect = (event.target as HTMLElement).getBoundingClientRect()
      openMenu(rect.left + 24, rect.bottom, row)
    } else if (event.key === 'ArrowDown') {
      focusRow(Math.min(index + 1, shown.length - 1))
    } else if (event.key === 'ArrowUp') {
      if (index === 0) findField.focus()
      else focusRow(index - 1)
    } else if (event.key === 'Home') {
      focusRow(0)
    } else if (event.key === 'End') {
      focusRow(shown.length - 1)
    } else if (event.key === 'ArrowRight' && row.kind === 'file') {
      if (collapsed.has(row.group.path)) collapsed.delete(row.group.path)
      else focusRow(index + 1)
    } else if (event.key === 'ArrowLeft') {
      if (row.kind === 'match') focusRow(shown.findIndex((r) => r.key === fileKey(row.group.path)))
      else collapsed.add(row.group.path)
    } else if (event.key === 'Enter' && !event.altKey) {
      open(row, true)
    } else if (event.key === ' ') {
      if (row.kind === 'file') toggle(row.group)
      else open(row, false)
    } else if (event.key === 'Delete' || event.key === 'Backspace') {
      hide(row)
    } else {
      return
    }
    event.preventDefault()
  }

  function onRowClick(row: Row) {
    active = row.key
    if (row.kind === 'file') toggle(row.group)
    else open(row, false)
  }

  function only(row: Row): Group {
    return row.kind === 'file' ? row.group : { path: row.group.path, matches: [row.match] }
  }

  function openMenu(x: number, y: number, row: Row) {
    active = row.key
    menu = {
      x,
      y,
      items: [
        { label: row.kind === 'file' ? 'Open file' : 'Open', shortcut: 'Enter', run: () => open(row, true) },
        ...(textSearch.showReplace ? [{ label: row.kind === 'file' ? 'Replace in this file' : 'Replace this match', disabled: replacing || stale, run: () => replace([only(row)]) }] : []),
        { label: 'Hide from results', shortcut: 'Delete', run: () => hide(row) },
        'separator',
        { label: 'Copy path', run: () => api.copyPaths([row.group.path], false).catch((e) => (notice = { text: errorText(e), tone: 'error', undo: null })) },
      ],
    }
  }

  function onRowMenu(event: MouseEvent, row: Row) {
    event.preventDefault()
    openMenu(event.clientX, event.clientY, row)
  }

  function scopeLabel(count: number, files: number) {
    return `${removing ? 'Remove' : 'Replace'} ${count.toLocaleString()} in ${plural(files, 'file')}`
  }

  function confirmation(scope: Group[], count: number, unsaved: number) {
    if (scope.length === 1 && !unsaved) return null
    const where = scope.length === 1 ? name(scope[0].path) : `${scope.length.toLocaleString()} files`
    const what = removing ? `Remove ${plural(count, 'match', 'matches')} in ${where}?` : `Replace ${plural(count, 'match', 'matches')} in ${where} with “${textSearch.replacement}”?`
    const saving = !unsaved ? '' : scope.length === 1 ? ' It has unsaved changes, which we’ll save first.' : unsaved === 1 ? ' One of them has unsaved changes, which we’ll save first.' : ` ${unsaved} of them have unsaved changes, which we’ll save first.`
    return { text: `${what}${saving} You can undo this afterwards.`, ok: unsaved ? 'Save and replace' : removing ? 'Remove' : 'Replace' }
  }

  function outcome(done: Replaced) {
    if (!done.replaced) return 'Nothing was replaced, because the files changed since the search. Check the new results.'
    const where = done.files.length === 1 ? name(done.files[0]) : `${done.files.length.toLocaleString()} files`
    const missed = done.missed ? ` ${plural(done.missed, 'match', 'matches')} had changed since the search, so we left ${done.missed === 1 ? 'it' : 'them'} alone.` : ''
    const undo = done.undoable ? '' : ' This replace was too large to undo.'
    return `${removing ? 'Removed' : 'Replaced'} ${plural(done.replaced, 'match', 'matches')} in ${where}.${missed}${undo}`
  }

  // A replace takes its rows out of the list, so we put focus back where it was, or on Undo
  // when nothing is left.
  async function restoreFocus(index: number) {
    await tick()
    if (shown.length) await focusRow(Math.min(index, shown.length - 1))
    else (undoButton ?? findField).focus()
  }

  async function replace(scope: Group[]) {
    if (!textSearch.showReplace || replacing || stale) return
    const count = scope.reduce((sum, group) => sum + group.matches.length, 0)
    if (!count) return
    const focusIndex = list?.contains(document.activeElement) ? activeIndex : null
    const unsaved = workspace.tabs.filter((tab) => tab.dirty && scope.some((group) => group.path === tab.path))
    const confirm = confirmation(scope, count, unsaved.length)
    if (confirm && !(await ask(confirm.text, { title: 'Replace in files', kind: 'warning', okLabel: confirm.ok, cancelLabel: 'Cancel' }))) return
    replacing = true
    notice = null
    try {
      for (const tab of unsaved) {
        await save(tab.path)
        if (tab.dirty) {
          notice = { text: `We couldn't save ${name(tab.path)}, so we didn't replace anything.`, tone: 'error', undo: null }
          return
        }
      }
      const targets = scope.map((group) => ({ path: group.path, matches: group.matches.map((m): [number, number] => [m.line, m.column]) }))
      const done = await api.replaceText(query, targets)
      await onDiskChange(done.files)
      notice = { text: outcome(done), tone: 'note', undo: done.undoable && done.files.length ? done.label : null }
    } catch (e) {
      notice = { text: errorText(e), tone: 'error', undo: null }
    } finally {
      replacing = false
    }
    await run()
    if (focusIndex !== null) await restoreFocus(focusIndex)
  }

  // Undo steps through the same history as Ctrl+Z in the file tree, so we only undo when the
  // replace is still the latest step there.
  async function undo(label: string) {
    try {
      const history = await api.fileHistory()
      if (history.undo !== label) {
        const text = history.undo ? `“${history.undo}” came after the replace, so undo that first with ${shortcut('Ctrl+Z')} in the file tree.` : 'There’s no replace left to undo.'
        notice = { text, tone: 'error', undo: null }
        return
      }
      const undone = await api.fileUndo(false)
      if (undone) await onDiskChange(undone.changed)
      notice = { text: 'Undid the replace.', tone: 'note', undo: null }
    } catch (e) {
      notice = { text: errorText(e), tone: 'error', undo: null }
    }
    await run()
    findField.focus()
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions (Ctrl+Alt+Enter replaces all from anywhere in the panel) -->
<div class="panel" onkeydown={onPanelKey}>
  <div class="head">
    <h2>Find in files</h2>
    <span class="tools">
      <button class="icon" aria-label={allCollapsed ? 'Expand all' : 'Collapse all'} title={allCollapsed ? 'Expand all' : 'Collapse all'} disabled={!groups.length} onclick={toggleAll}>
        {#if allCollapsed}
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 4.5l3.5 3.5 3.5-3.5M4.5 8.5l3.5 3.5 3.5-3.5" /></svg>
        {:else}
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 7.5L8 4l3.5 3.5M4.5 11.5L8 8l3.5 3.5" /></svg>
        {/if}
      </button>
      <button class="icon" aria-label="Search again" title="Search again" disabled={!textSearch.pattern} onclick={() => run()}>
        <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M13.25 8a5.25 5.25 0 1 1-1.54-3.71" /><path d="M12.25 1.75v3h-3" /></svg>
      </button>
      <button class="icon" aria-label="Close search" title="Back to the file tree (Esc)" onclick={onclose}>
        <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" /></svg>
      </button>
    </span>
  </div>

  <div class="form">
    <div class="line">
      <button
        class="expander"
        aria-expanded={textSearch.showReplace}
        aria-controls="text-search-replace"
        aria-label="Toggle replace"
        title="{textSearch.showReplace ? 'Hide' : 'Show'} replace ({shortcut('Ctrl+Shift+H')})"
        onclick={() => (textSearch.showReplace = !textSearch.showReplace)}
      >
        <Twisty open={textSearch.showReplace} />
      </button>
      <span class="field">
        <input
          bind:this={findField}
          bind:value={textSearch.pattern}
          class:invalid={!!error}
          placeholder="Search"
          aria-label="Search text in files"
          aria-invalid={!!error}
          aria-describedby="text-search-summary"
          spellcheck="false"
          autocomplete="off"
          onkeydown={(e) => onFieldKey(e, 'find')}
        />
        <span class="options" role="group" aria-label="Search options">
          {#each OPTIONS as option (option.id)}
            <button class="option" aria-pressed={textSearch[option.id]} aria-label={option.label} title="{option.label} ({altKey(option.key)})" onclick={() => toggleOption(option.id)}>
              <span class:word={option.id === 'wholeWord'}>{option.glyph}</span>
            </button>
          {/each}
        </span>
      </span>
    </div>
    {#if textSearch.showReplace}
      <div class="replace indented" id="text-search-replace">
        <span class="field">
          <input
            bind:this={replaceField}
            bind:value={textSearch.replacement}
            placeholder="Replace (leave empty to remove)"
            aria-label="Replace with"
            aria-describedby={textSearch.regex ? 'text-search-groups' : undefined}
            spellcheck="false"
            autocomplete="off"
            onkeydown={(e) => onFieldKey(e, 'replace')}
          />
        </span>
        {#if textSearch.regex}<p class="hint" id="text-search-groups">Use $1, $2 and so on to insert what each group matched.</p>{/if}
        <button class="replace-all" class:danger={removing} title="{removing ? 'Remove' : 'Replace'} every match shown ({shortcut('Ctrl+Alt+Enter')})" disabled={!matchCount || replacing || stale} onclick={() => replace(groups)}>
          {matchCount ? scopeLabel(matchCount, groups.length) : removing ? 'Remove all' : 'Replace all'}
        </button>
      </div>
    {/if}
    <div class="line indented filter-line">
      <button class="filter-toggle" aria-expanded={textSearch.showFilters} aria-controls="text-search-filters" onclick={() => (textSearch.showFilters = !textSearch.showFilters)}>
        <Twisty open={textSearch.showFilters} />Filter files
      </button>
      {#if !textSearch.showFilters && filterSummary}<span class="filter-summary" title={filterSummary}>{filterSummary}</span>{/if}
    </div>
    {#if textSearch.showFilters}
      <div class="filters indented" id="text-search-filters">
        <label>
          Files to include
          <input bind:value={textSearch.include} placeholder="config, *.toml" aria-describedby="text-search-patterns" spellcheck="false" autocomplete="off" onkeydown={(e) => onFieldKey(e, 'filter')} />
        </label>
        <label>
          Files to exclude
          <input bind:value={textSearch.exclude} placeholder="kubejs/assets" aria-describedby="text-search-patterns" spellcheck="false" autocomplete="off" onkeydown={(e) => onFieldKey(e, 'filter')} />
        </label>
        <p class="hint" id="text-search-patterns">Separate patterns with commas.</p>
        <label class="check" title="Files your .gitignore leaves out, like logs and saves">
          <input type="checkbox" bind:checked={textSearch.includeIgnored} aria-describedby="text-search-ignored" />Include ignored files
        </label>
        <span class="visually-hidden" id="text-search-ignored">Files your .gitignore leaves out, like logs and saves.</span>
      </div>
    {/if}
  </div>

  <!-- Screen readers only announce changes to a live region that was already on the page. -->
  <div role="status" aria-live="polite">
    {#if notice}
      <p class="notice" class:failed={notice.tone === 'error'}>
        {notice.text}
        {#if notice.undo}{@const label = notice.undo}<button class="link" bind:this={undoButton} onclick={() => undo(label)}>Undo</button>{/if}
      </p>
    {/if}
  </div>

  <p class="summary" id="text-search-summary" aria-live="polite" aria-busy={stale}>
    {#if error}
      <span class="failed">{error}</span>
    {:else if !textSearch.pattern}
      Search the text of every file in the pack.{textSearch.includeIgnored ? '' : ' We leave out ignored files, like logs, unless you include them under Filter files.'}
    {:else if !results || (stale && !matchCount)}
      Searching…
    {:else if !matchCount && results.total}
      {results.total === 1 ? 'You hid the only match.' : `You hid all ${results.total.toLocaleString()} matches.`}
      <button class="link" onclick={() => dismissed.clear()}>Show {results.total === 1 ? 'it' : 'them'}</button>
    {:else if !matchCount}
      No matches in the {plural(results.searched, 'file')} we searched.
    {:else}
      {plural(matchCount, 'match', 'matches')} in {plural(groups.length, 'file')}{#if stale}<span class="muted">, searching…</span>{/if}
      {#if hiddenCount > 0}<span> ({hiddenCount.toLocaleString()} hidden) <button class="link" onclick={() => dismissed.clear()}>Show</button></span>{/if}
    {/if}
    {#if !error && results?.truncated}<span> We stopped at the first {results.total.toLocaleString()} matches, so narrow the search to see the rest.</span>{/if}
    {#if !error && results?.tooLarge}<span> We skipped {plural(results.tooLarge, 'file')} over the editor’s size limit.</span>{/if}
  </p>

  {#if !textSearch.pattern}
    <dl class="legend">
      {#each OPTIONS as option (option.id)}
        <div><dt><span class:word={option.id === 'wholeWord'}>{option.glyph}</span></dt><dd>{option.label} <kbd>{altKey(option.key)}</kbd></dd></div>
      {/each}
      <div><dt><Twisty open={false} /><span class="visually-hidden">The arrow beside the search field</span></dt><dd>Replace <kbd>{shortcut('Ctrl+Shift+H')}</kbd></dd></div>
      <div><dt>F4</dt><dd>Next match, from anywhere in Files</dd></div>
    </dl>
  {/if}

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions (the tree handles keys for its rows) -->
  <ul class="results" class:stale class:with-replace={textSearch.showReplace} role="tree" aria-label="Search results" aria-describedby="text-search-keys" bind:this={list} onkeydown={onListKey}>
    {#each shown as row, i (row.key)}
      {@const isActive = i === activeIndex}
      {#if row.kind === 'file'}
        <!-- svelte-ignore a11y_click_events_have_key_events (the tree handles keys) -->
        <li
          class="row file-row"
          role="treeitem"
          aria-level={1}
          aria-expanded={!collapsed.has(row.group.path)}
          aria-selected={isActive}
          aria-label="{name(row.group.path)}, {plural(row.group.matches.length, 'match', 'matches')}"
          tabindex={isActive ? 0 : -1}
          data-key={row.key}
          title={row.group.path}
          onclick={() => onRowClick(row)}
          ondblclick={() => open(row, true)}
          oncontextmenu={(e) => onRowMenu(e, row)}
        >
          <Twisty open={!collapsed.has(row.group.path)} />
          <span class="name">{name(row.group.path)}</span>
          {#if folder(row.group.path)}<span class="where">{folder(row.group.path)}</span>{/if}
          <span class="count">{row.group.matches.length.toLocaleString()}</span>
          <!-- svelte-ignore a11y_no_static_element_interactions (keeps focus on the row when you click an action) -->
          <span class="row-actions" aria-hidden="true" onmousedown={(e) => e.preventDefault()}>
            {#if textSearch.showReplace}
              <button class="icon small" tabindex="-1" title="Replace in this file" disabled={replacing || stale} onclick={(e) => (e.stopPropagation(), replace([row.group]))}>
                <svg viewBox="0 0 16 16"><path d="M2.5 4.5h7M2.5 8h4.5M9.5 9.5l2.5 2.5 2.5-2.5M12 3.5v8.5" /></svg>
              </button>
            {/if}
            <button class="icon small" tabindex="-1" title="Hide from results (Delete)" onclick={(e) => (e.stopPropagation(), hide(row))}>
              <svg viewBox="0 0 16 16"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" /></svg>
            </button>
          </span>
        </li>
      {:else}
        {@const m = row.match}
        {@const hit = m.preview.slice(m.start, m.end)}
        <!-- svelte-ignore a11y_click_events_have_key_events (the tree handles keys) -->
        <li
          class="row match-row"
          class:opened={opened === row.key}
          class:replacing={m.replacement !== null}
          role="treeitem"
          aria-level={2}
          aria-selected={isActive}
          aria-label={m.replacement === null ? `Line ${m.line}: ${m.preview}` : `Line ${m.line}, ${m.replacement ? `replaces ${hit} with ${m.replacement}` : `removes ${hit}`}: ${m.preview}`}
          tabindex={isActive ? 0 : -1}
          data-key={row.key}
          title="Line {m.line}: {m.preview}"
          onclick={() => onRowClick(row)}
          ondblclick={() => open(row, true)}
          oncontextmenu={(e) => onRowMenu(e, row)}
        >
          <span class="number">{m.line}</span>
          <span class="text">
            <span class="before"><bdi>{before(m)}</bdi></span>
            <span class="hit"><mark>{hit}</mark></span>
            <span class="after">{m.preview.slice(m.end)}</span>
          </span>
          {#if m.replacement !== null}
            <span class="text result">
              <span class="before"><bdi>{before(m)}</bdi></span>
              <span class="hit"><ins>{m.replacement}</ins></span>
              <span class="after">{m.preview.slice(m.end)}</span>
            </span>
          {/if}
          <!-- svelte-ignore a11y_no_static_element_interactions (keeps focus on the row when you click an action) -->
          <span class="row-actions" aria-hidden="true" onmousedown={(e) => e.preventDefault()}>
            {#if textSearch.showReplace}
              <button class="icon small" tabindex="-1" title="Replace this match" disabled={replacing || stale} onclick={(e) => (e.stopPropagation(), replace([only(row)]))}>
                <svg viewBox="0 0 16 16"><path d="M2.5 4.5h7M2.5 8h4.5M9.5 9.5l2.5 2.5 2.5-2.5M12 3.5v8.5" /></svg>
              </button>
            {/if}
            <button class="icon small" tabindex="-1" title="Hide from results (Delete)" onclick={(e) => (e.stopPropagation(), hide(row))}>
              <svg viewBox="0 0 16 16"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" /></svg>
            </button>
          </span>
        </li>
      {/if}
    {/each}
  </ul>
  <span class="visually-hidden" id="text-search-keys">Enter opens a match, Delete hides it, and the context menu key lists the other actions.</span>
  {#if rows.length > shown.length}
    <button class="ghost more" onclick={() => (limit += ROW_STEP)}>Show {Math.min(ROW_STEP, rows.length - shown.length).toLocaleString()} more rows</button>
  {/if}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} label="Search result actions" items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }

  .head h2 {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--step-0);
  }

  .tools {
    display: flex;
    flex: none;
    gap: 2px;
    margin: -4px -6px -4px 0;
  }

  .icon {
    display: grid;
    place-items: center;
    flex: none;
    width: 28px;
    height: 28px;
    padding: 0;
    color: var(--muted);
    background: transparent;
    border-color: transparent;
  }

  .icon:hover:not(:disabled) {
    color: var(--text);
    background: var(--control);
  }

  .icon:disabled {
    background: transparent;
    border-color: transparent;
    opacity: 0.55;
  }

  .icon svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .icon.small {
    width: 22px;
    height: 22px;
  }

  .icon.small svg {
    width: 14px;
    height: 14px;
  }

  /* The form never takes more than half the panel, so the results keep room at the smallest window. */
  .form {
    flex: none;
    display: grid;
    gap: 6px;
    max-height: 50%;
    overflow: auto;
    padding: 8px 10px 8px 4px;
    border-bottom: 1px solid var(--line);
  }

  .line {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }

  .indented {
    padding-left: 22px;
  }

  .expander {
    align-self: stretch;
    flex: none;
    width: 18px;
    padding: 0;
    background: transparent;
    border-color: transparent;
  }

  .expander:hover:not(:disabled) {
    background: var(--control);
  }

  .field {
    position: relative;
    flex: 1;
    min-width: 0;
  }

  .field input {
    width: 100%;
    padding: 3px 8px;
    font-size: var(--step--1);
  }

  .line:first-child .field input {
    padding-right: 82px;
  }

  input.invalid {
    border-color: var(--fail-border);
    outline-color: var(--fail);
  }

  .options {
    position: absolute;
    top: 50%;
    right: 3px;
    display: flex;
    gap: 1px;
    transform: translateY(-50%);
  }

  .option {
    display: grid;
    place-items: center;
    width: 25px;
    height: 23px;
    padding: 0;
    font-family: var(--font-log);
    font-size: var(--step--2);
    font-weight: 600;
    color: var(--muted);
    background: transparent;
    border-color: transparent;
    border-radius: var(--radius-small);
  }

  .option:hover:not(:disabled) {
    color: var(--text);
    background: var(--control);
  }

  .option[aria-pressed='true'],
  .option[aria-pressed='true']:hover {
    color: var(--forest-hi);
    background: var(--ok-fill);
    border-color: var(--ok-border);
  }

  .word {
    padding: 0 1px;
    line-height: 1.1;
    border: 1.5px solid currentColor;
    border-top: 0;
  }

  .replace {
    display: grid;
    gap: 6px;
    justify-items: start;
  }

  .replace .field {
    width: 100%;
  }

  .replace-all {
    padding: 3px 10px;
    font-size: var(--step--1);
  }

  .filter-line {
    gap: 8px;
  }

  .filter-toggle {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    flex: none;
    margin-left: -18px;
    padding: 0 6px 0 2px;
    font-size: var(--step--2);
    font-weight: 400;
    color: var(--muted);
    background: transparent;
    border-color: transparent;
  }

  .filter-toggle:hover:not(:disabled) {
    color: var(--text);
    background: var(--control);
  }

  .filter-summary {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-log);
    font-size: var(--step--2);
    color: var(--forest);
  }

  .filters {
    display: grid;
    gap: 6px;
  }

  .filters label {
    font-size: var(--step--2);
  }

  .filters label > input:not([type='checkbox']) {
    padding: 3px 8px;
    font-size: var(--step--1);
  }

  .filters .check {
    display: flex;
    font-size: var(--step--1);
  }

  .hint {
    font-size: var(--step--2);
    color: var(--muted);
  }

  .notice {
    margin: 8px 10px 0;
    padding: 6px 10px;
    font-size: var(--step--2);
    background: var(--ok-fill);
    border: 1px solid var(--ok-border);
    border-radius: var(--radius-control);
  }

  .notice.failed {
    background: var(--fail-fill);
    border-color: var(--fail-border);
  }

  .notice .link {
    margin-left: 6px;
    font-weight: 700;
  }

  .summary {
    padding: 6px 12px;
    font-size: var(--step--2);
    color: var(--muted);
  }

  .summary .link {
    font-size: inherit;
  }

  .legend {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 4px 12px 12px;
    font-size: var(--step--2);
    color: var(--muted);
  }

  .legend div {
    display: grid;
    grid-template-columns: 26px 1fr;
    align-items: center;
    gap: 6px;
  }

  .legend dt {
    display: grid;
    place-items: center;
    font-family: var(--font-log);
    font-weight: 600;
    color: var(--text);
  }

  .legend dd {
    margin: 0;
  }

  .results {
    flex: 1;
    min-height: 0;
    overflow: hidden auto;
    margin: 0;
    padding: 0 0 6px;
    list-style: none;
    font-size: var(--step--1);
    container-type: inline-size;
  }

  /* Match rows keep a gutter for their actions, so showing them never covers the match. */
  .results {
    --gutter: 24px;
  }

  .results.with-replace {
    --gutter: 48px;
  }

  .results.stale {
    opacity: 0.55;
    pointer-events: none;
  }

  .row {
    --row-fill: var(--field);
    position: relative;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    column-gap: 6px;
    min-height: 26px;
    padding: 1px 10px 1px 8px;
    white-space: nowrap;
    background: var(--row-fill);
    cursor: pointer;
    user-select: none;
    content-visibility: auto;
    contain-intrinsic-size: auto 26px;
  }

  .row:hover {
    --row-fill: var(--surface);
  }

  .row.opened {
    --row-fill: var(--highlight);
  }

  /* The ochre selection marks where the keyboard is, so it only shows once the list has focus. */
  .results:focus-within .row[aria-selected='true'] {
    --row-fill: var(--here);
  }

  .row:focus-visible {
    outline-offset: -2px;
  }

  .file-row {
    flex-wrap: nowrap;
    gap: 4px;
  }

  .file-row .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 700;
  }

  .where {
    flex: 1 1 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-left: 4px;
    font-size: var(--step--2);
    color: var(--muted);
  }

  @container (max-width: 240px) {
    .where {
      display: none;
    }
  }

  .count {
    flex: none;
    margin-left: auto;
    padding: 0 6px;
    font-size: var(--step--2);
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    line-height: 1.4;
    color: var(--muted);
    background: var(--control);
    border-radius: 999px;
  }

  .match-row {
    padding-left: 26px;
    padding-right: calc(6px + var(--gutter));
    font-family: var(--font-log);
    font-size: var(--step--2);
  }

  .number {
    flex: none;
    min-width: 3ch;
    color: var(--muted);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  /* In a narrow sidebar the text after the match gives way first, then the text before it,
     which loses its start rather than its end, so the match itself stays in view. */
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    white-space: pre;
  }

  /* With a replacement, the row shows the line before and after, one above the other. */
  .text.result {
    flex-basis: 100%;
    padding-left: calc(3ch + 6px);
  }

  .before,
  .after,
  .hit {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .before {
    flex: 0 1 auto;
    direction: rtl;
    text-align: left;
  }

  .hit {
    flex: none;
    max-width: 100%;
  }

  .after {
    flex: 0 1000 auto;
  }

  mark {
    color: inherit;
    background: var(--hit-soft);
    outline: 1px solid var(--hit-edge);
    border-radius: 2px;
  }

  .results:focus-within .row[aria-selected='true'] mark,
  .row.opened mark {
    color: var(--hit-ink);
    background: var(--hit);
  }

  .replacing mark,
  .results:focus-within .replacing[aria-selected='true'] mark,
  .replacing.opened mark {
    color: var(--fail);
    background: var(--fail-fill);
    outline-color: var(--fail-border);
    text-decoration: line-through;
  }

  ins {
    color: var(--forest-hi);
    background: var(--ok-fill);
    outline: 1px solid var(--ok-border);
    border-radius: 2px;
    text-decoration: none;
  }

  /* The actions float over the end of the row, so showing them doesn't squeeze the preview. */
  .row-actions {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    display: none;
    align-items: center;
    gap: 1px;
    padding: 0 4px 0 18px;
    background: linear-gradient(to right, transparent, var(--row-fill) 14px);
  }

  .match-row .row-actions {
    padding-left: 0;
    background: none;
  }

  .row:hover .row-actions,
  .row:focus-visible .row-actions {
    display: flex;
  }

  .more {
    margin: 0 10px 10px;
  }
</style>
