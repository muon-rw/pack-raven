<script lang="ts">
  import { modSyncSummary } from '../lib/modsync'
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import { api, errorText, type ModDiff, type Overview } from '../lib/api'
  import { shortcut } from '../lib/platform'
  import { nav } from '../lib/nav.svelte'

  let { overview, onpublish }: { overview: Overview | null; onpublish: () => void } = $props()

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

  function onKey(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
      event.preventDefault()
      saveNotes()
    }
  }

  let changeCount = $derived(diff ? diff.added.length + diff.updated.length + diff.removed.length : 0)
  let outOfSync = $derived(overview ? overview.missing.length + overview.unlocked.length : 0)
  let archivedNote = $derived(
    overview?.lastTag ? `packraven archived the notes for ${overview.lastTag} to changelogs/${overview.lastTag.replace(/^v/, '')}.md when you published it.` : null,
  )
  let readiness = $derived(
    [
      overview ? modSyncSummary(overview) : null,
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
          <p class="muted">Missing or changed (packraven downloads these when you pull):</p>
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
        <textarea bind:value={notes} onblur={saveNotes} rows="16" placeholder="What's new? Don't worry about mod changes, we'll add those automatically." aria-describedby="notes-status"></textarea>
      </label>
      <div class="row">
        <button onclick={saveNotes} disabled={notes === savedNotes}>Save notes <kbd aria-hidden="true">{shortcut('Ctrl+S')}</kbd></button>
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
        packraven appends these to the release notes when you publish.
        {#if diff?.baseline}<button class="link" onclick={stopComparing}>Stop comparing with {diff.since}</button>{/if}
      </p>
      {#if diffError}
        <Notice tone="error"><p>{diffError}</p></Notice>
      {:else if diff && !diff.since}
        <p class="muted">
          No previous release tags; nothing to compare against. You can
          <button class="link" onclick={compareWithZip}>compare changes against an old version</button>.
        </p>
      {:else if diff && !changeCount}
        <p class="muted">No mods changed since {diff.since}.</p>
      {:else if diff}
        {#each [['Added', diff.added, 'ok'], ['Updated', diff.updated, 'change'], ['Removed', diff.removed, 'fail']] as const as [title, list, tone] (title)}
          {#if list.length}
            <h4>{title} <span class="chip {tone}">{list.length}</span></h4>
            <ul class="mod-list">
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

  .mod-list li {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 10px;
    padding: 2px 0;
  }
</style>
