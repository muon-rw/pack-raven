import type { EditorState, TransactionSpec } from '@codemirror/state'
import type { Problems } from './editor'
import { listen } from '@tauri-apps/api/event'
import { api, type FileContent } from './api'

export interface Tab {
  path: string
  state: EditorState | null
  saved: string
  dirty: boolean
  crlf: boolean
  bom: boolean
  unreadable: string | null
  conflict: 'changed' | 'deleted' | null
  disk: string | null
  problems: Problems
  preview: boolean
}

export const workspace = $state({ tabs: [] as Tab[], active: null as string | null, pack: null as string | null, error: null as string | null })

export const fs = $state({ seq: 0, dirs: [] as string[] })

const bridge: { path: string | null; dispatch: ((spec: TransactionSpec) => void) | null } = { path: null, dispatch: null }

export function bindView(path: string | null, dispatch: ((spec: TransactionSpec) => void) | null) {
  bridge.path = path
  bridge.dispatch = dispatch
}

export function activeTab(): Tab | undefined {
  return workspace.tabs.find((t) => t.path === workspace.active)
}

export function resetFor(pack: string | null) {
  if (workspace.pack === pack) return
  workspace.pack = pack
  workspace.tabs = []
  workspace.active = null
}

function tab(path: string) {
  return workspace.tabs.find((t) => t.path === path)
}

export function updateDirty(path: string, state: EditorState) {
  const current = tab(path)
  if (!current) return
  current.state = state
  current.dirty = state.doc.length !== current.saved.length || state.doc.toString() !== current.saved
  if (current.dirty) current.preview = false
}

async function editorState(path: string, text: string) {
  const { createState } = await import('./editor')
  return createState(
    path,
    text,
    (state) => updateDirty(path, state),
    () => save(path),
    (problems) => {
      const current = tab(path)
      if (current) current.problems = problems
    },
  )
}

export function pin(path: string) {
  const current = tab(path)
  if (current) current.preview = false
}

// Like VS Code, a preview tab shows a file until you open another one in its place. Editing
// the file or opening it with pinned set keeps it open.
export async function open(path: string, { pinned = true } = {}) {
  if (!tab(path)) {
    const content = await api.readFile(path)
    const text = content.text ?? ''
    const state = content.text === null ? null : await editorState(path, text)
    // A double-click opens the same file three times at once, so another call may have
    // added the tab while we were reading.
    if (!tab(path)) addTab(path, content, text, state, pinned)
  }
  if (pinned) pin(path)
  workspace.active = path
}

function addTab(path: string, content: FileContent, text: string, state: EditorState | null, pinned: boolean) {
  const opened: Tab = {
    path,
    saved: text,
    dirty: false,
    crlf: content.crlf,
    bom: content.bom,
    unreadable: content.unreadable,
    conflict: null,
    disk: null,
    problems: { errors: 0, warnings: 0 },
    preview: !pinned,
    state,
  }
  const replaced = pinned ? -1 : workspace.tabs.findIndex((t) => t.preview)
  if (replaced >= 0) workspace.tabs[replaced] = opened
  else workspace.tabs.push(opened)
}

export async function save(path = workspace.active) {
  const current = path ? tab(path) : undefined
  if (!current?.state || !current.dirty) return
  const text = current.state.doc.toString()
  try {
    await api.writeFile(current.path, text, current.crlf, current.bom)
    current.saved = text
    current.dirty = false
    current.conflict = null
    current.disk = null
    workspace.error = null
  } catch (error) {
    workspace.error = `Couldn't save ${current.path}: ${error}`
  }
}

export async function reload(path: string) {
  const current = tab(path)
  if (!current) return
  const content = await api.readFile(path)
  const text = content.text ?? ''
  Object.assign(current, {
    saved: text,
    dirty: false,
    crlf: content.crlf,
    bom: content.bom,
    unreadable: content.unreadable,
    conflict: null,
    disk: null,
    problems: { errors: 0, warnings: 0 },
    state: content.text === null ? null : await editorState(path, text),
  })
}

export function close(path: string) {
  const index = workspace.tabs.findIndex((t) => t.path === path)
  if (index < 0) return
  workspace.tabs.splice(index, 1)
  if (workspace.active === path) workspace.active = workspace.tabs[Math.min(index, workspace.tabs.length - 1)]?.path ?? null
}

export function dirtyCount() {
  return workspace.tabs.filter((t) => t.dirty).length
}

function replacement(current: string, next: string): TransactionSpec {
  let start = 0
  while (start < current.length && start < next.length && current[start] === next[start]) start++
  let end = 0
  while (end < current.length - start && end < next.length - start && current[current.length - 1 - end] === next[next.length - 1 - end]) end++
  return { changes: { from: start, to: current.length - end, insert: next.slice(start, next.length - end) } }
}

function applyDiskText(current: Tab, text: string) {
  if (!current.state) return
  const spec = replacement(current.state.doc.toString(), text)
  current.saved = text
  current.conflict = null
  current.disk = null
  if (bridge.path === current.path && bridge.dispatch) {
    bridge.dispatch(spec)
  } else {
    current.state = current.state.update(spec).state
    current.dirty = false
  }
}

export function useDiskVersion(path: string) {
  const current = tab(path)
  if (current?.disk !== null && current?.disk !== undefined) applyDiskText(current, current.disk)
}

export function keepMine(path: string) {
  const current = tab(path)
  if (!current?.state) return
  if (current.disk !== null) current.saved = current.disk
  current.conflict = null
  current.disk = null
  current.dirty = current.state.doc.toString() !== current.saved
}

function parentDir(path: string) {
  const index = path.lastIndexOf('/')
  return index < 0 ? '' : path.slice(0, index)
}

async function onDiskChange(paths: string[]) {
  fs.dirs = [...new Set(paths.flatMap((p) => [parentDir(p), p]))]
  fs.seq++
  for (const current of workspace.tabs.filter((t) => paths.includes(t.path) && t.state)) {
    let text: string | null
    try {
      text = (await api.readFile(current.path)).text
    } catch {
      current.conflict = 'deleted'
      continue
    }
    if (current.conflict === 'deleted') current.conflict = null
    if (text === null || text === current.saved) continue
    if (current.dirty) {
      current.conflict = 'changed'
      current.disk = text
    } else {
      applyDiskText(current, text)
    }
  }
}

let watching: Promise<unknown> | null = null

export function startWatching() {
  watching ??= listen<{ paths: string[] }>('fs-change', ({ payload }) => onDiskChange(payload.paths))
}
