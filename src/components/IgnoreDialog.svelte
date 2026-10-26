<script lang="ts">
  import { tick } from 'svelte'
  import { api, errorText, type IgnorePreview } from '../lib/api'
  import { ignoreDialog } from '../lib/ignoreDialog.svelte'
  import { reportUntracked, untrackConsequence } from '../lib/gitActions'

  const PREVIEW_DELAY = 200
  const MAX_LISTED = 100

  let dialog: HTMLDialogElement
  let field = $state<HTMLTextAreaElement>()
  let text = $state('')
  let preview = $state<IgnorePreview | null>(null)
  let previewError = $state<string | null>(null)
  let untrackMatches = $state(true)
  let busy = $state(false)
  let error = $state<string | null>(null)
  let previewTimer = 0
  let previewSeq = 0

  let request = $derived(ignoreDialog.request)
  let patterns = $derived(text.split('\n').filter((line) => line.trim()))
  let chosen = $derived(request?.options.findIndex((o) => o.patterns.join('\n') === patterns.join('\n')) ?? -1)
  let untrackMode = $derived(!!request?.untrack)
  let matchesNothing = $derived(untrackMode && !!preview && !preview.tracked.length)

  $effect(() => {
    if (!request) return
    text = request.options[request.chosen]?.patterns.join('\n') ?? ''
    error = null
    untrackMatches = true
    dialog.showModal()
    tick().then(() => field?.focus())
  })

  $effect(() => {
    const current = patterns
    clearTimeout(previewTimer)
    if (!request) return
    const seq = ++previewSeq
    previewTimer = setTimeout(() => {
      api
        .gitIgnorePreview(current)
        .then((result) => seq === previewSeq && ((preview = result), (previewError = null)))
        .catch((e) => seq === previewSeq && (previewError = errorText(e)))
    }, PREVIEW_DELAY)
  })

  function close() {
    clearTimeout(previewTimer)
    ignoreDialog.request = null
    preview = null
    if (dialog.open) dialog.close()
  }

  async function submit() {
    if (!request || !patterns.length || busy) return
    const { feedback } = request
    busy = true
    try {
      const tracked = await api.gitIgnore(patterns)
      if (tracked.length && (untrackMatches || untrackMode)) await api.gitUntrack(tracked)
      close()
      feedback.ondone()
      if (tracked.length && untrackMode) reportUntracked(feedback, tracked)
      else if (tracked.length && !untrackMatches) {
        const names = tracked.length === 1 ? tracked[0] : `${tracked.length} matching files`
        feedback.onnotice(`Git still tracks ${names}, so .gitignore won't apply until you stop tracking ${tracked.length === 1 ? 'it' : 'them'}.`)
      }
    } catch (e) {
      error = errorText(e)
    } finally {
      busy = false
    }
  }

  function count(n: number, noun: string) {
    return `${n} ${noun}${n === 1 ? '' : 's'}`
  }

  let previewSummary = $derived(
    !preview || !patterns.length
      ? ''
      : untrackMode
        ? [
            preview.tracked.length ? `Git will stop tracking ${count(preview.tracked.length, 'matching file')}.` : "These patterns don't match any tracked files.",
            preview.untracked.length ? `Git will also stop showing ${count(preview.untracked.length, 'new file')}.` : '',
          ].join(' ')
        : [
          preview.untracked.length ? `Git will stop showing ${count(preview.untracked.length, 'new file')}.` : "These patterns don't match any new files.",
          preview.tracked.length ? `Git already tracks ${count(preview.tracked.length, 'matching file')}.` : '',
        ].join(' '),
  )
</script>

{#snippet pathList(paths: string[], label: string)}
  <ul class="paths" aria-label={label}>
    {#each paths.slice(0, MAX_LISTED) as path (path)}<li class="file">{path}</li>{/each}
    {#if paths.length > MAX_LISTED}<li class="muted">and {paths.length - MAX_LISTED} more</li>{/if}
  </ul>
{/snippet}

<dialog bind:this={dialog} aria-labelledby="ignore-title" oncancel={close} onclose={() => ignoreDialog.request && close()}>
  {#if request}
    <form method="dialog" onsubmit={(e) => { e.preventDefault(); submit() }}>
      <h2 id="ignore-title">{untrackMode ? 'Stop tracking' : 'Add to .gitignore'}</h2>
      {#if untrackMode}
        <p class="lede">
          Your .gitignore doesn't cover {request.paths.length === 1 ? 'this file' : 'every file you picked'} yet, so git would list {request.paths.length === 1 ? 'it' : 'them'} as new
          again. Choose a pattern to add as we stop tracking.
        </p>
      {/if}

      {#if request.options.length > 1}
        <fieldset class="options">
          <legend class="visually-hidden">Suggested patterns</legend>
          {#each request.options as option, i (option.label)}
            <label class="check">
              <input type="radio" name="ignore-option" checked={chosen === i} onchange={() => (text = option.patterns.join('\n'))} />
              <span>{option.label}</span>
              <code class="muted">{option.patterns.length === 1 ? option.patterns[0] : `${option.patterns.length} lines`}</code>
            </label>
          {/each}
        </fieldset>
      {/if}

      <label class="patterns">
        <span>Patterns, one per line</span>
        <textarea bind:this={field} bind:value={text} rows={Math.min(Math.max(patterns.length, 2), 8)} spellcheck="false"
          onkeydown={(e) => (e.ctrlKey || e.metaKey) && e.key === 'Enter' && (e.preventDefault(), submit())}></textarea>
      </label>

      <p class="visually-hidden" role="status">{previewError ?? previewSummary}</p>
      <div class="preview">
        {#if previewError}
          <p class="failed">{previewError}</p>
        {:else if !patterns.length}
          <p class="muted">Enter a pattern to see which files it matches.</p>
        {:else if !preview}
          <p class="muted">Checking which files these patterns match.</p>
        {:else if untrackMode}
          {#if preview.tracked.length}
            <p>Git will stop tracking {count(preview.tracked.length, 'matching file')}:</p>
            {@render pathList(preview.tracked, 'Tracked files these patterns match')}
            <p>{untrackConsequence(preview.tracked.length)}</p>
          {:else}
            <p class="muted">These patterns don't match any tracked files.</p>
          {/if}
          {#if preview.untracked.length}
            <p class="muted">Git will also stop showing {count(preview.untracked.length, 'new file')} these patterns match.</p>
          {/if}
        {:else}
          {#if preview.untracked.length}
            <p>Git will stop showing {count(preview.untracked.length, 'new file')}:</p>
            {@render pathList(preview.untracked, 'New files these patterns ignore')}
          {:else}
            <p class="muted">These patterns don't match any new files.</p>
          {/if}
          {#if preview.tracked.length}
            {@const one = preview.tracked.length === 1}
            <p>Git already tracks {count(preview.tracked.length, 'matching file')}, and .gitignore doesn't apply to {one ? 'it' : 'those'}:</p>
            {@render pathList(preview.tracked, 'Tracked files these patterns match')}
            <label class="check untrack">
              <input type="checkbox" bind:checked={untrackMatches} />
              <span>Stop tracking {one ? 'it' : 'them'}. {untrackConsequence(preview.tracked.length)}</span>
            </label>
          {/if}
        {/if}
      </div>

      {#if error}<p class="failed" role="alert">{error}</p>{/if}

      <div class="actions">
        <button type="button" class="quiet" onclick={close}>Cancel</button>
        <button type="submit" class="primary" class:busy disabled={busy || !patterns.length || matchesNothing}>{untrackMode ? 'Stop tracking and ignore' : 'Add to .gitignore'}</button>
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

  .lede {
    margin-top: -6px;
    font-size: var(--step--1);
  }

  .options {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    border: 0;
  }

  .options label {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }

  .options code {
    margin-left: auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-log);
    font-size: var(--step--2);
  }

  .patterns {
    display: grid;
    gap: 4px;
  }

  .patterns textarea {
    width: 100%;
    font-family: var(--font-log);
    font-size: var(--step--1);
  }

  .preview {
    display: grid;
    gap: 6px;
    font-size: var(--step--1);
  }

  .paths {
    max-height: 140px;
    overflow: auto;
    margin: 0;
    padding: 6px 10px;
    list-style: none;
    font-size: var(--step--2);
    background: var(--field);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    overflow-wrap: anywhere;
  }

  .untrack {
    align-items: start;
  }

  .untrack input {
    margin-top: 0.2em;
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
