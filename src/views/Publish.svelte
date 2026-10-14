<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener'
  import { api, errorText, type Overview, type PackConfig } from '../lib/api'
  import { job, onJobEnd } from '../lib/jobs.svelte'
  import RenderedChangelog from '../components/RenderedChangelog.svelte'
  import { folderChangeCount } from '../lib/modsync'
  import { bumpVersion, parseVersion, type Bump } from '../lib/version'
  import JobLog from '../components/JobLog.svelte'
  import LinkedText from '../components/LinkedText.svelte'

  let {
    overview,
    onchange,
    onchanges,
    onsettings,
    onreleases,
  }: { overview: Overview | null; onchange: () => void; onchanges: () => void; onsettings: () => void; onreleases: () => void } = $props()

  let push = $state(true)
  let problems: string[] = $state([])
  let checking = $state(true)
  let config = $state<PackConfig | null>(null)
  let changelog = $state<string | null>(null)
  let changelogError: string | null = $state(null)
  let hasWebhook = $state(false)
  let previewing = $state(false)
  let previewError = $state<string | null>(null)
  let error: string | null = $state(null)
  let published: { version: string; pushed: boolean } | null = $state(null)

  // You choose the version here, where a tag collision matters. Publishing writes it to the
  // Better Compatibility Checker config, so until then the pack reports the last release.
  let version = $state('')
  let versionTouched = $state(false)
  let lastTag = $derived(overview?.lastTag ?? null)
  let lastVersion = $derived(lastTag?.replace(/^v/, '') ?? null)
  let suggested = $derived(lastVersion && parseVersion(lastVersion) ? bumpVersion(lastVersion, 'patch') : overview?.version?.trim() || '1.0.0')

  $effect(() => {
    if (!versionTouched && overview) version = suggested
  })

  function bump(kind: Bump) {
    if (!lastVersion) return
    version = bumpVersion(lastVersion, kind)
    versionTouched = true
  }

  onMount(() => {
    loadChangelog()
    api.getPackConfig().then(
      (c) => (config = c),
      () => (config = null),
    )
    api.getWebhook().then((w) => (hasWebhook = !!w), () => (hasWebhook = false))
  })

  onJobEnd('publish', (phase) => {
    if (phase !== 'done') return
    versionTouched = false
    loadChangelog()
  })

  async function previewInDiscohook() {
    previewing = true
    try {
      openUrl(await api.discohookPreview(trimmed || 'next'))
      previewError = null
    } catch (e) {
      previewError = `Couldn't open Discohook: ${errorText(e)}`
    } finally {
      previewing = false
    }
  }

  async function loadChangelog() {
    try {
      changelog = (await api.changelogPreview()).markdown
      changelogError = null
    } catch (e) {
      changelogError = `Couldn't build the changelog preview: ${errorText(e)}`
    }
  }

  let trimmed = $derived(version.trim())
  let outOfSync = $derived(overview ? folderChangeCount(overview.folder) : 0)

  $effect(() => {
    const wanted = trimmed
    void outOfSync
    void job('publish').finishedRuns
    let current = true
    checking = true
    const timer = setTimeout(() => {
      api.publishPreflight(wanted).then(
        (list) => current && (problems = list),
        (e) => current && (problems = [`Couldn't run the pre-publish checks: ${errorText(e)}`]),
      ).finally(() => current && (checking = false))
    }, 300)
    return () => {
      current = false
      clearTimeout(timer)
    }
  })

  // You can't take back an upload, so we block a push that would fail after it.
  let blockers = $derived(
    [...problems, overview?.behind && push ? `This branch is ${overview.behind} ${overview.behind === 1 ? 'commit' : 'commits'} behind its remote, so the push would fail after the upload. Pull first, or untick the push.` : null].filter(
      (p): p is string => !!p,
    ),
  )
  let tagTaken = $derived(problems.some((p) => p.startsWith('You already released')))
  let emptyNotes = $derived(changelog !== null && !changelog.trim())
  let warnings = $derived(
    [
      overview?.changed.length
        ? `${overview.changed.length} uncommitted ${overview.changed.length === 1 ? 'file' : 'files'}. packraven builds the packs from disk, so these changes ship but stay out of the release commit.`
        : null,
      emptyNotes ? 'No release notes and no mod changes, so the CurseForge changelog will be empty.' : null,
    ].filter((w): w is string => !!w),
  )

  let rolePing = $derived(!!config?.discord.role_id.trim())
  let announces = $derived(hasWebhook && !!config?.discord.announce_on_approval)

  async function publish() {
    const consequences = [
      `Upload ${overview?.name} ${trimmed} and its server pack to CurseForge as ${releaseType ? `a ${releaseType.toLowerCase()}` : 'a release'}${config?.curseforge.manual_release ? ', held until you release it' : ''}.`,
      `Commit and tag v${trimmed}${push ? ', then push' : ''}.`,
      'Archive the release notes and clear changelog.md for the next release.',
      announces ? `Post to Discord once CurseForge approves it${rolePing ? ', pinging the role' : ''}.` : null,
      emptyNotes ? 'The changelog on CurseForge will be empty.' : null,
    ].filter(Boolean)
    const confirmed = await ask(`${consequences.join('\n')}\n\nThis can't be undone.`, {
      title: `Publish ${trimmed}`,
      kind: 'warning',
      okLabel: `Publish ${trimmed}`,
      cancelLabel: 'Cancel',
    })
    if (!confirmed) return
    try {
      error = null
      published = { version: trimmed, pushed: push }
      await api.publish(trimmed, push)
      onchange()
    } catch (e) {
      error = errorText(e)
    }
  }

  async function build(action: () => Promise<void>, what: string) {
    try {
      error = null
      await action()
    } catch (e) {
      error = `Couldn't start the ${what}: ${errorText(e)}`
    }
  }

  function reveal(path: string) {
    revealItemInDir(path).catch((e) => (error = `Couldn't open the folder: ${errorText(e)}`))
  }

  let publishJob = $derived(job('publish'))
  let publishing = $derived(publishJob.phase === 'running')
  let serverJob = $derived(job('server-pack'))
  let clientJob = $derived(job('client-export'))
  let logLines = $derived(publishJob.lines.concat(serverJob.lines, clientJob.lines).sort((a, b) => a.id - b.id))
  let showLog = $derived(publishing || serverJob.phase === 'running' || clientJob.phase === 'running' || logLines.length > 0)
  let projectUrl = $derived(overview?.projectId ? `https://www.curseforge.com/projects/${overview.projectId}` : null)
  let releaseType = $derived(
    config ? ({ release: 'Release', beta: 'Beta', alpha: 'Alpha' } as Record<string, string>)[config.curseforge.release_type] ?? config.curseforge.release_type : null,
  )
  let canPublish = $derived(!publishing && !checking && !blockers.length && !!trimmed)
</script>

<div class="publish">
  <header class="head">
    <div>
      <h2>Publish</h2>
      <p class="muted">Review what ships, then publish at the bottom. Publishing can't be undone.</p>
    </div>
    <div class="builds">
      <span class="muted small">Build without publishing</span>
      <span class="row-start">
        <button class:busy={serverJob.phase === 'running'} disabled={serverJob.phase === 'running'} onclick={() => build(api.buildServerPack, 'server pack build')}>
          {serverJob.phase === 'running' ? 'Building' : 'Server pack'}
        </button>
        <button class:busy={clientJob.phase === 'running'} disabled={clientJob.phase === 'running'} onclick={() => build(api.buildClientExport, 'client pack build')}>
          {clientJob.phase === 'running' ? 'Building' : 'Client pack'}
        </button>
      </span>
      {#if (serverJob.phase === 'done' && serverJob.message) || (clientJob.phase === 'done' && clientJob.message)}
        <span class="reveal">
          {#if serverJob.phase === 'done' && serverJob.message}<button class="link" onclick={() => reveal(serverJob.message!)}>Show server pack</button>{/if}
          {#if clientJob.phase === 'done' && clientJob.message}<button class="link" onclick={() => reveal(clientJob.message!)}>Show client pack</button>{/if}
        </span>
      {/if}
    </div>
  </header>

  {#if publishJob.phase === 'done' && published}
    <Notice tone="ok">
      <p>
        <strong>{overview?.name} {published.version} is on CurseForge.</strong> packraven tagged the release commit v{published.version}{published.pushed ? ' and pushed it' : " but hasn't pushed it yet"}.
      </p>
      <p class="muted">
        packraven archived its notes to <span class="file">changelogs/{published.version}.md</span> and cleared changelog.md for the next release. {publishJob.message}
        <button class="link" onclick={onreleases}>Follow its review in Releases</button>.
      </p>
    </Notice>
  {/if}
  {#each [publishJob, serverJob, clientJob] as failed, index (index)}
    {#if failed.phase === 'failed'}<Notice tone="error"><p><LinkedText text={failed.message ?? ''} /></p></Notice>{/if}
  {/each}
  {#if error}<Notice tone="error"><p><LinkedText text={error} /></p></Notice>{/if}

  <section class="step" aria-labelledby="version-title">
    <h3 id="version-title">Version</h3>
    <div class="version-row">
      <label class="version-field">
        <span class="visually-hidden">Version to publish</span>
        <input bind:value={version} oninput={() => (versionTouched = true)} placeholder={suggested} spellcheck="false" aria-describedby="version-hint" aria-invalid={tagTaken} />
      </label>
      {#if lastVersion && parseVersion(lastVersion)}
        <span class="bumps" role="group" aria-label="Bump from {lastVersion}">
          {#each ['patch', 'minor', 'major'] as const as kind (kind)}
            {@const next = bumpVersion(lastVersion, kind)}
            <button type="button" class="ghost" aria-pressed={trimmed === next} onclick={() => bump(kind)}>
              {kind[0].toUpperCase() + kind.slice(1)} <span class="file">{next}</span>
            </button>
          {/each}
        </span>
      {/if}
    </div>
    <p class="muted small" id="version-hint">
      {#if tagTaken}
        <span class="failed">{trimmed} is already released (tag v{trimmed}). Pick a newer version.</span>
      {:else if lastTag}
        The last release is <span class="file">{lastTag}</span>. packraven writes this version into the pack when you publish.
      {:else}
        This is the first release. packraven writes this version into the pack when you publish.
      {/if}
    </p>
  </section>

  <section class="step" aria-labelledby="checks-title" aria-busy={checking}>
    <h3 id="checks-title">Checks</h3>
    <ul class="checks" id="publish-checks">
      {#each blockers as problem (problem)}
        <li class="bad">
          <svg viewBox="0 0 16 16" role="img" aria-label="Blocks publishing"><path d="M4 4l8 8M12 4l-8 8" /></svg>
          <span><LinkedText text={problem} /></span>
        </li>
      {/each}
      {#each warnings as warning (warning)}
        <li class="warn">
          <svg viewBox="0 0 16 16" role="img" aria-label="Warning"><path d="M8 3v6M8 12v1" /></svg>
          {warning}
        </li>
      {/each}
      {#if !blockers.length && !warnings.length}
        <li class="ok">
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8.5l3 3 7-7" /></svg>
          {checking ? 'Checking' : 'Ready to publish'}
        </li>
      {/if}
    </ul>
  </section>

  <section class="step" aria-labelledby="ships-title">
    <h3 id="ships-title">What ships</h3>
    <dl class="ships">
      <div>
        <dt>Client pack</dt>
        <dd>{#if trimmed}<span class="file">{overview?.name} {trimmed}.zip</span>{:else}<span class="muted">Set a version above</span>{/if}</dd>
      </div>
      <div>
        <dt>Server pack</dt>
        <dd>Uploaded as an additional file of the client pack</dd>
      </div>
      <div>
        <dt>Release type</dt>
        <dd>
          {releaseType ?? 'Unknown'}{config?.curseforge.manual_release ? ', held until you release it' : ''}
          <button class="link" onclick={onsettings}>Change</button>
        </dd>
      </div>
      <div>
        <dt>Project</dt>
        <dd>
          {#if projectUrl}<a href={projectUrl} onclick={(e) => { e.preventDefault(); openUrl(projectUrl!) }}>{overview?.projectId}</a>{:else}Not set{/if}
        </dd>
      </div>
      <div>
        <dt>Git</dt>
        <dd>{#if trimmed}Commit and tag <span class="file">v{trimmed}</span>{push ? ', then push' : ''}{:else}<span class="muted">Set a version above</span>{/if}</dd>
      </div>
      <div>
        <dt>Notes</dt>
        <dd>
          {#if trimmed}Archived to <span class="file">changelogs/{trimmed}.md</span>, and changelog.md is cleared{:else}<span class="muted">Set a version above</span>{/if}
        </dd>
      </div>
    </dl>
  </section>

  <section class="step" aria-labelledby="changelog-title">
    <div class="row">
      <h3 id="changelog-title">Changelog on CurseForge</h3>
      <span class="row-start">
        <button class="ghost" onclick={onchanges}>Edit notes in Changes</button>
        <button class="ghost" onclick={loadChangelog}>Refresh</button>
      </span>
    </div>
    {#if changelogError}
      <Notice tone="error"><p>{changelogError}</p></Notice>
    {:else if changelog === null}
      <p class="muted">Building the preview.</p>
    {:else if !changelog.trim()}
      <div class="empty-changelog">
        <p>No release notes and no mod changes yet, so the CurseForge changelog would be empty.</p>
        <p><button onclick={onchanges}>Write notes in Changes</button></p>
      </div>
    {:else}
      <RenderedChangelog markdown={changelog} label="Changelog preview" />
    {/if}
  </section>

  <section class="step" aria-labelledby="discord-title">
    <h3 id="discord-title">Discord</h3>
    <p class="muted">
      {#if !hasWebhook}
        No webhook set, so packraven won't announce this release.
      {:else if announces}
        packraven posts to Discord once CurseForge approves {trimmed || 'the release'}{rolePing ? ', and pings the role' : ''}.
      {:else}
        Automatic announcements are off. You can announce it from Releases.
      {/if}
      <button class="link" onclick={onsettings}>{hasWebhook ? 'Change Discord settings' : 'Set up a webhook'}</button>
    </p>
    <div class="row-start">
      <button class:busy={previewing} disabled={previewing} onclick={previewInDiscohook}>{previewing ? 'Opening' : 'Preview the announcement'}</button>
    </div>
    {#if previewError}<p class="failed">{previewError}</p>{/if}
  </section>

  {#if showLog}
    <section class="step log-block" aria-labelledby="log-title">
      <h3 id="log-title">Build log</h3>
      <JobLog lines={logLines} label="Build and publish log" />
    </section>
  {/if}

  <div class="go">
    <label class="check"><input type="checkbox" bind:checked={push} /> Push the release commit and tag</label>
    <button class="primary" class:busy={publishing} disabled={!canPublish} aria-describedby="publish-checks" onclick={publish}>
      {publishing ? 'Publishing' : `Publish ${trimmed || 'release'}`}
    </button>
  </div>
</div>

<style>
  .publish {
    display: grid;
    gap: 28px;
    max-width: 860px;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: end;
    gap: 16px 24px;
  }

  .head p {
    margin-top: 4px;
  }

  .builds {
    display: grid;
    justify-items: end;
    gap: 6px;
    margin-left: auto;
  }

  .small {
    font-size: var(--step--1);
  }

  .step {
    display: grid;
    gap: 12px;
    align-content: start;
  }

  .version-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 16px;
  }

  .version-field input {
    width: 160px;
    font-family: var(--font-log);
    font-size: var(--step-1);
  }

  .version-field input[aria-invalid='true'] {
    border-color: var(--fail-border);
  }

  .bumps {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .bumps button[aria-pressed='true'] {
    background: var(--here);
    border-color: var(--here-border);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .step {
    --changelog-max-height: 60vh;
  }

  .empty-changelog {
    display: grid;
    gap: 12px;
    padding: 18px 22px;
    color: var(--muted);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
  }

  .ships {
    display: grid;
    margin: 0;
    border-top: 1px solid var(--line);
  }

  .ships div {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: baseline;
    gap: 4px 16px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }

  .ships dt {
    color: var(--muted);
    font-size: var(--step--1);
  }

  .ships dd {
    margin: 0;
    text-align: right;
  }

  .ships .link {
    margin-left: 8px;
    font-size: var(--step--1);
  }

  .checks {
    display: grid;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .checks li {
    display: grid;
    grid-template-columns: 16px 1fr;
    gap: 10px;
    align-items: start;
  }

  .checks svg {
    width: 16px;
    height: 16px;
    margin-top: 3px;
    fill: none;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .checks .bad svg {
    stroke: var(--fail-icon);
  }

  .checks .warn svg {
    stroke: var(--warn-icon);
  }

  .checks .ok svg {
    stroke: var(--forest);
  }

  .log-block {
    grid-template-rows: auto minmax(240px, 40vh);
  }

  .row-start {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .reveal {
    display: flex;
    gap: 16px;
    font-size: var(--step--1);
  }

  /* We keep the button in reach while you read the review above it. */
  .go {
    position: sticky;
    bottom: -28px;
    display: flex;
    flex-wrap: wrap;
    justify-content: end;
    align-items: center;
    gap: 12px 20px;
    margin-bottom: -28px;
    padding: 16px 0 28px;
    background: var(--paper);
    border-top: 1px solid var(--line-strong);
  }

  .go .primary {
    padding: 10px 22px;
    font-size: var(--step-1);
  }

  .go .check {
    color: var(--muted);
    font-size: var(--step--1);
  }
</style>
