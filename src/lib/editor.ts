import { EditorState, type Extension } from '@codemirror/state'
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection, highlightSpecialChars, rectangularSelection, crosshairCursor, dropCursor } from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { bracketMatching, foldGutter, foldKeymap, indentOnInput, syntaxHighlighting, HighlightStyle, StreamLanguage } from '@codemirror/language'
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search'
import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete'
import { json } from '@codemirror/lang-json'
import { javascript } from '@codemirror/lang-javascript'
import { markdown } from '@codemirror/lang-markdown'
import { yaml } from '@codemirror/lang-yaml'
import { toml } from '@codemirror/legacy-modes/mode/toml'
import { properties } from '@codemirror/legacy-modes/mode/properties'
import { tags } from '@lezer/highlight'

const highlight = HighlightStyle.define([
  { tag: [tags.keyword, tags.bool, tags.null, tags.atom], color: 'var(--forest)', fontWeight: '700' },
  { tag: [tags.string, tags.special(tags.string)], color: 'var(--ochre-ink)' },
  { tag: [tags.number, tags.integer, tags.float], color: 'var(--warn)' },
  { tag: [tags.comment, tags.lineComment, tags.blockComment], color: 'var(--muted)', fontStyle: 'italic' },
  { tag: [tags.propertyName, tags.definition(tags.propertyName), tags.attributeName], color: 'var(--text)', fontWeight: '600' },
  { tag: [tags.heading, tags.strong], fontWeight: '700' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: [tags.link, tags.url], color: 'var(--forest)', textDecoration: 'underline' },
  { tag: [tags.bracket, tags.punctuation, tags.separator], color: 'var(--muted)' },
  { tag: tags.invalid, color: 'var(--fail)' },
])

const theme = EditorView.theme({
  '&': { height: '100%', backgroundColor: 'var(--field)', color: 'var(--text)', fontSize: 'var(--step--1)' },
  '.cm-scroller': { fontFamily: 'var(--font-log)', lineHeight: '1.55' },
  '.cm-content': { caretColor: 'var(--forest)', padding: '8px 0' },
  '.cm-cursor': { borderLeftColor: 'var(--forest)', borderLeftWidth: '2px' },
  '&.cm-focused': { outline: 'none' },
  '.cm-gutters': { backgroundColor: 'var(--paper)', color: 'var(--muted)', borderRight: '1px solid var(--line)' },
  '.cm-activeLine': { backgroundColor: 'color-mix(in srgb, var(--here) 45%, transparent)' },
  '.cm-activeLineGutter': { backgroundColor: 'var(--here)', color: 'var(--text)' },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection': { backgroundColor: 'var(--highlight) !important' },
  '.cm-selectionMatch': { backgroundColor: 'color-mix(in srgb, var(--ochre) 35%, transparent)' },
  '.cm-matchingBracket': { backgroundColor: 'var(--ok-fill)', outline: '1px solid var(--ok-border)' },
  '.cm-searchMatch': { backgroundColor: 'color-mix(in srgb, var(--ochre) 45%, transparent)' },
  '.cm-searchMatch.cm-searchMatch-selected': { backgroundColor: 'var(--ochre)' },
  '.cm-panels': { backgroundColor: 'var(--surface)', color: 'var(--text)', borderColor: 'var(--line)' },
  '.cm-panel input, .cm-panel button': { fontFamily: 'var(--font-ui)', fontSize: 'var(--step--1)' },
  '.cm-foldGutter span': { color: 'var(--muted)' },
})

function language(path: string): Extension {
  const ext = path.slice(path.lastIndexOf('.') + 1).toLowerCase()
  switch (ext) {
    case 'json':
    case 'json5':
    case 'mcmeta':
      return json()
    case 'toml':
      return StreamLanguage.define(toml)
    case 'properties':
    case 'cfg':
    case 'ini':
    case 'conf':
      return StreamLanguage.define(properties)
    case 'yml':
    case 'yaml':
      return yaml()
    case 'js':
    case 'mjs':
    case 'ts':
      return javascript({ typescript: ext === 'ts' })
    case 'md':
      return markdown()
    default:
      return []
  }
}

export function createState(path: string, doc: string, onChange: (state: EditorState) => void, onSave: () => void): EditorState {
  return EditorState.create({
    doc,
    extensions: [
      lineNumbers(),
      highlightActiveLineGutter(),
      highlightSpecialChars(),
      history(),
      foldGutter(),
      drawSelection(),
      dropCursor(),
      EditorState.allowMultipleSelections.of(true),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      rectangularSelection(),
      crosshairCursor(),
      highlightActiveLine(),
      highlightSelectionMatches(),
      syntaxHighlighting(highlight),
      language(path),
      theme,
      keymap.of([
        { key: 'Mod-s', preventDefault: true, run: () => (onSave(), true) },
        indentWithTab,
        ...closeBracketsKeymap,
        ...defaultKeymap,
        ...searchKeymap,
        ...historyKeymap,
        ...foldKeymap,
      ]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) onChange(update.state)
      }),
    ],
  })
}
