export type RenameMode = 'replace' | 'add' | 'number' | 'case'
export type NamePart = 'stem' | 'extension' | 'whole'
export type CaseStyle = 'lower' | 'upper' | 'title' | 'sentence'
export type CounterStyle = '1' | '01' | '001' | 'a' | 'A'
export type Separator = ' ' | '-' | '_' | '.' | '' | '()'
export type NumberOrder = 'name' | 'name-desc' | 'size' | 'size-desc'

export type RenameItem = { path: string; name: string; isDir: boolean; size: number }

/** A run of a name, marked when the rename removes it (in the old name) or adds it (in the new one). */
export type Span = { text: string; changed: boolean }

export type RenameRow = {
  item: RenameItem
  newName: string
  before: Span[]
  after: Span[]
  included: boolean
  /** Why we can't give the item its new name, if we can't. */
  problem: string | null
  /** A consequence worth a second look that doesn't stop the rename. */
  warning: string | null
}

const DEFAULTS = {
  mode: 'replace' as RenameMode,
  find: '',
  replacement: '',
  matchCase: false,
  regex: false,
  replacePart: 'stem' as NamePart,
  prefix: '',
  suffix: '',
  keepNames: false,
  baseName: '',
  counter: '1' as CounterStyle,
  start: 1,
  separator: ' ' as Separator,
  counterFirst: false,
  order: 'name' as NumberOrder,
  caseStyle: 'lower' as CaseStyle,
  casePart: 'stem' as NamePart,
}

// Like the text search, the dialog keeps your last settings for the next time you open it.
export const renameSettings = $state({ ...DEFAULTS })

export function resetRenameSettings() {
  Object.assign(renameSettings, DEFAULTS)
}

type Settings = typeof renameSettings
type Renamed = { name: string; before?: Span[]; after: Span[] }

const UNPORTABLE = /[<>:"/\\|?*\p{Cc}]/u
const RESERVED = /^(con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(\.|$)/i
const MAX_NAME_BYTES = 255
const utf8 = new TextEncoder()

/** Splits a file name before its last dot. Folders and dotfiles like .gitignore have no extension. */
export function splitName(name: string, isDir: boolean): [string, string] {
  const dot = name.lastIndexOf('.')
  return isDir || dot <= 0 ? [name, ''] : [name.slice(0, dot), name.slice(dot)]
}

function parentOf(path: string) {
  const index = path.lastIndexOf('/')
  return index < 0 ? '' : path.slice(0, index)
}

// Windows and macOS compare names case-insensitively, so we do too when checking for clashes.
function folded(name: string) {
  return name.toLowerCase()
}

function spans(...parts: (string | Span | Span[])[]): Span[] {
  const out: Span[] = []
  for (const part of parts.flat()) {
    const span = typeof part === 'string' ? { text: part, changed: false } : part
    if (!span.text) continue
    const last = out.at(-1)
    if (last && last.changed === span.changed) last.text += span.text
    else out.push({ ...span })
  }
  return out
}

function added(text: string): Span {
  return { text, changed: true }
}

function textOf(parts: Span[]) {
  return parts.map((s) => s.text).join('')
}

export function nameProblem(name: string) {
  if (!name.trim() || name === '.' || name === '..') return 'Empty name'
  const bad = name.match(UNPORTABLE)?.[0]
  if (bad) return bad.trim() ? `Can't contain ${bad}` : "Can't contain control characters"
  if (/[. ]$/.test(name)) return "Can't end with a dot or space"
  const reserved = name.match(RESERVED)?.[1]
  if (reserved) return `Windows reserves the name ${reserved.toUpperCase()}`
  if (utf8.encode(name).length > MAX_NAME_BYTES) return `Too long (over ${MAX_NAME_BYTES} bytes)`
  return null
}

function extensionWarning(item: RenameItem, newName: string) {
  if (item.isDir) return null
  const before = splitName(item.name, false)[1]
  const after = splitName(newName, false)[1]
  if (!before || before === after) return null
  return after ? `Extension changes from ${before} to ${after}` : `Loses its ${before} extension`
}

type PartChange = { before: Span[]; after: Span[] }

function applyToPart(item: RenameItem, part: NamePart, change: (text: string) => PartChange): Renamed {
  const wrap = (head: string, changed: PartChange, tail: string) => {
    const after = spans(head, changed.after, tail)
    return { name: textOf(after), before: spans(head, changed.before, tail), after }
  }
  if (part === 'whole') return wrap('', change(item.name), '')
  const [stem, extension] = splitName(item.name, item.isDir)
  if (part === 'stem') return wrap('', change(stem), extension)
  // Changing "" would let a regex like ^ add an extension to folders and dotfiles.
  return extension ? wrap(stem + '.', change(extension.slice(1)), '') : { name: item.name, after: spans(item.name) }
}

/** Fills in $1, $<name>, $& and $$ in a regex replacement the way String.replace does. */
function expand(replacement: string, match: RegExpExecArray) {
  return replacement.replace(/\$(\$|&|<([^>]*)>|(\d)(\d)?)/g, (token, kind: string, name?: string, first?: string, second?: string) => {
    if (kind === '$') return '$'
    if (kind === '&') return match[0]
    if (name !== undefined) return match.groups?.[name] ?? ''
    const two = second === undefined ? NaN : Number(first! + second)
    if (two > 0 && two < match.length) return match[two] ?? ''
    const one = Number(first)
    if (one > 0 && one < match.length) return (match[one] ?? '') + (second ?? '')
    return token
  })
}

function replaceMatches(text: string, pattern: RegExp, replace: (match: RegExpExecArray) => string): PartChange {
  const before: Span[] = []
  const after: Span[] = []
  let last = 0
  let previousEnd = -1
  for (const match of text.matchAll(pattern)) {
    const end = match.index + match[0].length
    // Like Rust's regex crate, we skip an empty match right after another match, so `.*`
    // replaces a name once rather than also adding the replacement at the end.
    if (match.index === end && match.index === previousEnd) continue
    const between = text.slice(last, match.index)
    before.push(...spans(between, added(match[0])))
    after.push(...spans(between, added(replace(match))))
    last = previousEnd = end
  }
  return { before: spans(before, text.slice(last)), after: spans(after, text.slice(last)) }
}

export function counterText(style: CounterStyle, value: number) {
  if (style === 'a' || style === 'A') {
    // Spreadsheet-style letters: a to z, then aa, ab and so on.
    let text = ''
    for (let n = Math.max(value, 1); n > 0; n = Math.floor((n - 1) / 26)) text = String.fromCharCode(97 + ((n - 1) % 26)) + text
    return style === 'A' ? text.toUpperCase() : text
  }
  return String(value).padStart(style.length, '0')
}

/** Returns the counter value of spreadsheet-style letters, or NaN for anything else. */
export function letterValue(text: string) {
  if (!/^[a-z]+$/i.test(text)) return NaN
  return [...text.toLowerCase()].reduce((value, c) => value * 26 + c.charCodeAt(0) - 96, 0)
}

export function usesLetters(style: CounterStyle) {
  return style === 'a' || style === 'A'
}

function changeCase(text: string, style: CaseStyle) {
  if (style === 'lower') return text.toLocaleLowerCase()
  if (style === 'upper') return text.toLocaleUpperCase()
  if (style === 'sentence') return text.toLocaleLowerCase().replace(/\p{L}/u, (c) => c.toLocaleUpperCase())
  // Mod file names are mostly CamelCase, so we only capitalize the first letter of each word
  // and leave the rest of it alone.
  return text.replace(/(^|[^\p{L}\p{N}'])(\p{L})/gu, (_, before: string, c: string) => before + c.toLocaleUpperCase())
}

function markChangedLetters(before: string, after: string): PartChange {
  if ([...before].length !== [...after].length) return { before: spans(before), after: spans(added(after)) }
  const a = [...before]
  return { before: spans(before), after: spans([...after].map((c, i) => (c === a[i] ? { text: c, changed: false } : added(c)))) }
}

function escapeRegex(text: string) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function regexError(e: unknown) {
  // Chromium and WebKit start the message with the whole pattern and its flags.
  const reason = (e as Error).message.replace(/^Invalid regular expression: \/.*\/[a-z]*: /s, '').replace(/^Invalid regular expression: /, '')
  return `That isn't a valid regex: ${reason.charAt(0).toLowerCase()}${reason.slice(1)}.`
}

/** Returns a function that gives an item its new name from its place in the list, or why the settings don't work. */
export function namer(settings: Settings): ((item: RenameItem, index: number) => Renamed) | { error: string } {
  switch (settings.mode) {
    case 'replace': {
      if (!settings.find) return (item) => ({ name: item.name, after: spans(item.name) })
      let pattern: RegExp
      try {
        pattern = new RegExp(settings.regex ? settings.find : escapeRegex(settings.find), `gu${settings.matchCase ? '' : 'i'}`)
      } catch (e) {
        return { error: regexError(e) }
      }
      // A plain replacement is literal text, so a $ in it stays a $.
      const replace = (match: RegExpExecArray) => (settings.regex ? expand(settings.replacement, match) : settings.replacement)
      return (item) => applyToPart(item, settings.replacePart, (text) => replaceMatches(text, pattern, replace))
    }
    case 'add':
      return (item) => {
        const [stem, extension] = splitName(item.name, item.isDir)
        const after = spans(added(settings.prefix), stem, added(settings.suffix), extension)
        return { name: textOf(after), after }
      }
    case 'number': {
      const start = Math.floor(settings.start)
      if (!Number.isFinite(start) || start < 0 || (start < 1 && usesLetters(settings.counter))) {
        return { error: usesLetters(settings.counter) ? 'Start the letters at a letter, like a or b.' : 'Start the counter at 0 or more.' }
      }
      return (item, index) => {
        const [stem, extension] = splitName(item.name, item.isDir)
        const base = settings.keepNames ? stem : settings.baseName
        // Only the counter differs from row to row, so that's the part we mark.
        const counter = added(counterText(settings.counter, start + index))
        let after: Span[]
        if (!base) after = spans(counter, extension)
        else if (settings.separator === '()') after = settings.counterFirst ? spans(added('('), counter, added(') '), base, extension) : spans(base, added(' ('), counter, added(')'), extension)
        else after = settings.counterFirst ? spans(counter, added(settings.separator), base, extension) : spans(base, added(settings.separator), counter, extension)
        return { name: textOf(after), after }
      }
    }
    case 'case':
      return (item) => applyToPart(item, settings.casePart, (text) => markChangedLetters(text, changeCase(text, settings.caseStyle)))
  }
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })

/** Puts the items in the order we number them in. */
export function ordered(items: RenameItem[], settings: Settings) {
  if (settings.mode !== 'number') return items
  const sorted = [...items]
  const [key, desc] = settings.order.split('-') as ['name' | 'size', string | undefined]
  sorted.sort((a, b) => (key === 'size' ? a.size - b.size : 0) || collator.compare(a.name, b.name))
  return desc ? sorted.reverse() : sorted
}

/**
 * Works out every item's new name and what stops it, given the names of everything already
 * in each item's folder. Settings that don't work leave every name as it is.
 */
export function plan(items: RenameItem[], excluded: Set<string>, settings: Settings, siblingsOf: (dir: string) => string[]) {
  const name = namer(settings)
  const error = typeof name === 'function' ? null : name.error
  let index = 0
  const rows: RenameRow[] = ordered(items, settings).map((item) => {
    const included = !excluded.has(item.path)
    const renamed = included && typeof name === 'function' ? name(item, index++) : { name: item.name, after: spans(item.name) }
    const changed = renamed.name !== item.name
    return {
      item,
      newName: renamed.name,
      before: changed ? (renamed.before ?? spans(item.name)) : spans(item.name),
      after: renamed.after,
      included,
      problem: null,
      warning: changed ? extensionWarning(item, renamed.name) : null,
    }
  })
  const renamed = rows.filter((r) => r.included && r.newName !== r.item.name)
  const moving = new Set(renamed.map((r) => r.item.path))
  // A folder ends up holding its untouched entries plus the new names, and two of those
  // matching would mean two entries with one name.
  const taken = new Map<string, Map<string, string>>()
  const namesIn = (dir: string) => {
    let names = taken.get(dir)
    if (!names) {
      names = new Map()
      for (const sibling of siblingsOf(dir)) {
        if (!moving.has(dir ? `${dir}/${sibling}` : sibling)) names.set(folded(sibling), sibling)
      }
      taken.set(dir, names)
    }
    return names
  }
  const claimed = new Map<string, RenameRow[]>()
  for (const row of renamed) {
    row.problem = nameProblem(row.newName)
    if (row.problem) continue
    const dir = parentOf(row.item.path)
    const existing = namesIn(dir).get(folded(row.newName))
    if (existing !== undefined) {
      row.problem = `${existing} already exists`
      continue
    }
    const key = `${dir}/${folded(row.newName)}`
    claimed.set(key, [...(claimed.get(key) ?? []), row])
  }
  for (const twins of claimed.values()) {
    if (twins.length < 2) continue
    for (const row of twins) row.problem = `${twins.length} items would be named ${row.newName}`
  }
  return { rows, error }
}
