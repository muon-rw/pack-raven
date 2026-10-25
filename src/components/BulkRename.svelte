<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import { api, errorText } from '../lib/api'
  import { shortcut } from '../lib/platform'
  import {
    counterText,
    letterValue,
    plan,
    renameSettings as settings,
    resetRenameSettings,
    usesLetters,
    type RenameItem,
    type RenameMode,
    type Span,
  } from '../lib/bulkRename.svelte'

  let {
    items,
    siblingsOf,
    onclose,
    ondone,
  }: {
    items: RenameItem[]
    siblingsOf: (dir: string) => string[]
    onclose: () => void
    ondone: (renamed: [string, string][]) => void
  } = $props()

  const MODES: { id: RenameMode; label: string }[] = [
    { id: 'replace', label: 'Find and replace' },
    { id: 'add', label: 'Add text' },
    { id: 'number', label: 'Number' },
    { id: 'case', label: 'Change case' },
  ]
  const ANNOUNCE_DELAY = 500

  let dialog: HTMLDialogElement
  let fields = $state<HTMLDivElement>()
  const excluded = new SvelteSet<string>()
  let busy = $state(false)
  let error = $state<string | null>(null)
  let letterStart = $state(counterText(settings.counter === 'A' ? 'A' : 'a', Math.max(settings.start, 1)))
  let announced = $state('')

  let result = $derived(plan(items, excluded, settings, siblingsOf))
  let rows = $derived(result.rows)
  let changing = $derived(rows.filter((r) => r.included && r.newName !== r.item.name))
  let problemRows = $derived(changing.filter((r) => r.problem))
  let warnings = $derived(changing.filter((r) => !r.problem && r.warning).length)
  let includedCount = $derived(rows.filter((r) => r.included).length)
  let manyFolders = $derived(new Set(items.map((i) => parentOf(i.path))).size > 1)
  let letters = $derived(usesLetters(settings.counter))
  let hasBase = $derived(settings.keepNames || !!settings.baseName)
  let canRename = $derived(!busy && !result.error && !problemRows.length && changing.length > 0)

  let summary = $derived.by(() => {
    if (result.error) return settings.mode === 'replace' ? 'Fix the regex to see the new names.' : 'Fix where the counter starts to see the new names.'
    if (!includedCount) return 'Tick at least one item to rename.'
    if (problemRows.length) {
      const n = problemRows.length
      return `${count(n, 'new name')} ${n === 1 ? 'has a problem' : 'have problems'}. Change the settings or skip ${n === 1 ? 'that item' : 'those items'}.`
    }
    if (!changing.length) return settings.mode === 'replace' && !settings.find ? 'Enter text to find.' : 'These settings leave every name as it is.'
    const unchanged = includedCount - changing.length
    const rest = unchanged ? `, and ${unchanged} will stay as ${unchanged === 1 ? 'it is' : 'they are'}` : ''
    const extensions = warnings ? ` ${count(warnings, 'item')} will get a different extension, which can stop Minecraft finding ${warnings === 1 ? 'it' : 'them'}.` : ''
    return `${count(changing.length, 'item')} will get a new name${rest}.${extensions}`
  })

  // Screen readers would read the summary on every keystroke, so we wait until you pause.
  $effect(() => {
    const text = summary
    const timer = setTimeout(() => (announced = text), ANNOUNCE_DELAY)
    return () => clearTimeout(timer)
  })

  onMount(() => {
    dialog.showModal()
    focusFirstField()
  })

  function parentOf(path: string) {
    const index = path.lastIndexOf('/')
    return index < 0 ? '' : path.slice(0, index)
  }

  function count(n: number, noun: string) {
    return `${n} ${noun}${n === 1 ? '' : 's'}`
  }

  async function focusFirstField() {
    await tick()
    fields?.querySelector<HTMLElement>('input:not([type="checkbox"]), select')?.focus()
  }

  // We read the key's position, since Option+1 on a Mac types ¡ rather than 1.
  function onKey(event: KeyboardEvent) {
    const index = Number(event.code.replace('Digit', '')) - 1
    if (!event.altKey || event.ctrlKey || event.metaKey || !MODES[index]) return
    event.preventDefault()
    settings.mode = MODES[index].id
    focusFirstField()
  }

  function chooseCounter(style: typeof settings.counter) {
    settings.counter = style
    if (usesLetters(style)) {
      settings.start = Math.max(settings.start, 1)
      letterStart = counterText(style, settings.start)
    }
  }

  function onLetterStart(text: string) {
    letterStart = text
    const value = letterValue(text)
    settings.start = Number.isNaN(value) ? -1 : value
  }

  function reset() {
    resetRenameSettings()
    excluded.clear()
    letterStart = 'a'
    focusFirstField()
  }

  function toggleAll(include: boolean) {
    if (include) excluded.clear()
    else for (const item of items) excluded.add(item.path)
  }

  function toggle(path: string, include: boolean) {
    if (include) excluded.delete(path)
    else excluded.add(path)
  }

  function skipProblems() {
    for (const row of problemRows) excluded.add(row.item.path)
  }

  async function submit() {
    if (!canRename) return
    busy = true
    error = null
    try {
      const renamed = await api.renamePaths(changing.map((r) => [r.item.path, r.newName]))
      dialog.close()
      ondone(renamed)
    } catch (e) {
      error = errorText(e)
      busy = false
    }
  }
</script>

{#snippet marked(parts: Span[], tag: 'del' | 'ins')}
  {#each parts as part, i (i)}{#if !part.changed}{part.text}{:else if tag === 'del'}<del>{part.text}</del>{:else}<ins>{part.text}</ins>{/if}{/each}
{/snippet}

{#snippet partSelect(id: string, value: 'stem' | 'extension' | 'whole', onchange: (value: 'stem' | 'extension' | 'whole') => void)}
  <div class="field">
    <label for={id}>Apply to</label>
    <select {id} {value} onchange={(e) => onchange(e.currentTarget.value as 'stem' | 'extension' | 'whole')}>
      <option value="stem">Names, not extensions</option>
      <option value="extension">Extensions only</option>
      <option value="whole">Names and extensions</option>
    </select>
  </div>
{/snippet}

<dialog bind:this={dialog} aria-labelledby="bulk-rename-title" onkeydown={onKey} oncancel={(e) => { e.preventDefault(); if (!busy) onclose() }}>
  <form aria-busy={busy} onsubmit={(e) => { e.preventDefault(); submit() }}>
    <header>
      <h2 id="bulk-rename-title">Rename {count(items.length, 'item')}</h2>
      <div class="modes" role="radiogroup" aria-label="How to rename">
        {#each MODES as mode, i (mode.id)}
          <label class="mode-option" title="{mode.label} ({shortcut(`Alt+${i + 1}`)})">
            <input type="radio" class="visually-hidden" name="rename-mode" value={mode.id} bind:group={settings.mode} aria-keyshortcuts="Alt+{i + 1}" />
            <span>{mode.label}</span>
          </label>
        {/each}
      </div>
    </header>

    <div class="fields" bind:this={fields}>
      {#if settings.mode === 'replace'}
        <div class="field grow">
          <label for="rename-find">Find</label>
          <input id="rename-find" bind:value={settings.find} spellcheck="false" class:mono={settings.regex}
            aria-invalid={!!result.error} aria-describedby={result.error ? 'rename-find-error' : undefined} />
        </div>
        <div class="field grow">
          <label for="rename-replacement">Replace with</label>
          <input id="rename-replacement" bind:value={settings.replacement} spellcheck="false" class:mono={settings.regex} />
        </div>
        {@render partSelect('rename-replace-part', settings.replacePart, (value) => (settings.replacePart = value))}
        {#if result.error}<p id="rename-find-error" class="field-error">{result.error}</p>{/if}
        <div class="toggles">
          <label class="check"><input type="checkbox" bind:checked={settings.matchCase} /> Match case</label>
          <label class="check"><input type="checkbox" bind:checked={settings.regex} /> Regular expression</label>
          <span class="hint">
            {settings.regex ? 'Use $1, $2 in the replacement for captured groups.' : 'Leave Replace with empty to remove the text.'}
          </span>
        </div>
      {:else if settings.mode === 'add'}
        <div class="field grow">
          <label for="rename-prefix">Add before the name</label>
          <input id="rename-prefix" bind:value={settings.prefix} spellcheck="false" />
        </div>
        <div class="field grow">
          <label for="rename-suffix">Add after the name</label>
          <input id="rename-suffix" bind:value={settings.suffix} spellcheck="false" />
        </div>
        <p class="hint row">We add text after the name and before the extension, so <span class="file">pack.zip</span> can become <span class="file">pack-old.zip</span>.</p>
      {:else if settings.mode === 'number'}
        <div class="field">
          <label for="rename-keep-names">Name</label>
          <select id="rename-keep-names" bind:value={settings.keepNames}>
            <option value={false}>One name for all</option>
            <option value={true}>Keep each name</option>
          </select>
        </div>
        {#if !settings.keepNames}
          <div class="field grow">
            <label for="rename-base-name">New name</label>
            <input id="rename-base-name" bind:value={settings.baseName} spellcheck="false" placeholder="Leave empty for just the counter" />
          </div>
        {/if}
        <div class="field">
          <label for="rename-order">Number in order of</label>
          <select id="rename-order" bind:value={settings.order}>
            <option value="name">Name, A to Z</option>
            <option value="name-desc">Name, Z to A</option>
            <option value="size">Size, smallest first</option>
            <option value="size-desc">Size, largest first</option>
          </select>
        </div>
        <div class="row-break"></div>
        <div class="field">
          <label for="rename-counter">Counter</label>
          <select id="rename-counter" value={settings.counter} onchange={(e) => chooseCounter(e.currentTarget.value as typeof settings.counter)}>
            <option value="1">1, 2, 3</option>
            <option value="01">01, 02, 03</option>
            <option value="001">001, 002, 003</option>
            <option value="a">a, b, c</option>
            <option value="A">A, B, C</option>
          </select>
        </div>
        <div class="field">
          <label for="rename-start">Start at</label>
          {#if letters}
            <input id="rename-start" class="start" value={letterStart} spellcheck="false" autocomplete="off"
              oninput={(e) => onLetterStart(e.currentTarget.value)} aria-invalid={!!result.error} aria-describedby={result.error ? 'rename-start-error' : undefined} />
          {:else}
            <input id="rename-start" type="number" class="start" min="0" bind:value={settings.start}
              aria-invalid={!!result.error} aria-describedby={result.error ? 'rename-start-error' : undefined} />
          {/if}
        </div>
        {#if hasBase}
          <div class="field">
            <label for="rename-separator">Between name and counter</label>
            <select id="rename-separator" bind:value={settings.separator}>
              <option value=" ">Space</option>
              <option value="-">Hyphen -</option>
              <option value="_">Underscore _</option>
              <option value=".">Dot .</option>
              <option value="()">Parentheses ( )</option>
              <option value="">Nothing</option>
            </select>
          </div>
          <div class="field">
            <label for="rename-counter-place">Counter goes</label>
            <select id="rename-counter-place" bind:value={settings.counterFirst}>
              <option value={false}>After the name</option>
              <option value={true}>Before the name</option>
            </select>
          </div>
        {/if}
        {#if result.error}<p id="rename-start-error" class="field-error">{result.error}</p>{/if}
      {:else}
        <div class="field">
          <label for="rename-case-style">Change to</label>
          <select id="rename-case-style" bind:value={settings.caseStyle}>
            <option value="lower">lowercase</option>
            <option value="upper">UPPERCASE</option>
            <option value="title">Capitalize Each Word</option>
            <option value="sentence">Sentence case</option>
          </select>
        </div>
        {@render partSelect('rename-case-part', settings.casePart, (value) => (settings.casePart = value))}
        <p class="hint row">
          {settings.caseStyle === 'title'
            ? 'We capitalize the first letter of each word and leave the rest as it is, so AetherCompat stays AetherCompat.'
            : settings.caseStyle === 'sentence'
              ? 'We lowercase everything, then capitalize the first letter.'
              : ''}
        </p>
      {/if}
    </div>

    <div class="preview" role="region" aria-label="Preview">
      <table>
        <thead>
          <tr>
            <th class="pick">
              <input
                type="checkbox"
                aria-label="Rename all"
                checked={includedCount === items.length}
                indeterminate={includedCount > 0 && includedCount < items.length}
                onchange={(e) => toggleAll(e.currentTarget.checked)}
              />
            </th>
            <th>Current name</th>
            <th>New name</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as row, i (row.item.path)}
            {@const same = row.newName === row.item.name}
            {@const note = row.problem ?? row.warning}
            <tr class:off={!row.included} class:invalid={!!row.problem}>
              <td class="pick">
                <input type="checkbox" aria-label="Rename {row.item.name}" aria-describedby={note ? `rename-note-${i}` : undefined}
                  checked={row.included} onchange={(e) => toggle(row.item.path, e.currentTarget.checked)} />
              </td>
              <td class="old" title={row.item.path}>
                <span class="name" class:dir={row.item.isDir}>{@render marked(row.before, 'del')}</span>
                {#if manyFolders}<span class="where"><span class="visually-hidden">in </span>{parentOf(row.item.path) || 'the pack folder'}</span>{/if}
              </td>
              <td class="new">
                {#if !row.included}
                  <span class="muted">Skipped</span>
                {:else if same}
                  <span class="muted">No change</span>
                {:else}
                  <span class="name" class:dir={row.item.isDir}>{@render marked(row.after, 'ins')}</span>
                  {#if note}<span id="rename-note-{i}" class="note" class:problem={!!row.problem}>{note}</span>{/if}
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <p class="summary" class:failed={!!result.error || problemRows.length > 0}>
      {summary}
      {#if problemRows.length}<button type="button" class="link" onclick={skipProblems}>Skip {problemRows.length === 1 ? 'it' : 'them'}</button>{/if}
    </p>
    <p class="visually-hidden" role="status">{announced}</p>
    {#if error}<p class="failed" role="alert">{error}</p>{/if}

    <div class="actions">
      <button type="button" class="quiet reset" onclick={reset} disabled={busy}>Reset settings</button>
      <span class="undo">You can undo with {shortcut('Ctrl+Z')} in the file tree.</span>
      <button type="button" class="quiet" onclick={onclose} disabled={busy}>Cancel</button>
      <button type="submit" class="primary" class:busy disabled={!canRename}>
        {canRename && changing.length > 1 ? `Rename ${changing.length} items` : 'Rename'}
      </button>
    </div>
  </form>
</dialog>

<style>
  dialog {
    width: min(760px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    padding: 0;
    color: var(--text);
    background: var(--paper);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-panel);
    box-shadow: 0 12px 32px var(--shadow);
  }

  dialog[open] {
    display: flex;
  }

  dialog::backdrop {
    background: var(--shadow);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
    min-height: 0;
    min-width: 0;
    padding: 18px 20px;
  }

  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 16px;
  }

  h2 {
    font-size: var(--step-1);
  }

  .mode-option span {
    display: block;
    padding: 1px 10px;
    font-size: var(--step--1);
    color: var(--muted);
    border: 1px solid transparent;
    border-radius: var(--radius-small);
    cursor: pointer;
  }

  .mode-option:hover span {
    color: var(--text);
  }

  .mode-option input:checked + span {
    color: var(--text);
    background: var(--field);
    border-color: var(--line-strong);
  }

  .mode-option input:focus-visible + span {
    outline: 2px solid var(--forest);
    outline-offset: 1px;
  }

  .fields {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 10px 12px;
  }

  .field {
    display: grid;
    gap: 3px;
    min-width: 0;
    font-size: var(--step--1);
    color: var(--muted);
  }

  .field.grow {
    flex: 1 1 200px;
  }

  .field input,
  .field select {
    padding-block: 5px;
    font-size: var(--step--1);
    color: var(--text);
  }

  .field input[aria-invalid='true'] {
    border-color: var(--fail);
  }

  .row-break,
  .row,
  .field-error,
  .toggles {
    flex-basis: 100%;
  }

  .row-break {
    height: 0;
  }

  .field-error {
    margin-top: -4px;
    font-size: var(--step--2);
    color: var(--fail);
  }

  .mono {
    font-family: var(--font-log);
  }

  .start {
    width: 6em;
  }

  .toggles {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 16px;
  }

  .toggles .check {
    font-size: var(--step--1);
  }

  .hint {
    font-size: var(--step--2);
    color: var(--muted);
  }

  .hint:empty {
    display: none;
  }

  .hint .file {
    font-size: inherit;
  }

  .preview {
    flex: 1 1 auto;
    min-height: 96px;
    overflow: auto;
    background: var(--field);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--step--1);
  }

  th {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 5px 8px;
    text-align: left;
    font-size: var(--step--2);
    font-weight: 600;
    color: var(--muted);
    background: var(--surface);
    border-bottom: 1px solid var(--line);
  }

  td {
    padding: 3px 8px;
    vertical-align: baseline;
    border-bottom: 1px solid var(--line);
    overflow-wrap: anywhere;
  }

  tr:last-child td {
    border-bottom: 0;
  }

  .pick {
    width: 1px;
    padding-right: 0;
  }

  .pick input {
    display: block;
    margin: 0;
  }

  td.old,
  td.new {
    width: 50%;
  }

  tr.off td {
    background: var(--surface);
  }

  .name.dir {
    font-weight: 600;
  }

  .old .name {
    color: var(--muted);
  }

  .where {
    display: block;
    font-size: var(--step--2);
    color: var(--muted);
  }

  del {
    color: var(--fail);
    background: var(--fail-fill);
    border-radius: 2px;
  }

  ins {
    color: var(--forest-hi);
    background: var(--ok-fill);
    outline: 1px solid var(--ok-border);
    border-radius: 2px;
    text-decoration: none;
  }

  tr.invalid .new .name,
  tr.invalid ins {
    color: var(--fail);
  }

  tr.invalid ins {
    background: var(--fail-fill);
    outline-color: var(--fail-border);
  }

  .note {
    display: block;
    font-size: var(--step--2);
    color: var(--warn);
  }

  .note.problem {
    color: var(--fail);
  }

  .summary {
    font-size: var(--step--1);
  }

  .summary .link {
    margin-left: 4px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: end;
    gap: 6px;
  }

  .reset {
    margin-right: auto;
  }

  .undo {
    flex: 0 1 auto;
    margin-right: 8px;
    text-align: end;
    font-size: var(--step--2);
    color: var(--muted);
  }
</style>
