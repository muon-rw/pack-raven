// Browser-only stand-in for the Tauri backend, so the UI can run in a plain browser during development.
// We load data from src/dev/fixtures.json when present (gitignored; you generate it from a real instance).

type Fixtures = {
  instance: string
  version: string
  minecraft: string
  loader: string
  lockCount: number
  dirs: Record<string, unknown[]>
  files: Record<string, unknown>
  status: { staged: { path: string; status: string }[]; unstaged: { path: string; status: string }[] } & Record<string, unknown>
  commits: unknown[]
  diffs: Record<string, unknown>
  changed: string[]
}

const found = import.meta.glob('./fixtures.json', { eager: true, import: 'default' }) as Record<string, Fixtures>
const fx: Fixtures = found['./fixtures.json'] ?? {
  instance: 'C:/Instances/Example Pack',
  version: '0.1.0',
  minecraft: '1.21.1',
  loader: 'neoforge-21.1.259',
  lockCount: 0,
  dirs: { '': [] },
  files: {},
  status: { staged: [], unstaged: [] },
  commits: [],
  diffs: {},
  changed: [],
}

const staged = fx.status.staged.length ? fx.status.staged : fx.status.unstaged.slice(0, 2)
const unstaged = fx.status.staged.length ? fx.status.unstaged : fx.status.unstaged.slice(2)
const diffFor = (path: string, isStaged: boolean) => fx.diffs[`${isStaged ? 's' : 'u'}:${path}`] ?? fx.diffs[`u:${path}`] ?? { text: '', binary: false, truncated: false }

const pack = {
  dir: fx.instance,
  name: "Raven's High Fantasy",
  minecraft: fx.minecraft,
  loader: fx.loader,
  addonCount: fx.lockCount,
  hasGit: true,
  hasLock: true,
  hooks: 'current',
  trackedBinaries: [],
  remote: 'https://github.com/muon-rw/Ravens-High-Fantasy.git',
  added: true,
}

const config = {
  author: 'muon-rw',
  curseforge: { project_id: 1234567, release_type: 'beta', manual_release: false },
  export: { exclude: [] },
  server: { exclude: [], client_only: [], keep: [], memory: '6G', properties: {} },
  test: { username: 'Dev', port: 25565, client_memory: '8G' },
  discord: { role_id: '112233445566778899', color: '#4d6b2f', announce_on_approval: true, title: '{project_name} - v{version}', show_author: true, header: '', footer: '[Install]({file_url})' },
}

const releases = [
  { version: '0.1.0', projectId: 1234567, fileId: 7001001, serverFileId: 7001002, publishedAt: '2026-10-03T18:20:00Z', status: 'Approved', serverStatus: 'Approved', serverPackLinked: true, announce: true, announcedAt: '2026-10-03T19:02:00Z' },
  { version: fx.version === '0.1.0' ? '0.1.1' : fx.version, projectId: 1234567, fileId: 7105511, serverFileId: 7105512, publishedAt: '2026-10-12T21:40:00Z', status: 'Under review', serverStatus: 'Under review', serverPackLinked: null, announce: true, announcedAt: null },
]

type MockEntry = { name: string; path: string; isDir: boolean; size: number; ignored: boolean }

function addEntry(path: string, isDir: boolean) {
  const index = path.lastIndexOf('/')
  const dir = index < 0 ? '' : path.slice(0, index)
  const list = (fx.dirs[dir] ??= []) as MockEntry[]
  if (list.some((e) => e.path === path)) throw `${path} already exists`
  list.push({ name: path.slice(index + 1), path, isDir, size: 0, ignored: false })
  list.sort((a, b) => Number(b.isDir) - Number(a.isDir) || a.name.localeCompare(b.name))
  if (isDir) fx.dirs[path] ??= []
}

function removeEntries(paths: string[]) {
  for (const dir of Object.keys(fx.dirs)) fx.dirs[dir] = (fx.dirs[dir] as MockEntry[]).filter((e) => !paths.includes(e.path))
}

function moveEntry(from: string, to: string) {
  const entry = Object.values(fx.dirs)
    .flat()
    .find((e) => (e as MockEntry).path === from) as MockEntry | undefined
  removeEntries([from])
  addEntry(to, entry?.isDir ?? false)
  for (const dir of Object.keys(fx.dirs)) {
    if (dir !== from && !dir.startsWith(`${from}/`)) continue
    fx.dirs[to + dir.slice(from.length)] = (fx.dirs[dir] as MockEntry[]).map((e) => ({ ...e, path: to + e.path.slice(from.length) }))
    if (dir !== to) delete fx.dirs[dir]
  }
}

const callbacks = new Map<number, (message: unknown) => void>()
const listeners: { event: string; handler: number }[] = []

// Lets you fake backend events from the console, such as a file drag from Explorer:
// __mockEmit('tauri://drag-over', { position: { x: 300, y: 200 } })
function emit(event: string, payload: unknown) {
  for (const l of listeners.filter((l) => l.event === event)) callbacks.get(l.handler)?.({ event, id: l.handler, payload })
}

const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
  list_packs: () => ({ active: pack.dir, packs: [pack], instancesRoot: 'C:/Instances' }),
  overview: () => ({
    name: pack.name,
    version: fx.version,
    minecraft: fx.minecraft,
    loader: fx.loader,
    lockCount: fx.lockCount,
    addons: { mods: fx.lockCount - 14, resourcePacks: 9, shaderPacks: 2, dataPacks: 3, other: 0 },
    missing: [],
    unlocked: [],
    branch: 'master',
    ahead: 1,
    behind: 0,
    changed: fx.changed,
    lastTag: 'v0.1.0',
    devJars: ['raven_core-dev.jar'],
    unpublishableJars: [],
    projectId: config.curseforge.project_id,
    runningJobs: [],
    serverRunning: false,
    clientRunning: false,
    hooks: 'current',
  }),
  get_settings: () => ({ instanceDir: pack.dir, packs: [pack.dir], installDir: 'C:/curseforge/minecraft/Install', java: null, defenderExcluded: [], defenderDismissed: false }),
  get_pack_config: () => config,
  get_webhook: () => 'https://discord.com/api/webhooks/0000/example',
  git_version: () => '2.56.0.windows.2',
  credentials_status: () => ({ download: { source: { kind: 'stored' }, present: true }, upload: { source: { kind: 'env', name: 'CURSEFORGE_TOKEN' }, present: true }, keychain: 'Windows Credential Manager' }),
  env_var_names: () => ['APPDATA', 'CF_DOWNLOAD_TOKEN', 'CURSEFORGE_TOKEN', 'PATH'],
  defender_status: () => ({ active: true, exclusionsReadable: false, missing: [], prompt: false }),
  git_overview: () => ({ status: { ...fx.status, staged, unstaged }, commits: fx.commits }),
  git_diff: (a) => diffFor(String(a.path), Boolean(a.staged)),
  list_dir: (a) => fx.dirs[String(a.path)] ?? [],
  read_file: (a) => fx.files[String(a.path)] ?? { text: `// ${a.path}\n`, size: 0, unreadable: null, crlf: false, bom: false },
  absolute_path: (a) => `${pack.dir}/${a.path}`,
  create_file: (a) => addEntry(String(a.path), false),
  create_dir: (a) => addEntry(String(a.path), true),
  trash_paths: (a) => removeEntries(a.paths as string[]),
  rename_path: (a) => moveEntry(String(a.from), String(a.to)),
  move_paths: (a) =>
    (a.paths as string[]).map((from) => {
      const to = `${a.dir ? `${a.dir}/` : ''}${from.slice(from.lastIndexOf('/') + 1)}`
      moveEntry(from, to)
      return [from, to]
    }),
  git_ignore: () => [],
  copy_files: () => null,
  import_paths: (a) => ({
    created: (a.sources as string[]).map((source) => {
      const path = `${a.dir ? `${a.dir}/` : ''}${source.split(/[\\/]/).pop()}`
      addEntry(path, false)
      return path
    }),
    moved: [],
  }),
  drag_out: () => null,
  file_history: () => ({ undo: 'Rename options.txt', redo: null }),
  file_undo: () => null,
  paste_files: () => {
    throw 'The clipboard has no files to paste.'
  },
  releases: () => releases,
  get_notes: () => '## Highlights\n\n- New dragon fights in the End\n- Rebalanced early armor tiers\n',
  changelog_preview: () => ({ diff: { since: 'v0.1.0', added: [], updated: [], removed: [] }, markdown: '' }),
  publish_preflight: () => [],
  discover_instances: () => [pack],
  server_sides: () => [],
  discohook_link: () => 'https://discohook.app/?share=example',
  discohook_preview: () => 'https://discohook.app/?share=example',
  'plugin:autostart|is_enabled': () => false,
  'plugin:event|listen': (a) => {
    listeners.push({ event: String(a.event), handler: Number(a.handler) })
    return Math.floor(Math.random() * 1e9)
  },
  'plugin:event|unlisten': () => null,
}

export function installMock() {
  let callbackId = 0
  const w = window as unknown as Record<string, unknown>
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} }
  w.__mockEmit = emit
  w.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
    transformCallback: (callback: (message: unknown) => void) => {
      callbacks.set(++callbackId, callback)
      return callbackId
    },
    unregisterCallback: () => {},
    convertFileSrc: (path: string) => path,
    invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
      const handler = handlers[cmd]
      if (handler) return handler(args)
      console.info('[mock] unhandled command', cmd, args)
      return null
    },
  }
}
