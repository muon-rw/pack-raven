import { invoke } from '@tauri-apps/api/core'

export interface Settings {
  instanceDir: string
  packs: string[]
  installDir: string
  java: string | null
}

export interface PackConfig {
  author: string
  curseforge: { project_id: number; release_type: string; manual_release: boolean }
  export: { exclude: string[] }
  server: { exclude: string[]; client_only: number[]; keep: number[]; memory: string; properties: Record<string, string> }
  test: { username: string; port: number; client_memory: string }
}

export interface Overview {
  name: string
  version: string | null
  minecraft: string | null
  loader: string | null
  lockCount: number
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

export const api = {
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
  startClient: () => invoke<void>('start_client'),
  serverCommand: (line: string) => invoke<void>('server_command', { line }),
  killProcess: (which: 'server' | 'client') => invoke<void>('kill_process', { which }),
  publish: (version: string, push: boolean) => invoke<void>('publish', { options: { version, push } }),
  releases: () => invoke<ReleaseStatus[]>('releases'),
  outputDir: () => invoke<string>('output_dir'),
}

export function errorText(error: unknown): string {
  return typeof error === 'string' ? error : error instanceof Error ? error.message : JSON.stringify(error)
}
