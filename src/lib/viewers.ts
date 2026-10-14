import { convertFileSrc } from '@tauri-apps/api/core'
import { api } from './api'

export type Viewer = 'image' | 'video' | 'audio' | 'nbt' | 'class' | 'archive' | 'compressed' | 'log'

const kinds: Record<string, Viewer> = {
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  gif: 'image',
  webp: 'image',
  bmp: 'image',
  ico: 'image',
  svg: 'image',
  mp4: 'video',
  webm: 'video',
  mov: 'video',
  m4v: 'video',
  ogv: 'video',
  ogg: 'audio',
  oga: 'audio',
  mp3: 'audio',
  wav: 'audio',
  flac: 'audio',
  m4a: 'audio',
  opus: 'audio',
  nbt: 'nbt',
  dat: 'nbt',
  dat_old: 'nbt',
  schem: 'nbt',
  schematic: 'nbt',
  litematic: 'nbt',
  class: 'class',
  jar: 'archive',
  zip: 'archive',
  gz: 'compressed',
}

export function extensionOf(path: string) {
  const name = path.slice(path.lastIndexOf('/') + 1)
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : ''
}

export function isGzip(path: string) {
  return extensionOf(path) === 'gz'
}

// Minecraft gzips each log when it starts a new one, so old logs end in .log.gz.
export function isLog(path: string) {
  return /\.log(\.gz)?$/i.test(path)
}

export function viewerFor(path: string): Viewer | null {
  return isLog(path) ? 'log' : (kinds[extensionOf(path)] ?? null)
}

/** Returns the pack-relative path of the jar or zip on disk that contains `path`. */
export function outerArchive(path: string) {
  const parts = path.split('/')
  const end = parts.findIndex((part) => /\.(jar|zip)$/i.test(part))
  return end < 0 ? path : parts.slice(0, end + 1).join('/')
}

const mimeTypes: Record<string, string> = { svg: 'image/svg+xml', ogg: 'audio/ogg', oga: 'audio/ogg', mp3: 'audio/mpeg', wav: 'audio/wav' }

// Files on disk stream through the asset protocol, which supports seeking in long videos.
// Files inside a jar have no path the webview can load, so we read their bytes instead.
export async function mediaUrl(path: string, archived: boolean): Promise<{ url: string; release: () => void }> {
  if (!archived) return { url: convertFileSrc(await api.mediaPath(path)), release: () => {} }
  const bytes = await api.readBytes(path)
  const url = URL.createObjectURL(new Blob([bytes], { type: mimeTypes[extensionOf(path)] ?? '' }))
  return { url, release: () => URL.revokeObjectURL(url) }
}
