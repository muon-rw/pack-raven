<script lang="ts">
  import { modSyncSummary } from '../lib/modsync'
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import { api, errorText, type ModDiff, type Overview } from '../lib/api'

  let { overview, onchange }: { overview: Overview | null; onchange: () => void } = $props()

  let version = $state('')
  let notes = $state('')
  let savedNotes = $state('')
  let diff = $state<ModDiff | null>(null)
  let diffError: string | null = $state(null)
  let loadingDiff = $state(false)
  let message: string | null = $state(null)
  let messageFailed = $state(false)
  let notesError: string | null = $state(null)

  let versionTouched = false

  $effect(() => {
    if (!versionTouched && overview?.version) version = overview.version
  })

  onMount(async () => {
    notes = savedNotes = await api.getNotes().catch(() => '')
    loadDiff()
  })

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

  async function stopComparing() {
    await api.clearCompareBaseline().catch((error) => (diffError = errorText(error)))
    await loadDiff()
  }

  async function saveVersion() {
    try {
      await api.setVersion(version)
      messageFailed = false
      message = `Version set to ${version.trim()}`
      onchange()
    } catch (error) {
      messageFailed = true
      message = `Couldn't set the version: ${errorText(error)}`
    }
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

  function onKey(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
      event.preventDefault()
      saveNotes()
    }
  }

  let changeCount = $derived(diff ? diff.added.length + diff.updated.length + diff.removed.length : 0)
  let outOfSync = $derived(overview ? overview.missing.length + overview.unlocked.length : 0)
  let archivedNote = $derived(
    overview?.lastTag ? `Notes for ${overview.lastTag} were archived to changelogs/${overview.lastTag.replace(/^v/, '')}.md when it was published.` : null,
  )
  let readiness = $derived(
    [
      overview ? modSyncSummary(overview) : null,
      diff?.since ? `${changeCount || 'No'} mod ${changeCount === 1 ? 'change' : 'changes'} since ${diff.since}` : null,
      overview?.devJars.length ? `${overview.devJars.length} local dev ${overview.devJars.length === 1 ? 'build' : 'builds'} excluded` : null,
    ]
      .filter(Boolean)
      .join('. ') + '.',
  )
</script>

<svelte:window onkeydown={onKey} />

<div class="prepare">
  <header class="head">
    <div>
      <h2>Prepare <span class="version-tag">{overview?.version ?? 'unversioned'}</span></h2>
      <p class="muted">{readiness}</p>
    </div>
    <form class="version" onsubmit={(e) => { e.preventDefault(); saveVersion() }}>
      <label>
        Next version
        <input bind:value={version} oninput={() => (versionTouched = true)} placeholder="0.2.0" />
      </label>
      <button type="submit" disabled={!version.trim() || version.trim() === overview?.version}>Set version</button>
    </form>
  </header>
  {#if message}<p class:muted={!messageFailed} class:failed={messageFailed} role="status">{message}</p>{/if}

  {#if overview && outOfSync}
    <Notice tone="warn">
      <p>Your mods folder has changes that aren't committed yet. Commit to save them, or pull to get back the committed set.</p>
      <details>
        <summary>Show {outOfSync} {outOfSync === 1 ? 'file' : 'files'}</summary>
        {#if overview.unlocked.length}
          <p class="muted">Not committed yet:</p>
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <ul tabindex="0" aria-label="Files not committed yet">{#each overview.unlocked as path (path)}<li class="file">{path}</li>{/each}</ul>
        {/if}
        {#if overview.missing.length}
          <p class="muted">Missing or changed, will download on pull:</p>
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <ul tabindex="0" aria-label="Files missing or changed on disk">{#each overview.missing as path (path)}<li class="file">{path}</li>{/each}</ul>
        {/if}
      </details>
    </Notice>
  {/if}
  {#if overview?.devJars.length}
    <Notice tone="warn">
      <p>Local dev builds are in the mods folder and won't be part of a release: <span class="file">{overview.devJars.join(', ')}</span></p>
    </Notice>
  {/if}

  <div class="columns">
    <div class="column">
      <label class="notes">
        <span class="label-row">Release notes <span class="file">changelog.md</span></span>
        <textarea bind:value={notes} onblur={saveNotes} rows="16" placeholder="What changed in this release, in Markdown." aria-describedby="notes-status"></textarea>
      </label>
      <div class="row">
        <button onclick={saveNotes} disabled={notes === savedNotes}>Save notes <kbd>Ctrl S</kbd></button>
        <span id="notes-status" class:muted={!notesError} class:failed={!!notesError} role="status">
          {notesError ?? (notes !== savedNotes ? 'Unsaved changes' : !notes && archivedNote ? archivedNote : '')}
        </span>
      </div>
    </div>

    <div class="column">
      <div class="row">
        <h3>Mod changes{diff?.since ? ` since ${diff.since}` : ''}</h3>
        <button onclick={loadDiff} disabled={loadingDiff}>{loadingDiff ? 'Checking' : 'Refresh'}</button>
      </div>
      <p class="muted">
        Added to the end of the release notes when you publish.
        {#if diff?.baseline}<button class="link" onclick={stopComparing}>Stop comparing with {diff.since}</button>{/if}
      </p>
      {#if diffError}
        <Notice tone="error"><p>{diffError}</p></Notice>
      {:else if diff && !diff.since}
        <p class="muted">
          No previous release tags; nothing to compare against. If you'd like, you can
          <button class="link" onclick={compareWithZip}>compare changes against an old version</button>.
        </p>
      {:else if diff && !changeCount}
        <p class="muted">No mods changed since {diff.since}.</p>
      {:else if diff}
        {#each [['Added', diff.added, 'ok'], ['Updated', diff.updated, 'change'], ['Removed', diff.removed, 'fail']] as const as [title, list, tone] (title)}
          {#if list.length}
            <h4>{title} <span class="chip {tone}">{list.length}</span></h4>
            <ul class="changes">
              {#each list as change (change.name)}
                <li>
                  {change.name}
                  {#if change.from}<span class="muted file">{change.from} to {change.to}</span>{/if}
                </li>
              {/each}
            </ul>
          {/if}
        {/each}
      {/if}
    </div>
  </div>
</div>

<style>
  .prepare {
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

  .version-tag {
    font-family: var(--font-log);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: var(--forest);
  }

  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr);
    gap: 40px;
    align-items: start;
    margin-top: 8px;
  }

  @media (max-width: 1000px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .label-row {
    display: flex;
    gap: 8px;
    align-items: baseline;
    color: var(--text);
    font-weight: 700;
  }

  .label-row .file {
    font-weight: 400;
    color: var(--muted);
  }

  summary {
    cursor: pointer;
    color: var(--text);
  }

  details[open] summary {
    margin-bottom: 6px;
  }

  .column {
    display: grid;
    gap: 16px;
  }

  .version {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 12px;
  }

  .version input {
    width: 140px;
    font-family: var(--font-log);
  }

  .notes textarea {
    min-height: 280px;
    font-size: var(--step-0);
    line-height: 1.6;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  h4 {
    margin: 8px 0 0;
    font-size: var(--step-0);
  }

  ul {
    margin: 4px 0 0;
    padding-left: 18px;
  }

  .changes li {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 10px;
    padding: 2px 0;
  }

  .link {
    display: inline;
    padding: 0;
    font: inherit;
    color: var(--forest);
    text-decoration: underline;
    text-underline-offset: 0.18em;
    background: transparent;
    border: 0;
  }

  .link:hover:not(:disabled) {
    color: var(--forest-hi);
    background: transparent;
  }
</style>
