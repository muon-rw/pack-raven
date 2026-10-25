export type DiffRowKind = 'add' | 'del' | 'context' | 'hunk' | 'meta'

export type DiffRow = {
  /** The row's line in the raw diff text, which the backend uses to rebuild the patch. */
  index: number
  kind: DiffRowKind
  text: string
  oldNo: number | null
  newNo: number | null
  hunk: number
  noNewline: boolean
  /** The changed part of a line paired with a line on the other side, as [start, end). */
  changed: [number, number] | null
  /** For hunk rows, the lines the block covers, such as "Lines 133–170". */
  range: string | null
}

const TAB_SIZE = 4
const FILE_HEADER = /^(diff |index |--- |\+\+\+ )/
// A line that differs in more than this share of its length reads better as wholly changed.
const MAX_CHANGED_SHARE = 0.7

function columns(text: string) {
  let width = 0
  for (const c of text) width = c === '\t' ? width + TAB_SIZE - (width % TAB_SIZE) : width + 1
  return width
}

function rangeLabel(start: number, count: number) {
  if (count <= 0) return `After line ${start}`
  return count === 1 ? `Line ${start}` : `Lines ${start}–${start + count - 1}`
}

// We pair each removed line with the added line in the same place of the run that follows it,
// and mark what lies between their common start and end.
function markChanges(removed: DiffRow[], added: DiffRow[]) {
  for (let i = 0; i < Math.min(removed.length, added.length); i++) {
    const [a, b] = [removed[i].text, added[i].text]
    let start = 0
    while (start < a.length && start < b.length && a[start] === b[start]) start++
    let end = 0
    while (end < a.length - start && end < b.length - start && a[a.length - 1 - end] === b[b.length - 1 - end]) end++
    const longest = Math.max(a.length, b.length)
    if (!longest || (Math.max(a.length, b.length) - start - end) / longest > MAX_CHANGED_SHARE) continue
    removed[i].changed = [start, a.length - end]
    added[i].changed = [start, b.length - end]
  }
}

// We leave out git's file header, since the diff's own heading names the file, but keep lines
// such as "new file mode" that say something the hunks don't.
export function diffRows(text: string): { rows: DiffRow[]; width: number; lastLine: number } {
  const rows: DiffRow[] = []
  let oldNo = 0
  let newNo = 0
  let hunk = -1
  let width = 0
  let removed: DiffRow[] = []
  let added: DiffRow[] = []
  const endRun = () => {
    markChanges(removed, added)
    removed = []
    added = []
  }
  const row = (index: number, kind: DiffRowKind, text: string, extra: Partial<DiffRow> = {}): DiffRow => ({ index, kind, text, oldNo: null, newNo: null, hunk, noNewline: false, changed: null, range: null, ...extra })
  text.split('\n').forEach((line, index) => {
    if (line.startsWith('@@')) {
      endRun()
      const match = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@ ?(.*)$/.exec(line)
      oldNo = Number(match?.[1] ?? 0)
      newNo = Number(match?.[3] ?? 0)
      const newCount = Number(match?.[4] ?? 1)
      hunk++
      rows.push(row(index, 'hunk', match?.[5] ?? '', { range: newCount ? rangeLabel(newNo, newCount) : rangeLabel(oldNo, Number(match?.[2] ?? 1)) }))
      return
    }
    if (hunk < 0) {
      if (line && !FILE_HEADER.test(line)) rows.push(row(index, 'meta', line))
      return
    }
    const sign = line[0]
    if (sign === '\\') {
      if (rows.length) rows[rows.length - 1].noNewline = true
      return
    }
    if (sign !== '+' && sign !== '-' && sign !== ' ') return
    const body = line.slice(1).replace(/\r$/, '')
    width = Math.max(width, columns(body))
    if (sign === '-') {
      if (added.length) endRun()
      removed.push(row(index, 'del', body, { oldNo: oldNo++ }))
      rows.push(removed[removed.length - 1])
    } else if (sign === '+') {
      added.push(row(index, 'add', body, { newNo: newNo++ }))
      rows.push(added[added.length - 1])
    } else {
      endRun()
      rows.push(row(index, 'context', body, { oldNo: oldNo++, newNo: newNo++ }))
    }
  })
  endRun()
  return { rows, width, lastLine: Math.max(oldNo, newNo) }
}

export function isChange(row: DiffRow) {
  return row.kind === 'add' || row.kind === 'del'
}
