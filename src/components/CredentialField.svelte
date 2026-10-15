<script lang="ts">
  import type { Snippet } from 'svelte'
  import { api, errorText, type Credential, type CredentialStatus } from '../lib/api'

  let {
    credential,
    label,
    status,
    keychain,
    envNames,
    onchange,
    help,
  }: {
    credential: Credential
    label: string
    status: CredentialStatus
    keychain: string
    envNames: string[]
    onchange: () => void
    help: Snippet
  } = $props()

  let mode = $state<'stored' | 'env'>('stored')
  let secret = $state('')
  let revealed = $state(false)
  let envName = $state('')
  let busy = $state(false)
  let error = $state<string | null>(null)

  $effect.pre(() => {
    mode = status.source.kind === 'env' ? 'env' : 'stored'
    envName = status.source.kind === 'env' ? status.source.name : ''
  })

  const summary = $derived.by(() => {
    const source = status.source
    if (source.kind === 'stored') return status.present ? `Saved in ${keychain}` : `Missing from ${keychain}; paste it again`
    if (source.kind === 'env') return status.present ? `Read from ${source.name}` : `${source.name} is empty or not set`
    return 'Not set'
  })

  async function run(action: () => Promise<void>) {
    busy = true
    try {
      await action()
      error = null
      secret = ''
      revealed = false
      onchange()
    } catch (e) {
      error = errorText(e)
    } finally {
      busy = false
    }
  }
</script>

<fieldset>
  <legend>{label}</legend>
  <p class:missing={!status.present} class="summary" role="status">{summary}</p>
  {@render help()}

  <div class="modes" role="radiogroup" aria-label="Where packraven gets the {label}">
    <label class="check"><input type="radio" bind:group={mode} value="stored" /> Paste the key</label>
    <label class="check"><input type="radio" bind:group={mode} value="env" /> Use an environment variable</label>
  </div>

  {#if mode === 'stored'}
    <form class="row" onsubmit={(e) => { e.preventDefault(); run(() => api.credentialStore(credential, secret)) }}>
      <label class="grow">
        <span class="visually-hidden">{label}</span>
        <input type={revealed ? 'text' : 'password'} bind:value={secret} autocomplete="off" spellcheck="false"
          placeholder={status.source.kind === 'stored' && status.present ? 'Paste a new key to replace the saved one' : 'Paste the key'} />
      </label>
      <button type="button" aria-pressed={revealed} onclick={() => (revealed = !revealed)}>{revealed ? 'Hide' : 'Show'}</button>
      <button type="submit" class="primary" class:busy disabled={busy || !secret.trim()}>Save</button>
    </form>
    <p class="muted hint">packraven stores this key in {keychain}; it doesn't hold secrets itself.</p>
  {:else}
    <form class="row" onsubmit={(e) => { e.preventDefault(); run(() => api.credentialUseEnv(credential, envName)) }}>
      <label class="grow">
        <span class="visually-hidden">Environment variable</span>
        <select bind:value={envName}>
          <option value="" disabled>Choose a variable…</option>
          {#each envNames as name (name)}<option value={name}>{name}</option>{/each}
        </select>
      </label>
      <button type="submit" class="primary" class:busy disabled={busy || !envName || (status.source.kind === 'env' && status.source.name === envName)}>Use</button>
    </form>
    <p class="muted hint">
      {keychain.startsWith('Windows')
        ? 'Select an existing Windows user variable. packraven picks up new user variables without a restart.'
        : 'packraven only sees variables that were set before it started, so restart it after you add one.'}
    </p>
  {/if}

  {#if status.source.kind !== 'none'}
    <div><button type="button" class="quiet" disabled={busy} onclick={() => run(() => api.credentialClear(credential))}>Remove</button></div>
  {/if}
  {#if error}<p class="failed" role="alert">{error}</p>{/if}
</fieldset>

<style>
  fieldset {
    display: grid;
    gap: 10px;
    margin: 0;
    padding: 0;
    border: 0;
  }

  legend {
    padding: 0;
    margin-bottom: 4px;
    font-weight: 600;
  }

  .summary {
    color: var(--muted);
  }

  .summary.missing {
    color: var(--warn);
  }

  .modes {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 20px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .grow {
    flex: 1;
  }

  .hint {
    font-size: var(--step--1);
  }
</style>
