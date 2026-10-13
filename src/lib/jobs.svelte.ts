import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type JobName = 'client-export' | 'server-pack' | 'test' | 'server' | 'client' | 'publish' | 'setup' | 'git'
export type JobPhase = 'idle' | 'running' | 'done' | 'failed'

export interface LogLine {
  id: number
  stream: string
  text: string
}

class JobRecord {
  phase = $state<JobPhase>('idle')
  message = $state<string | null>(null)
  lines = $state.raw<LogLine[]>([])
}

const MAX_LINES = 4000
const FLUSH_MS = 50
let nextId = 0

export const jobs: Record<string, JobRecord> = $state({})
export const testState = $state({ serverReady: false, players: [] as string[] })

const idle = new JobRecord()
const pending = new Map<string, LogLine[]>()
let flushTimer: ReturnType<typeof setTimeout> | null = null

function record(job: string): JobRecord {
  jobs[job] ??= new JobRecord()
  return jobs[job]
}

export function job(name: JobName): JobRecord {
  return jobs[name] ?? idle
}

function flushLogs() {
  if (flushTimer) clearTimeout(flushTimer)
  flushTimer = null
  for (const [name, batch] of pending) {
    const entry = record(name)
    const merged = entry.lines.concat(batch)
    entry.lines = merged.length > MAX_LINES ? merged.slice(-MAX_LINES) : merged
  }
  pending.clear()
}

function queueLine(job: string, line: LogLine) {
  const batch = pending.get(job)
  if (batch) {
    batch.push(line)
    if (batch.length > MAX_LINES) batch.splice(0, batch.length - MAX_LINES)
  } else {
    pending.set(job, [line])
  }
  flushTimer ??= setTimeout(flushLogs, FLUSH_MS)
}

let listeners: Promise<UnlistenFn[]> | null = null

if (import.meta.hot) {
  import.meta.hot.dispose(async () => (await listeners)?.forEach((unlisten) => unlisten()))
}

export function startListening(): Promise<UnlistenFn[]> {
  listeners ??= register()
  return listeners
}

function register(): Promise<UnlistenFn[]> {
  return Promise.all([
  listen<{ job: string; stream: string; text: string }>('job-log', ({ payload }) => {
    queueLine(payload.job, { id: nextId++, stream: payload.stream, text: payload.text })
  }),
  listen<{ job: string; state: JobPhase; message: string | null }>('job-state', ({ payload }) => {
    const entry = record(payload.job)
    if (payload.state === 'running') {
      pending.delete(payload.job)
      entry.lines = []
    } else {
      flushLogs()
    }
    entry.phase = payload.state
    entry.message = payload.message
  }),
  listen<{ kind: string; detail: string }>('test-event', ({ payload }) => {
    if (payload.kind === 'server-started') {
      testState.serverReady = false
      testState.players = []
    } else if (payload.kind === 'server-ready') {
      testState.serverReady = true
    } else if (payload.kind === 'server-stopped') {
      testState.serverReady = false
      testState.players = []
    } else if (payload.kind === 'player-joined' && !testState.players.includes(payload.detail)) {
      testState.players.push(payload.detail)
    }
  }),
  ])
}
