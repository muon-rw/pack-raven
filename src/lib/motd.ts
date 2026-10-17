// Vanilla's chat colors, in code order. MOTD text starts out gray.
export const motdColors = [
  ['0', 'Black', '#000000'],
  ['1', 'Dark blue', '#0000aa'],
  ['2', 'Dark green', '#00aa00'],
  ['3', 'Dark aqua', '#00aaaa'],
  ['4', 'Dark red', '#aa0000'],
  ['5', 'Dark purple', '#aa00aa'],
  ['6', 'Gold', '#ffaa00'],
  ['7', 'Gray', '#aaaaaa'],
  ['8', 'Dark gray', '#555555'],
  ['9', 'Blue', '#5555ff'],
  ['a', 'Green', '#55ff55'],
  ['b', 'Aqua', '#55ffff'],
  ['c', 'Red', '#ff5555'],
  ['d', 'Light purple', '#ff55ff'],
  ['e', 'Yellow', '#ffff55'],
  ['f', 'White', '#ffffff'],
] as const

export const motdStyles = [
  ['l', 'Bold'],
  ['o', 'Italic'],
  ['n', 'Underline'],
  ['m', 'Strikethrough'],
  ['k', 'Obfuscated'],
  ['r', 'Reset'],
] as const

// We fill these in when we build a server pack, so the MOTD follows each release.
export const motdPlaceholders = [
  ['{name}', 'Pack name'],
  ['{version}', 'Version'],
  ['{minecraft}', 'Minecraft version'],
] as const

export const defaultMotd = '{name} {version}'

export function fillMotd(text: string, values: { name: string; version: string; minecraft: string }): string {
  return text.replaceAll('{name}', values.name).replaceAll('{version}', values.version).replaceAll('{minecraft}', values.minecraft)
}

export interface MotdRun {
  text: string
  color: string
  bold: boolean
  italic: boolean
  underline: boolean
  strike: boolean
  obfuscated: boolean
}

const gray = '#aaaaaa'
const plain = { color: gray, bold: false, italic: false, underline: false, strike: false, obfuscated: false }

// We keep line breaks as the two characters \n in packraven.toml, since that's how server.properties
// spells them, and pack authors often paste § escapes from MOTD generators.
export function motdFromProperty(value: string): string {
  return value.replace(/\\u00a7/gi, '§').replaceAll('\\n', '\n')
}

export function motdToProperty(text: string): string {
  return text.replaceAll('\r', '').replaceAll('\n', '\\n')
}

export function motdLines(text: string): MotdRun[][] {
  return text.split('\n').map((line) => {
    const runs: MotdRun[] = []
    let style = { ...plain }
    let rest = line
    while (rest) {
      const at = rest.indexOf('§')
      const text = at < 0 ? rest : rest.slice(0, at)
      if (text) runs.push({ text, ...style })
      if (at < 0) break
      const code = rest[at + 1]?.toLowerCase() ?? ''
      rest = rest.slice(at + 2)
      // Vanilla resets bold, italics and the rest whenever a color code starts.
      const color = motdColors.find(([c]) => c === code)
      if (color) style = { ...plain, color: color[2] }
      else if (code === 'r') style = { ...plain }
      else if (code === 'l') style.bold = true
      else if (code === 'o') style.italic = true
      else if (code === 'n') style.underline = true
      else if (code === 'm') style.strike = true
      else if (code === 'k') style.obfuscated = true
    }
    return runs
  })
}
