<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { revealItemInDir } from '@tauri-apps/plugin-opener'
  import { api, errorText, type FileChange, type GitCommit, type GitDiff, type GitStatus, type ModConflict, type Prefer } from '../lib/api'
  import { job, onJobEnd } from '../lib/jobs.svelte'
  import { nav, showInFiles } from '../lib/nav.svelte'
  import { addToGitignore, confirmDiscard, discardChanges } from '../lib/gitActions'
  import { fileManager, shortcut } from '../lib/platform'
  import type { MenuItem } from '../lib/menu'
  import ContextMenu from '../components/ContextMenu.svelte'
  import Notice from '../components/Notice.svelte'
  import LogView from '../components/LogView.svelte'
  import LinkedText from '../components/LinkedText.svelte'

  let { onchange }: { onchange: () => void } = $props()

  type Selection = { path: string; staged: boolean; untracked: boolean }

  let status = $state<GitStatus | null>(null)
  let commits = $state<GitCommit[]>([])
  let selected = $state<Selection | null>(null)
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
        if (selected && !stillListed(selected)) selected = null
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

  function stillListed(s: Selection) {
    const list = s.staged ? status?.staged : status?.unstaged
    return list?.some((c) => c.path === s.path) ?? false
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
    const unstaged = status.unstaged.find((c) => c.path === path)
    const change = unstaged ?? status.staged.find((c) => c.path === path)
    if (!change) return
    selected = { path, staged: !unstaged, untracked: change.status === '?' }
    tick().then(() => document.querySelector('.changes li.selected')?.scrollIntoView({ block: 'nearest' }))
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
    if (await confirmDiscard(changes)) await run(() => discardChanges(changes))
  }

  async function attempt(action: () => Promise<void>) {
    try {
      error = null
      await action()
    } catch (e) {
      error = errorText(e)
    }
  }

  function select(change: FileChange, staged: boolean) {
    selected = { path: change.path, staged, untracked: change.status === '?' }
  }

  function onChangeMenu(event: MouseEvent | KeyboardEvent, change: FileChange, staged: boolean) {
    event.preventDefault()
    event.stopPropagation()
    select(change, staged)
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
    const fromKeyboard = !('clientX' in event) || (!event.clientX && !event.clientY)
    openChangeMenu(fromKeyboard ? rect.left + 24 : event.clientX, fromKeyboard ? rect.bottom : event.clientY, change, staged)
  }

  // A deleted file has nothing on disk to open or show, but you can still copy its path.
  function openChangeMenu(x: number, y: number, change: FileChange, staged: boolean) {
    const path = change.path
    const exists = change.status !== 'D'
    menu = {
      x,
      y,
      items: [
        staged
          ? { label: 'Unstage', run: () => run(() => api.gitUnstage([path])) }
          : { label: 'Stage', run: () => run(() => api.gitStage([path])) },
        'separator',
        ...(exists
          ? [
              { label: 'Open in Files', run: () => showInFiles(path) },
              { label: 'Open in default app', run: () => attempt(() => api.openWithDefaultApp(path)) },
              { label: `Show in ${fileManager}`, run: () => attempt(async () => revealItemInDir(await api.absolutePath(path))) },
            ]
          : []),
        'separator',
        { label: 'Copy path', run: () => attempt(() => api.copyPaths([path], true)) },
        { label: 'Copy relative path', run: () => attempt(() => api.copyPaths([path], false)) },
        'separator',
        {
          label: 'Add to .gitignore',
          run: () =>
            addToGitignore([path], {
              ondone: () => (load(), onchange()),
              onerror: (text) => (error = text),
              onnotice: (text) => (notice = text),
            }),
        },
        ...(!staged ? ['separator' as const, { label: 'Discard changes', danger: true, run: () => discard([change]) }] : []),
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
        ? 'Stage the files you want to include, then commit.'
        : 'Nothing to commit.'
      : !message.trim()
        ? 'Write a message describing the change.'
        : null,
  )
  let diffLines = $derived(diff ? diff.text.split('\n').filter((l) => !l.startsWith('\\ No newline')) : [])
  let gitJob = $derived(job('git'))
</script>

{#snippet changeRow(change: FileChange, staged: boolean)}
  {@const parts = split(change.path)}
  {@const isSelected = selected?.path === change.path && selected?.staged === staged}
  <li class:selected={isSelected} oncontextmenu={(e) => onChangeMenu(e, change, staged)}>
    <button
      class="file-button"
      onclick={() => select(change, staged)}
      onkeydown={(e) => (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) && onChangeMenu(e, change, staged)}
      ondblclick={() => run(() => (staged ? api.gitUnstage([change.path]) : api.gitStage([change.path])))}
      title="Double-click to {staged ? 'unstage' : 'stage'}"
      aria-pressed={isSelected}
    >
      <span class="badge s{change.status === '?' ? 'N' : change.status}" title={statusNames[change.status] ?? change.status}>{change.status === '?' ? 'N' : change.status}</span>
      <span class="visually-hidden">{statusNames[change.status] ?? change.status}:</span>
      <span class="path"><span class="dir">{parts.dir}</span><span class="name">{parts.name}</span></span>
    </button>
    <span class="row-actions">
      {#if staged}
        <button class="icon" aria-label="Unstage {change.path}" title="Unstage" onclick={() => run(() => api.gitUnstage([change.path]))}>−</button>
      {:else}
        <button class="icon" aria-label="Discard changes to {change.path}" title="Discard changes" onclick={() => discard([change])}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M5 3 2 6l3 3M2.5 6H10a4 4 0 0 1 0 8H7" /></svg>
        </button>
        <button class="icon" aria-label="Stage {change.path}" title="Stage" onclick={() => run(() => api.gitStage([change.path]))}>+</button>
      {/if}
    </span>
  </li>
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
            <button onclick={() => pullPreferring('theirs')}>Pull, keeping theirs</button>
            <button onclick={() => pullPreferring('mine')}>Pull, keeping mine</button>
            <button class="quiet" onclick={() => (conflicts = null)}>Cancel</button>
          </div>
        </div>
      </Notice>
    {/if}
    {#if status?.conflicted.length}
      <Notice tone="warn"><p>{status.conflicted.length === 1 ? '1 file has a merge conflict' : `${status.conflicted.length} files have merge conflicts`}. Resolve them in an editor, then stage them.</p></Notice>
    {/if}

    <section class="group" aria-labelledby="staged-heading">
      <div class="group-head">
        <h3 id="staged-heading">Staged <span class="count">{status?.staged.length ?? 0}</span></h3>
        {#if status?.staged.length}
          <button class="link" onclick={() => run(() => api.gitUnstage(status!.staged.map((c) => c.path)))}>Unstage all</button>
        {/if}
      </div>
      {#if status?.staged.length}
        <ul class="changes">{#each status.staged as change (change.path)}{@render changeRow(change, true)}{/each}</ul>
      {:else}
        <p class="muted small empty">Stage changes below to include them in the next commit.</p>
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
        <ul class="changes">{#each status.unstaged as change (change.path)}{@render changeRow(change, false)}{/each}</ul>
      {:else if status}
        <p class="muted small empty">No unstaged changes. Config edits show up here, and packraven records mod changes in mods.lock.json.</p>
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
    {#if !selected}
      <div class="placeholder">
        <p>Select a file to see what changed.</p>
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
    gap: 8px;
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
    margin: 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--line);
  }

  .changes li {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--line);
  }

  .changes li:hover,
  .changes li.selected {
    background: var(--surface);
  }

  .changes li.selected {
    box-shadow: inset 3px 0 0 var(--forest);
  }

  .file-button {
    flex: 1;
    overflow: hidden;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 6px 8px;
    font-weight: 400;
    text-align: left;
    background: transparent;
    border: 0;
    border-radius: 0;
  }

  .file-button:hover:not(:disabled) {
    background: transparent;
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

  .changes li:hover .row-actions,
  .changes li:focus-within .row-actions,
  .changes li.selected .row-actions {
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
    place-items: center;
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
