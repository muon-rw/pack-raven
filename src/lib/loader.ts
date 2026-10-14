const NAMES: Record<string, string> = { neoforge: 'NeoForge', forge: 'Forge', fabric: 'Fabric', quilt: 'Quilt' }

// CurseForge IDs loaders as neoforge-21.1.259, so we show NeoForge 21.1.259 instead.
export function loaderName(loader: string | null | undefined): string {
  if (!loader) return ''
  const match = /^([a-z]+)-(.+)$/i.exec(loader)
  if (!match) return NAMES[loader.toLowerCase()] ?? loader
  return `${NAMES[match[1].toLowerCase()] ?? match[1]} ${match[2]}`
}
