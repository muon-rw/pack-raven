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
import { java } from '@codemirror/legacy-modes/mode/clike'
import { tags } from '@lezer/highlight'
import { forEachDiagnostic, lintKeymap } from '@codemirror/lint'
import { lintFor } from './lint'

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
  '&.cm-focused': { outline: '1px solid var(--forest)', outlineOffset: '-1px' },
  '.cm-gutters': { backgroundColor: 'var(--paper)', color: 'var(--muted)', borderRight: '1px solid var(--line)' },
  '.cm-activeLine': { backgroundColor: 'color-mix(in srgb, var(--here) 45%, transparent)' },
  '.cm-activeLineGutter': { backgroundColor: 'var(--here)', color: 'var(--text)' },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection': { backgroundColor: 'var(--highlight) !important' },
  '.cm-selectionMatch': { backgroundColor: 'var(--hit-soft)' },
  '.cm-matchingBracket': { backgroundColor: 'var(--ok-fill)', outline: '1px solid var(--ok-border)' },
  '.cm-searchMatch': { backgroundColor: 'var(--hit-soft)', outline: '1px solid var(--hit-edge)' },
  // The selected match is a solid marker in both themes, so we force dark ink over the syntax colors.
  '.cm-searchMatch.cm-searchMatch-selected, .cm-searchMatch.cm-searchMatch-selected *': { backgroundColor: 'var(--hit)', color: 'var(--hit-ink) !important' },
  '.cm-panels': { backgroundColor: 'var(--surface)', color: 'var(--text)', borderColor: 'var(--line)' },
  '.cm-panel input, .cm-panel button': { fontFamily: 'var(--font-ui)', fontSize: 'var(--step--1)' },
  // CodeMirror's base theme paints its own light buttons and fields, which leave light text
  // unreadable in dark mode, so we restyle them like the rest of the app.
  '.cm-textfield': { margin: '0', padding: '3px 8px', color: 'var(--text)', backgroundColor: 'var(--field)', border: '1px solid var(--edge)', borderRadius: 'var(--radius-control)' },
  '.cm-button': {
    margin: '0',
    padding: '3px 10px',
    fontWeight: '400',
    color: 'var(--text)',
    backgroundColor: 'var(--control)',
    backgroundImage: 'none',
    border: '1px solid var(--control-edge)',
    borderRadius: 'var(--radius-control)',
  },
  '.cm-button:hover': { backgroundColor: 'var(--control-hi)' },
  '.cm-button:active': { backgroundImage: 'none', backgroundColor: 'var(--here)' },
  // The search panel separates its find and replace rows with a <br>, which flex layout
  // ignores, so a full-width pseudo-element ordered between the rows breaks the line instead.
  '.cm-panel.cm-search': { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '6px', padding: '8px 40px 8px 10px' },
  '.cm-panel.cm-search::after': { content: '""', order: '1', flexBasis: '100%', height: '0', margin: '-3px 0' },
  '.cm-panel.cm-search br': { display: 'none' },
  '.cm-panel.cm-search [name=replace], .cm-panel.cm-search [name=replaceAll]': { order: '2' },
  '.cm-panel.cm-search input, .cm-panel.cm-search button, .cm-panel.cm-search label': { margin: '0' },
  '.cm-panel.cm-search .cm-textfield': { flex: '0 1 18em', minWidth: '8em' },
  '.cm-panel.cm-search label': { display: 'inline-flex', alignItems: 'center', gap: '4px', fontSize: 'var(--step--1)', color: 'var(--text)', cursor: 'pointer' },
  '.cm-panel.cm-search label:first-of-type': { marginLeft: '8px' },
  '.cm-panel.cm-search input[type=checkbox]': { margin: '0', accentColor: 'var(--forest)' },
  '.cm-panel.cm-search [name=close]': {
    top: '6px',
    right: '8px',
    width: '26px',
    height: '26px',
    fontSize: 'var(--step-1)',
    lineHeight: '1',
    color: 'var(--muted)',
    borderRadius: 'var(--radius-control)',
    cursor: 'pointer',
  },
  '.cm-panel.cm-search [name=close]:hover': { color: 'var(--text)', backgroundColor: 'var(--control)' },
  '.cm-foldGutter span': { color: 'var(--muted)' },
  '.cm-lintRange': { backgroundImage: 'none', textDecoration: 'underline wavy', textDecorationSkipInk: 'none', textUnderlineOffset: '3px' },
  '.cm-lintRange-error': { textDecorationColor: 'var(--fail-icon)' },
  '.cm-lintRange-warning': { textDecorationColor: 'var(--warn-icon)' },
  '.cm-lintRange-active': { backgroundColor: 'var(--hit-soft)' },
  '.cm-gutter-lint': { width: '1.2em' },
  '.cm-gutter-lint .cm-gutterElement': { display: 'flex', alignItems: 'center', justifyContent: 'center', padding: '0' },
  '.cm-lint-marker': { content: 'none', width: '8px', height: '8px', borderRadius: '50%' },
  '.cm-lint-marker-error': { backgroundColor: 'var(--fail-icon)' },
  '.cm-lint-marker-warning': { border: '2px solid var(--warn-icon)' },
  '.cm-tooltip': { backgroundColor: 'var(--surface)', color: 'var(--text)', border: '1px solid var(--line-strong)', borderRadius: 'var(--radius-control)', overflow: 'hidden' },
  '.cm-diagnostic': { fontFamily: 'var(--font-ui)', padding: '6px 10px 6px 8px' },
  '.cm-diagnostic-error': { borderLeft: '4px solid var(--fail-icon)' },
  '.cm-diagnostic-warning': { borderLeft: '4px solid var(--warn-icon)' },
  '.cm-panel.cm-panel-lint ul': { maxHeight: '140px' },
  '.cm-panel.cm-panel-lint ul [aria-selected]': { backgroundColor: 'var(--here)', color: 'var(--text)' },
  '.cm-panel.cm-panel-lint ul:focus [aria-selected]': { backgroundColor: 'var(--highlight)', color: 'var(--text)' },
})

// CodeMirror ignores typing in a read-only editor without a sign, so we report each attempt
// for the view to explain.
function blockedEdits(onBlocked: () => void) {
  const editKeys = new Set(['Backspace', 'Delete', 'Enter', 'Tab'])
  return EditorView.domEventHandlers({
    keydown: (event) => {
      const command = event.ctrlKey || event.metaKey
      const typing = event.key.length === 1 && !command && !event.altKey
      const editing = editKeys.has(event.key) || (command && ['v', 'x'].includes(event.key.toLowerCase()))
      if (typing || editing) onBlocked()
      return false
    },
    drop: () => (onBlocked(), false),
  })
}

// CodeMirror's default panel labels are lowercase shorthand like "all" and "regexp".
const phrases = EditorState.phrases.of({
  next: 'Next',
  previous: 'Previous',
  all: 'Select all',
  'match case': 'Match case',
  regexp: 'Regex',
  'by word': 'Whole word',
  replace: 'Replace',
  'replace all': 'Replace all',
  close: 'Close',
  'Go to line': 'Go to line',
  go: 'Go',
})

function language(path: string): Extension {
  const ext = path.slice(path.lastIndexOf('.') + 1).toLowerCase()
  switch (ext) {
    case 'json':
    case 'jsonc':
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
    // We show decompiled Java source for class files.
    case 'class':
    case 'java':
      return StreamLanguage.define(java)
    default:
      return []
  }
}

export interface Problems {
  errors: number
  warnings: number
}

function countProblems(state: EditorState): Problems {
  const problems = { errors: 0, warnings: 0 }
  forEachDiagnostic(state, (d) => {
    if (d.severity === 'error') problems.errors++
    else if (d.severity === 'warning') problems.warnings++
  })
  return problems
}

export interface EditorOptions {
  readOnly: boolean
  /** Leaves out the features that scan the whole file, for files over the size you set. */
  simplified: boolean
  onBlocked: () => void
  onLintPaused: () => void
}

export function createState(
  path: string,
  doc: string,
  onChange: (state: EditorState) => void,
  onSave: () => void,
  onProblems: (problems: Problems) => void,
  { readOnly, simplified, onBlocked, onLintPaused }: EditorOptions,
): EditorState {
  let reported = ''
  return EditorState.create({
    doc,
    extensions: [
      lineNumbers(),
      highlightActiveLineGutter(),
      highlightSpecialChars(),
      history(),
      simplified ? [] : foldGutter(),
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
      simplified ? [] : lintFor(path, onLintPaused),
      EditorState.readOnly.of(readOnly),
      readOnly ? blockedEdits(onBlocked) : [],
      theme,
      phrases,
      EditorView.contentAttributes.of({ 'aria-label': path.split(/[\\/]/).pop()! }),
      keymap.of([
        { key: 'Mod-s', preventDefault: true, run: () => (onSave(), true) },
        indentWithTab,
        ...closeBracketsKeymap,
        ...defaultKeymap,
        ...searchKeymap,
        ...historyKeymap,
        ...(simplified ? [] : foldKeymap),
        ...lintKeymap,
      ]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) onChange(update.state)
        const problems = countProblems(update.state)
        const key = `${problems.errors}/${problems.warnings}`
        if (key !== reported) {
          reported = key
          onProblems(problems)
        }
      }),
    ],
  })
}
