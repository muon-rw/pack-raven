<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import { EditorView } from '@codemirror/view'
  import { openLintPanel } from '@codemirror/lint'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { revealItemInDir } from '@tauri-apps/plugin-opener'
  import { api, errorText } from '../lib/api'
  import { workspace, activeTab, open, pin, save, close, reload, resetFor, bindView, fs, useDiskVersion, keepMine, type Tab } from '../lib/workspace.svelte'
  import FileTree from '../components/FileTree.svelte'
  import Notice from '../components/Notice.svelte'

  let { pack }: { pack: string | null } = $props()

  let host: HTMLDivElement = $state()!
  let view: EditorView | null = null
  let tree: FileTree = $state()!
  let changes = $state(new Map<string, string>())
  let openError = $state<string | null>(null)

  $effect.pre(() => resetFor(pack))

  async function loadChanges() {
    try {
      const { status } = await api.gitOverview()
      const map = new Map<string, string>()
      for (const c of status.staged) map.set(c.path, c.status)
      for (const c of status.unstaged) map.set(c.path, c.status)
      changes = map
    } catch {
      changes = new Map()
    }
  }

  onMount(() => {
    loadChanges()
    const timer = setInterval(loadChanges, 5000)
    const pinActive = () => workspace.active && pin(workspace.active)
    host.addEventListener('mousedown', pinActive)
    return () => {
      clearInterval(timer)
      host.removeEventListener('mousedown', pinActive)
      const current = activeTab()
      if (view && current?.state) current.state = view.state
      bindView(null, null)
      view?.destroy()
      view = null
    }
  })

  $effect(() => {
    if (fs.seq) loadChanges()
  })

  $effect(() => {
    const path = workspace.active
    const current = untrack(() => workspace.tabs.find((t) => t.path === path))
    if (!host) return
    if (!current?.state) {
      bindView(null, null)
      view?.destroy()
      view = null
      return
    }
    const state = untrack(() => current.state!)
    if (view) view.setState(state)
    else view = new EditorView({ state, parent: host })
    const editor = view
    bindView(path, (spec) => editor.dispatch(spec))
    view.focus()
  })

  $effect(() => {
    const index = workspace.tabs.findIndex((t) => t.path === workspace.active)
    if (index >= 0) document.getElementById(`tab-${index}`)?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  })

  async function openFile(path: string, pinned: boolean) {
    const current = activeTab()
    if (view && current?.state) current.state = view.state
    try {
      openError = null
      await open(path, { pinned })
    } catch (e) {
      openError = errorText(e)
    }
  }

  function selectTab(tab: Tab) {
    const current = activeTab()
    if (view && current?.state) current.state = view.state
    workspace.active = tab.path
  }

  async function closeTab(tab: Tab) {
    if (tab.dirty) {
      const discard = await ask(`${name(tab.path)} has unsaved changes. Close it and lose them?`, {
        title: 'Unsaved changes',
        kind: 'warning',
        okLabel: 'Close without saving',
        cancelLabel: 'Keep editing',
      })
      if (!discard) return
    }
    close(tab.path)
  }

  async function reloadTab(tab: Tab) {
    if (tab.dirty) {
      const confirmed = await ask(`Reload ${name(tab.path)} from disk? You'll lose your unsaved changes.`, {
        title: 'Reload file',
        kind: 'warning',
        okLabel: 'Reload',
        cancelLabel: 'Cancel',
      })
      if (!confirmed) return
    }
    await reload(tab.path)
    if (workspace.active === tab.path && view) {
      const state = workspace.tabs.find((t) => t.path === tab.path)?.state
      if (state) view.setState(state)
    }
  }

  async function reveal(path: string) {
    await revealItemInDir(await api.absolutePath(path))
  }

  function name(path: string) {
    return path.slice(path.lastIndexOf('/') + 1)
  }

  function onKey(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey)) return
    const current = activeTab()
    if (event.key.toLowerCase() === 's' && current) {
      event.preventDefault()
      save(current.path).then(loadChanges)
    } else if (event.key.toLowerCase() === 'w' && current) {
      event.preventDefault()
      closeTab(current)
    }
  }

  // Most mice only scroll vertically, so we turn vertical wheel movement into horizontal
  // scrolling once the tabs overflow. Trackpads already scroll sideways and pass through.
  function scrollTabs(event: WheelEvent) {
    const bar = event.currentTarget as HTMLElement
    if (bar.scrollWidth <= bar.clientWidth || Math.abs(event.deltaX) >= Math.abs(event.deltaY)) return
    event.preventDefault()
    const lines = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? 40 : 1
    bar.scrollLeft += event.deltaY * lines
  }

  function onTabKey(event: KeyboardEvent, index: number) {
    const tabs = workspace.tabs
    const to = event.key === 'ArrowRight' ? index + 1 : event.key === 'ArrowLeft' ? index - 1 : null
    if (to === null || !tabs[to]) return
    event.preventDefault()
    selectTab(tabs[to])
    document.getElementById(`tab-${to}`)?.focus()
  }

  function plural(count: number, noun: string) {
    return `${count} ${noun}${count === 1 ? '' : 's'}`
  }

  function showProblems() {
    if (view) openLintPanel(view)
  }

  let current = $derived(workspace.tabs.find((t) => t.path === workspace.active))
  let problems = $derived.by(() => {
    if (!current?.state) return null
    const { errors, warnings } = current.problems
    const parts = [errors && plural(errors, 'error'), warnings && plural(warnings, 'warning')].filter(Boolean)
    return parts.length ? { text: parts.join(', '), tone: errors ? 'error' : 'warn' } : null
  })
</script>

<svelte:window onkeydown={onKey} />

<div class="files">
  <aside class="browser">
    <div class="browser-head">
      <h2>Files</h2>
      <button class="link" onclick={() => tree.refresh()}>Refresh</button>
    </div>
    <FileTree bind:this={tree} selected={workspace.active} {changes} onopen={openFile} />
  </aside>

  <section class="editor">
    {#if workspace.tabs.length}
      <div class="tabs" role="tablist" aria-label="Open files" onwheel={scrollTabs}>
        {#each workspace.tabs as tab, index (tab.path)}
          <div class="tab" class:active={tab.path === workspace.active} class:preview={tab.preview}>
            <button
              id="tab-{index}"
              role="tab"
              aria-selected={tab.path === workspace.active}
              tabindex={tab.path === workspace.active ? 0 : -1}
              title={tab.path}
              onclick={() => selectTab(tab)}
              ondblclick={() => pin(tab.path)}
              onkeydown={(e) => onTabKey(e, index)}
            >
<span class="name">{name(tab.path)}</span>{#if tab.dirty}<span class="dirty" aria-label="unsaved"></span>{/if}
            </button>
            <button class="close" aria-label="Close {name(tab.path)}" title="Close (Ctrl+W)" onclick={() => closeTab(tab)}>×</button>
          </div>
        {/each}
      </div>
    {/if}

    {#if current}
      <div class="toolbar">
        <span class="file path" title={current.path}>{current.path}</span>
        <span class="actions">
          {#if problems}
            <button class="link problems {problems.tone}" title="Show problems (Ctrl+Shift+M)" onclick={showProblems}>{problems.text}</button>
          {/if}
          {#if current.state}
            <span class="muted small">{current.crlf ? 'CRLF' : 'LF'}</span>
          {/if}
          <button class="link" onclick={() => reveal(current!.path)}>Show in folder</button>
          <button class="link" onclick={() => reloadTab(current!)}>Reload</button>
          {#if current.state}
            <button class="primary save" disabled={!current.dirty} onclick={() => save(current!.path).then(loadChanges)}>
              {current.dirty ? 'Save' : 'Saved'} <kbd>Ctrl S</kbd>
            </button>
          {/if}
        </span>
      </div>
    {/if}

    {#if current?.conflict === 'changed'}
      <div class="pad">
        <Notice tone="warn">
          <div class="conflict">
            <p>{name(current.path)} changed on disk while you were editing it.</p>
            <span>
              <button onclick={() => useDiskVersion(current!.path)}>Use the disk version</button>
              <button onclick={() => keepMine(current!.path)}>Keep mine</button>
            </span>
          </div>
        </Notice>
      </div>
    {:else if current?.conflict === 'deleted'}
      <div class="pad"><Notice tone="warn"><p>{name(current.path)} no longer exists on disk. Save to create it again.</p></Notice></div>
    {/if}
    {#if workspace.error}<div class="pad"><Notice tone="error"><p>{workspace.error}</p></Notice></div>{/if}
    {#if openError}<div class="pad"><Notice tone="error"><p>{openError}</p></Notice></div>{/if}

    <div class="host" bind:this={host} class:hidden={!current?.state}></div>

    {#if current && !current.state}
      <div class="placeholder"><p>{current.unreadable}</p></div>
    {:else if !current}
      <div class="placeholder">
        <p>Open a file from the tree to edit it.</p>
        <p class="muted small">Press Ctrl+S to save, Ctrl+F to search, or Ctrl+W to close the tab.</p>
      </div>
    {/if}
  </section>
</div>

<style>
  .files {
    display: grid;
    grid-template-columns: minmax(220px, 300px) minmax(0, 1fr);
    gap: 20px;
    height: 100%;
    min-height: 0;
  }

  .browser {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
    background: var(--field);
    overflow: hidden;
  }

  .browser-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 12px;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }

  .browser-head h2 {
    font-size: var(--step-0);
  }

  .link {
    padding: 2px 6px;
    font-weight: 400;
    font-size: var(--step--1);
    color: var(--forest);
    background: transparent;
    border-color: transparent;
  }

  .link:hover:not(:disabled) {
    background: var(--control);
  }

  .editor {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
    background: var(--field);
    overflow: hidden;
  }

  .tabs {
    display: flex;
    overflow-x: auto;
    background: var(--surface);
    border-bottom: 1px solid var(--line);
    scrollbar-width: thin;
  }

  /* A nowrap name's min-content is its full width, so the tab can't derive its minimum from
     its contents. We set it here instead: 5ch of name plus the button padding and close button,
     with the tab's font size set so ch matches the name. */
  .tab {
    display: flex;
    align-items: center;
    flex: 0 1 auto;
    min-width: calc(5ch + 40px);
    font-size: var(--step--1);
    border-right: 1px solid var(--line);
  }

  .tab:has(.dirty) {
    min-width: calc(5ch + 53px);
  }

  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tab.active {
    background: var(--field);
    box-shadow: inset 0 -2px 0 var(--forest);
  }

  .tab button {
    font-weight: 400;
    font-size: var(--step--1);
    background: transparent;
    border: 0;
    border-radius: 0;
  }

  .tab [role='tab'] {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 8px 4px 8px 12px;
    color: var(--muted);
  }

  .dirty,
  .close {
    flex: none;
  }

  .tab.preview .name {
    font-style: italic;
  }

  .tab.active [role='tab'] {
    color: var(--text);
    font-weight: 700;
  }

  .tab button:focus-visible {
    outline-offset: -2px;
  }

  .dirty {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ochre-ink);
  }

  .close {
    width: 24px;
    padding: 4px 10px 4px 4px;
    color: var(--muted);
  }

  .close:hover:not(:disabled) {
    color: var(--text);
    background: transparent;
  }

  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 6px 10px 6px 14px;
    border-bottom: 1px solid var(--line);
  }

  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
  }

  .problems {
    font-weight: 700;
  }

  .problems.error {
    color: var(--fail);
  }

  .problems.warn {
    color: var(--warn);
  }

  .save {
    padding: 4px 10px;
  }

  .save kbd {
    margin-left: 4px;
  }

  .small {
    font-size: var(--step--1);
  }

  .pad {
    padding: 10px;
  }

  .conflict {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 6px 16px;
  }

  .conflict span {
    display: flex;
    gap: 4px;
  }

  .host {
    flex: 1;
    min-height: 0;
  }

  .host.hidden {
    display: none;
  }

  .host :global(.cm-editor) {
    height: 100%;
  }

  .placeholder {
    flex: 1;
    display: grid;
    place-content: center;
    gap: 6px;
    text-align: center;
    color: var(--muted);
  }
</style>
