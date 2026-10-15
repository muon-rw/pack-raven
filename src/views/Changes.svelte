<script lang="ts">
  import { notOnCurseforgeSummary } from '../lib/modsync'
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import { api, errorText, type ModDiff, type Overview } from '../lib/api'
  import { shortcut } from '../lib/platform'
  import { nav } from '../lib/nav.svelte'
  import RenderedChangelog from '../components/RenderedChangelog.svelte'
  import LinkedText from '../components/LinkedText.svelte'

  let { overview, onpublish, onchange }: { overview: Overview | null; onpublish: () => void; onchange: () => void } = $props()

  let notes = $state('')
  let savedNotes = $state('')
  let diff = $state<ModDiff | null>(null)
  let diffError: string | null = $state(null)
  let loadingDiff = $state(false)
  let notesError: string | null = $state(null)
  let keys = $state<{ download: boolean; upload: boolean } | null>(null)

  onMount(async () => {
    api.credentialsStatus().then((c) => (keys = { download: c.download.present, upload: c.upload.present }), () => {})
    notes = savedNotes = await api.getNotes().catch(() => '')
    loadDiff()
  })

  // Before the first release we list what publishing needs here, where the work starts, rather
  // than only in Publish's checks at the end.
  let firstRelease = $derived(
    overview && !overview.lastTag && keys
      ? [
          { done: overview.projectId > 0, text: 'Set the CurseForge project ID', page: 'pack' as const, where: 'Pack settings' },
          { done: keys.download, text: 'Add a CurseForge API key', page: 'settings' as const, where: 'App settings' },
          { done: keys.upload, text: 'Add a CurseForge upload token', page: 'settings' as const, where: 'App settings' },
        ]
      : null,
  )

  async function loadDiff() {
    loadingDiff = true
    try {
      diff = (await api.changelogPreview()).diff
      diffError = null
    } catch (error) {
      diffError = errorText(error)
    } finally {
      loadingDiff = false
    }
  }

  async function compareWithZip() {
    const zip = await open({ title: 'Choose an older CurseForge export', filters: [{ name: 'CurseForge modpack', extensions: ['zip'] }] })
    if (typeof zip !== 'string') return
    try {
      await api.setCompareBaseline(zip)
      await loadDiff()
    } catch (error) {
      diffError = errorText(error)
    }
  }

  let unknownBusy = $state<'track' | 'trash' | null>(null)
  let unknownResult = $state<{ ok: boolean; text: string } | null>(null)

  async function resolveUnknown(action: 'track' | 'trash') {
    if (!overview) return
    const paths = overview.notOnCurseforge
    unknownBusy = action
    try {
      if (action === 'track') {
        await api.trackDirectly(paths)
        unknownResult = { ok: true, text: `Added ${paths.length === 1 ? 'it' : 'them'} to Git. Your next commit includes ${paths.length === 1 ? 'it' : 'them'}.` }
      } else {
        await api.trashPaths(paths)
        unknownResult = { ok: true, text: `Moved ${paths.length === 1 ? 'it' : 'them'} to the trash.` }
      }
      onchange()
      loadDiff()
    } catch (error) {
      unknownResult = { ok: false, text: errorText(error) }
    } finally {
      unknownBusy = null
    }
  }

  async function stopComparing() {
    await api.clearCompareBaseline().catch((error) => (diffError = errorText(error)))
    await loadDiff()
  }

  async function saveNotes() {
    if (notes === savedNotes) return
    const text = notes
    try {
      await api.saveNotes(text)
      savedNotes = text
      notesError = null
    } catch (error) {
      notesError = `Couldn't save the notes: ${errorText(error)}`
    }
  }

  type NotesMode = 'write' | 'split' | 'preview'

  const NOTES_MODES: { id: NotesMode; label: string }[] = [
    { id: 'write', label: 'Write' },
    { id: 'split', label: 'Split' },
    { id: 'preview', label: 'Preview' },
  ]
  const NOTES_MODE_KEY = 'packraven.notesMode'

  let notesMode = $state<NotesMode>(storedNotesMode())

  function storedNotesMode(): NotesMode {
    try {
      const stored = localStorage.getItem(NOTES_MODE_KEY)
      return NOTES_MODES.some((m) => m.id === stored) ? (stored as NotesMode) : 'write'
    } catch {
      return 'write'
    }
  }

  function setNotesMode(mode: NotesMode) {
    notesMode = mode
    try {
      localStorage.setItem(NOTES_MODE_KEY, mode)
    } catch {}
  }

  function onKey(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey)) return
    const key = event.key.toLowerCase()
    if (key === 's' && !event.shiftKey) {
      event.preventDefault()
      saveNotes()
    } else if (key === 'v' && event.shiftKey) {
      event.preventDefault()
      setNotesMode(NOTES_MODES[(NOTES_MODES.findIndex((m) => m.id === notesMode) + 1) % NOTES_MODES.length].id)
    }
  }

  let changeCount = $derived(diff ? diff.added.length + diff.updated.length + diff.removed.length : 0)
  let archivedNote = $derived(
    overview?.lastTag ? `packraven archived the notes for ${overview.lastTag} to changelogs/${overview.lastTag.replace(/^v/, '')}.md when you published it.` : null,
  )
  let readiness = $derived(
    [
      diff?.since ? `${changeCount || 'No'} mod ${changeCount === 1 ? 'change' : 'changes'} since ${diff.since}` : null,
      overview?.devJars.length ? `${overview.devJars.length} local dev ${overview.devJars.length === 1 ? 'build' : 'builds'} excluded` : null,
    ]
      .filter(Boolean)
      .map((part) => `${part}.`)
      .join(' '),
  )
</script>

<svelte:window onkeydown={onKey} />

<div class="view-changes">
  <header class="head">
    <div>
      <h2>Changes</h2>
      <p class="muted">{readiness || 'Checking the mods folder.'}</p>
    </div>
    <div class="released">
      <span class="muted small">Last release</span>
      <span class="file tag">{overview?.lastTag ?? 'None yet'}</span>
      <button class="link small" onclick={onpublish}>Choose the next version in Publish</button>
    </div>
  </header>

  {#if firstRelease && firstRelease.some((item) => !item.done)}
    <section class="first-release" aria-labelledby="first-release-title">
      <h3 id="first-release-title">Before your first release</h3>
      <ul>
        {#each firstRelease as item (item.text)}
          <li class:done={item.done}>
            <span class="mark" aria-hidden="true">{item.done ? '✓' : ''}</span>
            <span>
              {item.text}{item.done ? '' : ' in '}{#if !item.done}<button class="link" onclick={() => (nav.view = item.page)}>{item.where}</button>{/if}<span class="visually-hidden">{item.done ? ', done' : ', not done yet'}</span>
            </span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if overview?.notOnCurseforge.length}
    {@const paths = overview.notOnCurseforge}
    <Notice tone="warn">
      <div class="mod-list-notice">
        <p>
          <strong>{notOnCurseforgeSummary(paths)}.</strong> packraven only downloads mods from CurseForge, so releases and collaborators won't get
          {paths.length === 1 ? 'this file' : 'these files'} unless you commit {paths.length === 1 ? 'it' : 'them'} directly. You can't publish until you add
          {paths.length === 1 ? 'it' : 'them'} to Git or remove {paths.length === 1 ? 'it' : 'them'}.
        </p>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <ul class="folder-files" tabindex="0" aria-label="Files not on CurseForge">
          {#each paths as path (path)}<li class="file">{path}</li>{/each}
        </ul>
        <div class="actions">
          <button class:busy={unknownBusy === 'track'} disabled={!!unknownBusy} onclick={() => resolveUnknown('track')}>
            {unknownBusy === 'track' ? 'Adding' : 'Add to Git'}
          </button>
          <button class="quiet" class:busy={unknownBusy === 'trash'} disabled={!!unknownBusy} onclick={() => resolveUnknown('trash')}>
            {unknownBusy === 'trash' ? 'Moving' : 'Move to trash'}
          </button>
        </div>
      </div>
    </Notice>
  {/if}
  {#if overview?.modListError}
    <Notice tone="error"><p>Couldn't update the mod list: <LinkedText text={overview.modListError} /></p></Notice>
  {/if}
  {#if unknownResult}
    <p class:muted={unknownResult.ok} class:failed={!unknownResult.ok} role="status">
      <LinkedText text={unknownResult.text} />
    </p>
  {/if}
  {#if overview?.devJars.length}
    <Notice tone="warn">
      <p>Local dev builds are in the mods folder and won't be part of a release: <span class="file">{overview.devJars.join(', ')}</span></p>
    </Notice>
  {/if}

  <div class="columns" class:wide={notesMode === 'split'}>
    <div class="column">
      <div class="label-row">
        <label for="release-notes">Release notes <span class="file">changelog.md</span></label>
        <span class="modes" role="group" aria-label="Release notes view">
          {#each NOTES_MODES as mode (mode.id)}
            <button class="mode" aria-pressed={notesMode === mode.id} title="{mode.label} ({shortcut('Ctrl+Shift+V')} switches)" onclick={() => setNotesMode(mode.id)}>
              {mode.label}
            </button>
          {/each}
        </span>
      </div>
      <div class="notes" class:split={notesMode === 'split'}>
        {#if notesMode !== 'preview'}
          <textarea
            id="release-notes"
            bind:value={notes}
            onblur={saveNotes}
            rows="16"
            placeholder="What's new? Don't worry about mod changes, we'll add those automatically."
            aria-describedby="notes-status"
          ></textarea>
        {/if}
        {#if notesMode !== 'write'}
          {#if notes.trim()}
            <RenderedChangelog markdown={notes} label="Release notes preview" />
          {:else}
            <p class="muted empty-preview">
              {notesMode === 'split' ? 'Your notes show here as CurseForge will render them.' : 'No release notes yet. Switch to Write or Split to add some.'}
            </p>
          {/if}
        {/if}
      </div>
      <div class="row">
        <button onclick={saveNotes} disabled={notes === savedNotes}>Save notes <kbd aria-hidden="true">{shortcut('Ctrl+S')}</kbd></button>
        <span id="notes-status" class:muted={!notesError} class:failed={!!notesError} role="status">
          {notesError ?? (notes !== savedNotes ? 'Unsaved changes' : !notes && archivedNote ? archivedNote : '')}
        </span>
      </div>
    </div>

    <div class="column">
      <div class="row">
        <h3>{diff?.since ? `Changes since ${diff.since}` : 'Changes since the last release'}</h3>
        <button onclick={loadDiff} disabled={loadingDiff}>{loadingDiff ? 'Checking' : 'Refresh'}</button>
      </div>
      <p class="muted">
        {#if diff?.baseline}
          Compared with the export you chose. <button class="link" onclick={stopComparing}>Compare with the last release instead</button>
        {:else if diff?.since}
          packraven will add these automatically to your changelog. <button class="link" onclick={compareWithZip}>Compare with an older export</button>
        {:else}
          packraven will add these automatically to your changelog.
        {/if}
      </p>
      {#if diffError}
        <Notice tone="error"><p>{diffError}</p></Notice>
      {:else if diff && !diff.since}
        <p class="muted">
          No release yet, so there's nothing to compare with. To list changes for the first release, you can
          <button class="link" onclick={compareWithZip}>compare with an older export</button>.
        </p>
      {:else if diff && !changeCount}
        <p class="muted">No mods changed since {diff.since}.</p>
      {:else if diff}
        {#each [['Added', diff.added, 'ok'], ['Updated', diff.updated, 'change'], ['Removed', diff.removed, 'fail']] as const as [title, list, tone] (title)}
          {#if list.length}
            <section class="change-group" aria-label="{title} mods">
              <h4>{title} <span class="chip {tone}">{list.length}</span></h4>
              <ul class="release-changes">
                {#each list as change (change.name)}
                  <li>
                    {change.name}
                    {#if change.from}<span class="muted file">{change.from} to {change.to}</span>{/if}
                  </li>
                {/each}
              </ul>
            </section>
          {/if}
        {/each}
      {/if}
    </div>
  </div>
</div>

<style>
  .view-changes {
    display: grid;
    gap: 18px;
  }

  .head {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: end;
    gap: 16px 24px;
  }

  .head p {
    margin-top: 4px;
  }

  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr);
    gap: 40px;
    align-items: start;
    margin-top: 8px;
  }

  /* Split needs the width, so we stack Mod changes below the notes. */
  .columns.wide {
    grid-template-columns: minmax(0, 1fr);
  }

  @media (max-width: 1000px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .label-row {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 6px 12px;
  }

  .label-row label {
    display: flex;
    gap: 8px;
    align-items: baseline;
    color: var(--text);
    font-size: var(--step-0);
    font-weight: 700;
  }

  .notes {
    --changelog-max-height: 60vh;
    display: grid;
  }

  .notes.split {
    --changelog-max-height: none;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    grid-auto-rows: minmax(280px, 60vh);
    gap: 16px;
  }

  .notes.split textarea {
    height: 100%;
    resize: none;
  }

  .empty-preview {
    padding: 18px 22px;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius-panel);
  }

  .label-row .file {
    font-weight: 400;
    color: var(--muted);
  }

  .column {
    display: grid;
    gap: 16px;
  }

  .released {
    display: grid;
    justify-items: end;
    gap: 2px;
  }

  .released .tag {
    font-size: var(--step-1);
  }

  .small {
    font-size: var(--step--1);
  }

  .first-release {
    display: grid;
    gap: 8px;
    padding: 14px 18px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
  }

  .first-release h3 {
    font-size: var(--step-0);
  }

  .first-release ul {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .first-release li {
    display: flex;
    gap: 10px;
    align-items: baseline;
  }

  .first-release li.done {
    color: var(--muted);
  }

  .mark {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: 16px;
    height: 16px;
    font-size: var(--step--2);
    color: var(--forest);
    border: 1.5px solid var(--control-edge);
    border-radius: var(--radius-small);
    transform: translateY(2px);
  }

  .done .mark {
    border-color: var(--forest);
  }

  .notes textarea {
    width: 100%;
    min-height: 280px;
    font-size: var(--step-0);
    line-height: 1.6;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .change-group {
    display: grid;
    gap: 4px;
  }

  h4 {
    margin: 0;
    font-size: var(--step-0);
  }

  ul {
    margin: 4px 0 0;
    padding-left: 18px;
  }

  .change-group ul {
    margin: 0;
  }

  .release-changes li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 10px;
    padding: 2px 0;
  }

  .mod-list-notice {
    display: grid;
    gap: 10px;
  }

  .mod-list-notice .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .folder-files {
    display: grid;
    gap: 2px;
    max-height: 220px;
    overflow: auto;
    margin: 6px 0 0;
    padding: 0;
    list-style: none;
  }

</style>
