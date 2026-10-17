<script lang="ts" module>
  export type Dependent = { name: string; path: string; depth: number }
</script>

<script lang="ts">
  import { tick } from 'svelte'

  let {
    request,
    onconfirm,
    oncancel,
  }: { request: { name: string; dependents: Dependent[] } | null; onconfirm: (cascade: boolean) => void; oncancel: () => void } = $props()

  let dialog: HTMLDialogElement
  let confirm = $state<HTMLButtonElement>()
  let cascade = $state(true)

  $effect(() => {
    if (!request) return
    cascade = true
    dialog.showModal()
    tick().then(() => confirm?.focus())
  })

  function finish(confirmed: boolean) {
    if (dialog.open) dialog.close()
    if (confirmed) onconfirm(cascade)
    else oncancel()
  }

  let total = $derived((request?.dependents.length ?? 0) + 1)
</script>

<dialog bind:this={dialog} aria-labelledby="disable-title" aria-describedby="disable-body" oncancel={(e) => { e.preventDefault(); finish(false) }}>
  {#if request}
    <form method="dialog" onsubmit={(e) => { e.preventDefault(); finish(true) }}>
      <h2 id="disable-title">Disable {request.name}</h2>
      <p id="disable-body">
        {request.dependents.length === 1 ? 'This mod requires' : 'These mods require'}
        {request.name}, directly or through another mod, and may not load without it:
      </p>
      <ul class="dependents" aria-label="Mods that require {request.name}">
        {#each request.dependents as dependent (dependent.path)}
          <li style="padding-left: {(dependent.depth - 1) * 18}px">
            {#if dependent.depth > 1}<span class="branch" aria-hidden="true"></span>{/if}{dependent.name}
          </li>
        {/each}
      </ul>
      <label class="check">
        <input type="checkbox" bind:checked={cascade} />
        <span>Disable {request.dependents.length === 1 ? 'it' : `these ${request.dependents.length} mods`} too</span>
      </label>
      <div class="actions">
        <button type="button" class="quiet" onclick={() => finish(false)}>Cancel</button>
        <button type="submit" class="primary" bind:this={confirm}>{cascade ? `Disable ${total} files` : `Disable ${request.name} only`}</button>
      </div>
    </form>
  {/if}
</dialog>

<style>
  dialog {
    width: min(520px, calc(100vw - 32px));
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
  }

  .dependents {
    max-height: 220px;
    overflow: auto;
    margin: 0;
    padding: 6px 10px;
    list-style: none;
    background: var(--field);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
  }

  .dependents li {
    padding-block: 2px;
  }

  /* An elbow shows which mod each nested one needs. */
  .branch {
    display: inline-block;
    width: 10px;
    height: 0.6em;
    margin-right: 6px;
    border-left: 1px solid var(--line-strong);
    border-bottom: 1px solid var(--line-strong);
    vertical-align: 0.1em;
  }

  .actions {
    display: flex;
    justify-content: end;
    gap: 6px;
  }
</style>
