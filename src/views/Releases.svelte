<script lang="ts">
  import { onMount } from 'svelte'
  import { listen } from '@tauri-apps/api/event'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { api, errorText, type ReleaseStatus } from '../lib/api'
  import { onJobEnd } from '../lib/jobs.svelte'

  let { onsettings }: { onsettings: () => void } = $props()

  let releases: ReleaseStatus[] = $state([])
  let releasesError: string | null = $state(null)
  let loading = $state(true)
  let hasWebhook = $state(false)
  let announcing = $state<string | null>(null)
  let discordError = $state<string | null>(null)
  let rolePing = $state(false)

  onMount(() => {
    loadReleases()
    api.getWebhook().then((w) => (hasWebhook = !!w), () => (hasWebhook = false))
    api.getPackConfig().then((c) => (rolePing = !!c.discord.role_id.trim()), () => {})
    const unlisten = listen('release-announced', () => loadReleases())
    return () => {
      unlisten.then((stop) => stop())
    }
  })

  onJobEnd('publish', (phase) => phase === 'done' && loadReleases())

  async function loadReleases() {
    loading = true
    try {
      releases = await api.releases()
      releasesError = null
    } catch (e) {
      releasesError = `Couldn't load releases from CurseForge: ${errorText(e)}`
    } finally {
      loading = false
    }
  }

  // An announcement is public and can ping a whole role, so we ask first and say when CurseForge
  // hasn't approved the file yet.
  async function announceNow(release: ReleaseStatus) {
    const { version } = release
    const approved = statusTone(release.status) === 'ok'
    const lines = [
      `Post the ${version} announcement to the Discord channel your webhook points at${rolePing ? ', pinging the role from Pack settings' : ''}.`,
      approved ? null : `CurseForge still lists the file as "${release.status}", so players may not be able to download it yet.`,
      release.announcedAt ? `You already announced it on ${new Date(release.announcedAt).toLocaleDateString()}, so this posts it again.` : null,
    ].filter(Boolean)
    const confirmed = await ask(`${lines.join('\n\n')}\n\nYou can't unsend it from packraven.`, {
      title: approved ? `Announce ${version}` : `Announce ${version} before approval`,
      kind: 'warning',
      okLabel: release.announcedAt ? 'Announce again' : 'Announce',
      cancelLabel: 'Cancel',
    })
    if (!confirmed) return
    announcing = version
    try {
      await api.announceRelease(version)
      discordError = null
      await loadReleases()
    } catch (e) {
      discordError = errorText(e)
    } finally {
      announcing = null
    }
  }

  async function editInDiscohook(version: string) {
    try {
      openUrl(await api.discohookLink(version))
      discordError = null
    } catch (e) {
      discordError = `Couldn't open Discohook: ${errorText(e)}`
    }
  }

  function discordState(release: ReleaseStatus) {
    if (release.announcedAt) return `Announced ${new Date(release.announcedAt).toLocaleDateString()}`
    if (release.announce) return 'packraven posts this once CurseForge approves it'
    return 'Not announced'
  }

  function statusTone(status: string) {
    return /approved|released/i.test(status) ? 'ok' : /reject|delete|fail/i.test(status) ? 'fail' : 'pending'
  }
</script>

<div class="releases">
  <header class="head">
    <div>
      <h2>Releases</h2>
      <p class="muted">
        Versions published from packraven on this machine, their CurseForge review status, and Discord announcements.
        {#if !hasWebhook}No Discord webhook is set, so packraven won't announce anything.{/if}
      </p>
    </div>
    <div class="actions">
      <button class="ghost settings" onclick={onsettings}>Discord settings</button>
      <button class:busy={loading} disabled={loading} onclick={loadReleases}>{loading ? 'Checking' : 'Refresh'}</button>
    </div>
  </header>

  {#if releasesError}<p class="failed" role="alert">{releasesError}</p>{/if}
  {#if discordError}<p class="failed" role="alert">{discordError}</p>{/if}

  {#if releases.length}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable region needs focus so you can scroll it with keys) -->
    <div class="scroll" role="region" aria-label="Releases" tabindex="0">
    <table>
      <thead>
        <tr><th>Version</th><th>Published</th><th>Client pack</th><th>Server pack</th><th>Discord</th></tr>
      </thead>
      <tbody>
        {#each [...releases].reverse() as release (release.fileId)}
          <tr>
            <td class="file">{release.version}</td>
            <td>{new Date(release.publishedAt).toLocaleDateString()}</td>
            <td><span class="chip {statusTone(release.status)}">{release.status}</span></td>
            <td>
              <span class="chip {statusTone(release.serverStatus)}">{release.serverStatus}</span>
              {#if release.serverPackLinked === false}<span class="chip pending">Not linked as the server pack</span>{/if}
            </td>
            <td>
              <div class="discord">
                <span class:muted={!release.announcedAt}>{discordState(release)}</span>
                <span class="discord-actions">
                  {#if hasWebhook}
                    <button class="ghost" class:busy={announcing === release.version} disabled={announcing !== null} onclick={() => announceNow(release)}>
                      {release.announcedAt ? 'Announce again' : statusTone(release.status) === 'ok' ? 'Announce now' : 'Announce before approval'}
                    </button>
                  {/if}
                  <button class="ghost" onclick={() => editInDiscohook(release.version)} aria-describedby="discohook-hint">Edit in Discohook</button>
                </span>
              </div>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
    </div>
    <p class="muted small" id="discohook-hint">
      Discohook is a web editor for Discord messages. packraven opens the announcement there, so you can change it and post it yourself.
    </p>
  {:else if loading}
    <p class="muted" role="status">Checking CurseForge for releases.</p>
  {:else if !releasesError}
    <p class="muted">Releases you publish from packraven show up here with their CurseForge review status.</p>
  {/if}
</div>

<style>
  .releases {
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

  .actions {
    display: flex;
    gap: 6px;
  }

  .settings {
    color: var(--forest);
  }

  .small {
    font-size: var(--step--1);
  }

  /* In a narrow pane the table scrolls sideways on its own, so the rest of the view keeps its width. */
  .scroll {
    overflow-x: auto;
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
    color: var(--muted);
    padding: 6px 12px 6px 0;
  }

  td {
    padding: 8px 12px 8px 0;
    border-top: 1px solid var(--line);
    vertical-align: top;
  }

  .discord {
    display: grid;
    gap: 2px;
  }

  .discord-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    margin-left: -8px;
  }

  .discord-actions button {
    color: var(--forest);
  }
</style>
