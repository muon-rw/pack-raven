<script lang="ts">
  import { hostsOf, notOnHostsSummary } from '../lib/modsync'
  import Notice from '../components/Notice.svelte'
  import { onMount, tick } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { api, errorText, type ModDiff, type ModrinthChoice, type Overview } from '../lib/api'
  import { shortcut } from '../lib/platform'
  import { nav } from '../lib/nav.svelte'
  import RenderedChangelog from '../components/RenderedChangelog.svelte'
  import LinkedText from '../components/LinkedText.svelte'
  import { job, onJobEnd } from '../lib/jobs.svelte'
  import { paneFocus } from '../lib/panes.svelte'

  let { overview, onchange }: { overview: Overview | null; onchange: () => void } = $props()

  let notes = $state('')
  let savedNotes = $state('')
  let diff = $state<ModDiff | null>(null)
  let diffError: string | null = $state(null)
  let loadingDiff = $state(false)
  let notesError: string | null = $state(null)
  let keys = $state<{ upload: boolean; modrinth: boolean } | null>(null)

  onMount(async () => {
    api.credentialsStatus().then((c) => (keys = { upload: c.upload.present, modrinth: c.modrinth.present }), () => {})
    notes = savedNotes = await api.getNotes().catch(() => '')
    loadDiff()
  })

  // Before the first release we list what publishing needs here, where the work starts, rather
  // than only in Publish's checks at the end.
  // We only ask for the tokens of the platforms you publish to.
  let firstRelease = $derived.by(() => {
    if (!overview || overview.lastTag || !keys) return null
    const curseforge = overview.projectId > 0
    const modrinth = !!overview.modrinthProjectId
    return [
      { done: curseforge || modrinth, text: 'Set a CurseForge or Modrinth project', page: 'pack' as const, anchor: 'publishing-title', where: 'Pack settings' },
      curseforge ? { done: keys.upload, text: 'Add a CurseForge upload token', page: 'settings' as const, anchor: 'curseforge-access-title', where: 'App settings' } : null,
      modrinth ? { done: keys.modrinth, text: 'Add a Modrinth token', page: 'settings' as const, anchor: 'modrinth-access-title', where: 'App settings' } : null,
    ].filter((item) => item !== null)
  })

  function go(page: 'pack' | 'settings', anchor: string) {
    nav.anchor = anchor
    nav.view = page
  }

  let savedVersion = $derived(overview?.version ?? null)
  let lastVersion = $derived(overview?.lastTag?.replace(/^v/, '') ?? null)
  let installingBcc = $state(false)
  let bccError: string | null = $state(null)
  let bccBusy = $derived(installingBcc && job('mods').phase === 'running')

  let bccSetUp = $state<'installed' | 'configured' | null>(null)

  onJobEnd('mods', (phase) => {
    if (!installingBcc) return
    installingBcc = false
    bccError = phase === 'failed' ? `Couldn't install Better Compatibility Checker: ${job('mods').message ?? 'the install failed'}` : null
    if (phase === 'done') bccSetUp = 'installed'
    onchange()
  })

  async function setUpBcc() {
    bccError = null
    try {
      installingBcc = !overview?.bccInstalled
      await api.setUpVersionChecker(lastVersion ?? '1.0.0')
      if (!installingBcc) {
        bccSetUp = 'configured'
        onchange()
      }
    } catch (error) {
      installingBcc = false
      bccError = `Couldn't set up Better Compatibility Checker: ${errorText(error)}`
    }
  }

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

  const PERMISSIONS_GUIDE = 'https://support.modrinth.com/en/articles/8797527-obtaining-modpack-permissions'
  // Past a handful of rows, the notice would push the release notes out of view.
  const MODRINTH_ROWS = 4
  // Long enough to catch the second press of a double click on a row that just moved up.
  const SETTLE_MS = 400
  type Undo = { kind: 'files'; paths: string[] } | { kind: 'author'; author: string }
  let showAllModrinth = $state(false)
  let choosing = $state<string | null>(null)
  let settling = $state(false)
  let choiceResult = $state<{ ok: boolean; text: string; undo?: Undo } | null>(null)
  // Where your place goes once the list catches up: the row that moved up after you ship, the same
  // button after you keep one off, or the result after a choice about several files.
  let refocus = $state<{ paths: string[]; index: number; target: 'row' | 'leaveOut' | 'result' } | null>(null)
  let retrying = $state(false)

  async function chooseForModrinth(paths: string[], choice: ModrinthChoice, index: number, label: string, target: 'row' | 'leaveOut' | 'result') {
    choosing = `${choice}:${paths.length === 1 ? paths[0] : 'all'}`
    try {
      await api.setModrinthChoice(paths, choice)
      const it = paths.length === 1 ? 'it' : 'them'
      const text = choice === 'ship' ? `The Modrinth pack ships ${label}.` : `${label} ${paths.length === 1 ? 'stays' : 'stay'} off Modrinth. The CurseForge pack still has ${it}.`
      choiceResult = { ok: true, text, undo: { kind: 'files', paths } }
      refocus = { paths, index, target }
      onchange()
    } catch (error) {
      choiceResult = { ok: false, text: `Couldn't save the choice for ${paths.length === 1 ? paths[0] : 'those files'}: ${errorText(error)}` }
    } finally {
      choosing = null
    }
  }

  async function undoChoice(undo: Undo) {
    choosing = 'undo'
    try {
      if (undo.kind === 'files') await api.setModrinthChoice(undo.paths, null)
      else await api.setModrinthAuthor(undo.author, false)
      choiceResult = { ok: true, text: undo.kind === 'files' ? 'Undone. Decide again below.' : `Undone. packraven no longer ships everything by ${undo.author}.` }
      onchange()
    } catch (error) {
      choiceResult = { ok: false, text: `Couldn't undo that: ${errorText(error)}` }
    } finally {
      choosing = null
    }
  }

  $effect(() => {
    const paths = (overview?.notOnModrinth ?? []).map((file) => file.path)
    if (!refocus || paths.some((path) => refocus!.paths.includes(path))) return
    const index = Math.min(refocus.index, paths.length - 1)
    const id = refocus.target === 'result' || index < 0 ? 'modrinth-choice-result' : refocus.target === 'row' ? `modrinth-row-${index}` : `modrinth-leaveOut-${index}`
    refocus = null
    settling = true
    setTimeout(() => (settling = false), SETTLE_MS)
    tick().then(() => document.getElementById(id)?.focus())
  })

  // We keep the authors and licenses of only the files that need a decision, so we look them up as they appear.
  let lookedUp = ''
  $effect(() => {
    const unknown = (overview?.notOnModrinth ?? []).filter((file) => !file.known).map((file) => file.path)
    const key = unknown.join('\n')
    if (!unknown.length || key === lookedUp) return
    lookedUp = key
    api.lookUpModrinthCandidates().then(onchange, () => {})
  })

  let permittingAuthor = $state('')

  async function shipByAuthor(author: string, paths: string[]) {
    choosing = `author:${author}`
    try {
      await api.setModrinthAuthor(author, true)
      choiceResult = { ok: true, text: `The Modrinth pack ships everything by ${author}, including mods of theirs you add later.`, undo: { kind: 'author', author } }
      permittingAuthor = ''
      refocus = { paths, index: 0, target: 'result' }
      onchange()
    } catch (error) {
      choiceResult = { ok: false, text: `Couldn't save ${author}'s permission: ${errorText(error)}` }
    } finally {
      choosing = null
    }
  }

  async function retryModrinth() {
    retrying = true
    try {
      await api.resyncModList(true)
      onchange()
    } finally {
      retrying = false
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

  const inFocusedPane = paneFocus()

  function onKey(event: KeyboardEvent) {
    if (!inFocusedPane()) return
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
      {#if savedVersion !== null}<button class="link small" onclick={() => (nav.editVersion = true)}>Change the version</button>{/if}
    </div>
  </header>

  {#if overview && savedVersion === null}
    <Notice tone="warn">
      <div class="bcc-notice">
        <p>
          <strong>{overview.bccInstalled ? 'Better Compatibility Checker has no config yet.' : "This pack doesn't have Better Compatibility Checker."}</strong>
          packraven keeps the pack version in its config, so you need it to set a version and publish.
          {overview.bccInstalled
            ? 'Better Compatibility Checker writes its config the first time the game loads, or packraven can write it now.'
            : 'packraven can install it from CurseForge and write its config.'}
        </p>
        <div class="actions">
          <button class:busy={bccBusy} disabled={bccBusy} onclick={setUpBcc}>
            {bccBusy ? 'Installing' : overview.bccInstalled ? 'Write the config' : 'Install Better Compatibility Checker'}
          </button>
        </div>
        {#if bccError}<p class="failed"><LinkedText text={bccError} /></p>{/if}
      </div>
    </Notice>
  {:else if bccSetUp && savedVersion !== null}
    <Notice tone="ok">
      <p class="bcc-done" role="status">
        packraven {bccSetUp === 'installed' ? 'installed Better Compatibility Checker and set' : 'wrote the Better Compatibility Checker config and set'} the version to
        <span class="file">{savedVersion}</span>.
        <button class="link" onclick={() => (nav.editVersion = true)}>Change the version</button>
      </p>
    </Notice>
  {/if}

  {#if firstRelease && firstRelease.some((item) => !item.done)}
    <section class="first-release" aria-labelledby="first-release-title">
      <h3 id="first-release-title">Before your first release</h3>
      <ul>
        {#each firstRelease as item (item.text)}
          <li class:done={item.done}>
            <span class="mark" aria-hidden="true">{item.done ? '✓' : ''}</span>
            <span>
              {item.text}{item.done ? '' : ' in '}{#if !item.done}<button class="link" onclick={() => go(item.page, item.anchor)}>{item.where}</button>{/if}<span class="visually-hidden">{item.done ? ', done' : ', not done yet'}</span>
            </span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if overview?.notOnCurseforge.length}
    {@const paths = overview.notOnCurseforge}
    {@const hosts = hostsOf(overview)}
    <Notice tone="warn">
      <div class="mod-list-notice">
        <p>
          <strong>{notOnHostsSummary(paths, hosts)}.</strong> packraven only downloads mods from {hosts}, so releases and collaborators won't get
          {paths.length === 1 ? 'this file' : 'these files'} unless you commit {paths.length === 1 ? 'it' : 'them'} directly. You can't publish until you add
          {paths.length === 1 ? 'it' : 'them'} to Git or remove {paths.length === 1 ? 'it' : 'them'}.
        </p>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <ul class="folder-files" tabindex="0" aria-label="Files not on {hosts}">
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
  {#if overview?.modrinthError}
    <Notice tone="warn">
      <p>
        Couldn't check which files Modrinth hosts, so you can't publish to Modrinth until packraven can. <LinkedText text={overview.modrinthError} />
      </p>
      <p><button class:busy={retrying} disabled={retrying} onclick={retryModrinth}>{retrying ? 'Checking' : 'Check again'}</button></p>
    </Notice>
  {/if}
  {#if overview?.notOnModrinth.length}
    {@const candidates = overview.notOnModrinth}
    {@const shown = showAllModrinth ? candidates : candidates.slice(0, MODRINTH_ROWS)}
    {@const paths = candidates.map((file) => file.path)}
    {@const openlyLicensed = candidates.filter((file) => file.openLicense)}
    {@const authors = [...new Set(candidates.flatMap((file) => file.authors))].sort((a, b) => a.localeCompare(b))}
    {@const filesBy = (author: string) => candidates.filter((file) => file.authors.includes(author)).map((file) => file.path)}
    {@const busy = !!choosing || settling}
    <Notice tone="warn">
      <div class="mod-list-notice">
        <p>
          <strong>{notOnHostsSummary(paths, 'Modrinth')}.</strong> Modrinth's moderators check that you may share each file you ship inside a pack, through
          its license, its project page or its author, and they may ask you for proof. Ship the ones you may share on Modrinth, and keep the rest off it.
          The CurseForge pack keeps them either way. <button class="link" onclick={() => openUrl(PERMISSIONS_GUIDE)}>How Modrinth checks permissions</button>
        </p>
        <!-- Each way to decide several files at once rests on a reason you can check: a license, or an author's word. -->
        {#if openlyLicensed.length || authors.length || candidates.length > 1}
          <div class="reasons">
            {#if openlyLicensed.length}
              {@const names = openlyLicensed.map((file) => file.name ?? file.path)}
              <button class:busy={choosing === `ship:${openlyLicensed.length === 1 ? openlyLicensed[0].path : 'all'}`} disabled={busy} onclick={() => chooseForModrinth(openlyLicensed.map((file) => file.path), 'ship', 0, names.length <= 3 ? names.join(', ') : `the ${names.length} files with open licenses`, 'result')}>
                Ship the {openlyLicensed.length === 1 ? 'one' : openlyLicensed.length} with {openlyLicensed.length === 1 ? 'an open license' : 'open licenses'}
              </button>
            {/if}
            {#if authors.length}
              {@const covered = permittingAuthor ? filesBy(permittingAuthor) : []}
              <span class="by-author">
                <label for="permitting-author">An author's permission</label>
                <select id="permitting-author" bind:value={permittingAuthor}>
                  <option value="" disabled>Choose an author</option>
                  {#each authors as author (author)}
                    {@const count = filesBy(author).length}
                    <option value={author}>{author} ({count} {count === 1 ? 'file' : 'files'})</option>
                  {/each}
                </select>
                <button class:busy={choosing === `author:${permittingAuthor}`} disabled={busy || !permittingAuthor} onclick={() => shipByAuthor(permittingAuthor, covered)}>
                  {permittingAuthor ? `Ship ${permittingAuthor}'s ${covered.length === 1 ? 'file' : `${covered.length} files`} and future ones` : 'Ship their files and future ones'}
                </button>
              </span>
            {/if}
            {#if candidates.length > 1}
              <button class:busy={choosing === 'leaveOut:all'} disabled={busy} onclick={() => chooseForModrinth(paths, 'leaveOut', 0, `${paths.length} files`, 'result')}>Keep all {paths.length} off</button>
            {/if}
          </div>
        {/if}
        <ul class="decisions" class:all={showAllModrinth} aria-label="Files not on Modrinth">
          {#each shown as file, index (file.path)}
            {@const label = file.name ?? file.path}
            <li id="modrinth-row-{index}" tabindex="-1" aria-label={label}>
              <span class="what">
                {#if file.name && file.url}
                  <a href={file.url} onclick={(e) => { e.preventDefault(); openUrl(file.url!) }}>{file.name}<span class="visually-hidden"> on CurseForge</span></a>
                {:else if file.name}
                  <span>{file.name}</span>
                {/if}
                {#if file.name}
                  <span class="facts muted">
                    {#if file.authors.length}by {file.authors.join(', ')} ·{/if}
                    {#if file.license}{file.license}{#if file.openLicense}{' '}<span class="chip ok">open license</span>{/if}{:else if file.known}No license listed{:else}Looking up its license{/if}
                  </span>
                {/if}
                <span class="file" class:muted={!!file.name}>{file.path}</span>
              </span>
              <span class="choices">
                <button id="modrinth-ship-{index}" class:busy={choosing === `ship:${file.path}`} disabled={busy} onclick={() => chooseForModrinth([file.path], 'ship', index, label, 'row')}>
                  Ship<span class="visually-hidden"> {label} on Modrinth</span>
                </button>
                <button id="modrinth-leaveOut-{index}" class:busy={choosing === `leaveOut:${file.path}`} disabled={busy} onclick={() => chooseForModrinth([file.path], 'leaveOut', index, label, 'leaveOut')}>
                  Keep off<span class="visually-hidden"> {label}, off Modrinth</span>
                </button>
              </span>
            </li>
          {/each}
        </ul>
        {#if candidates.length > shown.length}
          <button class="link show-all" onclick={() => (showAllModrinth = true)}>Show all {candidates.length} files</button>
        {/if}
      </div>
    </Notice>
  {/if}
  <!-- Mounted all along, so screen readers announce each result as it changes. -->
  <p id="modrinth-choice-result" tabindex="-1" class:muted={choiceResult?.ok} class:failed={choiceResult && !choiceResult.ok} role="status">
    {#if choiceResult}
      {choiceResult.text}
      {#if choiceResult.undo}
        {@const undo = choiceResult.undo}
        {' '}<button class="link" disabled={!!choosing} onclick={() => undoChoice(undo)}>Undo</button>, or change it later in
        <button class="link" onclick={() => go('pack', 'choices-title')}>Pack settings</button>.
      {/if}
    {/if}
  </p>
  {#if overview?.modListError}
    <Notice tone="error"><p>Couldn't update the mod list: <LinkedText text={overview.modListError} /></p></Notice>
  {/if}
  {#if overview?.watchError}
    <Notice tone="warn">
      <p>
        packraven can't watch this pack's folder, so the mod list only catches up when you switch back to packraven, pull or publish. Restart packraven to
        try again. <LinkedText text={overview.watchError} />
      </p>
    </Notice>
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

  /* 732px is the pane's width in a 1000px window with no split. */
  @container pane (max-width: 732px) {
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

  .bcc-notice {
    display: grid;
    gap: 10px;
  }

  .bcc-notice p,
  .bcc-done {
    max-width: 65ch;
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

  /* A cap keeps each file within reach of its buttons in a wide window. Notice caps the height of its
     lists for plain file names, but we show at most a few of these taller rows until you ask for all. */
  .mod-list-notice .decisions {
    display: grid;
    gap: 8px;
    max-width: 840px;
    max-height: none;
    overflow: visible;
    margin: 6px 0 0;
    padding: 0;
    list-style: none;
  }

  .mod-list-notice .decisions.all {
    max-height: 50vh;
    overflow: auto;
  }

  .decisions li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 4px 12px;
  }

  .what {
    display: grid;
    gap: 1px;
    min-width: 0;
  }

  .what .file,
  .facts {
    overflow-wrap: anywhere;
    font-size: var(--step--1);
  }

  .reasons {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
  }

  .by-author {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .decisions li:focus-visible {
    outline-offset: 2px;
  }

  .choices {
    display: flex;
    gap: 6px;
  }

  .show-all {
    justify-self: start;
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
