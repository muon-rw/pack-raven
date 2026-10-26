import { invoke } from '@tauri-apps/api/core'

export interface Settings {
  instanceDir: string
  packs: string[]
  installDir: string
  javaByMajor: Record<string, string>
  defenderExcluded: string[]
  defenderDismissed: boolean
  minecraftEulaAccepted: boolean
  minecraftOnlineMode: boolean
  editorAskMb: number
  editorMaxMb: number
  gitName: string
  gitEmail: string
  autoUpdate: boolean
  runArgs: RunArgs
}

export type JavaSource = 'curseForge' | 'packraven' | 'javaHome' | 'path' | 'system' | 'intelliJ' | 'gradle' | 'prism' | 'modrinth'

export interface JavaInstall {
  path: string
  major: number
  version: string
  distribution: string
  source: JavaSource
  foreignArch: string | null
}

export interface JavaScan {
  installs: JavaInstall[]
  // The Java packraven picks for each Java version you haven't chosen one for, keyed by major version.
  automatic: Record<string, string>
  // The GraalVM version packraven can download for each Java version.
  downloads: Record<string, number>
  // The Java versions the packs you added need.
  needed: number[]
}

export interface RunArgs {
  clientJvmArgs: string[]
  clientArgs: string[]
  serverJvmArgs: string[]
  serverArgs: string[]
}

export interface PackJavaDefault {
  major: number
  path: string | null
  error: string | null
}

export type UpdatePhase =
  | { phase: 'unchecked' | 'checking' | 'upToDate' }
  | { phase: 'available' | 'ready'; version: string }
  | { phase: 'downloading'; version: string; percent: number | null }
  | { phase: 'failed'; stage: 'check' | 'download' | 'install'; error: string }

export interface UpdateView {
  current: string
  auto: boolean
  supported: boolean
  blocked: string | null
  status: UpdatePhase
}

export interface MinecraftAccount {
  name: string
  uuid: string
}

export interface PackConfig {
  author: string
  curseforge: { project_id: number; release_type: string; manual_release: boolean }
  export: { exclude: string[] }
  server: {
    exclude: string[]
    client_only: number[]
    keep: number[]
    memory: string
    jvm_args: string[]
    args: string[]
    graalvm: string
    properties: Record<string, string>
  }
  test: { username: string; port: number; client_memory: string }
  discord: { role_id: string; color: string; announce_on_approval: boolean; title: string; show_author: boolean; header: string; footer: string }
  updates: { channel: ReleaseType; locked: number[] }
}

export type ReleaseType = 'release' | 'beta' | 'alpha'

export interface UploadNames {
  displayName: string
  fileName: string
}

export interface InstalledMod {
  path: string
  projectId: number
  fileId: number
  name: string
  url: string | null
  authors: string[]
  logo: string | null
}

export interface ModFile {
  fileId: number
  fileName: string
  releaseType: ReleaseType
  fileDate: string | null
}

export interface ModCheck {
  path: string
  projectId: number
  logo: string | null
  releaseType: ReleaseType | null
  requires: number[]
  newer: ModFile[]
}

export interface ModRequest {
  fileId: number
  replaces: string | null
}

export type AddonKind = 'mods' | 'resourcepacks' | 'shaderpacks' | 'datapacks'
export type SearchSort = 'relevance' | 'popularity' | 'downloads' | 'newest' | 'updated'

export interface SearchRequest {
  text: string
  kind: AddonKind
  categoryId: number | null
  sort: SearchSort
  index: number
}

export interface SearchResult {
  projectId: number
  name: string
  summary: string
  authors: string[]
  downloads: number
  updated: string
  logo: string | null
  url: string | null
  file: ModFile
}

export interface ModPreview {
  dependencies: string[]
  unavailable: string[]
  // Every project the install would add, its dependencies included.
  projects: number[]
}

export interface Overview {
  name: string
  version: string | null
  bccInstalled: boolean
  minecraft: string | null
  loader: string | null
  lockCount: number
  addons: { mods: number; resourcePacks: number; shaderPacks: number; dataPacks: number; other: number }
  notOnCurseforge: string[]
  modListError: string | null
  branch: string | null
  ahead: number
  behind: number
  incoming: ModCounts | null
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

export interface ModCounts {
  added: number
  updated: number
  removed: number
}

export interface ModConflict {
  name: string
  mine: string | null
  theirs: string | null
}

export interface PullPreview {
  incoming: ModCounts
  conflicts: ModConflict[]
}

export type Prefer = 'mine' | 'theirs'

export interface ModChange {
  name: string
  url: string | null
  from: string | null
  to: string | null
  inModList: boolean
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
  missingMods: number
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
  trackedIgnored: string[]
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

export type LineAction = 'stage' | 'unstage' | 'discard'

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
  changed: string[]
}

export interface TextQuery {
  pattern: string
  regex: boolean
  matchCase: boolean
  wholeWord: boolean
  include: string
  exclude: string
  includeIgnored: boolean
  replace: string | null
}

/** `column` and `length` place the match in the file; `start` and `end` place it in `preview`. */
export interface TextMatch {
  line: number
  column: number
  length: number
  preview: string
  start: number
  end: number
  replacement: string | null
}

export interface TextResults {
  files: { path: string; matches: TextMatch[] }[]
  total: number
  truncated: boolean
  searched: number
  tooLarge: number
}

export interface Replaced {
  label: string
  files: string[]
  replaced: number
  missed: number
  undoable: boolean
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
  large: boolean
}

export type PackKind = 'data' | 'resource'

export interface PackEntry {
  path: string
  name: string
  zipped: boolean
  /** Whether options.txt turns on a pack in the instance's resourcepacks folder. */
  enabled?: boolean
}

export interface PackFolder {
  path: string
  exists: boolean
  canCreate: boolean
  packs: PackEntry[]
}

export interface PackLoader {
  id: 'paxi' | 'openloader' | 'globalpacks'
  name: string
  installed: boolean
  projectIds: number[]
  folders: PackFolder[]
}

export interface CopiedIntoPack {
  path: string
  created: string[]
  existing: number
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

export interface IgnoreOption {
  label: string
  patterns: string[]
}

export interface UntrackPreview {
  ignored: string[]
  uncovered: string[]
}

export interface IgnorePreview {
  untracked: string[]
  tracked: string[]
}

export type Credential = 'upload' | 'github'
export type CredentialSource = { kind: 'none' } | { kind: 'stored' } | { kind: 'env'; name: string }

export interface CredentialStatus {
  source: CredentialSource
  present: boolean
}

export interface CredentialsView {
  builtInApiKey: boolean
  upload: CredentialStatus
  github: CredentialStatus
  keychain: string
}

export const api = {
  gitVersion: () => invoke<string | null>('git_version'),
  installGit: () => invoke<string>('install_git'),
  credentialsStatus: () => invoke<CredentialsView>('credentials_status'),
  credentialStore: (credential: Credential, secret: string) => invoke<void>('credential_store', { credential, secret }),
  credentialUseEnv: (credential: Credential, name: string) => invoke<void>('credential_use_env', { credential, name }),
  credentialClear: (credential: Credential) => invoke<void>('credential_clear', { credential }),
  githubIdentity: () => invoke<{ name: string; email: string }>('github_identity'),
  envVarNames: () => invoke<string[]>('env_var_names'),
  setIconTheme: (dark: boolean) => invoke<void>('set_icon_theme', { dark }),
  getPackJava: () => invoke<string | null>('get_pack_java'),
  setPackJava: (java: string | null) => invoke<void>('set_pack_java', { java }),
  packJavaDefault: () => invoke<PackJavaDefault>('pack_java_default'),
  getRunArgs: () => invoke<RunArgs>('get_run_args'),
  setRunArgs: (args: RunArgs) => invoke<void>('set_run_args', { args }),
  scanJava: () => invoke<JavaScan>('scan_java'),
  downloadJava: (major: number) => invoke<JavaInstall>('download_java', { major }),
  cancelJavaDownload: (graalvm: number) => invoke<void>('cancel_java_download', { graalvm }),
  getServerIcon: () => invoke<string | null>('get_server_icon'),
  setServerIcon: (image: string) => invoke<string | null>('set_server_icon', { image }),
  useProjectLogoAsServerIcon: () => invoke<string | null>('use_project_logo_as_server_icon'),
  removeServerIcon: () => invoke<void>('remove_server_icon'),
  serverLaunch: () => invoke<{ windows: string; unix: string }>('server_launch'),
  getWebhook: () => invoke<string | null>('get_webhook'),
  setWebhook: (url: string) => invoke<void>('set_webhook', { url }),
  discordTest: () => invoke<void>('discord_test'),
  announceRelease: (version: string) => invoke<void>('announce_release', { version }),
  discohookLink: (version: string) => invoke<string>('discohook_link', { version }),
  discohookPreview: (version: string) => invoke<string>('discohook_preview', { version }),
  defenderStatus: () => invoke<DefenderView>('defender_status'),
  defenderExclude: (paths: string[]) => invoke<void>('defender_exclude', { paths }),
  defenderDismiss: (dismissed: boolean) => invoke<void>('defender_dismiss', { dismissed }),
  acceptMinecraftEula: () => invoke<void>('accept_minecraft_eula'),
  minecraftAccount: () => invoke<MinecraftAccount | null>('minecraft_account'),
  // Resolves to null when you cancel.
  minecraftSignIn: () => invoke<MinecraftAccount | null>('minecraft_sign_in'),
  minecraftSignInCancel: () => invoke<void>('minecraft_sign_in_cancel'),
  minecraftSignOut: () => invoke<void>('minecraft_sign_out'),
  setMinecraftOnlineMode: (online: boolean) => invoke<void>('set_minecraft_online_mode', { online }),
  listDir: (path: string) => invoke<DirEntry[]>('list_dir', { path }),
  readFile: (path: string, allowLarge = false) => invoke<FileContent>('read_file', { path, allowLarge }),
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
  readLog: (path: string, offset: number) =>
    invoke<{ text: string; offset: number; reset: boolean; truncated: boolean; followable: boolean }>('read_log', { path, offset }),
  decompileClass: (path: string) => invoke<string>('decompile_class', { path }),
  extractArchive: (path: string) => invoke<string>('extract_archive', { path }),
  compressPaths: (paths: string[]) => invoke<string>('compress_paths', { paths }),
  mediaPath: (path: string) => invoke<string>('media_path', { path }),
  packLoaders: (kind: PackKind) => invoke<PackLoader[]>('pack_loaders', { kind }),
  packFolder: (path: string) => invoke<PackFolder>('pack_folder', { path }),
  copyIntoPack: (source: string, pack: string, create: boolean) => invoke<CopiedIntoPack>('copy_into_pack', { source, pack, create }),
  indexFiles: () => invoke<{ paths: string[]; truncated: boolean }>('index_files'),
  searchText: (query: TextQuery) => invoke<TextResults | null>('search_text', { query }),
  replaceText: (query: TextQuery, targets: { path: string; matches: [number, number][] }[]) => invoke<Replaced>('replace_text', { query, targets }),
  importPaths: (dir: string, sources: string[]) => invoke<Pasted>('import_paths', { dir, sources }),
  dragOut: (paths: string[]) => invoke<void>('drag_out', { paths }),
  fileHistory: () => invoke<{ undo: string | null; redo: string | null }>('file_history'),
  fileUndo: (redo: boolean) => invoke<HistoryOutcome | null>('file_undo', { redo }),
  renamePath: (from: string, to: string) => invoke<void>('rename_path', { from, to }),
  renamePaths: (renames: [string, string][]) => invoke<[string, string][]>('rename_paths', { renames }),
  movePaths: (paths: string[], dir: string) => invoke<[string, string][]>('move_paths', { paths, dir }),
  gitOverview: () => invoke<{ status: GitStatus; commits: GitCommit[] }>('git_overview'),
  gitDiff: (path: string, staged: boolean, untracked: boolean) => invoke<GitDiff>('git_diff', { path, staged, untracked }),
  gitBlob: (path: string, version: 'head' | 'index') => invoke<ArrayBuffer>('git_blob', { path, version }),
  gitApplyLines: (path: string, diff: string, selected: number[], action: LineAction) => invoke<void>('git_apply_lines', { path, diff, selected, action }),
  gitStage: (paths: string[]) => invoke<void>('git_stage', { paths }),
  gitUnstage: (paths: string[]) => invoke<void>('git_unstage', { paths }),
  gitIgnoreOptions: (paths: string[]) => invoke<IgnoreOption[]>('git_ignore_options', { paths }),
  gitIgnorePreview: (patterns: string[]) => invoke<IgnorePreview>('git_ignore_preview', { patterns }),
  gitIgnore: (patterns: string[]) => invoke<string[]>('git_ignore', { patterns }),
  gitUntrackPreview: (paths: string[]) => invoke<UntrackPreview>('git_untrack_preview', { paths }),
  gitKeepTracking: (paths: string[]) => invoke<string[]>('git_keep_tracking', { paths }),
  gitUntrack: (paths: string[]) => invoke<void>('git_untrack', { paths }),
  gitDiscard: (tracked: string[], untracked: string[]) => invoke<void>('git_discard', { tracked, untracked }),
  gitCommit: (message: string) => invoke<void>('git_commit', { message }),
  gitSync: (action: 'fetch' | 'pull' | 'push', prefer: Prefer | null = null) => invoke<void>('git_sync', { action, prefer }),
  gitPullPreview: () => invoke<PullPreview>('git_pull_preview'),
  listPacks: () => invoke<PackList>('list_packs'),
  discoverInstances: (root?: string) => invoke<InstanceInfo[]>('discover_instances', { root: root ?? null }),
  inspectInstance: (dir: string) => invoke<InstanceInfo>('inspect_instance', { dir }),
  setupInstance: (dir: string, options: SetupOptions) => invoke<void>('setup_instance', { dir, options }),
  joinPack: (dir: string, remote: string) => invoke<void>('join_pack', { dir, remote }),
  switchPack: (dir: string, killTests: boolean) => invoke<void>('switch_pack', { dir, killTests }),
  removePack: (dir: string) => invoke<void>('remove_pack', { dir }),
  updateHooks: () => invoke<void>('update_hooks'),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  updateStatus: () => invoke<UpdateView>('update_status'),
  checkForUpdate: () => invoke<UpdateView>('check_for_update'),
  installUpdate: () => invoke<void>('install_update'),
  setAutoUpdate: (enabled: boolean) => invoke<void>('set_auto_update', { enabled }),
  overview: () => invoke<Overview>('overview'),
  getPackConfig: () => invoke<PackConfig>('get_pack_config'),
  savePackConfig: (config: PackConfig) => invoke<void>('save_pack_config', { config }),
  getNotes: () => invoke<string>('get_notes'),
  saveNotes: (text: string) => invoke<void>('save_notes', { text }),
  changelogPreview: () => invoke<{ diff: ModDiff; markdown: string }>('changelog_preview'),
  setCompareBaseline: (zip: string) => invoke<void>('set_compare_baseline', { zip }),
  clearCompareBaseline: () => invoke<void>('clear_compare_baseline'),
  trackDirectly: (paths: string[]) => invoke<void>('track_directly', { paths }),
  revertMods: () => invoke<void>('revert_mods'),
  serverSides: () => invoke<ModSide[]>('server_sides'),
  setSideOverride: (projectId: number, mode: Override) => invoke<void>('set_side_override', { projectId, mode }),
  installedMods: () => invoke<{ mods: InstalledMod[]; locked: number[] }>('installed_mods'),
  checkModUpdates: () => invoke<ModCheck[]>('check_mod_updates'),
  modVersions: (path: string, projectId: number) => invoke<ModFile[]>('mod_versions', { path, projectId }),
  setUpdateChannel: (channel: ReleaseType) => invoke<void>('set_update_channel', { channel }),
  searchMods: (request: SearchRequest) => invoke<{ results: SearchResult[]; nextIndex: number | null }>('search_mods', { request }),
  modCategories: (kind: AddonKind) => invoke<{ id: number; name: string }[]>('mod_categories', { kind }),
  previewMods: (requests: ModRequest[]) => invoke<ModPreview>('preview_mods', { requests }),
  changeMods: (requests: ModRequest[]) => invoke<number>('change_mods', { requests }),
  cancelModChange: (id: number) => invoke<boolean>('cancel_mod_change', { id }),
  setVersion: (version: string) => invoke<void>('set_version', { version }),
  versionProblem: (version: string) => invoke<string | null>('version_problem', { version }),
  setUpVersionChecker: (version: string) => invoke<void>('set_up_version_checker', { version }),
  setModLocked: (projectId: number, locked: boolean) => invoke<void>('set_mod_locked', { projectId, locked }),
  setModsEnabled: (paths: string[], enabled: boolean) => invoke<string[]>('set_mods_enabled', { paths, enabled }),
  modChangelog: (projectId: number, fileId: number) => invoke<string>('mod_changelog', { projectId, fileId }),
  modDescription: (projectId: number) => invoke<string>('mod_description', { projectId }),
  removeMods: (paths: string[]) => invoke<void>('remove_mods', { paths }),
  publishPreflight: (version: string) =>
    invoke<{ problems: string[]; warnings: string[]; tagTaken: boolean; client: UploadNames; server: UploadNames }>('publish_preflight', { version }),
  buildClientExport: () => invoke<void>('build_client_export'),
  buildServerPack: () => invoke<void>('build_server_pack'),
  startTest: (recopyAll: boolean, connect: boolean) => invoke<void>('start_test', { recopyAll, connect }),
  startClient: (join: boolean) => invoke<void>('start_client', { join }),
  serverCommand: (line: string) => invoke<void>('server_command', { line }),
  killProcess: (which: 'server' | 'client') => invoke<void>('kill_process', { which }),
  publish: (options: { version: string; message: string; push: boolean; client: UploadNames; server: UploadNames }) => invoke<void>('publish', { options }),
  releases: () => invoke<ReleaseStatus[]>('releases'),
  outputDir: () => invoke<string>('output_dir'),
}

export function errorText(error: unknown): string {
  return typeof error === 'string' ? error : error instanceof Error ? error.message : JSON.stringify(error)
}
