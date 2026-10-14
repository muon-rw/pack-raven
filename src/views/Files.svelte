<script lang="ts">
  import { onMount, tick, untrack } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import { EditorView } from '@codemirror/view'
  import { openLintPanel } from '@codemirror/lint'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { revealItemInDir } from '@tauri-apps/plugin-opener'
  import { api, errorText, type GitStatus } from '../lib/api'
  import { drag } from '../lib/drag.svelte'
  import { workspace, activeTab, open, pin, save, close, reload, resetFor, bindView, fs, useDiskVersion, keepMine, canReopen, reopenClosed, showSource, openSimplified, type Tab } from '../lib/workspace.svelte'
  import { commandKey, fileManager, isWindows, shortcut, trashName } from '../lib/platform'
  import { nav } from '../lib/nav.svelte'
  import FileTree from '../components/FileTree.svelte'
  import FileSearchResults from '../components/FileSearchResults.svelte'
  import ImageView from '../components/ImageView.svelte'
  import MediaView from '../components/MediaView.svelte'
  import NbtView from '../components/NbtView.svelte'
  import LogViewer from '../components/LogViewer.svelte'
  import MarkdownPreview from '../components/MarkdownPreview.svelte'
  import { SvelteMap } from 'svelte/reactivity'
  import { isGzip, outerArchive, viewerFor } from '../lib/viewers'
  import ContextMenu from '../components/ContextMenu.svelte'
  import type { MenuItem } from '../lib/menu'
  import Notice from '../components/Notice.svelte'

  let { pack }: { pack: string | null } = $props()

  let host: HTMLDivElement = $state()!
  let view: EditorView | null = null
  let tree: FileTree = $state()!
  let status = $state<GitStatus | null>(null)
  let openError = $state<string | null>(null)
  // A single click in the tree previews a file but leaves focus in the tree, as in VS Code, so
  // tree shortcuts like Ctrl+C keep working. Opening a file to keep moves focus to the editor.
  let focusEditor = true
  const selection = new SvelteSet<string>()
  let tabMenu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null)
  let query = $state('')
  let searchField: HTMLInputElement
  let results: FileSearchResults | undefined = $state()
  let activeResult = $state(0)
  let openedFromSearch: string | null = null
  let searching = $derived(!!query.trim())

  $effect.pre(() => resetFor(pack))

  async function loadChanges() {
    try {
      status = (await api.gitOverview()).status
    } catch {
      status = null
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

  // The Git view's "Open in Files" sends you here with a file to open and show in the tree.
  $effect(() => {
    const path = nav.filePath
    if (!path || !tree) return
    nav.filePath = null
    untrack(() => {
      query = ''
      tree.show(path).then(() => openFile(path, true))
    })
  })

  $effect(() => {
    const path = workspace.active
    void activeHasText
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
    else {
      view = new EditorView({ state, parent: host })
      view.scrollDOM.addEventListener('scroll', onEditorScroll)
    }
    const editor = view
    bindView(path, (spec) => editor.dispatch(spec))
    if (focusEditor) view.focus()
  })

  $effect(() => {
    const index = workspace.tabs.findIndex((t) => t.path === workspace.active)
    if (index >= 0) document.getElementById(`tab-${index}`)?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  })

  async function openFile(path: string, pinned: boolean) {
    const current = activeTab()
    if (view && current?.state) current.state = view.state
    focusEditor = pinned
    try {
      openError = null
      await open(path, { pinned })
    } catch (e) {
      openError = errorText(e)
    }
    await tick()
    if (pinned) view?.focus()
    focusEditor = true
  }

  function selectTab(tab: Tab) {
    const current = activeTab()
    if (view && current?.state) current.state = view.state
    workspace.active = tab.path
  }

  async function closeTabs(tabs: Tab[]) {
    const dirty = tabs.filter((t) => t.dirty)
    if (dirty.length) {
      const message =
        dirty.length === 1
          ? `${name(dirty[0].path)} has unsaved changes. Close it and lose them?`
          : `${dirty.length} files have unsaved changes. Close them and lose the changes?`
      const discard = await ask(message, {
        title: 'Unsaved changes',
        kind: 'warning',
        okLabel: 'Close without saving',
        cancelLabel: 'Keep editing',
      })
      if (!discard) return false
    }
    for (const tab of tabs) close(tab.path)
    return true
  }

  function closeTab(tab: Tab) {
    return closeTabs([tab])
  }

  // Closing a group of tabs from a tab's menu leaves you on the tab you right-clicked,
  // as in VS Code and JetBrains IDEs.
  async function closeAround(tab: Tab, which: (index: number, at: number) => boolean) {
    const at = workspace.tabs.indexOf(tab)
    const closing = workspace.tabs.filter((_, index) => index !== at && which(index, at))
    const activeClosing = closing.some((t) => t.path === workspace.active)
    if ((await closeTabs(closing)) && activeClosing) workspace.active = tab.path
  }

  function openTabMenu(x: number, y: number, tab: Tab | null) {
    const index = tab ? workspace.tabs.indexOf(tab) : -1
    const last = workspace.tabs.length - 1
    tabMenu = {
      x,
      y,
      items: [
        ...(tab
          ? [
              { label: 'Close', shortcut: shortcut('Ctrl+W'), run: () => closeTab(tab) },
              { label: 'Close others', disabled: last === 0, run: () => closeAround(tab, () => true) },
              { label: 'Close to the left', disabled: index === 0, run: () => closeAround(tab, (i, at) => i < at) },
              { label: 'Close to the right', disabled: index === last, run: () => closeAround(tab, (i, at) => i > at) },
            ]
          : []),
        { label: 'Close all', run: () => closeTabs([...workspace.tabs]) },
        'separator',
        { label: 'Reopen closed tab', shortcut: shortcut('Ctrl+Shift+T'), disabled: !canReopen(), run: reopenTab },
      ],
    }
  }

  function onTabContextMenu(event: MouseEvent, tab: Tab | null) {
    event.preventDefault()
    event.stopPropagation()
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
    if (!event.clientX && !event.clientY) openTabMenu(rect.left + 12, rect.bottom, tab)
    else openTabMenu(event.clientX, event.clientY, tab)
  }

  async function reopenTab() {
    const current = activeTab()
    if (view && current?.state) current.state = view.state
    try {
      openError = null
      await reopenClosed()
    } catch (e) {
      openError = errorText(e)
    }
  }

  async function remove(paths: string[]) {
    paths = tree.writable(paths)
    if (!paths.length) return
    const what = paths.length === 1 ? name(paths[0]) : `${paths.length} items`
    const confirmed = await ask(`Move ${what} to the ${trashName}?`, {
      title: 'Delete',
      kind: 'warning',
      okLabel: `Move to ${trashName}`,
      cancelLabel: 'Cancel',
    })
    if (!confirmed) return
    try {
      await api.trashPaths(paths)
    } catch (e) {
      tree.report(errorText(e))
      return
    }
    closeRemoved(paths)
    tree.forget(paths)
    tree.refresh()
  }

  // We keep tabs with unsaved changes open, so you can still save them somewhere.
  function closeRemoved(paths: string[]) {
    const removed = (path: string) => paths.some((p) => path === p || path.startsWith(`${p}/`))
    for (const tab of workspace.tabs.filter((t) => removed(t.path) && !t.dirty)) close(tab.path, { remember: false })
  }

  async function openPackFolder() {
    try {
      await api.openPackFolder()
    } catch (e) {
      tree.report(`Couldn't open the pack folder: ${errorText(e)}`)
    }
  }

  async function reloadTab(tab: Tab) {
    if (viewer === 'log' && tab.path === current?.path) {
      await logViewer?.reload()
      return
    }
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

  async function openWithDefaultApp(path: string) {
    try {
      openError = null
      await api.openWithDefaultApp(path)
    } catch (e) {
      openError = errorText(e)
    }
  }

  function name(path: string) {
    return path.slice(path.lastIndexOf('/') + 1)
  }

  function onKey(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey)) return
    const current = activeTab()
    const key = event.key.toLowerCase()
    if (key === 'v' && event.shiftKey && isMarkdown) {
      event.preventDefault()
      setMarkdownMode(MARKDOWN_MODES[(MARKDOWN_MODES.findIndex((m) => m.id === markdownMode) + 1) % MARKDOWN_MODES.length].id)
    } else if (key === 's' && current) {
      event.preventDefault()
      save(current.path).then(loadChanges)
    } else if (key === 'w' && current) {
      event.preventDefault()
      closeTab(current)
    } else if (key === 'p' && !event.shiftKey && commandKey(event)) {
      event.preventDefault()
      searchField.focus()
      searchField.select()
    } else if (key === 't' && event.shiftKey && commandKey(event)) {
      event.preventDefault()
      reopenTab()
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
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault()
      const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
      openTabMenu(rect.left + 12, rect.bottom, tabs[index])
      return
    }
    const to = event.key === 'ArrowRight' ? index + 1 : event.key === 'ArrowLeft' ? index - 1 : null
    if (to === null || !tabs[to]) return
    event.preventDefault()
    selectTab(tabs[to])
    document.getElementById(`tab-${to}`)?.focus()
  }

  function onSearchKey(event: KeyboardEvent) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      results?.move(event.key === 'ArrowDown' ? 1 : -1)
    } else if (event.key === 'Enter') {
      event.preventDefault()
      results?.choose(true)
    } else if (event.key === 'Escape') {
      event.preventDefault()
      endSearch()
    }
  }

  function openFromSearch(path: string, pinned: boolean) {
    openedFromSearch = path
    openFile(path, pinned)
  }

  function showFolder(path: string) {
    openedFromSearch = null
    query = ''
    tree.show(path, true)
  }

  // Leaving the search shows the file you last opened from it in the tree, so you can see
  // what sits next to it.
  function endSearch() {
    const path = openedFromSearch === workspace.active ? openedFromSearch : null
    openedFromSearch = null
    query = ''
    if (path) tree.show(path)
    else searchField.blur()
  }

  function plural(count: number, noun: string) {
    return `${count} ${noun}${count === 1 ? '' : 's'}`
  }

  function showProblems() {
    if (view) openLintPanel(view)
  }

  let current = $derived(workspace.tabs.find((t) => t.path === workspace.active))
  // A boolean, so the editor effect reruns when a tab gains text, not on every keystroke.
  let activeHasText = $derived(!!current?.state)
  let readOnly = $derived(!!current && (current.archived || current.compressed))
  let isMarkdown = $derived(!!current?.state && !current.large && /\.(md|markdown)$/i.test(current.path))
  let markdownMode = $derived.by((): MarkdownMode => {
    if (!isMarkdown || !current) return 'code'
    return tabModes.get(current.path) ?? (readOnly ? 'preview' : (rememberedMode ?? (editorWidth >= SPLIT_MIN_WIDTH ? 'split' : 'code')))
  })
  let viewer = $derived(current && !current.state ? viewerFor(current.path) : null)
  let viewerError = $state<{ path: string; message: string } | null>(null)
  let logViewer: LogViewer | undefined = $state()
  let failure = $derived(viewerError && viewerError.path === current?.path ? viewerError.message : null)
  const decompiling = new Set<string>()

  // We decompile a class as soon as you open it, since there's nothing else to show for it.
  $effect(() => {
    const path = current?.path
    if (!path || viewer !== 'class' || !current?.archived || failure || decompiling.has(path)) return
    decompiling.add(path)
    api
      .decompileClass(path)
      .then((source) => showSource(path, source))
      .catch((e) => (viewerError = { path, message: `We couldn't decompile this class. ${errorText(e)}` }))
      .finally(() => decompiling.delete(path))
  })

  let blockedNotice = $state(false)
  let blockedTimer = 0
  let blockedSeen = workspace.blocked

  $effect(() => {
    if (workspace.blocked === blockedSeen) return
    blockedSeen = workspace.blocked
    blockedNotice = true
    clearTimeout(blockedTimer)
    blockedTimer = setTimeout(() => (blockedNotice = false), 3000)
  })

  async function openLarge(path: string) {
    try {
      openError = null
      await openSimplified(path)
    } catch (e) {
      openError = errorText(e)
    }
  }

  // The linter is part of the lazily loaded editor bundle, so we load it only when you ask.
  async function checkNow() {
    const lint = await import('../lib/lint')
    if (view) lint.checkNow(view)
  }

  type MarkdownMode = 'code' | 'split' | 'preview'
  const MARKDOWN_MODES: { id: MarkdownMode; label: string }[] = [
    { id: 'code', label: 'Code' },
    { id: 'split', label: 'Split' },
    { id: 'preview', label: 'Preview' },
  ]
  const MARKDOWN_MODE_KEY = 'packraven.markdownMode'
  const SPLIT_MIN_WIDTH = 800
  const PREVIEW_DELAY_MS = 200

  let editorWidth = $state(0)
  let preview: MarkdownPreview | undefined = $state()
  let previewDoc = $state<{ path: string; text: string } | null>(null)
  let editorScrollIgnoredUntil = 0
  // Your choice carries to the next Markdown file you edit, and a tab keeps the mode you picked
  // for it. Files you can't edit open as a preview, since there's nothing to edit.
  let rememberedMode = $state<MarkdownMode | null>(readStoredMode())
  const tabModes = new SvelteMap<string, MarkdownMode>()

  function readStoredMode(): MarkdownMode | null {
    try {
      const stored = localStorage.getItem(MARKDOWN_MODE_KEY)
      return MARKDOWN_MODES.some((m) => m.id === stored) ? (stored as MarkdownMode) : null
    } catch {
      return null
    }
  }

  function setMarkdownMode(mode: MarkdownMode) {
    if (!current) return
    tabModes.set(current.path, mode)
    if (readOnly) return
    rememberedMode = mode
    try {
      localStorage.setItem(MARKDOWN_MODE_KEY, mode)
    } catch {
      // Without storage, the choice lasts until you close packraven.
    }
  }

  // Rendering on every keystroke would stall typing in a long file, so the preview waits for a
  // pause. Switching files shows the new one at once.
  $effect(() => {
    const state = current?.state
    const path = current?.path
    if (!isMarkdown || !state || !path || markdownMode === 'code') return
    const wait = untrack(() => previewDoc?.path === path) ? PREVIEW_DELAY_MS : 0
    const timer = setTimeout(() => (previewDoc = { path, text: state.doc.toString() }), wait)
    return () => clearTimeout(timer)
  })

  function onEditorScroll() {
    if (markdownMode !== 'split' || !view || !preview || performance.now() < editorScrollIgnoredUntil) return
    const top = view.scrollDOM.scrollTop
    const block = view.lineBlockAtHeight(top)
    const line = view.state.doc.lineAt(block.from).number - 1 + (block.height ? (top - block.top) / block.height : 0)
    preview.scrollToLine(line)
  }

  function scrollEditorToLine(line: number) {
    if (!view) return
    const doc = view.state.doc
    const number = Math.min(doc.lines, Math.max(1, Math.floor(line) + 1))
    const block = view.lineBlockAt(doc.line(number).from)
    editorScrollIgnoredUntil = performance.now() + 120
    view.scrollDOM.scrollTop = block.top + (line % 1) * block.height
  }

  function retryViewer() {
    viewerError = null
  }

  function showViewerFailure(path: string, message: string) {
    viewerError = { path, message }
  }
  let extension = $derived.by(() => {
    const name = current ? current.path.slice(current.path.lastIndexOf('/') + 1) : ''
    const dot = name.lastIndexOf('.')
    return dot > 0 ? name.slice(dot + 1) : ''
  })
  let defaultApp = $state<string | null>(null)

  // Only Windows tells us which app opens a file type, so elsewhere we show just the extension.
  $effect(() => {
    const wanted = current && !current.state && isWindows ? extension : ''
    defaultApp = null
    if (wanted) api.defaultApp(wanted).then((app) => extension === wanted && (defaultApp = app), () => {})
  })

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
      <span class="search">
        <svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4.25" /><path d="M10.25 10.25l3.5 3.5" /></svg>
        <input
          bind:this={searchField}
          bind:value={query}
          type="search"
          placeholder="Search names"
          aria-label="Search file and folder names"
          aria-controls="file-results"
          aria-activedescendant={searching ? `result-${activeResult}` : undefined}
          title="Search file and folder names ({shortcut('Ctrl+P')})"
          spellcheck="false"
          autocomplete="off"
          onkeydown={onSearchKey}
        />
      </span>
      <span class="tools">
        <button class="icon" aria-label="Refresh" title="Refresh" onclick={() => tree.refresh()}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M13.25 8a5.25 5.25 0 1 1-1.54-3.71" /><path d="M12.25 1.75v3h-3" /></svg>
        </button>
        <button
          class="icon"
          class:armed={drag.paths.length > 0}
          class:over={drag.overTrash}
          data-drop="trash"
          aria-label="Delete selected"
          aria-disabled={!selection.size}
          title={selection.size ? `Move the selection to the ${trashName}, or drop files here` : 'Select files to delete, or drag them here'}
          onclick={() => selection.size && remove(tree.selectedPaths())}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2.5 4h11M6 4V2.5h4V4M3.75 4l.75 9.5h7l.75-9.5M6.5 6.75v4.5M9.5 6.75v4.5" /></svg>
        </button>
        <button class="icon" aria-label="Open the pack folder in {fileManager}" title="Open the pack folder in {fileManager}" onclick={openPackFolder}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M1.75 3.25h4.5l1.5 1.75h6.5v7.75H1.75z" /></svg>
        </button>
      </span>
    </div>
    {#if searching}
      <FileSearchResults bind:this={results} bind:active={activeResult} {query} onopen={openFromSearch} onshowfolder={showFolder} />
    {/if}
    <FileTree bind:this={tree} hidden={searching} selected={workspace.active} {status} {selection} onopen={openFile} ondelete={remove} onremoved={closeRemoved} ongitchange={loadChanges} />
  </aside>

  <section class="editor" bind:clientWidth={editorWidth}>
    {#if workspace.tabs.length}
      <div class="tabs" role="tablist" aria-label="Open files" tabindex="-1" onwheel={scrollTabs} oncontextmenu={(e) => onTabContextMenu(e, null)}>
        {#each workspace.tabs as tab, index (tab.path)}
          <!-- svelte-ignore a11y_no_static_element_interactions (the tab button handles Ctrl+W and the menu key) -->
          <div
            class="tab"
            class:active={tab.path === workspace.active}
            class:preview={tab.preview}
            oncontextmenu={(e) => onTabContextMenu(e, tab)}
            onmousedown={(e) => e.button === 1 && e.preventDefault()}
            onauxclick={(e) => e.button === 1 && closeTab(tab)}
          >
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
          {#if isMarkdown}
            <span class="modes" role="group" aria-label="Markdown view">
              {#each MARKDOWN_MODES as mode (mode.id)}
                <button class="mode" aria-pressed={markdownMode === mode.id} title="{mode.label} ({shortcut('Ctrl+Shift+V')} switches)" onclick={() => setMarkdownMode(mode.id)}>{mode.label}</button>
              {/each}
            </span>
          {/if}
          {#if problems}
            <button class="link problems {problems.tone}" title="Show problems (Ctrl+Shift+M)" onclick={showProblems}>{problems.text}</button>
          {/if}
          {#if current.state && current.large}
            <span class="muted small" title="Checking and folding are off for this large file, so typing stays responsive.">Simplified editor</span>
          {:else if current.state && current.lintPaused}
            <span class="muted small" title="Checking this file took long enough to slow down typing, so we only check it when you ask.">
              Checking paused
              <button class="link" onclick={checkNow}>Check now</button>
            </span>
          {/if}
          {#if readOnly && viewer !== 'log'}<span class="read-only" class:flash={blockedNotice}>Read-only</span>{/if}
          <button class="link" onclick={() => reveal(outerArchive(current!.path))}>{current.archived ? 'Show the archive in folder' : 'Show in folder'}</button>
          <button class="link" onclick={() => reloadTab(current!)}>Reload</button>
          {#if current.state && !readOnly}
            <button class="primary save" disabled={!current.dirty} onclick={() => save(current!.path).then(loadChanges)}>
              {current.dirty ? 'Save' : 'Saved'} <kbd>Ctrl S</kbd>
            </button>
          {/if}
        </span>
        {#if readOnly && blockedNotice}
          <p class="blocked-note" role="status">
            {#if current.archived}
              This file is inside {name(outerArchive(current.path))}, so you can't change it here. Extract the archive to edit its files.
            {:else}
              We show this file decompressed, so you can't change it here. Extract it to edit a copy.
            {/if}
          </p>
        {/if}
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

    <div class="panes" class:hidden={!current?.state} class:split={markdownMode === 'split'}>
      <div class="host" bind:this={host} class:hidden={markdownMode === 'preview'}></div>
      {#if current && isMarkdown && markdownMode !== 'code' && previewDoc?.path === current.path}
        <MarkdownPreview
          bind:this={preview}
          source={previewDoc.text}
          path={current.path}
          onopenpath={(path) => openFile(path, true)}
          onscrollline={markdownMode === 'split' ? scrollEditorToLine : undefined}
        />
      {/if}
    </div>

    {#if current && viewer === 'log'}
      {#key current.path}<LogViewer bind:this={logViewer} path={current.path} />{/key}
    {:else if current && !current.state && !failure && viewer === 'image'}
      <ImageView path={current.path} archived={current.archived} />
    {:else if current && !current.state && !failure && (viewer === 'video' || viewer === 'audio')}
      <MediaView path={current.path} archived={current.archived} kind={viewer} />
    {:else if current && !current.state && !failure && viewer === 'nbt'}
      <NbtView path={current.path} onfail={(message) => showViewerFailure(current!.path, `We couldn't read this as NBT. ${message}`)} />
    {:else if current && !current.state && !failure && viewer === 'class' && current.archived}
      <div class="placeholder"><p>Decompiling with Vineflower…</p></div>
    {:else if current && !current.state}
      <div class="placeholder">
        <p>{failure ?? current.unreadable}</p>
        <p class="buttons">
          {#if failure}<button onclick={retryViewer}>Try again</button>{/if}
          {#if current.large}<button class="primary" onclick={() => openLarge(current!.path)}>Open in simplified editor</button>{/if}
          {#if (viewer === 'archive' || viewer === 'compressed') && !current.archived}<button onclick={() => tree.extract(current!.path)}>Extract here</button>{/if}
          {#if !current.archived}<button onclick={() => openWithDefaultApp(current!.path)}>Open in default app</button>{/if}
        </p>
        {#if viewer === 'archive'}<p class="muted small">Expand it in the file tree to browse what's inside.</p>{/if}
        {#if !current.archived}
        <p class="muted small">
          {#if !extension}
            This file has no extension{isWindows ? ', so Windows will ask which app to use' : ''}.
          {:else if defaultApp}
            <span class="file">.{extension}</span> files open in {defaultApp}.
          {:else if isWindows}
            No app is set for <span class="file">.{extension}</span> files, so Windows will ask which one to use.
          {:else}
            <span class="file">.{extension}</span> file
          {/if}
        </p>
        {/if}
      </div>
    {:else if !current}
      <div class="placeholder">
        <p>Open a file from the tree to edit it.</p>
        <p class="muted small">Press Ctrl+S to save, Ctrl+F to search, or Ctrl+W to close the tab.</p>
      </div>
    {/if}
  </section>
</div>

{#if tabMenu}
  <ContextMenu x={tabMenu.x} y={tabMenu.y} label="Tab actions" items={tabMenu.items} onclose={() => (tabMenu = null)} />
{/if}

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
    gap: 10px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }

  .browser-head h2 {
    flex: none;
    font-size: var(--step-0);
  }

  .search {
    position: relative;
    flex: 1;
    min-width: 0;
    margin: -4px 0;
  }

  .search svg {
    position: absolute;
    left: 7px;
    top: 50%;
    width: 13px;
    height: 13px;
    transform: translateY(-50%);
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.5;
    stroke-linecap: round;
    pointer-events: none;
  }

  .search input {
    width: 100%;
    padding: 3px 6px 3px 25px;
    font-size: var(--step--1);
  }

  .tools {
    display: flex;
    gap: 2px;
    margin: -4px -6px -4px 0;
  }

  .icon {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    color: var(--muted);
    background: transparent;
    border-color: transparent;
  }

  .icon:hover:not([aria-disabled='true']) {
    color: var(--text);
    background: var(--control);
  }

  .icon[aria-disabled='true'] {
    cursor: default;
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

  .icon.armed {
    color: var(--fail);
    opacity: 1;
    border-color: color-mix(in srgb, var(--fail) 45%, transparent);
    border-style: dashed;
  }

  .icon.over {
    color: var(--fail);
    background: var(--fail-fill);
    border-color: var(--fail-border);
    border-style: solid;
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
    position: relative;
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

  .panes {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .panes.hidden {
    display: none;
  }

  .host {
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .panes.split > .host {
    border-right: 1px solid var(--line);
  }

  .modes {
    display: flex;
    padding: 2px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
  }

  .mode {
    padding: 1px 10px;
    font-weight: 400;
    font-size: var(--step--1);
    color: var(--muted);
    background: transparent;
    border-color: transparent;
  }

  .mode[aria-pressed='true'] {
    color: var(--text);
    background: var(--field);
    border-color: var(--line-strong);
  }

  .host.hidden {
    display: none;
  }

  .host :global(.cm-editor) {
    height: 100%;
  }

  .read-only {
    padding: 1px 8px;
    font-size: var(--step--1);
    color: var(--ochre-ink);
    border: 1px solid transparent;
    border-radius: var(--radius-control);
  }

  .read-only.flash {
    background: var(--warn-fill);
    border-color: color-mix(in srgb, var(--warn) 45%, transparent);
    animation: nudge 0.3s ease;
  }

  @keyframes nudge {
    25% {
      transform: translateX(-3px);
    }
    75% {
      transform: translateX(3px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .read-only.flash {
      animation: none;
    }
  }

  .blocked-note {
    position: absolute;
    z-index: 5;
    top: calc(100% + 6px);
    right: 10px;
    max-width: min(420px, 90%);
    padding: 6px 10px;
    font-size: var(--step--1);
    background: var(--surface);
    border: 1px solid color-mix(in srgb, var(--warn) 45%, transparent);
    border-radius: var(--radius-control);
    box-shadow: 0 6px 16px var(--shadow);
  }

  .buttons {
    display: flex;
    justify-content: center;
    gap: 6px;
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
