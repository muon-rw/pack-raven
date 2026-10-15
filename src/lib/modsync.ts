import type { FolderChanges } from './api'

export function folderChangeCount(folder: FolderChanges): number {
  return folder.updated.length + folder.added.length + folder.missing.length
}

// "The mod list" is mods.lock.json. These counts compare the mods folder with it, never with a
// release, so we keep "since v1.2.0" wording for the release diff alone.
export function modListSummary(folder: FolderChanges): string {
  const parts = [
    folder.updated.length ? `${folder.updated.length} updated` : null,
    folder.added.length ? `${folder.added.length} new` : null,
    folder.missing.length ? `${folder.missing.length} missing` : null,
  ].filter(Boolean)
  return parts.length ? `Mods folder differs from packraven\'s mod list: ${parts.join(', ')}` : 'Mods folder matches packraven\'s mod list'
}
