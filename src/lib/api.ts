import { invoke } from '@tauri-apps/api/core'

export interface Settings {
  instanceDir: string
  packs: string[]
  installDir: string
  java: string | null
  defenderExcluded: string[]
  defenderDismissed: boolean
}

export interface PackConfig {
  author: string
  curseforge: { project_id: number; release_type: string; manual_release: boolean }
  export: { exclude: string[] }
  server: { exclude: string[]; client_only: number[]; keep: number[]; memory: string; properties: Record<string, string> }
  test: { username: string; port: number; client_memory: string }
  discord: { role_id: string; color: string; announce_on_approval: boolean; title: string; show_author: boolean; header: string; footer: string }
}

export interface Overview {
  name: string
  version: string | null
  minecraft: string | null
  loader: string | null
  lockCount: number
  addons: { mods: number; resourcePacks: number; shaderPacks: number; dataPacks: number; other: number }
  missing: string[]
  unlocked: string[]
  branch: string | null
  ahead: number
  behind: number
  changed: string[]
  lastTag: string | null
  devJars: string[]
  unpublishableJars: string[]
  projectId: number
  runningJobs: string[]
  serverRunning: boolean
  clientRunning: boolean
  hooks: HooksState
}

export interface ModChange {
  name: string
  url: string | null
  from: string | null
  to: string | null
}

export interface ModDiff {
  since: string | null
  baseline: boolean
  added: ModChange[]
  updated: ModChange[]
  removed: ModChange[]
}

export type Side = 'both' | 'client' | 'server'
export type Override = 'auto' | 'client' | 'keep'

export interface ModSide {
  path: string
  projectId: number
  name: string
  url: string | null
  side: Side
  reason: string
  override: Override
}

export interface ReleaseStatus {
  version: string
  projectId: number
  fileId: number
  serverFileId: number
  publishedAt: string
  status: string
  serverStatus: string
  serverPackLinked: boolean | null
  announce: boolean
  announcedAt: string | null
}

export type HooksState = 'missing' | 'outdated' | 'current'

export interface TrackedBinary {
  path: string
  kind: 'curseforge' | 'local' | 'generated'
}

export interface InstanceInfo {
  dir: string
  name: string
  minecraft: string | null
  loader: string | null
  addonCount: number
  hasGit: boolean
  hasLock: boolean
  hooks: HooksState
  trackedBinaries: TrackedBinary[]
  remote: string | null
  added: boolean
}

export interface PackList {
  active: string | null
  packs: InstanceInfo[]
  instancesRoot: string
}

export interface SetupOptions {
  remote: string | null
  author: string
  projectId: number
  untrackBinaries: boolean
  commit: { message: string; author: string | null } | null
}

export interface FileChange {
  path: string
  origPath: string | null
  status: string
}

export interface GitStatus {
  branch: string | null
  upstream: string | null
  ahead: number
  behind: number
  hasCommits: boolean
  hasRemote: boolean
  staged: FileChange[]
  unstaged: FileChange[]
  conflicted: FileChange[]
}

export interface GitCommit {
  hash: string
  short: string
  author: string
  when: string
  subject: string
}

export interface GitDiff {
  text: string
  binary: boolean
  truncated: boolean
}

export interface DirEntry {
  name: string
  path: string
  isDir: boolean
  size: number
  ignored: boolean
  archive: boolean
  archived: boolean
}

export interface HistoryOutcome {
  label: string
  moved: [string, string][]
  removed: string[]
  added: string[]
}

export interface Pasted {
  created: string[]
  moved: [string, string][]
}

export interface FileContent {
  text: string | null
  size: number
  unreadable: string | null
  crlf: boolean
  bom: boolean
  archived: boolean
  compressed: boolean
}

export type NbtTag =
  | { type: 'byte' | 'short' | 'int' | 'float' | 'double'; value: number }
  | { type: 'long' | 'string'; value: string }
  | { type: 'byteArray' | 'intArray'; value: { length: number; items: number[] } }
  | { type: 'longArray'; value: { length: number; items: string[] } }
  | { type: 'list'; value: { itemType: string; items: NbtTag[] } }
  | { type: 'compound'; value: [string, NbtTag][] }

export interface NbtDocument {
  compression: 'gzip' | 'zlib' | 'none'
  rootName: string
  root: NbtTag
}

export interface DefenderView {
  active: boolean
  exclusionsReadable: boolean
  missing: string[]
  prompt: boolean
}

export type Credential = 'download' | 'upload'
export type CredentialSource = { kind: 'none' } | { kind: 'stored' } | { kind: 'env'; name: string }

export interface CredentialStatus {
  source: CredentialSource
  present: boolean
}

export interface CredentialsView {
  download: CredentialStatus
  upload: CredentialStatus
  keychain: string
}

export const api = {
  gitVersion: () => invoke<string | null>('git_version'),
  installGit: () => invoke<string>('install_git'),
  credentialsStatus: () => invoke<CredentialsView>('credentials_status'),
  credentialStore: (credential: Credential, secret: string) => invoke<void>('credential_store', { credential, secret }),
  credentialUseEnv: (credential: Credential, name: string) => invoke<void>('credential_use_env', { credential, name }),
  credentialClear: (credential: Credential) => invoke<void>('credential_clear', { credential }),
  envVarNames: () => invoke<string[]>('env_var_names'),
  getWebhook: () => invoke<string | null>('get_webhook'),
  setWebhook: (url: string) => invoke<void>('set_webhook', { url }),
  discordTest: () => invoke<void>('discord_test'),
  announceRelease: (version: string) => invoke<void>('announce_release', { version }),
  discohookLink: (version: string) => invoke<string>('discohook_link', { version }),
  discohookPreview: (version: string) => invoke<string>('discohook_preview', { version }),
  defenderStatus: () => invoke<DefenderView>('defender_status'),
  defenderExclude: (paths: string[]) => invoke<void>('defender_exclude', { paths }),
  defenderDismiss: (dismissed: boolean) => invoke<void>('defender_dismiss', { dismissed }),
  listDir: (path: string) => invoke<DirEntry[]>('list_dir', { path }),
  readFile: (path: string) => invoke<FileContent>('read_file', { path }),
  writeFile: (path: string, text: string, crlf: boolean, bom: boolean) => invoke<void>('write_file', { path, text, crlf, bom }),
  absolutePath: (path: string) => invoke<string>('absolute_path', { path }),
  createFile: (path: string) => invoke<void>('create_file', { path }),
  createDir: (path: string) => invoke<void>('create_dir', { path }),
  trashPaths: (paths: string[]) => invoke<void>('trash_paths', { paths }),
  copyFiles: (paths: string[], cut: boolean) => invoke<void>('copy_files', { paths, cut }),
  pasteFiles: (dir: string, cut: string[]) => invoke<Pasted>('paste_files', { dir, cut }),
  openPackFolder: () => invoke<void>('open_pack_folder'),
  openWithDefaultApp: (path: string) => invoke<void>('open_with_default_app', { path }),
  defaultApp: (extension: string) => invoke<string | null>('default_app', { extension }),
  copyPaths: (paths: string[], absolute: boolean) => invoke<void>('copy_paths', { paths, absolute }),
  readBytes: (path: string) => invoke<ArrayBuffer>('read_bytes', { path }),
  readNbt: (path: string) => invoke<NbtDocument>('read_nbt', { path }),
  decompileClass: (path: string) => invoke<string>('decompile_class', { path }),
  extractArchive: (path: string) => invoke<string>('extract_archive', { path }),
  compressPaths: (paths: string[]) => invoke<string>('compress_paths', { paths }),
  mediaPath: (path: string) => invoke<string>('media_path', { path }),
  indexFiles: () => invoke<{ paths: string[]; truncated: boolean }>('index_files'),
  importPaths: (dir: string, sources: string[]) => invoke<Pasted>('import_paths', { dir, sources }),
  dragOut: (paths: string[]) => invoke<void>('drag_out', { paths }),
  fileHistory: () => invoke<{ undo: string | null; redo: string | null }>('file_history'),
  fileUndo: (redo: boolean) => invoke<HistoryOutcome | null>('file_undo', { redo }),
  renamePath: (from: string, to: string) => invoke<void>('rename_path', { from, to }),
  movePaths: (paths: string[], dir: string) => invoke<[string, string][]>('move_paths', { paths, dir }),
  gitOverview: () => invoke<{ status: GitStatus; commits: GitCommit[] }>('git_overview'),
  gitDiff: (path: string, staged: boolean, untracked: boolean) => invoke<GitDiff>('git_diff', { path, staged, untracked }),
  gitStage: (paths: string[]) => invoke<void>('git_stage', { paths }),
  gitUnstage: (paths: string[]) => invoke<void>('git_unstage', { paths }),
  gitIgnore: (paths: string[]) => invoke<string[]>('git_ignore', { paths }),
  gitDiscard: (tracked: string[], untracked: string[]) => invoke<void>('git_discard', { tracked, untracked }),
  gitCommit: (message: string) => invoke<void>('git_commit', { message }),
  gitSync: (action: 'fetch' | 'pull' | 'push') => invoke<void>('git_sync', { action }),
  listPacks: () => invoke<PackList>('list_packs'),
  discoverInstances: (root?: string) => invoke<InstanceInfo[]>('discover_instances', { root: root ?? null }),
  inspectInstance: (dir: string) => invoke<InstanceInfo>('inspect_instance', { dir }),
  setupInstance: (dir: string, options: SetupOptions) => invoke<void>('setup_instance', { dir, options }),
  switchPack: (dir: string) => invoke<void>('switch_pack', { dir }),
  removePack: (dir: string) => invoke<void>('remove_pack', { dir }),
  updateHooks: () => invoke<void>('update_hooks'),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  overview: () => invoke<Overview>('overview'),
  getPackConfig: () => invoke<PackConfig>('get_pack_config'),
  savePackConfig: (config: PackConfig) => invoke<void>('save_pack_config', { config }),
  setVersion: (version: string) => invoke<void>('set_version', { version }),
  getNotes: () => invoke<string>('get_notes'),
  saveNotes: (text: string) => invoke<void>('save_notes', { text }),
  changelogPreview: () => invoke<{ diff: ModDiff; markdown: string }>('changelog_preview'),
  setCompareBaseline: (zip: string) => invoke<void>('set_compare_baseline', { zip }),
  clearCompareBaseline: () => invoke<void>('clear_compare_baseline'),
  serverSides: () => invoke<ModSide[]>('server_sides'),
  setSideOverride: (projectId: number, mode: Override) => invoke<void>('set_side_override', { projectId, mode }),
  publishPreflight: (version: string) => invoke<string[]>('publish_preflight', { version }),
  buildClientExport: () => invoke<void>('build_client_export'),
  buildServerPack: () => invoke<void>('build_server_pack'),
  startTest: (rebuild: boolean, connect: boolean) => invoke<void>('start_test', { rebuild, connect }),
  startClient: (join: boolean) => invoke<void>('start_client', { join }),
  serverCommand: (line: string) => invoke<void>('server_command', { line }),
  killProcess: (which: 'server' | 'client') => invoke<void>('kill_process', { which }),
  publish: (version: string, push: boolean) => invoke<void>('publish', { options: { version, push } }),
  releases: () => invoke<ReleaseStatus[]>('releases'),
  outputDir: () => invoke<string>('output_dir'),
}

export function errorText(error: unknown): string {
  return typeof error === 'string' ? error : error instanceof Error ? error.message : JSON.stringify(error)
}
