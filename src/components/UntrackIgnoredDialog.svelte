<script lang="ts" module>
  export type UntrackResult = { untracked: string[]; kept: string[]; stillIgnored: string[] }
</script>

<script lang="ts">
  import { tick } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import { api, errorText } from '../lib/api'
  import { untrackConsequence } from '../lib/gitActions'

  let { paths, ondone, oncancel }: { paths: string[] | null; ondone: (result: UntrackResult) => void; oncancel: () => void } = $props()

  let dialog: HTMLDialogElement
  let list = $state<HTMLUListElement>()
  let all = $state<HTMLInputElement>()
  const chosen = new SvelteSet<string>()
  let keepUnchecked = $state(true)
  let busy = $state(false)
  let error = $state<string | null>(null)

  // We focus a checkbox rather than the button, so Enter right after opening doesn't untrack everything.
  $effect(() => {
    if (!paths) return
    chosen.clear()
    for (const path of paths) chosen.add(path)
    keepUnchecked = true
    error = null
    dialog.showModal()
    tick().then(() => (all ?? list?.querySelector('input'))?.focus())
  })

  let allChosen = $derived(!!paths && chosen.size === paths.length)
  let unchecked = $derived(paths?.filter((p) => !chosen.has(p)) ?? [])
  let keeping = $derived(keepUnchecked && unchecked.length > 0)

  $effect(() => {
    if (all) all.indeterminate = chosen.size > 0 && !allChosen
  })

  function toggleAll() {
    if (allChosen) chosen.clear()
    else for (const path of paths ?? []) chosen.add(path)
  }

  function toggle(path: string) {
    if (!chosen.delete(path)) chosen.add(path)
  }

  // Chromium submits the form on Enter in a checkbox, which would untrack everything as the dialog opens.
  function ignoreEnter(e: KeyboardEvent) {
    if (e.key === 'Enter') e.preventDefault()
  }

  function cancel() {
    if (dialog.open) dialog.close()
    oncancel()
  }

  function files(n: number) {
    return n === 1 ? '1 file' : `${n} files`
  }

  let submitLabel = $derived(chosen.size ? `Stop tracking ${files(chosen.size)}` : keeping ? `Keep ${files(unchecked.length)} tracked` : 'Stop tracking')

  async function submit() {
    if (!paths || !(chosen.size || keeping) || busy) return
    const untracked = paths.filter((p) => chosen.has(p))
    const kept = keeping ? unchecked : []
    busy = true
    try {
      if (untracked.length) await api.gitUntrack(untracked)
      const stillIgnored = kept.length ? await api.gitKeepTracking(kept) : []
      if (dialog.open) dialog.close()
      ondone({ untracked, kept, stillIgnored })
    } catch (e) {
      error = errorText(e)
    } finally {
      busy = false
    }
  }
</script>

<dialog bind:this={dialog} aria-labelledby="untrack-title" aria-describedby="untrack-body untrack-consequence" oncancel={(e) => { e.preventDefault(); cancel() }}>
  {#if paths}
    {@const one = paths.length === 1}
    <form method="dialog" onsubmit={(e) => { e.preventDefault(); submit() }}>
      <h2 id="untrack-title">Stop tracking ignored files</h2>
      <p id="untrack-body">
        Your .gitignore leaves {one ? 'this file' : 'these files'} out, but git still tracks {one ? 'it' : 'them'}, because .gitignore has no effect on files
        already in the repo.
      </p>
      {#if !one}
        <label class="check all">
          <input type="checkbox" bind:this={all} checked={allChosen} onchange={toggleAll} onkeydown={ignoreEnter} />
          <span>All {paths.length} files</span>
        </label>
      {/if}
      <ul class="paths" bind:this={list} aria-label="Tracked files your .gitignore leaves out">
        {#each paths as path (path)}
          <li>
            <label class="check">
              <input type="checkbox" checked={chosen.has(path)} onchange={() => toggle(path)} onkeydown={ignoreEnter} />
              <span class="file">{path}</span>
            </label>
          </li>
        {/each}
      </ul>
      {#if chosen.size}<p id="untrack-consequence">{untrackConsequence(chosen.size)}</p>{/if}
      {#if unchecked.length}
        <label class="check keep">
          <input type="checkbox" bind:checked={keepUnchecked} onkeydown={ignoreEnter} />
          <span>
            Keep {unchecked.length === 1 ? 'the unchecked file' : `the ${unchecked.length} unchecked files`} tracked. We add
            {unchecked.length === 1 ? 'an exception' : 'exceptions'} to .gitignore, so packraven stops asking.
          </span>
        </label>
      {/if}

      {#if error}<p class="failed" role="alert">{error}</p>{/if}

      <div class="actions">
        <button type="button" class="quiet" onclick={cancel}>Cancel</button>
        <button type="submit" class="primary" class:busy disabled={busy || !(chosen.size || keeping)}>{submitLabel}</button>
      </div>
    </form>
  {/if}
</dialog>

<style>
  dialog {
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
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
    display: grid;
    gap: 14px;
    padding: 20px;
  }

  h2 {
    font-size: var(--step-1);
  }

  p {
    margin: 0;
    font-size: var(--step--1);
  }

  .all,
  .keep {
    font-size: var(--step--1);
  }

  .keep {
    align-items: start;
  }

  .keep input {
    margin-top: 0.2em;
  }

  .paths {
    max-height: 220px;
    overflow: auto;
    margin: 0;
    padding: 4px 10px;
    list-style: none;
    background: var(--field);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
  }

  .paths .check {
    align-items: start;
    padding-block: 3px;
    font-size: var(--step--2);
  }

  .paths input {
    margin-top: 0.15em;
  }

  .paths .file {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  /* At high zoom the dialog scrolls, and the actions stay in view. */
  .actions {
    position: sticky;
    bottom: 0;
    display: flex;
    justify-content: end;
    gap: 6px;
    margin: 0 -20px -20px;
    padding: 12px 20px 20px;
    background: var(--paper);
  }
</style>
