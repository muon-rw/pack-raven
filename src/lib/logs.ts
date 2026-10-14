export type Level = 'error' | 'warn' | 'info' | 'debug' | 'none'

export interface Head {
  time: string
  thread: string
  level: string
  logger: string
  marker: string
  message: string
}

export interface Entry {
  start: number
  level: Level
  head: Head | null
}

// Minecraft and the loaders write "[time] [thread/LEVEL]", then optionally "[logger/MARKER]" (Forge
// and NeoForge) or "(logger)" (Fabric), then the message. A line that doesn't match, like a
// stack trace frame or a row of the mod list, continues the entry above it.
const HEADER = /^\[([^\]]+)\] \[(.+?)\/(TRACE|DEBUG|INFO|WARN|WARNING|ERROR|FATAL|SEVERE)\](?: \[([^\]]*)\])?(?: \(([^)]*)\))?:? ?(.*)$/

const LEVELS: Record<string, Level> = {
  TRACE: 'debug',
  DEBUG: 'debug',
  INFO: 'info',
  WARN: 'warn',
  WARNING: 'warn',
  ERROR: 'error',
  FATAL: 'error',
  SEVERE: 'error',
}

function parseHead(line: string): Head | null {
  const match = HEADER.exec(line)
  if (!match) return null
  const [, time, thread, level, bracketLogger, fabricLogger, message] = match
  // debug.log stamps the date too, which repeats on every line, so we keep the time.
  const clock = time.includes(' ') ? time.slice(time.lastIndexOf(' ') + 1) : time
  // Forge and NeoForge write "logger/MARKER", where the marker names a subsystem like SCAN.
  const [logger, marker = ''] = (bracketLogger ?? fabricLogger ?? '').split('/')
  return { time: clock, thread, level, logger, marker, message }
}

/** Holds a parsed log that can grow as Minecraft writes to it. */
export class LogModel {
  lines: string[] = []
  lineEntry: number[] = []
  entries: Entry[] = []
  counts: Record<Level, number> = { error: 0, warn: 0, info: 0, debug: 0, none: 0 }
  longest = 0

  clear() {
    this.lines = []
    this.lineEntry = []
    this.entries = []
    this.counts = { error: 0, warn: 0, info: 0, debug: 0, none: 0 }
    this.longest = 0
  }

  append(text: string) {
    const added = text.split('\n')
    if (added[added.length - 1] === '') added.pop()
    for (const raw of added) {
      const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw
      const head = parseHead(line)
      if (head || !this.entries.length) {
        const level = head ? LEVELS[head.level] : 'none'
        this.entries.push({ start: this.lines.length, level, head })
        this.counts[level]++
      }
      this.lineEntry.push(this.entries.length - 1)
      this.lines.push(line)
      this.longest = Math.max(this.longest, line.length)
    }
  }

  /** Returns the index one past the entry's last line. */
  entryEnd(entry: number) {
    return this.entries[entry + 1]?.start ?? this.lines.length
  }
}
