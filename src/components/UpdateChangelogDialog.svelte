<script lang="ts">
  import { tick } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { api, errorText, type ReleaseNotes } from '../lib/api'
  import { renderMarkdown } from '../lib/markdown'
  import { closeUpdateChangelog, installNow, selfUpdate, updateWaiting } from '../lib/selfUpdate.svelte'

  let dialog: HTMLDialogElement
  let primaryButton = $state<HTMLButtonElement>()
  let closeButton = $state<HTMLButtonElement>()
  let releases = $state<ReleaseNotes[]>([])
  let loading = $state(false)
  let error = $state<string | null>(null)
  let fetchSeq = 0

  let request = $derived(selfUpdate.changelog)
  let view = $derived(selfUpdate.view)
  let singleMatch = $derived(releases.length === 1 && releases[0].version === request?.through)

  $effect(() => {
    if (!request) {
      if (dialog?.open) dialog.close()
      return
    }
    load(request.since, request.through)
    if (!dialog.open) dialog.showModal()
    tick().then(() => {
      const target = primaryButton && !primaryButton.disabled ? primaryButton : closeButton
      target?.focus()
    })
  })

  async function load(since: string, through: string) {
    const seq = ++fetchSeq
    loading = true
    error = null
    releases = []
    try {
      const result = await api.updateChangelog(since, through)
      if (seq !== fetchSeq) return
      releases = result
    } catch (e) {
      if (seq !== fetchSeq) return
      releases = []
      error = errorText(e)
    } finally {
      if (seq === fetchSeq) loading = false
    }
  }

  function openLink(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest('a')
    if (!link) return
    event.preventDefault()
    openUrl(link.href)
  }

  let subtitle = $derived.by(() => {
    if (!request) return null
    const singleDate = singleMatch && releases[0].publishedAt ? releases[0].publishedAt : null
    if (request.mode === 'available') {
      if (releases.length > 1) return `You have ${request.since} (${releases.length} releases behind).`
      return singleDate ? `You have ${request.since} · ${singleDate}` : `You have ${request.since}.`
    }
    if (request.since && request.since !== request.through) {
      if (releases.length > 1) return `Updated from ${request.since} across ${releases.length} releases.`
      return singleDate ? `Updated from ${request.since} · ${singleDate}` : `Updated from ${request.since}.`
    }
    return singleDate ? `Released ${singleDate}` : null
  })
</script>

<dialog
  bind:this={dialog}
  aria-labelledby="update-changelog-title"
  oncancel={(e) => {
    e.preventDefault()
    closeUpdateChangelog()
  }}
  onclose={() => selfUpdate.changelog && closeUpdateChangelog()}
>
  {#if request}
    <div class="modal">
      <header class="head">
        <h2 id="update-changelog-title">
          {request.mode === 'updated' ? `Updated to packraven ${request.through}` : `What's new in packraven ${request.through}`}
        </h2>
        {#if subtitle}
          <p class="lede muted">
            {subtitle}
          </p>
        {/if}
      </header>

      <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable region needs focus so you can scroll it with keys) -->
      <div class="notes" role="region" aria-label="Release notes" tabindex="0">
        {#if loading}
          <p class="muted status-msg" role="status">Loading release notes.</p>
        {:else if error}
          <div class="error-state" role="alert">
            <p class="failed">Couldn't load the release notes: {error}</p>
            <button type="button" onclick={() => load(request.since, request.through)}>Try again</button>
          </div>
        {:else if releases.length}
          {#each releases as release (release.version)}
            <section class="release" aria-labelledby={!singleMatch ? `release-heading-${release.version}` : undefined}>
              {#if !singleMatch}
                <header class="release-head">
                  <h3 id="release-heading-{release.version}" class="file">{release.version}</h3>
                  {#if release.publishedAt}
                    <time class="muted date" datetime={release.publishedAt}>{release.publishedAt}</time>
                  {/if}
                </header>
              {/if}
              {#if release.body}
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (links inside are real links; this only routes their clicks) -->
                <div class="markdown" onclick={openLink}>
                  {@html renderMarkdown(release.body, { headingOffset: 2 })}
                </div>
              {:else}
                <p class="muted">No release notes for this version.</p>
              {/if}
            </section>
          {/each}
        {:else}
          <p class="muted status-msg">No release notes found for packraven {request.through}.</p>
        {/if}
      </div>

      <div class="actions">
        {#if request.mode === 'available' && (updateWaiting(view) || view?.status.phase === 'downloading')}
          {#if view.blocked}
            <span class="muted blocked-note" id="changelog-update-blocked">{view.blocked}</span>
          {/if}
          <button type="button" class="quiet" bind:this={closeButton} onclick={closeUpdateChangelog}>Close</button>
          {#if view.status.phase === 'downloading'}
            <button type="button" class="primary busy" disabled>Downloading update</button>
          {:else}
            <button
              type="button"
              class="primary"
              bind:this={primaryButton}
              disabled={!!view.blocked}
              aria-describedby={view.blocked ? 'changelog-update-blocked' : undefined}
              onclick={installNow}
            >
              {view.status.phase === 'available' ? 'Update and restart' : view.auto ? 'Restart now' : 'Restart to update'}
            </button>
          {/if}
        {:else}
          <button type="button" class="primary" bind:this={primaryButton} onclick={closeUpdateChangelog}>Close</button>
        {/if}
      </div>
    </div>
  {/if}
</dialog>

<style>
  dialog {
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    padding: 0;
    color: var(--text);
    background: var(--paper);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-panel);
    box-shadow: 0 12px 32px var(--shadow);
  }

  dialog::backdrop {
    background: var(--shadow);
  }

  .modal {
    display: flex;
    flex-direction: column;
    gap: 12px;
    max-height: calc(100vh - 50px);
    padding: 20px;
  }

  .head {
    display: grid;
    gap: 4px;
  }

  h2 {
    font-size: var(--step-1);
    font-variant-numeric: tabular-nums;
  }

  .lede {
    font-size: var(--step--1);
    font-variant-numeric: tabular-nums;
  }

  .notes {
    flex: 1;
    min-height: min(120px, 30vh);
    max-height: 420px;
    overflow: auto;
    padding: 14px 16px;
    background: var(--field);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    line-height: 1.55;
  }

  .status-msg {
    font-size: var(--step--1);
  }

  .error-state {
    display: grid;
    justify-items: start;
    gap: 10px;
    font-size: var(--step--1);
  }

  .release + .release {
    margin-top: 18px;
    padding-top: 18px;
    border-top: 1px solid var(--line);
  }

  .release-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 6px;
  }

  .release-head h3 {
    font-size: var(--step-0);
    font-weight: 700;
    color: var(--ochre-ink);
  }

  .date {
    font-size: var(--step--2);
    font-variant-numeric: tabular-nums;
  }

  .markdown {
    font-size: var(--step--1);
  }

  .markdown :global(h2),
  .markdown :global(h3),
  .markdown :global(h4),
  .markdown :global(h5),
  .markdown :global(h6) {
    margin: 10px 0 4px;
    font-size: var(--step--1);
    font-weight: 700;
    line-height: 1.3;
    color: var(--text);
  }

  .markdown :global(h6) {
    color: var(--muted);
  }

  .markdown > :global(:first-child) {
    margin-top: 0;
  }

  .markdown :global(p),
  .markdown :global(ul),
  .markdown :global(ol) {
    margin: 0 0 8px;
  }

  .markdown > :global(:last-child) {
    margin-bottom: 0;
  }

  .markdown :global(ul),
  .markdown :global(ol) {
    padding-left: 18px;
  }

  .markdown :global(li + li) {
    margin-top: 4px;
  }

  .markdown :global(:not(pre) > code) {
    padding: 1px 5px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-small);
  }

  .markdown :global(pre) {
    margin: 0 0 8px;
    padding: 8px 10px;
    overflow-x: auto;
    background: var(--paper);
    border: 1px solid var(--line);
    border-radius: var(--radius-small);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: end;
    gap: 6px;
  }

  .blocked-note {
    flex: 1 1 220px;
    margin-right: auto;
    font-size: var(--step--2);
  }
</style>
