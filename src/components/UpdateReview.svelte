<script lang="ts" module>
  import type { InstalledMod, ModFile } from '../lib/api'

  export type PendingUpdate = { mod: InstalledMod; file: ModFile; kind: string | null }
</script>

<script lang="ts">
  import { tick } from 'svelte'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { api, errorText } from '../lib/api'
  import { remoteTextHtml } from '../lib/markdown'

  let { request, onconfirm, oncancel }: { request: PendingUpdate[] | null; onconfirm: (chosen: PendingUpdate[]) => void; oncancel: () => void } = $props()

  const typeLabels = { release: 'Release', beta: 'Beta', alpha: 'Alpha' }

  let dialog: HTMLDialogElement
  let confirm = $state<HTMLButtonElement>()
  let chosen = $state(new Set<string>())
  let open = $state<string | null>(null)
  let changelogs: Record<string, string | { error: string }> = $state({})

  $effect(() => {
    if (!request) return
    // A disabled mod isn't in the game, so we don't update it unless you ask.
    chosen = new Set(request.filter((u) => !u.mod.path.endsWith('.disabled')).map((u) => u.mod.path))
    open = null
    dialog.showModal()
    tick().then(() => confirm?.focus())
  })

  function finish(confirmed: boolean) {
    if (dialog.open) dialog.close()
    if (confirmed && request) onconfirm(request.filter((u) => chosen.has(u.mod.path)))
    else oncancel()
  }

  function toggle(path: string, on: boolean) {
    const next = new Set(chosen)
    if (on) next.add(path)
    else next.delete(path)
    chosen = next
  }

  async function showChangelog(update: PendingUpdate) {
    open = open === update.mod.path ? null : update.mod.path
    const key = update.mod.key
    if (open === null || !key || changelogs[update.file.file] !== undefined) return
    try {
      changelogs[update.file.file] = remoteTextHtml(await api.modChangelog(key, update.file.file)) || '<p>The author left this changelog empty.</p>'
    } catch (error) {
      changelogs[update.file.file] = { error: `Couldn't load the changelog from ${key.startsWith('mr:') ? 'Modrinth' : 'CurseForge'}: ${errorText(error)}` }
    }
  }

  function onChangelogClick(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest('a')
    if (!link) return
    event.preventDefault()
    const external = link.getAttribute('data-external')
    if (external) openUrl(external)
  }

  function fileName(path: string) {
    return path.slice(path.lastIndexOf('/') + 1).replace(/\.disabled$/, '')
  }

  let count = $derived(chosen.size)
</script>

<dialog bind:this={dialog} aria-labelledby="review-title" oncancel={(e) => { e.preventDefault(); finish(false) }}>
  {#if request}
    <form method="dialog" onsubmit={(e) => { e.preventDefault(); finish(true) }}>
      <div class="head">
        <h2 id="review-title">Review {request.length === 1 ? 'the update' : `${request.length} updates`}</h2>
        <p class="muted">
          packraven downloads each update, replaces the old file, and installs any mods it newly requires. Untick anything you want to leave for now.
        </p>
      </div>
      {#if request.length > 1}
        <div class="bulk">
          <button type="button" class="link" onclick={() => (chosen = new Set(request.map((u) => u.mod.path)))}>Select all</button>
          <button type="button" class="link" onclick={() => (chosen = new Set())}>Select none</button>
        </div>
      {/if}
      <ul class="updates" aria-label="Updates">
        {#each request as update (update.mod.path)}
          {@const disabled = update.mod.path.endsWith('.disabled')}
          {@const changelog = changelogs[update.file.file]}
          <li>
            <label class="check">
              <input type="checkbox" checked={chosen.has(update.mod.path)} onchange={(e) => toggle(update.mod.path, e.currentTarget.checked)} />
              <span class="name">{update.mod.name}</span>
              {#if update.kind}<span class="muted note">{update.kind}</span>{/if}
              {#if disabled}<span class="muted note">Disabled</span>{/if}
            </label>
            <div class="files">
              <span class="file muted">{fileName(update.mod.path)}</span>
              <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2 6h8M7 3l3 3-3 3" /></svg>
              <span class="file next">{update.file.fileName}</span>
              {#if update.file.releaseType !== 'release'}<span class="chip pending">{typeLabels[update.file.releaseType]}</span>{/if}
              <button type="button" class="link changelog-link" aria-expanded={open === update.mod.path} onclick={() => showChangelog(update)}>
                {open === update.mod.path ? 'Hide changelog' : 'Changelog'}
              </button>
            </div>
            {#if open === update.mod.path}
              <div class="changelog">
                {#if changelog === undefined}
                  <p class="muted">Loading the changelog from CurseForge.</p>
                {:else if typeof changelog !== 'string'}
                  <p class="failed">{changelog.error}</p>
                {:else}
                  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (links inside are real links; this only routes their clicks) -->
                  <div onclick={onChangelogClick}>{@html changelog}</div>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
      <div class="actions">
        <button type="button" class="quiet" onclick={() => finish(false)}>Cancel</button>
        <button type="submit" class="primary" bind:this={confirm} disabled={!count}>
          {count === 1 ? 'Update 1 file' : `Update ${count} files`}
        </button>
      </div>
    </form>
  {/if}
</dialog>

<style>
  dialog {
    width: min(640px, calc(100vw - 32px));
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

  form {
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
  }

  p {
    margin: 0;
  }

  .bulk {
    display: flex;
    gap: 14px;
    font-size: var(--step--1);
  }

  .updates {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 0;
    list-style: none;
    background: var(--field);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
  }

  .updates li {
    display: grid;
    gap: 2px;
    padding: 8px 12px;
  }

  .updates li + li {
    border-top: 1px solid var(--line);
  }

  .name {
    font-weight: 700;
  }

  .note {
    font-size: var(--step--1);
  }

  .files {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px 6px;
    padding-left: 26px;
    font-size: var(--step--1);
  }

  .files .file {
    overflow-wrap: anywhere;
  }

  .files .next {
    color: var(--ochre-ink);
  }

  .files svg {
    width: 11px;
    height: 11px;
    fill: none;
    stroke: var(--ochre-ink);
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .changelog-link {
    margin-left: auto;
  }

  .changelog {
    margin: 4px 0 2px 26px;
    padding: 8px 10px;
    max-height: 200px;
    overflow: auto;
    font-size: var(--step--1);
    background: var(--paper);
    border-radius: var(--radius-control);
  }

  .changelog :global(img) {
    max-width: 100%;
    height: auto;
  }

  .changelog :global(h1),
  .changelog :global(h2),
  .changelog :global(h3) {
    font-size: var(--step-0);
    margin: 6px 0 4px;
  }

  .changelog :global(ul) {
    margin: 4px 0;
    padding-left: 18px;
  }

  .actions {
    display: flex;
    justify-content: end;
    gap: 6px;
  }
</style>
