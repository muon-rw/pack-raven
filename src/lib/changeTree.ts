import type { FileChange } from './api'

export type ChangeRow = {
  path: string
  name: string
  depth: number
  isDir: boolean
  change: FileChange | null
  count: number
}

type Folder = { name: string; path: string; folders: Map<string, Folder>; files: FileChange[]; count: number }

const byName = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: 'base', numeric: true })

function nameOf(path: string) {
  return path.slice(path.lastIndexOf('/') + 1)
}

function build(changes: FileChange[]) {
  const root: Folder = { name: '', path: '', folders: new Map(), files: [], count: 0 }
  for (const change of changes) {
    const parts = change.path.split('/')
    let folder = root
    folder.count++
    for (const part of parts.slice(0, -1)) {
      const path = folder.path ? `${folder.path}/${part}` : part
      let next = folder.folders.get(part)
      if (!next) folder.folders.set(part, (next = { name: part, path, folders: new Map(), files: [], count: 0 }))
      next.count++
      folder = next
    }
    folder.files.push(change)
  }
  return root
}

// Like VS Code's compact folders, we show a folder that only holds one subfolder on the same row
// as that subfolder, so deep config paths don't push file names off screen.
function compact(folder: Folder): Folder {
  let current = folder
  while (current.files.length === 0 && current.folders.size === 1) {
    const [only] = current.folders.values()
    current = { ...only, name: `${current.name}/${only.name}` }
  }
  return current
}

export function changeRows(changes: FileChange[], tree: boolean, collapsed: (dir: string) => boolean): ChangeRow[] {
  if (!tree) return changes.map((change) => ({ path: change.path, name: nameOf(change.path), depth: 0, isDir: false, change, count: 1 }))
  const rows: ChangeRow[] = []
  const walk = (folder: Folder, depth: number) => {
    for (const child of [...folder.folders.values()].map(compact).sort((a, b) => byName(a.name, b.name))) {
      rows.push({ path: child.path, name: child.name, depth, isDir: true, change: null, count: child.count })
      if (!collapsed(child.path)) walk(child, depth + 1)
    }
    for (const change of [...folder.files].sort((a, b) => byName(a.path, b.path))) {
      rows.push({ path: change.path, name: nameOf(change.path), depth, isDir: false, change, count: 1 })
    }
  }
  walk(build(changes), 0)
  return rows
}
