<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { openPath, openUrl } from '@tauri-apps/plugin-opener'
  import { api, errorText, type Overview, type PackConfig, type ReleaseStatus } from '../lib/api'
  import { job } from '../lib/jobs.svelte'
  import { renderMarkdown } from '../lib/markdown'
  import LogView from '../components/LogView.svelte'

  let { overview, onchange, onprepare }: { overview: Overview | null; onchange: () => void; onprepare: () => void } = $props()

  let push = $state(true)
  let problems: string[] = $state([])
  let checking = $state(true)
  let config = $state<PackConfig | null>(null)
  let changelog: string | null = $state(null)
  let changelogError: string | null = $state(null)
  let releases: ReleaseStatus[] = $state([])
  let releasesError: string | null = $state(null)
  let error: string | null = $state(null)
  let published: { version: string; pushed: boolean } | null = $state(null)

  onMount(() => {
    loadChangelog()
    loadReleases()
    api.getPackConfig().then(
      (c) => (config = c),
      () => (config = null),
    )
  })

  async function loadChangelog() {
    try {
      changelog = (await api.changelogPreview()).markdown
      changelogError = null
    } catch (e) {
      changelogError = `Couldn't build the changelog preview: ${errorText(e)}`
    }
  }

  async function loadReleases() {
    try {
      releases = await api.releases()
      releasesError = null
    } catch (e) {
      releasesError = `Couldn't load releases from CurseForge: ${errorText(e)}`
    }
  }

  let version = $derived(overview?.version?.trim() ?? '')
  let outOfSync = $derived(overview ? overview.missing.length + overview.unlocked.length : 0)

  $effect(() => {
    const wanted = version
    void outOfSync
    void job('publish').phase
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

  $effect(() => {
    if (job('publish').phase === 'done') {
      loadReleases()
      loadChangelog()
    }
  })

  let warnings = $derived(
    [
      overview?.changed.length
        ? `${overview.changed.length} uncommitted ${overview.changed.length === 1 ? 'file' : 'files'}. The packs are built from disk, so these changes ship but stay out of the release commit.`
        : null,
      overview?.behind && push
        ? `This branch is ${overview.behind} ${overview.behind === 1 ? 'commit' : 'commits'} behind its remote, so the push would fail after the upload. Pull first.`
        : null,
    ].filter((w): w is string => !!w),
  )

  async function publish() {
    const confirmed = await ask(
      `Upload ${overview?.name} ${version} and its server pack to CurseForge, then commit and tag v${version}${push ? ' and push' : ''}? This can't be undone.`,
      { title: 'Publish release', kind: 'warning', okLabel: `Publish ${version}`, cancelLabel: 'Cancel' },
    )
    if (!confirmed) return
    try {
      error = null
      published = { version, pushed: push }
      await api.publish(version, push)
      onchange()
    } catch (e) {
      error = errorText(e)
    }
  }

  async function exportOnly() {
    try {
      error = null
      await api.buildClientExport()
    } catch (e) {
      error = errorText(e)
    }
  }

  function openLink(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest('a')
    if (!link) return
    event.preventDefault()
    openUrl(link.href)
  }

  let publishJob = $derived(job('publish'))
  let publishing = $derived(publishJob.phase === 'running')
  let exportJob = $derived(job('client-export'))
  let logLines = $derived(publishJob.lines.concat(exportJob.lines))
  let showLog = $derived(publishing || exportJob.phase === 'running' || logLines.length > 0)
  let projectUrl = $derived(overview?.projectId ? `https://www.curseforge.com/projects/${overview.projectId}` : null)
  let releaseType = $derived(
    config ? ({ release: 'Release', beta: 'Beta', alpha: 'Alpha' } as Record<string, string>)[config.curseforge.release_type] ?? config.curseforge.release_type : null,
  )
</script>

<div class="publish">
  <header class="head">
    <div>
      <h2>Release <span class="version-tag">{version || 'unversioned'}</span></h2>
      <p class="muted">Review exactly what goes to CurseForge. The version and notes come from Prepare.</p>
    </div>
    <button onclick={onprepare}>{version ? 'Edit in Prepare' : 'Set a version in Prepare'}</button>
  </header>

  {#if publishJob.phase === 'done' && published}
    <Notice tone="ok">
      <p>
        <strong>{overview?.name} {published.version} is on CurseForge.</strong> The release commit is tagged v{published.version}{published.pushed ? ' and pushed' : ', not pushed yet'}.
      </p>
      <p class="muted">Its notes were archived to <span class="file">changelogs/{published.version}.md</span>, and changelog.md is empty for the next release. {publishJob.message}</p>
    </Notice>
  {/if}
  {#if publishJob.phase === 'failed'}<Notice tone="error"><p>{publishJob.message}</p></Notice>{/if}
  {#if error}<Notice tone="error"><p>{error}</p></Notice>{/if}

  <div class="columns">
    <section class="review" aria-labelledby="changelog-title">
      <div class="row">
        <h3 id="changelog-title">Changelog on CurseForge</h3>
        <button onclick={loadChangelog}>Refresh</button>
      </div>
      {#if changelogError}
        <Notice tone="error"><p>{changelogError}</p></Notice>
      {:else if changelog === null}
        <p class="muted">Building the preview.</p>
      {:else if !changelog.trim()}
        <div class="changelog empty">
          <p>No release notes and no mod changes yet, so the CurseForge changelog would be empty.</p>
          <p><button onclick={onprepare}>Write notes in Prepare</button></p>
        </div>
      {:else}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <article class="changelog" onclick={openLink}>{@html renderMarkdown(changelog)}</article>
      {/if}
    </section>

    <aside class="side">
      <section aria-labelledby="ships-title">
        <h3 id="ships-title">What ships</h3>
        <dl class="ships">
          <div>
            <dt>Client pack</dt>
            <dd><span class="file">{overview?.name} {version}.zip</span></dd>
          </div>
          <div>
            <dt>Server pack</dt>
            <dd>Attached to the client file</dd>
          </div>
          <div>
            <dt>Release type</dt>
            <dd>{releaseType ?? 'Unknown'}{config?.curseforge.manual_release ? ', held until you release it' : ''}</dd>
          </div>
          <div>
            <dt>Project</dt>
            <dd>
              {#if projectUrl}<a href={projectUrl} onclick={(e) => { e.preventDefault(); openUrl(projectUrl!) }}>{overview?.projectId}</a>{:else}Not set{/if}
            </dd>
          </div>
          <div>
            <dt>Git</dt>
            <dd>Commit and tag <span class="file">v{version || '?'}</span></dd>
          </div>
        </dl>
        <label class="check"><input type="checkbox" bind:checked={push} /> Push the release commit and tag</label>
      </section>

      <section aria-labelledby="checks-title" aria-busy={checking}>
        <h3 id="checks-title">Checks</h3>
        <ul class="checks" id="publish-checks">
          {#each problems as problem (problem)}
            <li class="bad">
              <svg viewBox="0 0 16 16" role="img" aria-label="Blocks publishing"><path d="M4 4l8 8M12 4l-8 8" /></svg>
              {problem}
            </li>
          {/each}
          {#each warnings as warning (warning)}
            <li class="warn">
              <svg viewBox="0 0 16 16" role="img" aria-label="Warning"><path d="M8 3v6M8 12v1" /></svg>
              {warning}
            </li>
          {/each}
          {#if !problems.length && !warnings.length}
            <li class="ok">
              <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8.5l3 3 7-7" /></svg>
              {checking ? 'Checking' : 'Ready to publish'}
            </li>
          {/if}
        </ul>
      </section>

      <div class="go">
        <button class="primary" class:busy={publishing} disabled={publishing || checking || problems.length > 0} aria-describedby="publish-checks" onclick={publish}>
          {publishing ? 'Publishing' : `Publish ${version || 'release'}`}
        </button>
        <p class="muted">After upload, changelog.md is archived to <span class="file">changelogs/{version || 'version'}.md</span> and cleared.</p>
      </div>
    </aside>
  </div>

  {#if showLog}
    <div class="log-block">
      <h3>Publish log</h3>
      <LogView lines={logLines} label="Publish log" />
    </div>
  {/if}

  <section class="history" aria-labelledby="history-title">
    <div class="row">
      <h3 id="history-title">Published releases</h3>
      <div class="row">
        <button onclick={exportOnly} class:busy={exportJob.phase === 'running'} disabled={exportJob.phase === 'running'}>Build client zip only</button>
        {#if exportJob.phase === 'done' && exportJob.message}
          <button onclick={() => openPath(exportJob.message!.replace(/[\\/][^\\/]+$/, ''))}>Show client zip</button>
        {/if}
        <button onclick={loadReleases}>Refresh</button>
      </div>
    </div>
    {#if exportJob.phase === 'failed'}<Notice tone="error"><p>{exportJob.message}</p></Notice>{/if}
    {#if releasesError}<p class="failed">{releasesError}</p>{/if}
    {#if releases.length}
      <table>
        <thead>
          <tr><th>Version</th><th>Published</th><th>Pack file</th><th>Server pack</th></tr>
        </thead>
        <tbody>
          {#each [...releases].reverse() as release (release.fileId)}
            <tr>
              <td class="file">{release.version}</td>
              <td>{new Date(release.publishedAt).toLocaleDateString()}</td>
              <td>{release.status}</td>
              <td>
                {release.serverStatus}
                {#if release.serverPackLinked === false}<span class="unlinked">not linked as the server pack</span>{/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else if !releasesError}
      <p class="muted">Releases you publish from packraven show up here with their CurseForge review status.</p>
    {/if}
  </section>
</div>

<style>
  .publish {
    display: grid;
    gap: 22px;
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

  .version-tag {
    font-family: var(--font-log);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: var(--teal);
  }

  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 320px;
    gap: 36px;
    align-items: start;
  }

  @media (max-width: 1150px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .review,
  .side section,
  .history,
  .log-block {
    display: grid;
    gap: 12px;
    align-content: start;
  }

  .side {
    display: grid;
    gap: 26px;
    align-content: start;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .changelog {
    max-height: 60vh;
    overflow: auto;
    padding: 18px 22px;
    background: var(--plume);
    border: 1px solid var(--quill);
    border-radius: var(--radius-panel);
    line-height: 1.6;
  }

  .changelog.empty {
    display: grid;
    gap: 12px;
    color: var(--ash);
  }

  .changelog :global(h1),
  .changelog :global(h2),
  .changelog :global(h3) {
    margin: 20px 0 6px;
    font-size: var(--step-1);
  }

  .changelog :global(h3) {
    font-size: var(--step-0);
    color: var(--ash);
  }

  .changelog > :global(:first-child) {
    margin-top: 0;
  }

  .changelog :global(p),
  .changelog :global(ul),
  .changelog :global(ol) {
    margin: 0 0 10px;
  }

  .changelog :global(ul),
  .changelog :global(ol) {
    padding-left: 20px;
  }

  .changelog :global(code) {
    font-family: var(--font-log);
    font-size: 0.9em;
  }

  .ships {
    display: grid;
    margin: 0;
    border-top: 1px solid var(--quill);
  }

  .ships div {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: baseline;
    gap: 4px 16px;
    padding: 8px 0;
    border-bottom: 1px solid var(--quill);
  }

  .ships dt {
    color: var(--ash);
    font-size: var(--step--1);
  }

  .ships dd {
    margin: 0;
    text-align: right;
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
    stroke: var(--rose);
  }

  .checks .warn svg {
    stroke: var(--amber);
  }

  .checks .ok svg {
    stroke: var(--teal);
  }

  .go {
    display: grid;
    gap: 10px;
  }

  .go .primary {
    padding-block: 11px;
    font-size: var(--step-1);
  }

  .go p {
    font-size: var(--step--1);
  }

  .log-block {
    grid-template-rows: auto minmax(220px, 1fr);
  }

  .history {
    padding-top: 22px;
    border-top: 1px solid var(--quill);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }

  th {
    text-align: left;
    font-size: var(--step--1);
    font-weight: 400;
    color: var(--ash);
    padding: 6px 12px 6px 0;
  }

  td {
    padding: 8px 12px 8px 0;
    border-top: 1px solid var(--quill);
  }

  .unlinked {
    color: var(--amber);
    margin-left: 6px;
  }
</style>
