<script lang="ts">
  import { SvelteMap, SvelteSet } from 'svelte/reactivity'
  import { onMount, tick, untrack } from 'svelte'
  import { revealItemInDir } from '@tauri-apps/plugin-opener'
  import { getCurrentWebview, type DragDropEvent } from '@tauri-apps/api/webview'
  import { api, errorText, type DirEntry, type GitStatus, type Pasted } from '../lib/api'
  import { fs, moved } from '../lib/workspace.svelte'
  import { drag } from '../lib/drag.svelte'
  import { gitMenu } from '../lib/gitActions'
  import { commandKey, fileManager, isMac, shortcut } from '../lib/platform'
  import ContextMenu from './ContextMenu.svelte'
  import type { MenuItem } from '../lib/menu'

  let {
    selected,
    status,
    selection,
    onopen,
    ondelete,
    ongitchange,
  }: {
    selected: string | null
    status: GitStatus | null
    selection: SvelteSet<string>
    onopen: (path: string, pinned: boolean) => void
    ondelete: (paths: string[]) => void
    ongitchange: () => void
  } = $props()

  const DRAG_THRESHOLD = 5
  const EXPAND_DELAY = 600
  const SCROLL_EDGE = 28
  const SLOW_IMPORT = 400

  const children = new SvelteMap<string, DirEntry[]>()
  const expanded = new SvelteSet<string>()
  const loading = new SvelteSet<string>()
  let focused = $state<string | null>(null)
  let anchor: string | null = null
  let loadError = $state<string | null>(null)
  let message = $state<{ text: string; tone: 'fail' | 'note' } | null>(null)
  let edit = $state<{ kind: 'file' | 'folder' | 'rename'; parent: string; path: string | null; error: string | null; busy: boolean } | null>(null)
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null)
  let cutPaths = $state<string[]>([])
  let tree: HTMLDivElement

  let press: { x: number; y: number; row: Row; pointerId: number; target: HTMLElement } | null = null
  let ghost = $state<{ x: number; y: number } | null>(null)
  let dropDir = $state<string | null>(null)
  let hover: { path: string; timer: number } | null = null
  let pointerX = 0
  let pointerY = 0
  let systemDrag = false
  let suppressClick = false

  type Row = { entry: DirEntry; depth: number; parent: string }
  type DraftLine = { draft: true; depth: number }

  async function load(dir: string) {
    loading.add(dir)
    try {
      const entries = await api.listDir(dir)
      children.set(dir, entries)
      const present = new Set(entries.map((e) => e.path))
      for (const path of selection) if (parentOf(path) === dir && !present.has(path)) selection.delete(path)
      loadError = null
    } catch (e) {
      loadError = errorText(e)
    } finally {
      loading.delete(dir)
    }
  }

  export function refresh() {
    for (const dir of ['', ...expanded]) load(dir)
  }

  export function report(text: string, tone: 'fail' | 'note' = 'fail') {
    message = { text, tone }
  }

  export function forget(paths: string[]) {
    const gone = (path: string) => paths.some((p) => path === p || path.startsWith(`${p}/`))
    for (const path of [...expanded]) if (gone(path)) expanded.delete(path)
    for (const dir of [...children.keys()]) if (gone(dir)) children.delete(dir)
    for (const path of [...selection]) if (gone(path)) selection.delete(path)
    cutPaths = cutPaths.filter((p) => !gone(p))
  }

  // Selected descendants of a selected folder go with the folder, so we leave them out.
  // Selected rows inside a collapsed folder aren't visible, so we leave those out too.
  export function selectedPaths() {
    const visible = rows.map((r) => r.entry.path).filter((p) => selection.has(p))
    return visible.filter((p) => !visible.some((other) => p.startsWith(`${other}/`)))
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

  // Like VS Code, opening a file from a tab or elsewhere selects it in the tree.
  $effect(() => {
    const path = selected
    untrack(() => path && !selection.has(path) && selectOnly(path))
  })

  let lines = $derived.by(() => {
    const out: (Row | DraftLine)[] = []
    const creating = edit && edit.kind !== 'rename' ? edit.parent : null
    const walk = (dir: string, depth: number) => {
      if (creating === dir) out.push({ draft: true, depth })
      for (const entry of children.get(dir) ?? []) {
        out.push({ entry, depth, parent: dir })
        if (entry.isDir && expanded.has(entry.path)) walk(entry.path, depth + 1)
      }
    }
    walk('', 0)
    return out
  })

  let rows = $derived(lines.filter((line): line is Row => !('draft' in line)))

  let changes = $derived.by(() => {
    const map = new Map<string, string>()
    for (const c of status?.staged ?? []) map.set(c.path, c.status)
    for (const c of status?.unstaged ?? []) map.set(c.path, c.status)
    return map
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

  function parentOf(path: string) {
    const index = path.lastIndexOf('/')
    return index < 0 ? '' : path.slice(0, index)
  }

  function nameOf(path: string) {
    return path.slice(path.lastIndexOf('/') + 1)
  }

  function targetDir(row: Row | null) {
    if (!row) return ''
    return row.entry.isDir ? row.entry.path : row.parent
  }

  function toggle(entry: DirEntry, open = !expanded.has(entry.path)) {
    if (!entry.isDir) return
    if (open) expandDir(entry.path)
    else expanded.delete(entry.path)
  }

  function expandDir(dir: string) {
    if (!dir) return
    expanded.add(dir)
    if (!children.has(dir)) return load(dir)
  }

  async function revealInTree(path: string) {
    const parts = path.split('/')
    for (let i = 1; i < parts.length; i++) await expandDir(parts.slice(0, i).join('/'))
  }

  function activate(row: Row) {
    focused = row.entry.path
    if (row.entry.isDir) toggle(row.entry)
    else onopen(row.entry.path, false)
  }

  function selectOnly(path: string) {
    selection.clear()
    selection.add(path)
    anchor = path
  }

  function selectRange(to: string) {
    const paths = rows.map((r) => r.entry.path)
    const end = paths.indexOf(to)
    const start = anchor && paths.includes(anchor) ? paths.indexOf(anchor) : end
    selection.clear()
    for (const path of paths.slice(Math.min(start, end), Math.max(start, end) + 1)) selection.add(path)
  }

  function fromNameField(event: Event) {
    return event.target instanceof HTMLInputElement
  }

  function onRowClick(event: MouseEvent, row: Row) {
    if (suppressClick || fromNameField(event)) return
    const path = row.entry.path
    focused = path
    if (event.shiftKey) {
      selectRange(path)
    } else if (commandKey(event)) {
      if (selection.has(path)) selection.delete(path)
      else selection.add(path)
      anchor = path
    } else {
      selectOnly(path)
      activate(row)
    }
  }

  function onRowDoubleClick(event: MouseEvent, row: Row) {
    if (suppressClick || fromNameField(event) || row.entry.isDir || event.shiftKey || commandKey(event)) return
    onopen(row.entry.path, true)
  }

  async function focusRow(path: string) {
    focused = path
    await tick()
    tree.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)?.focus()
  }

  function onKey(event: KeyboardEvent) {
    if (fromNameField(event)) return
    const index = rows.findIndex((r) => r.entry.path === focused)
    const row = rows[index]
    if (!row) return
    const move = (to: number) => {
      const target = rows[to]?.entry.path
      if (!target) return
      if (event.shiftKey) selectRange(target)
      else selectOnly(target)
      focusRow(target)
    }
    const key = event.key.toLowerCase()
    if (commandKey(event) && key === 'a') {
      for (const r of rows) selection.add(r.entry.path)
    } else if (commandKey(event) && (key === 'c' || key === 'x')) {
      copy(selectedPaths(), key === 'x')
    } else if (commandKey(event) && key === 'v') {
      paste(targetDir(row))
    } else if (event.key === 'F2') {
      startRename(row.entry.path)
    } else if (event.key === 'Delete' || (isMac && event.metaKey && event.key === 'Backspace')) {
      ondelete(selectedPaths())
    } else if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      const rect = (event.target as HTMLElement).getBoundingClientRect()
      openMenu(rect.left + 24, rect.bottom, row)
    } else if (event.key === 'ArrowDown') {
      move(index + 1)
    } else if (event.key === 'ArrowUp') {
      move(index - 1)
    } else if (event.key === 'Home') {
      move(0)
    } else if (event.key === 'End') {
      move(rows.length - 1)
    } else if (event.key === 'ArrowRight') {
      if (row.entry.isDir && !expanded.has(row.entry.path)) toggle(row.entry, true)
      else if (row.entry.isDir) move(index + 1)
    } else if (event.key === 'ArrowLeft') {
      if (row.entry.isDir && expanded.has(row.entry.path)) toggle(row.entry, false)
      else if (row.parent) move(rows.findIndex((r) => r.entry.path === row.parent))
    } else if (event.key === 'Enter' || event.key === ' ') {
      selectOnly(row.entry.path)
      activate(row)
    } else {
      return
    }
    event.preventDefault()
  }

  function onContextMenu(event: MouseEvent, row: Row | null) {
    if (fromNameField(event)) return
    event.preventDefault()
    event.stopPropagation()
    if (row && !selection.has(row.entry.path)) selectOnly(row.entry.path)
    if (row) focused = row.entry.path
    // The keyboard menu key fires contextmenu without pointer coordinates.
    if (row && !event.clientX && !event.clientY) {
      const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
      openMenu(rect.left + 24, rect.bottom, row)
    } else {
      openMenu(event.clientX, event.clientY, row)
    }
  }

  function openMenu(x: number, y: number, row: Row | null) {
    const dir = targetDir(row)
    const paths = row ? selectedPaths() : []
    const count = paths.length > 1 ? ` ${paths.length} items` : ''
    const git = gitMenu(paths, status, (path) => rows.find((r) => r.entry.path === path)?.entry.ignored ?? false, {
      ondone: ongitchange,
      onerror: (text) => report(text),
      onnotice: (text) => report(text, 'note'),
    })
    menu = {
      x,
      y,
      items: [
        { label: 'New file', run: () => startCreate(dir, 'file') },
        { label: 'New folder', run: () => startCreate(dir, 'folder') },
        'separator',
        ...(paths.length
          ? [
              { label: `Cut${count}`, shortcut: shortcut('Ctrl+X'), run: () => copy(paths, true) },
              { label: `Copy${count}`, shortcut: shortcut('Ctrl+C'), run: () => copy(paths) },
            ]
          : []),
        { label: 'Paste', shortcut: shortcut('Ctrl+V'), run: () => paste(dir) },
        ...(paths.length === 1 ? [{ label: 'Rename', shortcut: 'F2', run: () => startRename(paths[0]) }] : []),
        'separator',
        ...(git ? [git] : []),
        ...(paths.length === 1 ? [{ label: `Show in ${fileManager}`, run: () => reveal(paths[0]) }] : []),
        'separator',
        ...(paths.length ? [{ label: `Delete${count}`, shortcut: shortcut('Delete'), danger: true, run: () => ondelete(paths) }] : []),
      ],
    }
  }

  async function startCreate(parent: string, kind: 'file' | 'folder') {
    message = null
    await expandDir(parent)
    edit = { kind, parent, path: null, error: null, busy: false }
  }

  function startRename(path: string) {
    message = null
    edit = { kind: 'rename', parent: parentOf(path), path, error: null, busy: false }
  }

  function nameField(input: HTMLInputElement) {
    if (edit?.kind === 'rename' && edit.path) {
      const name = nameOf(edit.path)
      input.value = name
      const isDir = rows.find((r) => r.entry.path === edit!.path)?.entry.isDir
      const dot = name.lastIndexOf('.')
      input.setSelectionRange(0, isDir || dot <= 0 ? name.length : dot)
    }
    input.focus()
  }

  async function commitEdit(input: HTMLInputElement, fromBlur: boolean) {
    if (!edit || edit.busy) return
    const name = input.value.trim()
    const unchanged = edit.kind === 'rename' && edit.path && name === nameOf(edit.path)
    if (!name || unchanged || (fromBlur && edit.error)) {
      edit = null
      return
    }
    const { kind, parent, path: from } = edit
    const invalid = name.includes('\\') || (kind === 'rename' && name.includes('/'))
    const path = parent ? `${parent}/${name}` : name
    edit.busy = true
    try {
      if (invalid) throw kind === 'rename' ? "Names can't contain / or \\." : "Use / to separate folders, not \\."
      if (kind === 'rename') await api.renamePath(from!, path)
      else await (kind === 'file' ? api.createFile(path) : api.createDir(path))
    } catch (e) {
      if (fromBlur) {
        edit = null
        report(errorText(e))
      } else {
        edit.error = errorText(e)
        edit.busy = false
        await tick()
        input.focus()
      }
      return
    }
    edit = null
    if (kind === 'rename') {
      await afterMove([[from!, path]])
    } else {
      await load(parent)
      await revealInTree(path)
    }
    selectOnly(path)
    if (kind === 'file') onopen(path, true)
    else focusRow(path)
  }

  function onNameKey(event: KeyboardEvent) {
    const input = event.currentTarget as HTMLInputElement
    if (event.key === 'Enter') {
      event.preventDefault()
      commitEdit(input, false)
    } else if (event.key === 'Escape') {
      event.preventDefault()
      edit = null
      if (focused) focusRow(focused)
    } else if (edit?.error) {
      edit.error = null
    }
  }

  // A moved folder takes its expanded subfolders with it, so we carry them over to the new
  // paths and reload everything that's open.
  async function afterMove(pairs: [string, string][]) {
    await moved(pairs)
    for (const path of [...expanded]) {
      for (const [from, to] of pairs) {
        if (path === from || path.startsWith(`${from}/`)) {
          expanded.delete(path)
          expanded.add(to + path.slice(from.length))
        }
      }
    }
    for (const dir of [...children.keys()]) if (pairs.some(([from]) => dir === from || dir.startsWith(`${from}/`))) children.delete(dir)
    refresh()
  }

  async function moveInto(paths: string[], dir: string) {
    message = null
    try {
      const pairs = await api.movePaths(paths, dir)
      await expandDir(dir)
      await afterMove(pairs)
      selection.clear()
      for (const [, to] of pairs) selection.add(to)
      anchor = pairs[0]?.[1] ?? null
    } catch (e) {
      report(errorText(e))
    }
  }

  async function copy(paths: string[], cut = false) {
    if (!paths.length) return
    message = null
    try {
      await api.copyFiles(paths, cut)
      cutPaths = cut ? paths : []
    } catch (e) {
      report(errorText(e))
    }
  }

  async function paste(dir: string) {
    message = null
    try {
      const pasted = await api.pasteFiles(dir, cutPaths)
      cutPaths = []
      await showPasted(dir, pasted)
    } catch (e) {
      report(errorText(e))
    }
  }

  async function showPasted(dir: string, { created, moved }: Pasted) {
    await expandDir(dir)
    if (moved.length) await afterMove(moved)
    else await load(dir)
    selection.clear()
    for (const path of created) selection.add(path)
    anchor = created[0] ?? null
    if (created[0]) focusRow(created[0])
  }

  async function importInto(dir: string, sources: string[]) {
    message = null
    const name = sources[0].split(/[\\/]/).pop()
    const slow = setTimeout(() => report(sources.length === 1 ? `Adding ${name}…` : `Adding ${sources.length} items…`, 'note'), SLOW_IMPORT)
    try {
      await showPasted(dir, await api.importPaths(dir, sources))
      message = null
    } catch (e) {
      report(errorText(e))
    } finally {
      clearTimeout(slow)
    }
  }

  // Tauri reports drags from Explorer or Finder in physical pixels, so we convert them to CSS
  // pixels before finding the folder under the pointer.
  function onSystemDrag({ payload }: { payload: DragDropEvent }) {
    if (payload.type === 'leave') {
      endSystemDrag()
      return
    }
    locate(payload.position.x / devicePixelRatio, payload.position.y / devicePixelRatio, false)
    if (payload.type === 'drop') {
      const dir = dropDir
      endSystemDrag()
      if (dir !== null && payload.paths.length) importInto(dir, payload.paths)
    } else if (!systemDrag) {
      systemDrag = true
      requestAnimationFrame(autoScroll)
    }
  }

  function endSystemDrag() {
    systemDrag = false
    dropDir = null
    expandAfterHover(null)
  }

  onMount(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(onSystemDrag)
    return () => {
      unlisten.then((stop) => stop())
    }
  })

  async function reveal(path: string) {
    try {
      await revealItemInDir(await api.absolutePath(path))
    } catch (e) {
      report(errorText(e))
    }
  }

  // We drag with pointer events rather than HTML drag and drop, because Tauri's native file
  // drop handling disables HTML drag and drop in WebView2.
  function onPointerDown(event: PointerEvent, row: Row) {
    if (event.button !== 0 || fromNameField(event)) return
    press = { x: event.clientX, y: event.clientY, row, pointerId: event.pointerId, target: event.currentTarget as HTMLElement }
  }

  function onPointerMove(event: PointerEvent) {
    if (!press) return
    if (!drag.paths.length) {
      if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < DRAG_THRESHOLD) return
      startDrag(press)
    }
    if (event.clientX < 0 || event.clientY < 0 || event.clientX >= innerWidth || event.clientY >= innerHeight) dragOut()
    else trackDrag(event)
  }

  // Once you drag past the window's edge, we hand the drag to the OS, so you can drop the files
  // into Explorer, Finder, Discord or anywhere else that takes files. The drop target decides
  // whether to move or copy them, so we reload the tree afterwards.
  function dragOut() {
    const paths = drag.paths
    press = null
    endDrag()
    api
      .dragOut(paths)
      .then(refresh)
      .catch((e) => report(errorText(e)))
  }

  function startDrag({ row, target, pointerId }: NonNullable<typeof press>) {
    if (!selection.has(row.entry.path)) selectOnly(row.entry.path)
    drag.paths = selectedPaths()
    target.setPointerCapture(pointerId)
    requestAnimationFrame(autoScroll)
  }

  function canMoveInto(dir: string) {
    return drag.paths.every((p) => dir !== p && !dir.startsWith(`${p}/`)) && drag.paths.some((p) => parentOf(p) !== dir)
  }

  function trackDrag(event: PointerEvent) {
    ghost = { x: event.clientX, y: event.clientY }
    locate(event.clientX, event.clientY, true)
  }

  // Files dragged in from the OS can't go in the trash, and the backend checks whether they
  // can go where you drop them.
  function locate(x: number, y: number, internal: boolean) {
    pointerX = x
    pointerY = y
    const under = document.elementFromPoint(x, y)
    drag.overTrash = internal && !!under?.closest('[data-drop="trash"]')
    const inTree = !drag.overTrash && !!under && tree.contains(under)
    const path = inTree ? under!.closest<HTMLElement>('[data-path]')?.dataset.path : undefined
    const row = path ? (rows.find((r) => r.entry.path === path) ?? null) : null
    const dir = inTree ? targetDir(row) : null
    dropDir = dir !== null && (!internal || canMoveInto(dir)) ? dir : null
    expandAfterHover(row?.entry.isDir && !expanded.has(row.entry.path) ? row.entry.path : null)
  }

  function expandAfterHover(path: string | null) {
    if (hover?.path === path) return
    if (hover) clearTimeout(hover.timer)
    hover = path ? { path, timer: setTimeout(() => expandDir(path), EXPAND_DELAY) } : null
  }

  function autoScroll() {
    if (!drag.paths.length && !systemDrag) return
    const rect = tree.getBoundingClientRect()
    const overColumn = pointerX >= rect.left && pointerX <= rect.right
    if (overColumn && pointerY < rect.top + SCROLL_EDGE) tree.scrollTop -= 8
    else if (overColumn && pointerY > rect.bottom - SCROLL_EDGE) tree.scrollTop += 8
    requestAnimationFrame(autoScroll)
  }

  function endDrag() {
    drag.paths = []
    drag.overTrash = false
    dropDir = null
    ghost = null
    expandAfterHover(null)
  }

  function onPointerUp() {
    press = null
    if (!drag.paths.length) return
    const paths = drag.paths
    const overTrash = drag.overTrash
    const dir = dropDir
    endDrag()
    suppressClick = true
    setTimeout(() => (suppressClick = false))
    if (overTrash) ondelete(paths)
    else if (dir !== null) moveInto(paths, dir)
  }

  function cancelDrag() {
    press = null
    if (drag.paths.length) endDrag()
  }

  function badge(path: string) {
    const status = changes.get(path)
    return status === '?' ? 'N' : status
  }
</script>

<svelte:window
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={cancelDrag}
  onkeydown={(e) => e.key === 'Escape' && drag.paths.length && (e.preventDefault(), cancelDrag())}
/>

{#snippet nameInput()}
  <input
    use:nameField
    class="name-field"
    aria-label={edit?.kind === 'rename' ? 'New name' : `New ${edit?.kind} name`}
    aria-invalid={!!edit?.error}
    placeholder={edit?.kind === 'file' ? 'name.json' : edit?.kind === 'folder' ? 'folder name' : undefined}
    spellcheck="false"
    disabled={edit?.busy}
    onkeydown={onNameKey}
    onblur={(e) => commitEdit(e.currentTarget, true)}
  />
{/snippet}

<div
  class="tree"
  class:dragging={!!ghost}
  class:drop-root={dropDir === ''}
  role="tree"
  aria-label="Pack files"
  aria-multiselectable="true"
  bind:this={tree}
  tabindex="-1"
  onkeydown={onKey}
  oncontextmenu={(e) => onContextMenu(e, null)}
>
  {#if loadError}<p class="failed small">{loadError}</p>{/if}
  {#if message}
    <p class="message" class:failed={message.tone === 'fail'} class:muted={message.tone === 'note'}>
      <span>{message.text}</span>
      <button class="dismiss" aria-label="Dismiss" onclick={() => (message = null)}>×</button>
    </p>
  {/if}
  {#each lines as line ('draft' in line ? '\0draft' : line.entry.path)}
    {#if 'draft' in line}
      <div class="row editing" style="--depth: {line.depth}">
        <span class="twisty" aria-hidden="true"></span>
        {@render nameInput()}
      </div>
      {#if edit?.error}<p class="failed small edit-error" style="--depth: {line.depth}">{edit.error}</p>{/if}
    {:else}
      {@const row = line}
      {@const status = row.entry.isDir ? null : badge(row.entry.path)}
      {@const renaming = edit?.kind === 'rename' && edit.path === row.entry.path}
      <!-- svelte-ignore a11y_click_events_have_key_events (the tree container handles keys) -->
      <div
        role="treeitem"
        aria-level={row.depth + 1}
        aria-expanded={row.entry.isDir ? expanded.has(row.entry.path) : undefined}
        aria-selected={selection.has(row.entry.path)}
        tabindex={row.entry.path === (focused ?? rows[0]?.entry.path) ? 0 : -1}
        data-path={row.entry.path}
        class="row"
        class:selected={selection.has(row.entry.path)}
        class:ignored={row.entry.ignored}
        class:cut={cutPaths.includes(row.entry.path)}
        class:editing={renaming}
        class:drop={dropDir !== null && dropDir === row.entry.path}
        style="--depth: {row.depth}"
        onclick={(e) => onRowClick(e, row)}
        ondblclick={(e) => onRowDoubleClick(e, row)}
        oncontextmenu={(e) => onContextMenu(e, row)}
        onpointerdown={(e) => onPointerDown(e, row)}
        onlostpointercapture={cancelDrag}
        onfocus={() => (focused = row.entry.path)}
      >
        <span class="twisty" aria-hidden="true">
          {#if row.entry.isDir}
            <svg viewBox="0 0 12 12" class:open={expanded.has(row.entry.path)}><path d="M4.5 3l3 3-3 3" /></svg>
          {/if}
        </span>
        {#if renaming}
          {@render nameInput()}
        {:else}
          <span class="name" class:dir={row.entry.isDir}>{row.entry.name}</span>
        {/if}
        {#if loading.has(row.entry.path)}<span class="small muted">loading</span>{/if}
        {#if status}
          <span class="status s{status}" title="Changed in git">{status}</span>
        {:else if row.entry.isDir && changedDirs.has(row.entry.path)}
          <span class="dot" title="Contains changes"></span>
        {/if}
      </div>
      {#if renaming && edit?.error}<p class="failed small edit-error" style="--depth: {row.depth}">{edit.error}</p>{/if}
    {/if}
  {/each}
</div>

{#if ghost}
  <div class="ghost" style:left="{ghost.x + 14}px" style:top="{ghost.y + 10}px" aria-hidden="true">
    {drag.paths.length === 1 ? nameOf(drag.paths[0]) : `${drag.paths.length} items`}
  </div>
{/if}

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} label="File actions" items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .tree {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 4px 0;
    font-size: var(--step--1);
    outline: none;
  }

  .tree.drop-root {
    box-shadow: inset 0 0 0 2px var(--forest);
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

  .row.drop {
    background: var(--ok-fill);
    box-shadow: inset 0 0 0 1px var(--ok-border);
  }

  .tree.dragging .row {
    cursor: grabbing;
  }

  .row.cut .name {
    opacity: 0.55;
  }

  .row.ignored .name {
    color: var(--muted);
    opacity: 0.75;
  }

  .row.editing {
    padding-block: 1px;
    cursor: default;
  }

  .name-field {
    flex: 1;
    min-width: 0;
    padding: 1px 6px;
    font-size: var(--step--1);
    border-color: var(--forest);
  }

  .name-field[aria-invalid='true'] {
    border-color: var(--fail);
  }

  .edit-error {
    padding: 2px 10px 4px calc(26px + var(--depth) * 16px);
    white-space: normal;
  }

  .message {
    display: flex;
    align-items: start;
    gap: 6px;
    margin: 2px 6px 6px;
    padding: 4px 4px 4px 8px;
    font-size: var(--step--2);
    border-radius: var(--radius-control);
    background: var(--surface);
  }

  .message span {
    flex: 1;
  }

  .dismiss {
    flex: none;
    padding: 0 6px;
    font-weight: 400;
    color: var(--muted);
    background: transparent;
    border-color: transparent;
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

  .ghost {
    position: fixed;
    z-index: 30;
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 2px 8px;
    font-size: var(--step--1);
    font-weight: 700;
    color: var(--text);
    background: var(--here);
    border: 1px solid var(--here-border);
    border-radius: var(--radius-control);
    box-shadow: 0 6px 16px var(--shadow);
    pointer-events: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .twisty svg {
      transition: none;
    }
  }
</style>
