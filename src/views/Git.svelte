<script lang="ts">
  import { onMount, tick, untrack } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import { revealItemInDir } from '@tauri-apps/plugin-opener'
  import { api, errorText, type FileChange, type GitCommit, type GitDiff, type GitStatus, type ModConflict, type Prefer } from '../lib/api'
  import { job, onJobEnd } from '../lib/jobs.svelte'
  import { nav, showInFiles } from '../lib/nav.svelte'
  import { confirmDiscard, discardChanges, ignoreItems, type Feedback } from '../lib/gitActions'
  import { openIgnoreDialog } from '../lib/ignoreDialog.svelte'
  import { changeRows, type ChangeRow } from '../lib/changeTree'
  import { RangeSelection, isPlainClick } from '../lib/rangeSelection'
  import { commandKey, fileManager, isMac, shortcut } from '../lib/platform'
  import type { MenuItem } from '../lib/menu'
  import ContextMenu from '../components/ContextMenu.svelte'
  import Twisty from '../components/Twisty.svelte'
  import Notice from '../components/Notice.svelte'
  import LogView from '../components/LogView.svelte'
  import LinkedText from '../components/LinkedText.svelte'

  let { onchange }: { onchange: () => void } = $props()

  type Group = 'staged' | 'unstaged'
  type Layout = 'list' | 'tree'
  type Selection = { path: string; staged: boolean; untracked: boolean }

  const LAYOUT_KEY = 'packraven.gitLayout'

  let status = $state<GitStatus | null>(null)
  let commits = $state<GitCommit[]>([])
  // Selection keys are the group and path, with a trailing slash for folder rows in the tree.
  const keys = new SvelteSet<string>()
  const picker = new RangeSelection(keys)
  const collapsed = new SvelteSet<string>()
  let focused = $state<string | null>(null)
  let layout = $state<Layout>(storedLayout())
  let diff = $state<GitDiff | null>(null)
  let diffLoading = $state(false)
  let message = $state('')
  let error = $state<string | null>(null)
  let loadError = $state<string | null>(null)
  let lastAction = $state<string | null>(null)
  let notice = $state<string | null>(null)
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null)
  let conflicts = $state<ModConflict[] | null>(null)
  let checkingPull = $state(false)

  const statusNames: Record<string, string> = {
    M: 'Modified',
    A: 'Added',
    D: 'Deleted',
    R: 'Renamed',
    C: 'Copied',
    T: 'Type changed',
    '?': 'New',
    U: 'Conflict',
  }

  // The poll clears only its own error, so a failed stage or commit stays on screen until your next action.
  let loading: Promise<void> | null = null

  function load() {
    loading ??= (async () => {
      try {
        const result = await api.gitOverview()
        status = result.status
        commits = result.commits
        loadError = null
        for (const key of [...keys]) if (!changesFor(key).length) keys.delete(key)
      } catch (e) {
        loadError = errorText(e)
      } finally {
        loading = null
      }
    })()
    return loading
  }

  // Each tick we also re-read the open diff, so edits on disk show up without you reselecting the file.
  function poll() {
    if (document.hidden || loading) return
    load().then(() => selected && loadDiff(selected, false))
  }

  function storedLayout(): Layout {
    try {
      return localStorage.getItem(LAYOUT_KEY) === 'tree' ? 'tree' : 'list'
    } catch {
      return 'list'
    }
  }

  // Folder rows only exist in the tree, so we drop them from the selection when you leave it.
  function setLayout(next: Layout) {
    layout = next
    if (next === 'list') for (const key of [...keys]) if (key.endsWith('/')) keys.delete(key)
    try {
      localStorage.setItem(LAYOUT_KEY, next)
    } catch {}
  }

  function keyOf(group: Group, row: { path: string; isDir: boolean }) {
    return `${group}:${row.path}${row.isDir ? '/' : ''}`
  }

  function parseKey(key: string) {
    const at = key.indexOf(':')
    const rest = key.slice(at + 1)
    const isDir = rest.endsWith('/')
    return { group: key.slice(0, at) as Group, path: isDir ? rest.slice(0, -1) : rest, isDir }
  }

  function changesFor(key: string): FileChange[] {
    const { group, path, isDir } = parseKey(key)
    return groupChanges[group].filter((c) => (isDir ? c.path.startsWith(`${path}/`) : c.path === path))
  }

  function selectedChanges(group: Group) {
    const found = new Map<string, FileChange>()
    for (const key of keys) if (key.startsWith(`${group}:`)) for (const change of changesFor(key)) found.set(change.path, change)
    return [...found.values()]
  }

  function selectedPaths() {
    return [...new Set([...keys].map((key) => parseKey(key).path))]
  }

  onMount(() => {
    load()
    const timer = setInterval(poll, 5000)
    return () => clearInterval(timer)
  })

  onJobEnd('git', (phase) => {
    load()
    onchange()
    if (phase === 'done' && lastAction === 'commit') message = ''
  })

  // We clear the old diff on a new selection, so the header never sits over another file's changes.
  function loadDiff(current: Selection, fresh: boolean) {
    if (fresh) {
      diff = null
      diffLoading = true
    }
    api
      .gitDiff(current.path, current.staged, current.untracked)
      .then((d) => {
        if (selected === current) diff = d
      })
      .catch((e) => selected === current && (error = errorText(e)))
      .finally(() => selected === current && (diffLoading = false))
  }

  $effect(() => {
    const current = selected
    if (!current) {
      diff = null
      return
    }
    loadDiff(current, true)
  })

  // The Files tree's "Show diff" sends you here with a path to select.
  $effect(() => {
    const path = nav.diffPath
    if (!path || !status) return
    nav.diffPath = null
    const group: Group | null = status.unstaged.some((c) => c.path === path) ? 'unstaged' : status.staged.some((c) => c.path === path) ? 'staged' : null
    if (!group) return
    for (let at = path.indexOf('/'); at > 0; at = path.indexOf('/', at + 1)) collapsed.delete(`${group}:${path.slice(0, at)}`)
    const key = keyOf(group, { path, isDir: false })
    picker.only(key)
    focused = key
    tick().then(() => rowElement(key)?.scrollIntoView({ block: 'nearest' }))
  })

  async function run(action: () => Promise<void>) {
    try {
      error = null
      notice = null
      await action()
      await load()
      onchange()
    } catch (e) {
      error = errorText(e)
    }
  }

  async function discard(changes: FileChange[]) {
    if (changes.length && (await confirmDiscard(changes))) await run(() => discardChanges(changes))
  }

  function stage(changes: FileChange[]) {
    if (changes.length) run(() => api.gitStage(changes.map((c) => c.path)))
  }

  function unstage(changes: FileChange[]) {
    if (changes.length) run(() => api.gitUnstage(changes.map((c) => c.path)))
  }

  const feedback: Feedback = {
    ondone: () => (load(), onchange()),
    onerror: (text) => (error = text),
    onnotice: (text) => (notice = text),
  }

  async function ignoreSelection() {
    const paths = selectedPaths()
    try {
      openIgnoreDialog({ paths, options: await api.gitIgnoreOptions(paths), chosen: 0, feedback })
    } catch (e) {
      error = errorText(e)
    }
  }

  async function attempt(action: () => Promise<void>) {
    try {
      error = null
      await action()
    } catch (e) {
      error = errorText(e)
    }
  }

  function rowElement(key: string) {
    return document.querySelector<HTMLElement>(`.changes [data-key="${CSS.escape(key)}"]`)
  }

  async function focusKey(key: string) {
    focused = key
    await tick()
    rowElement(key)?.focus()
  }

  function toggleFolder(group: Group, path: string, open = collapsed.has(`${group}:${path}`)) {
    if (open) collapsed.delete(`${group}:${path}`)
    else collapsed.add(`${group}:${path}`)
  }

  function onRowClick(event: MouseEvent, group: Group, row: ChangeRow) {
    const key = keyOf(group, row)
    focused = key
    picker.click(event, key, order[group])
    if (isPlainClick(event) && row.isDir) toggleFolder(group, row.path)
  }

  function onRowDoubleClick(event: MouseEvent, group: Group, row: ChangeRow) {
    if (row.isDir || !isPlainClick(event)) return
    const changes = changesFor(keyOf(group, row))
    if (group === 'staged') unstage(changes)
    else stage(changes)
  }

  // We add the focused row to the selection first, so keys act on the row you can see is focused.
  function onGroupKey(event: KeyboardEvent, group: Group) {
    const list = order[group]
    const index = list.indexOf(focused ?? '')
    const row = rowsByGroup[group][index]
    if (!row) return
    const key = list[index]
    const move = (to: number) => {
      const target = list[Math.max(0, Math.min(to, list.length - 1))]
      if (event.shiftKey) picker.range(list, target)
      else picker.only(target)
      focusKey(target)
    }
    const ensureSelected = () => keys.has(key) || picker.only(key)
    const folderOpen = row.isDir && !collapsed.has(`${group}:${row.path}`)
    if (commandKey(event) && event.key.toLowerCase() === 'a') {
      picker.replace(list)
    } else if (event.key === 'ArrowDown') {
      move(index + 1)
    } else if (event.key === 'ArrowUp') {
      move(index - 1)
    } else if (event.key === 'Home') {
      move(0)
    } else if (event.key === 'End') {
      move(list.length - 1)
    } else if (event.key === 'ArrowRight' && row.isDir) {
      if (folderOpen) move(index + 1)
      else toggleFolder(group, row.path, true)
    } else if (event.key === 'ArrowLeft' && folderOpen) {
      toggleFolder(group, row.path, false)
    } else if (event.key === 'ArrowLeft' && row.depth > 0) {
      move(rowsByGroup[group].findLastIndex((r, i) => i < index && r.depth === row.depth - 1))
    } else if (event.key === ' ') {
      if (commandKey(event)) picker.toggle(key)
      else if (event.shiftKey) picker.range(list, key)
      else picker.only(key)
    } else if (event.key === 'Enter') {
      ensureSelected()
      if (group === 'staged') unstage(selectedStaged)
      else stage(selectedUnstaged)
    } else if (event.key === 'Delete' || (isMac && event.metaKey && event.key === 'Backspace')) {
      ensureSelected()
      discard(selectedUnstaged)
    } else if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      ensureSelected()
      const rect = (event.target as HTMLElement).getBoundingClientRect()
      openSelectionMenu(rect.left + 24, rect.bottom)
    } else {
      return
    }
    event.preventDefault()
  }

  function onRowMenu(event: MouseEvent, group: Group, row: ChangeRow) {
    event.preventDefault()
    event.stopPropagation()
    const key = keyOf(group, row)
    if (!keys.has(key)) picker.only(key)
    focused = key
    // The keyboard menu key fires contextmenu without pointer coordinates.
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
    const fromKeyboard = !event.clientX && !event.clientY
    openSelectionMenu(fromKeyboard ? rect.left + 24 : event.clientX, fromKeyboard ? rect.bottom : event.clientY)
  }

  // A deleted file has nothing on disk to open or show, but you can still copy its path.
  async function openSelectionMenu(x: number, y: number) {
    const staged = selectedStaged
    const unstaged = selectedUnstaged
    const paths = selectedPaths()
    const only = keys.size === 1 ? [...keys][0] : null
    const single = only ? parseKey(only) : null
    const exists = single?.isDir || (!!only && changesFor(only)[0]?.status !== 'D')
    const mixed = staged.length > 0 && unstaged.length > 0
    const files = (n: number) => (n === 1 && !mixed ? '' : ` ${n} ${n === 1 ? 'file' : 'files'}`)
    const ignore = await ignoreItems(paths, feedback)
    menu = {
      x,
      y,
      items: [
        ...(unstaged.length ? [{ label: `Stage${files(unstaged.length)}`, shortcut: 'Enter', run: () => stage(unstaged) }] : []),
        ...(staged.length ? [{ label: `Unstage${files(staged.length)}`, shortcut: unstaged.length ? undefined : 'Enter', run: () => unstage(staged) }] : []),
        'separator',
        ...(single && exists
          ? [
              { label: 'Open in Files', run: () => showInFiles(single.path) },
              ...(single.isDir ? [] : [{ label: 'Open in default app', run: () => attempt(() => api.openWithDefaultApp(single.path)) }]),
              { label: `Show in ${fileManager}`, run: () => attempt(async () => revealItemInDir(await api.absolutePath(single.path))) },
            ]
          : []),
        'separator',
        { label: paths.length > 1 ? 'Copy paths' : 'Copy path', run: () => attempt(() => api.copyPaths(paths, true)) },
        { label: paths.length > 1 ? 'Copy relative paths' : 'Copy relative path', run: () => attempt(() => api.copyPaths(paths, false)) },
        'separator',
        ...(ignore.length ? [{ label: 'Add to .gitignore', items: ignore }] : []),
        ...(unstaged.length
          ? ['separator' as const, { label: `Discard changes${unstaged.length === 1 ? '' : ` to ${unstaged.length} files`}`, shortcut: shortcut('Delete'), danger: true, run: () => discard(unstaged) }]
          : []),
      ],
    }
  }

  async function commit() {
    if (!canCommit) return
    lastAction = 'commit'
    await run(() => api.gitCommit(message))
  }

  async function sync(action: 'fetch' | 'push') {
    lastAction = action
    await run(() => api.gitSync(action))
  }

  // We fetch and merge the mod lists first, so you choose between conflicting mods before git changes anything.
  async function pull() {
    lastAction = 'pull'
    conflicts = null
    checkingPull = true
    try {
      error = null
      const preview = await api.gitPullPreview()
      if (preview.conflicts.length) {
        conflicts = preview.conflicts
        return
      }
    } catch (e) {
      error = `Couldn't check the remote: ${errorText(e)}`
      return
    } finally {
      checkingPull = false
    }
    await run(() => api.gitSync('pull'))
  }

  async function pullPreferring(prefer: Prefer) {
    conflicts = null
    await run(() => api.gitSync('pull', prefer))
  }

  function split(path: string) {
    const index = path.lastIndexOf('/')
    return index < 0 ? { dir: '', name: path } : { dir: path.slice(0, index + 1), name: path.slice(index + 1) }
  }

  function diffClass(line: string) {
    if (line.startsWith('@@')) return 'hunk'
    if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('diff ') || line.startsWith('index ')) return 'meta'
    if (line.startsWith('+')) return 'add'
    if (line.startsWith('-')) return 'del'
    return ''
  }

  let busy = $derived(job('git').phase === 'running' || checkingPull)
  let canCommit = $derived(!busy && !!status?.staged.length && message.trim().length > 0)
  let commitHint = $derived(
    !status?.staged.length
      ? status?.unstaged.length
        ? 'Stage something, then you can commit it.'
        : 'Nothing to commit.'
      : !message.trim()
        ? 'Write a message describing the change.'
        : null,
  )
  let diffLines = $derived(diff ? diff.text.split('\n').filter((l) => !l.startsWith('\\ No newline')) : [])
  let groupChanges = $derived({ staged: status?.staged ?? [], unstaged: status?.unstaged ?? [] })
  let rowsByGroup = $derived({
    staged: changeRows(groupChanges.staged, layout === 'tree', (dir) => collapsed.has(`staged:${dir}`)),
    unstaged: changeRows(groupChanges.unstaged, layout === 'tree', (dir) => collapsed.has(`unstaged:${dir}`)),
  })
  let order = $derived({ staged: rowsByGroup.staged.map((r) => keyOf('staged', r)), unstaged: rowsByGroup.unstaged.map((r) => keyOf('unstaged', r)) })
  let tabStop = $derived({
    staged: focused && order.staged.includes(focused) ? focused : order.staged[0],
    unstaged: focused && order.unstaged.includes(focused) ? focused : order.unstaged[0],
  })
  let selectedStaged = $derived(selectedChanges('staged'))
  let selectedUnstaged = $derived(selectedChanges('unstaged'))
  // We show a diff when you select exactly one file, and read the status untracked so the poll
  // doesn't reload a diff that's already open.
  let diffKey = $derived(keys.size === 1 && ![...keys][0].endsWith('/') ? [...keys][0] : null)
  let selected = $derived.by((): Selection | null => {
    if (!diffKey) return null
    const { group, path } = parseKey(diffKey)
    const change = untrack(() => groupChanges[group].find((c) => c.path === path))
    return { path, staged: group === 'staged', untracked: change?.status === '?' }
  })
  let gitJob = $derived(job('git'))
</script>

{#snippet changeRow(group: Group, row: ChangeRow)}
  {@const key = keyOf(group, row)}
  {@const code = row.change ? (row.change.status === '?' ? 'N' : row.change.status) : null}
  {@const parts = split(row.path)}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_tabindex (the group handles keys, and Svelte can't read the role) -->
  <div
    role={layout === 'tree' ? 'treeitem' : 'option'}
    aria-selected={keys.has(key)}
    aria-level={layout === 'tree' ? row.depth + 1 : undefined}
    aria-expanded={row.isDir ? !collapsed.has(`${group}:${row.path}`) : undefined}
    tabindex={key === tabStop[group] ? 0 : -1}
    data-key={key}
    class="row"
    class:selected={keys.has(key)}
    style="--depth: {row.depth}"
    title={row.isDir ? undefined : `Double-click to ${group === 'staged' ? 'unstage' : 'stage'}`}
    onclick={(e) => onRowClick(e, group, row)}
    ondblclick={(e) => onRowDoubleClick(e, group, row)}
    oncontextmenu={(e) => onRowMenu(e, group, row)}
    onfocus={() => (focused = key)}
  >
    {#if layout === 'tree'}<Twisty open={row.isDir ? !collapsed.has(`${group}:${row.path}`) : null} />{/if}
    {#if row.isDir}
      <span class="path"><span class="name folder">{row.name}</span></span>
      <span class="count">{row.count}</span>
    {:else if row.change}
      <span class="badge s{code}" title={statusNames[row.change.status] ?? row.change.status}>{code}</span>
      <span class="visually-hidden">{statusNames[row.change.status] ?? row.change.status}:</span>
      <span class="path">{#if layout === 'list'}<span class="dir">{parts.dir}</span>{/if}<span class="name">{parts.name}</span></span>
    {/if}
    <!-- Hidden from screen readers because options can't hold buttons; Enter, Delete and the menu key cover them. -->
    <!-- svelte-ignore a11y_no_static_element_interactions (only stops the row's double-click) -->
    <span class="row-actions" aria-hidden="true" ondblclick={(e) => e.stopPropagation()}>
      {#if group === 'staged'}
        <button class="icon" tabindex="-1" aria-label="Unstage {row.path}" title="Unstage" onclick={(e) => (e.stopPropagation(), unstage(changesFor(key)))}>−</button>
      {:else}
        <button class="icon" tabindex="-1" aria-label="Discard changes to {row.path}" title="Discard changes" onclick={(e) => (e.stopPropagation(), discard(changesFor(key)))}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M5 3 2 6l3 3M2.5 6H10a4 4 0 0 1 0 8H7" /></svg>
        </button>
        <button class="icon" tabindex="-1" aria-label="Stage {row.path}" title="Stage" onclick={(e) => (e.stopPropagation(), stage(changesFor(key)))}>+</button>
      {/if}
    </span>
  </div>
{/snippet}

{#snippet changeGroup(group: Group, heading: string)}
  <div class="changes" role={layout === 'tree' ? 'tree' : 'listbox'} aria-multiselectable="true" aria-labelledby={heading} tabindex="-1" onkeydown={(e) => onGroupKey(e, group)}>
    {#each rowsByGroup[group] as row (keyOf(group, row))}{@render changeRow(group, row)}{/each}
  </div>
{/snippet}

<div class="git">
  <header class="head">
    <div>
      <h2>Git</h2>
      <p class="muted">
        On <span class="file">{status?.branch ?? 'no branch'}</span>.
        {#if !status?.hasRemote}
          No remote set, so there's nowhere to push yet.
        {:else if !status.upstream}
          Not pushed yet. Push to create the branch on the remote.
        {:else if status.ahead || status.behind}
          {[status.ahead && `${status.ahead} to push`, status.behind && `${status.behind} to pull`].filter(Boolean).join(', ')}.
        {:else}
          Up to date with {status.upstream}.
        {/if}
      </p>
    </div>
    <div class="sync">
      <button disabled={busy || !status?.hasRemote} onclick={() => sync('fetch')} class:busy={busy && lastAction === 'fetch'}>Fetch</button>
      <button disabled={busy || !status?.upstream} onclick={pull} class:busy={busy && lastAction === 'pull'} class:primary={!!status?.behind}>Pull</button>
      <button disabled={busy || !status?.hasRemote || !status?.hasCommits} onclick={() => sync('push')} class:busy={busy && lastAction === 'push'} class:primary={!!status?.ahead && !status?.behind}>Push</button>
    </div>
  </header>

  {#if conflicts}
    <Notice tone="warn">
      <div class="conflicts">
        <p>
          You and the remote both changed {conflicts.length === 1 ? 'this mod' : `these ${conflicts.length} mods`}. Choose whose versions to keep; packraven
          keeps every other change from both sides.
        </p>
        <ul class="conflict-list">
          {#each conflicts as conflict (conflict.name)}
            <li>
              <span class="file">{conflict.name}</span>
              <dl>
                <dt>Yours</dt><dd class:file={!!conflict.mine}>{conflict.mine ?? 'Removed'}</dd>
                <dt>Theirs</dt><dd class:file={!!conflict.theirs}>{conflict.theirs ?? 'Removed'}</dd>
              </dl>
            </li>
          {/each}
        </ul>
        <div class="conflict-actions">
          <button disabled={busy} onclick={() => pullPreferring('mine')}>Pull, keeping yours</button>
          <button disabled={busy} onclick={() => pullPreferring('theirs')}>Pull, keeping theirs</button>
          <button class="quiet" onclick={() => (conflicts = null)}>Cancel</button>
        </div>
      </div>
    </Notice>
  {/if}

  <div class="work">
  <div class="side">

    <form class="commit" onsubmit={(e) => { e.preventDefault(); commit() }}>
      <label for="commit-message" class="visually-hidden">Commit message</label>
      <textarea
        id="commit-message"
        bind:value={message}
        rows="3"
        placeholder="Commit message"
        aria-describedby="commit-hint"
        onkeydown={(e) => (e.ctrlKey || e.metaKey) && e.key === 'Enter' && (e.preventDefault(), commit())}
      ></textarea>
      <div class="commit-row">
        <span id="commit-hint" class="muted small">{commitHint ?? `Press ${shortcut('Ctrl+Enter')} to commit.`}</span>
        <button type="submit" class="primary" class:busy={busy && lastAction === 'commit'} disabled={!canCommit}>
          {busy && lastAction === 'commit' ? 'Committing' : `Commit ${status?.staged.length ?? 0} ${status?.staged.length === 1 ? 'file' : 'files'}`}
        </button>
      </div>
    </form>

    {#if error}<Notice tone="error"><p>{error}</p></Notice>{/if}
    {#if loadError}<Notice tone="error"><p>Couldn't read the repository: {loadError}</p></Notice>{/if}
    {#if notice}<Notice tone="warn"><p>{notice}</p></Notice>{/if}
    {#if gitJob.phase === 'failed'}<Notice tone="error"><p><LinkedText text={gitJob.message ?? ''} /></p></Notice>{/if}
    {#if status?.conflicted.length}
      <Notice tone="warn"><p>{status.conflicted.length === 1 ? '1 file has a merge conflict' : `${status.conflicted.length} files have merge conflicts`}. Resolve them in an editor, then stage them.</p></Notice>
    {/if}

    <div class="layout-row">
      <span class="modes" role="group" aria-label="Show changes as">
        <button class="mode" aria-pressed={layout === 'list'} onclick={() => setLayout('list')}>List</button>
        <button class="mode" aria-pressed={layout === 'tree'} onclick={() => setLayout('tree')}>Tree</button>
      </span>
    </div>

    <section class="group" aria-labelledby="staged-heading">
      <div class="group-head">
        <h3 id="staged-heading">Staged <span class="count">{status?.staged.length ?? 0}</span></h3>
        {#if status?.staged.length}
          <button class="link" onclick={() => run(() => api.gitUnstage(status!.staged.map((c) => c.path)))}>Unstage all</button>
        {/if}
      </div>
      {#if status?.staged.length}
        {@render changeGroup('staged', 'staged-heading')}
      {:else}
        <p class="muted small empty">Staged files will appear here.</p>
      {/if}
    </section>

    <section class="group" aria-labelledby="changes-heading">
      <div class="group-head">
        <h3 id="changes-heading">Changes <span class="count">{status?.unstaged.length ?? 0}</span></h3>
        {#if status?.unstaged.length}
          <button class="link" onclick={() => run(() => api.gitStage(status!.unstaged.map((c) => c.path)))}>Stage all</button>
        {/if}
      </div>
      {#if status?.unstaged.length}
        {@render changeGroup('unstaged', 'changes-heading')}
      {:else if status}
        <p class="muted small empty">No unstaged changes. Mod changes will only show here as a change to mods.lock.json.</p>
      {/if}
    </section>

    {#if gitJob.lines.length}
      <section class="group">
        <h3>Last git run</h3>
        <div class="git-log"><LogView lines={gitJob.lines} label="Git output" /></div>
      </section>
    {/if}

    <section class="group" aria-labelledby="history-heading">
      <h3 id="history-heading">Recent commits</h3>
      {#if commits.length}
        <ol class="history">
          {#each commits as c (c.hash)}
            <li>
              <span class="subject">{c.subject}</span>
              <span class="muted small"><span class="file">{c.short}</span> {c.author}, {c.when}</span>
            </li>
          {/each}
        </ol>
      {:else}
        <p class="muted small empty">No commits yet.</p>
      {/if}
    </section>
  </div>

  <section class="diff" aria-label="Diff">
    {#if !selected && keys.size}
      {@const total = selectedStaged.length + selectedUnstaged.length}
      <div class="diff-head">
        <span class="selection-count">{total === 1 ? '1 file selected' : `${total} files selected`}</span>
        <span class="muted small state">{[selectedStaged.length && `${selectedStaged.length} staged`, selectedUnstaged.length && `${selectedUnstaged.length} not staged`].filter(Boolean).join(', ')}</span>
      </div>
      <div class="pad summary">
        <div class="summary-actions">
          {#if selectedUnstaged.length}<button class="primary" onclick={() => stage(selectedUnstaged)}>Stage {selectedUnstaged.length === 1 ? '1 file' : `${selectedUnstaged.length} files`}</button>{/if}
          {#if selectedStaged.length}<button onclick={() => unstage(selectedStaged)}>Unstage {selectedStaged.length === 1 ? '1 file' : `${selectedStaged.length} files`}</button>{/if}
          <button onclick={ignoreSelection}>Add to .gitignore…</button>
          {#if selectedUnstaged.length}<button class="danger" onclick={() => discard(selectedUnstaged)}>Discard changes{selectedUnstaged.length === 1 ? '' : ` to ${selectedUnstaged.length} files`}</button>{/if}
        </div>
        <ul class="summary-list" aria-label="Selected files">
          {#each [...selectedStaged.map((c) => ({ c, group: 'Staged' })), ...selectedUnstaged.map((c) => ({ c, group: 'Not staged' }))] as item (`${item.group}:${item.c.path}`)}
            <li>
              <span class="badge s{item.c.status === '?' ? 'N' : item.c.status}" title={statusNames[item.c.status] ?? item.c.status}>{item.c.status === '?' ? 'N' : item.c.status}</span>
              <span class="file path-text">{item.c.path}</span>
              <span class="muted small state">{item.group}</span>
            </li>
          {/each}
        </ul>
      </div>
    {:else if !selected}
      <div class="placeholder">
        <p>Select a file to see what changed.</p>
        <p class="small">Shift or {isMac ? '⌘' : 'Ctrl'}+click to select several.</p>
      </div>
    {:else}
      <div class="diff-head">
        <span class="file path-text" title={selected.path}>{selected.path}</span>
        <span class="muted small state">{selected.staged ? 'Staged' : selected.untracked ? 'New file' : 'Not staged'}</span>
      </div>
      {#if !diff && diffLoading}
        <p class="muted pad">Loading the diff.</p>
      {:else if diff?.binary}
        <p class="muted pad">This is a binary file, so there's no text diff to show.</p>
      {:else if diff && !diff.text.trim()}
        <p class="muted pad">No content changes. Git flagged this file over line endings or file metadata only; you can stage it to clear the flag.</p>
      {:else if diff}
        {#if diff.truncated}<div class="pad"><Notice tone="warn"><p>This diff is very large, so packraven shows only the first part.</p></Notice></div>{/if}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div class="diff-body" role="region" aria-label="Changes in {selected.path}" tabindex="0">
          {#each diffLines as line, i (i)}
            <div class="line {diffClass(line)}">{line || ' '}</div>
          {/each}
        </div>
      {/if}
    {/if}
  </section>
  </div>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} label="Change actions" items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .conflicts {
    display: grid;
    gap: 10px;
  }

  .conflict-list {
    display: grid;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
    overflow-wrap: anywhere;
  }

  .conflict-list dl {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 0 10px;
    margin: 2px 0 0;
    font-size: var(--step--1);
  }

  .conflict-list dt {
    color: var(--muted);
  }

  .conflict-list dd {
    margin: 0;
  }

  .conflict-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .git {
    display: flex;
    flex-direction: column;
    gap: 18px;
    flex: 1;
    min-height: 420px;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: end;
    gap: 12px 24px;
  }

  .head p {
    margin-top: 4px;
  }

  .work {
    flex: 1;
    display: grid;
    grid-template-columns: minmax(260px, 34%) minmax(0, 1fr);
    gap: 24px;
    min-height: 0;
  }

  .side {
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-height: 0;
    overflow-y: auto;
    padding-right: 4px;
  }

  .small {
    font-size: var(--step--1);
  }

  .sync {
    display: flex;
    gap: 4px;
  }

  .sync button {
    padding: 5px 10px;
  }

  .commit {
    display: grid;
    gap: 8px;
  }

  .commit textarea {
    width: 100%;
    min-height: 72px;
  }

  .commit-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }

  .commit-row button {
    flex: none;
    white-space: nowrap;
  }

  .group {
    display: grid;
    gap: 6px;
  }

  .group-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  h3 {
    font-size: var(--step-0);
    font-weight: 800;
  }

  .count {
    margin-left: 4px;
    color: var(--muted);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }

  .group-head .link {
    font-size: var(--step--1);
    font-weight: 400;
  }

  .empty {
    padding: 6px 0;
  }

  .changes {
    min-width: 0;
    border-top: 1px solid var(--line);
    outline: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    min-height: 34px;
    padding: 0 0 0 calc(8px + var(--depth) * 16px);
    border-bottom: 1px solid var(--line);
    cursor: pointer;
    user-select: none;
  }

  .row:hover {
    background: var(--surface);
  }

  .row.selected {
    background: var(--here);
  }

  .row:focus-visible {
    outline: 2px solid var(--forest);
    outline-offset: -2px;
  }

  .folder {
    font-weight: 600;
  }

  .row .count {
    flex: none;
    font-size: var(--step--2);
  }

  .layout-row {
    display: flex;
    justify-content: end;
  }

  .badge {
    flex: none;
    width: 20px;
    text-align: center;
    font-family: var(--font-log);
    font-size: var(--step--2);
    font-weight: 700;
    border-radius: var(--radius-small);
    color: var(--muted);
    background: var(--highlight);
  }

  .badge.sA,
  .badge.sN {
    color: var(--forest-hi);
    background: var(--ok-fill);
  }

  .badge.sD {
    color: var(--fail);
    background: var(--fail-fill);
  }

  .badge.sM,
  .badge.sR {
    color: var(--warn);
    background: var(--warn-fill);
  }

  .path {
    flex: 1;
    display: flex;
    min-width: 0;
    white-space: nowrap;
    font-size: var(--step--1);
  }

  .dir {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--muted);
  }

  .name {
    flex: 0 0 auto;
    min-width: 0;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row-actions {
    flex: none;
    display: flex;
    gap: 2px;
    padding-right: 4px;
    color: var(--muted);
  }

  .row-actions .icon {
    color: inherit;
  }

  .row:hover .row-actions,
  .row:focus-within .row-actions,
  .row.selected .row-actions {
    color: var(--text);
  }

  .icon {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    font-size: var(--step-1);
    line-height: 1;
    background: transparent;
    border-color: transparent;
  }

  .icon:hover:not(:disabled) {
    background: var(--control);
    border-color: var(--control-edge);
  }

  .icon svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .git-log {
    display: flex;
    height: 180px;
  }

  .history {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .history li {
    display: grid;
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
  }

  .subject {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .diff {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
    background: var(--field);
    overflow: hidden;
  }

  .placeholder {
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 4px;
    flex: 1;
    color: var(--muted);
  }

  .diff-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    min-width: 0;
    gap: 12px;
    padding: 10px 14px;
    background: var(--surface);
    border-bottom: 1px solid var(--line);
  }

  .path-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .state {
    flex: none;
  }

  .pad {
    padding: 14px;
  }

  .summary {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    align-content: start;
    gap: 14px;
    min-height: 0;
    overflow: auto;
  }

  .summary-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .summary-list {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .summary-list li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 5px 0;
    font-size: var(--step--1);
    border-bottom: 1px solid var(--line);
  }

  .summary-list .path-text {
    flex: 1;
  }

  .selection-count {
    font-weight: 700;
  }

  .diff-body {
    flex: 1;
    overflow: auto;
    padding: 8px 0;
    font-family: var(--font-log);
    font-size: var(--step--2);
    line-height: 1.5;
  }

  .line {
    padding: 0 14px;
    white-space: pre;
    content-visibility: auto;
    contain-intrinsic-size: auto 1.5em;
  }

  .line.add {
    background: var(--ok-fill);
  }

  .line.del {
    background: var(--fail-fill);
  }

  .line.hunk {
    color: var(--muted);
    background: var(--surface);
    margin-top: 6px;
  }

  .line.meta {
    color: var(--muted);
  }
</style>
