<script lang="ts">
  import Notice from '../components/Notice.svelte'
  import { onMount } from 'svelte'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener'
  import { api, errorText, type Overview, type PackConfig } from '../lib/api'
  import { job } from '../lib/jobs.svelte'
  import { renderMarkdown } from '../lib/markdown'
  import LogView from '../components/LogView.svelte'
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
  let changelog: string | null = $state(null)
  let changelogError: string | null = $state(null)
  let hasWebhook = $state(false)
  let previewing = $state(false)
  let previewError = $state<string | null>(null)
  let error: string | null = $state(null)
  let published: { version: string; pushed: boolean } | null = $state(null)

  onMount(() => {
    loadChangelog()
    api.getPackConfig().then(
      (c) => (config = c),
      () => (config = null),
    )
    api.getWebhook().then((w) => (hasWebhook = !!w), () => (hasWebhook = false))
  })

  async function previewInDiscohook() {
    previewing = true
    try {
      openUrl(await api.discohookPreview(version || 'next'))
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
    if (job('publish').phase === 'done') loadChangelog()
  })

  let warnings = $derived(
    [
      overview?.changed.length
        ? `${overview.changed.length} uncommitted ${overview.changed.length === 1 ? 'file' : 'files'}. packraven builds the packs from disk, so these changes ship but stay out of the release commit.`
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

  function openLink(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest('a')
    if (!link) return
    event.preventDefault()
    openUrl(link.href)
  }

  let publishJob = $derived(job('publish'))
  let publishing = $derived(publishJob.phase === 'running')
  let serverJob = $derived(job('server-pack'))
  let clientJob = $derived(job('client-export'))
  let logLines = $derived(publishJob.lines.concat(serverJob.lines, clientJob.lines))
  let showLog = $derived(publishing || serverJob.phase === 'running' || clientJob.phase === 'running' || logLines.length > 0)
  let projectUrl = $derived(overview?.projectId ? `https://www.curseforge.com/projects/${overview.projectId}` : null)
  let releaseType = $derived(
    config ? ({ release: 'Release', beta: 'Beta', alpha: 'Alpha' } as Record<string, string>)[config.curseforge.release_type] ?? config.curseforge.release_type : null,
  )
</script>

<div class="publish">
  <header class="head">
    <div>
      <h2>Release <span class="version-tag">{version || 'unversioned'}</span></h2>
      <p class="muted">
        Review what you'll send to CurseForge. The version and notes come from
        <button class="link" onclick={onchanges}>Changes</button>.
      </p>
    </div>
    <div class="go">
      <button class="primary" class:busy={publishing} disabled={publishing || checking || problems.length > 0} aria-describedby="publish-checks" onclick={publish}>
        {publishing ? 'Publishing' : `Publish ${version || 'release'}`}
      </button>
      <label class="check"><input type="checkbox" bind:checked={push} /> Push the release commit and tag</label>
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
          <p><button onclick={onchanges}>Write notes in Changes</button></p>
        </div>
      {:else}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <article class="changelog" onclick={openLink}>{@html renderMarkdown(changelog)}</article>
      {/if}
    </section>

    <aside class="side">
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

      <section aria-labelledby="ships-title">
        <h3 id="ships-title">Publish Settings</h3>
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
            <dd>Commit and tag <span class="file">v{version || '?'}</span>{push ? ', then push' : ''}</dd>
          </div>
          <div>
            <dt>Notes</dt>
            <dd>Archived to <span class="file">changelogs/{version || 'version'}.md</span></dd>
          </div>
        </dl>
      </section>

      <section aria-labelledby="discord-title">
        <h3 id="discord-title">Discord</h3>
        <p class="muted">
          {#if !hasWebhook}
            No webhook set, so packraven won't announce this release.
          {:else if config?.discord.announce_on_approval}
            Post to Discord once CurseForge approves {version || 'the release'}{config.discord.role_id.trim() ? ', pinging the role' : ''}.
          {:else}
            Automatic announcements are off. Announce it from Releases once CurseForge approves it.
          {/if}
          <button class="link" onclick={onsettings}>{hasWebhook ? 'Change Discord settings' : 'Set up a webhook'}</button>
        </p>
        <div class="row-start">
          <button class:busy={previewing} disabled={previewing} onclick={previewInDiscohook}>{previewing ? 'Opening' : 'Preview in Discohook'}</button>
        </div>
        {#if previewError}<p class="failed">{previewError}</p>{/if}
      </section>

      <section aria-labelledby="build-title">
        <h3 id="build-title">Build without publishing</h3>
        <p class="muted">Create publish-ready zips only, without uploading, committing or tagging.</p>
        <div class="row-start">
          <button class:busy={serverJob.phase === 'running'} disabled={serverJob.phase === 'running'} onclick={() => build(api.buildServerPack, 'server pack build')}>
            {serverJob.phase === 'running' ? 'Building' : 'Server pack'}
          </button>
          <button class:busy={clientJob.phase === 'running'} disabled={clientJob.phase === 'running'} onclick={() => build(api.buildClientExport, 'client zip')}>
            {clientJob.phase === 'running' ? 'Building' : 'Client zip'}
          </button>
        </div>
        {#if (serverJob.phase === 'done' && serverJob.message) || (clientJob.phase === 'done' && clientJob.message)}
          <p class="reveal">
            {#if serverJob.phase === 'done' && serverJob.message}<button class="link" onclick={() => reveal(serverJob.message!)}>Show server pack</button>{/if}
            {#if clientJob.phase === 'done' && clientJob.message}<button class="link" onclick={() => reveal(clientJob.message!)}>Show client zip</button>{/if}
          </p>
        {/if}
      </section>
    </aside>
  </div>

  {#if showLog}
    <div class="log-block">
      <h3>Build log</h3>
      <LogView lines={logLines} label="Build and publish log" />
    </div>
  {/if}
</div>

<style>
  .publish {
    display: grid;
    gap: 22px;
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

  .go {
    display: grid;
    justify-items: end;
    gap: 8px;
  }

  .go .primary {
    padding: 10px 22px;
    font-size: var(--step-1);
  }

  .go .check {
    font-size: var(--step--1);
    color: var(--muted);
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
  .log-block {
    display: grid;
    gap: 12px;
    align-content: start;
  }

  .side {
    display: grid;
    gap: 28px;
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
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
    line-height: 1.6;
  }

  .changelog.empty {
    display: grid;
    gap: 12px;
    color: var(--muted);
  }

  .changelog :global(h1),
  .changelog :global(h2),
  .changelog :global(h3) {
    margin: 20px 0 6px;
    font-size: var(--step-1);
  }

  .changelog :global(h3) {
    font-size: var(--step-0);
    color: var(--muted);
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
    grid-template-rows: auto minmax(220px, 1fr);
  }

  .row-start {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .reveal {
    display: flex;
    gap: 16px;
  }
</style>
