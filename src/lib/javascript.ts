import type { ParserOptions } from 'prettier'
import { format } from 'prettier/standalone'
import * as acorn from 'prettier/plugins/acorn'
import * as estree from 'prettier/plugins/estree'

export interface JsSyntaxError {
  line: number
  column: number
  message: string
}

type Node = { [key: string]: unknown }

export function parseJs(text: string): Node {
  // The acorn parser doesn't read any options; only Prettier's printer does.
  return acorn.parsers.acorn.parse(text, {} as ParserOptions) as Node
}

export function syntaxErrorOf(e: unknown): JsSyntaxError | null {
  if (!(e instanceof SyntaxError)) return null
  const at = (e as SyntaxError & { loc?: { start?: { line: number; column: number } } }).loc?.start
  if (!at) return null
  return { line: at.line, column: at.column, message: e.message.replace(/ \(\d+:\d+\)$/, '') }
}

// Positions (including Prettier's own __ fields), raw source text and comment layout change
// when we reformat, so we leave them out to compare only what the code does.
const LAYOUT_KEYS = new Set(['start', 'end', 'range', 'loc', 'raw', 'extra', 'comments'])

export function jsMeaning(text: string): unknown {
  return JSON.parse(JSON.stringify(parseJs(text), (key, value) => (LAYOUT_KEYS.has(key) || key.startsWith('__') ? undefined : typeof value === 'bigint' ? `${value}n` : value)))
}

export function formatJs(text: string, indent: string): Promise<string> {
  return format(text, {
    parser: 'acorn',
    plugins: [acorn, estree],
    useTabs: indent === '\t',
    tabWidth: indent === '\t' ? 4 : indent.length,
    printWidth: 100,
    singleQuote: prefersSingleQuotes(text),
    semi: /;[ \t]*(\/\/.*)?$/m.test(text),
    // Rhino, which runs KubeJS scripts, rejects a trailing comma after the last argument.
    trailingComma: 'es5',
    // Unquoting a key would turn a string literal into an identifier, which our meaning check
    // would read as a change.
    quoteProps: 'preserve',
    arrowParens: 'avoid',
  })
}

function prefersSingleQuotes(text: string): boolean {
  const single = text.match(/'(?:[^'\\\n]|\\.)*'/g)?.length ?? 0
  const double = text.match(/"(?:[^"\\\n]|\\.)*"/g)?.length ?? 0
  return single > double
}
