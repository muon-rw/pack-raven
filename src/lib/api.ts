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
  discord: { role_id: string; color: string; announce_on_approval: boolean }
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
  hasCoreKey: boolean
  hasUploadToken: boolean
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
}

export interface FileContent {
  text: string | null
  size: number
  unreadable: string | null
  crlf: boolean
  bom: boolean
}

export interface DefenderView {
  active: boolean
  exclusionsReadable: boolean
  missing: string[]
  prompt: boolean
}

export const api = {
  getWebhook: () => invoke<string | null>('get_webhook'),
  setWebhook: (url: string) => invoke<void>('set_webhook', { url }),
  discordTest: () => invoke<void>('discord_test'),
  announceRelease: (version: string) => invoke<void>('announce_release', { version }),
  discohookLink: (version: string) => invoke<string>('discohook_link', { version }),
  defenderStatus: () => invoke<DefenderView>('defender_status'),
  defenderExclude: (paths: string[]) => invoke<void>('defender_exclude', { paths }),
  defenderDismiss: (dismissed: boolean) => invoke<void>('defender_dismiss', { dismissed }),
  listDir: (path: string) => invoke<DirEntry[]>('list_dir', { path }),
  readFile: (path: string) => invoke<FileContent>('read_file', { path }),
  writeFile: (path: string, text: string, crlf: boolean, bom: boolean) => invoke<void>('write_file', { path, text, crlf, bom }),
  absolutePath: (path: string) => invoke<string>('absolute_path', { path }),
  gitOverview: () => invoke<{ status: GitStatus; commits: GitCommit[] }>('git_overview'),
  gitDiff: (path: string, staged: boolean, untracked: boolean) => invoke<GitDiff>('git_diff', { path, staged, untracked }),
  gitStage: (paths: string[]) => invoke<void>('git_stage', { paths }),
  gitUnstage: (paths: string[]) => invoke<void>('git_unstage', { paths }),
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
