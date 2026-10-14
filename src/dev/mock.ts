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
  const list = (fx.dirs[dir] ??= listDir(dir)) as MockEntry[]
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

// A pretend mod jar, so you can try archive browsing, the image viewer and decompiling.
const jar: Record<string, [string, boolean][]> = {
  '': [['assets', true], ['com', true], ['META-INF', true], ['fabric.mod.json', false], ['README.md', false]],
  assets: [['example', true]],
  'assets/example': [['lang', true], ['textures', true]],
  'assets/example/lang': [['en_us.json', false]],
  'assets/example/textures': [['block', true]],
  'assets/example/textures/block': [['fire.png', false], ['fire.png.mcmeta', false], ['stone.png', false]],
  com: [['example', true]],
  'com/example': [['ExampleMod$Inner.class', false], ['ExampleMod.class', false]],
  'META-INF': [['MANIFEST.MF', false]],
}

function splitArchive(path: string) {
  const parts = path.split('/')
  const end = parts.findIndex((p) => /\.(jar|zip)$/i.test(p))
  return end < 0 ? null : { archive: parts.slice(0, end + 1).join('/'), inner: parts.slice(end + 1).join('/') }
}

function listDir(path: string) {
  const inside = splitArchive(path)
  if (inside) {
    return (jar[inside.inner] ?? []).map(([name, isDir]) => ({ name, path: `${path}/${name}`, isDir, size: 1024, ignored: false, archive: false, archived: true }))
  }
  const fallback: Record<string, MockEntry[]> = {
    mods: [{ name: 'example-mod.jar', path: 'mods/example-mod.jar', isDir: false, size: 40960, ignored: false }],
    logs: [
      { name: '2026-10-13-1.log.gz', path: 'logs/2026-10-13-1.log.gz', isDir: false, size: 812, ignored: true },
      { name: 'latest.log', path: 'logs/latest.log', isDir: false, size: 4096, ignored: true },
    ],
  }
  const listed = [...((fx.dirs[path] as MockEntry[] | undefined) ?? fallback[path] ?? [])]
  if (path === '' && !listed.some((e) => e.name === 'huge-recipes.json')) listed.push({ name: 'huge-recipes.json', path: 'huge-recipes.json', isDir: false, size: 72e6, ignored: false })
  if (path === '' && !listed.some((e) => e.name === 'README.md')) listed.push({ name: 'README.md', path: 'README.md', isDir: false, size: 2048, ignored: false })
  return listed.map((e) => ({ ...e, archive: !e.isDir && /\.(jar|zip)$/i.test(e.name), archived: false }))
}

// Exercises headings, tables, task lists, raw HTML, relative images and every kind of link.
const sampleReadme = [
  '<p align="center"><img src="mods/example-mod.jar/assets/example/textures/block/stone.png" width="64" alt="Pack icon"></p>',
  '',
  "# Raven's High Fantasy",
  '',
  'A **fantasy** pack with _dragons_, dungeons and `kubejs` tweaks. See [Installing](#installing) or the [config notes](config/apotheosis/apotheosis.cfg).',
  '',
  '![Fire](mods/example-mod.jar/assets/example/textures/block/fire.png) ![Badge](https://img.shields.io/badge/minecraft-1.21.1-green)',
  '',
  '## Installing',
  '',
  '1. Install the CurseForge app',
  '2. Import the pack',
  '   - Give it **8 GB** of memory',
  '',
  '## Roadmap',
  '',
  '- [x] Dragons',
  '- [ ] More dungeons',
  '',
  '| Mod | Version |',
  '| --- | --- |',
  '| Apotheosis | 8.1.2 |',
  '| Iron\'s Spells | 3.4.0 |',
  '',
  '> Report bugs on [GitHub](https://github.com/muon-rw/Ravens-High-Fantasy/issues).',
  '',
  '<script>alert("no")</script>',
  '',
  '```js',
  "ServerEvents.recipes(event => event.remove({ id: 'minecraft:stick' }))",
  '```',
  '',
  ...Array.from({ length: 40 }, (_, i) => `Filler paragraph ${i + 1} so the preview scrolls.\n`),
].join('\n')

function readFile(path: string) {
  const archived = !!splitArchive(path)?.inner
  if (path.endsWith('README.md')) return { text: sampleReadme, size: sampleReadme.length, unreadable: null, crlf: false, bom: false, archived, compressed: false, large: false }
  if (path.endsWith('.gz')) {
    const log = '[12:00:00] [main/INFO] [net.minecraft.server.Main/]: Starting server\n[12:00:01] [main/WARN] [mixin/]: Reference map not found\n'
    return { text: log, size: 120, unreadable: null, crlf: false, bom: false, archived: false, compressed: true }
  }
  const binary = { text: null, size: 2048, unreadable: "This is a binary file, so packraven can't edit it.", crlf: false, bom: false, archived }
  if (path.endsWith('fire.png.mcmeta')) return { text: '{ "animation": { "frametime": 4, "interpolate": true } }', size: 60, unreadable: null, crlf: false, bom: false, archived }
  if (path.endsWith('.mcmeta') && archived) throw `${path} isn't in the archive`
  if (/\.(png|jar|zip|nbt|dat|class|ogg|mp4)$/.test(path)) return binary
  return (fx.files[path] as object | undefined) ?? { text: `// ${path}\n`, size: 0, unreadable: null, crlf: false, bom: false, archived }
}

// Draws a 16 px texture, or for fire.png a strip of four frames, as a PNG data URL.
function mockImage(path: string) {
  const canvas = document.createElement('canvas')
  const frames = path.includes('fire') ? 4 : 1
  canvas.width = 16
  canvas.height = 16 * frames
  const context = canvas.getContext('2d')!
  const colors = ['#d9480f', '#f08c00', '#fcc419', '#e8590c']
  for (let f = 0; f < frames; f++) {
    for (let y = 0; y < 16; y++) {
      for (let x = 0; x < 16; x++) {
        if (frames === 1) context.fillStyle = (x + y) % 4 < 2 ? '#868e96' : '#495057'
        else context.fillStyle = (x * 7 + y * 3 + f * 5) % 6 < 3 ? colors[f] : 'transparent'
        context.fillRect(x, f * 16 + y, 1, 1)
      }
    }
  }
  return canvas.toDataURL('image/png')
}

function dataUrlBytes(url: string) {
  return Uint8Array.from(atob(url.split(',')[1]), (c) => c.charCodeAt(0)).buffer
}

// A short NeoForge log that grows by a line on each read, so you can watch the viewer follow it.
const sampleLog = [
  '[12:00:00] [main/INFO] [cpw.mods.modlauncher.Launcher/MODLAUNCHER]: ModLauncher running',
  '[12:00:01] [main/DEBUG] [mixin/]: Registering new injector for @SugarWrapper',
  '[12:00:02] [main/WARN] [mixin/]: Reference map \'example.refmap.json\' for example.mixins.json could not be read',
  '[12:00:03] [Render thread/ERROR] [net.minecraft.client.Minecraft/]: Failed to load texture',
  'java.io.FileNotFoundException: example:textures/block/missing.png',
  '\tat net.minecraft.server.packs.resources.ResourceManager.getResourceOrThrow(ResourceManager.java:25)',
  '\tat net.minecraft.client.renderer.texture.SimpleTexture.load(SimpleTexture.java:40)',
  '[12:00:04] [main/INFO] [net.neoforged.fml.loading.moddiscovery.ModDiscoverer/SCAN]: Mod List:',
  '\t\tName Version (Mod Id)',
  '\t\t[NoCube\'s] Realm RPG: Treasure Balloons 1.0.0 (realmrpg_balloons)',
  '[12:00:05] [Server thread/INFO] [net.minecraft.server.MinecraftServer/]: Done (3.2s)!',
]
let logReads = 0

const sampleNbt = {
  compression: 'gzip',
  rootName: '',
  root: {
    type: 'compound',
    value: [
      [
        'servers',
        {
          type: 'list',
          value: {
            itemType: 'compound',
            items: [
              {
                type: 'compound',
                value: [
                  ['name', { type: 'string', value: 'Raven Test Server' }],
                  ['ip', { type: 'string', value: 'localhost:25565' }],
                  ['acceptTextures', { type: 'byte', value: 1 }],
                ],
              },
            ],
          },
        },
      ],
      ['DataVersion', { type: 'int', value: 3955 }],
      ['Seed', { type: 'long', value: '-1234567890123456789' }],
      ['Heights', { type: 'longArray', value: { length: 600, items: ['0', '1', '2'] } }],
    ],
  },
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
  get_settings: () => ({ instanceDir: pack.dir, packs: [pack.dir], installDir: 'C:/curseforge/minecraft/Install', java: null, defenderExcluded: [], defenderDismissed: false, editorAskMb: 50, editorMaxMb: 100 }),
  get_pack_config: () => config,
  get_webhook: () => 'https://discord.com/api/webhooks/0000/example',
  git_version: () => '2.56.0.windows.2',
  credentials_status: () => ({ download: { source: { kind: 'stored' }, present: true }, upload: { source: { kind: 'env', name: 'CURSEFORGE_TOKEN' }, present: true }, keychain: 'Windows Credential Manager' }),
  env_var_names: () => ['APPDATA', 'CF_DOWNLOAD_TOKEN', 'CURSEFORGE_TOKEN', 'PATH'],
  defender_status: () => ({ active: true, exclusionsReadable: false, missing: [], prompt: false }),
  git_overview: () => ({ status: { ...fx.status, staged, unstaged }, commits: fx.commits }),
  git_diff: (a) => diffFor(String(a.path), Boolean(a.staged)),
  list_dir: (a) => listDir(String(a.path)),
  read_file: (a) =>
    String(a.path).endsWith('huge-recipes.json')
      ? a.allowLarge
        ? { text: '[\n' + '  {"type": "minecraft:crafting_shaped"},\n'.repeat(20000) + '  {}\n]\n', size: 72e6, unreadable: null, crlf: false, bom: false, archived: false, compressed: false, large: true }
        : { text: null, size: 72e6, unreadable: 'This file is 72.0 MB, over the 50.0 MB you set to ask about. You can open it in a simplified editor without checking or folding.', crlf: false, bom: false, archived: false, compressed: false, large: true }
      : readFile(String(a.path)),
  read_bytes: (a) => dataUrlBytes(mockImage(String(a.path))),
  read_nbt: () => sampleNbt,
  read_log: (a) => {
    if (!a.offset) return { text: `${sampleLog.join('\n')}\n`, offset: 1, reset: true, truncated: false, followable: !String(a.path).endsWith('.gz') }
    logReads++
    return { text: `[12:00:${String(5 + logReads).padStart(2, '0')}] [Server thread/INFO] [net.minecraft.server.MinecraftServer/]: Tick ${logReads}\n`, offset: 1 + logReads, reset: false, truncated: false, followable: true }
  },
  media_path: (a) => `${pack.dir}/${a.path}`,
  decompile_class: (a) =>
    new Promise((resolve) =>
      setTimeout(() => resolve(`package com.example;\n\n// Decompiled from ${a.path}\npublic class ExampleMod {\n    public static final String ID = "example";\n\n    static class Inner {\n    }\n}\n`), 600),
    ),
  extract_archive: (a) => {
    if (String(a.path).endsWith('.gz')) {
      const file = String(a.path).slice(0, -3)
      addEntry(file, false)
      return file
    }
    const folder = String(a.path).replace(/\.(jar|zip)$/i, '')
    addEntry(folder, true)
    return folder
  },
  compress_paths: (a) => {
    const first = (a.paths as string[])[0]
    const zip = `${first.replace(/\.[^./]+$/, '')}.zip`
    addEntry(zip, false)
    return zip
  },
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
  copy_paths: (a) => console.info('[mock] copied paths', a),
  open_with_default_app: (a) => console.info('[mock] opened', a.path),
  default_app: (a) => ({ png: 'Photos', zip: 'Windows Explorer' })[String(a.extension)] ?? null,
  index_files: () => ({
    paths: Object.values(fx.dirs)
      .flat()
      .map((e) => ((e as MockEntry).isDir ? `${(e as MockEntry).path}/` : (e as MockEntry).path)),
    truncated: false,
  }),
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
    convertFileSrc: (path: string) => (/\.(png|jpe?g|gif|webp)$/i.test(path) ? mockImage(path) : path),
    invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
      const handler = handlers[cmd]
      if (handler) return handler(args)
      console.info('[mock] unhandled command', cmd, args)
      return null
    },
  }
}
