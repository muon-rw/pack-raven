<script lang="ts">
  import Twisty from './Twisty.svelte'
  import { api, errorText, type OverrideChoice, type OverrideChoices, type OverrideFileView, type Platform } from '../lib/api'

  let {
    platform,
    targeted,
    reloadKey,
    onchange,
  }: {
    platform: Platform
    // The pack goes to this platform, so we show the section even before you decide about a file.
    targeted: boolean
    // Pack settings changes this after a save, which can move files between groups.
    reloadKey: unknown
    onchange: () => void
  } = $props()

  type Action = { label: string; path?: string; choice: OverrideChoice | null; quiet?: boolean }

  const NAME = { curseforge: 'CurseForge', modrinth: 'Modrinth' } as const
  let name = $derived(NAME[platform])
  const id = (part: string) => `${platform}-${part}`

  // Publish saves each choice as you make it, so changing one here saves right away too.
  let choices = $state<OverrideChoices | null>(null)
  let changing = $state<string | null>(null)
  let error = $state<string | null>(null)
  let newAuthor = $state('')
  let groupsOpen = $state<Record<string, boolean>>({})

  let openLicense = $derived(platform === 'modrinth' ? (choices?.shipped.filter((file) => file.openLicense) ?? []) : [])
  let onYourWord = $derived(platform === 'modrinth' ? (choices?.shipped.filter((file) => !file.openLicense) ?? []) : (choices?.shipped ?? []))

  const shippedActions = (file: OverrideFileView): Action[] => [
    ...(file.anyVersion ? [{ label: 'Include every version', path: file.anyVersion, choice: 'ship' as const }] : []),
    { label: 'Leave out instead', choice: 'leaveOut' },
    { label: 'Decide again', choice: null, quiet: true },
  ]

  const keptOffActions = (file: OverrideFileView): Action[] => [
    ...(file.anyVersion ? [{ label: 'Leave out every version', path: file.anyVersion, choice: 'leaveOut' as const }] : []),
    { label: 'Include instead', choice: 'ship' },
    { label: 'Decide again', choice: null, quiet: true },
  ]

  async function load() {
    choices = await api.overrideChoices(platform).catch(() => null)
  }

  $effect(() => {
    void reloadKey
    load()
  })

  async function changeChoice(path: string, choice: OverrideChoice | null) {
    changing = path
    try {
      await api.setOverrideChoice(platform, [path], choice)
      error = null
      await load()
      onchange()
    } catch (failure) {
      error = `Couldn't change the choice for ${path}: ${errorText(failure)}`
    } finally {
      changing = null
    }
  }

  async function setAuthor(author: string, allowed: boolean) {
    changing = author
    try {
      await api.setModrinthAuthor(author, allowed)
      error = null
      await load()
      onchange()
      return true
    } catch (failure) {
      error = `Couldn't ${allowed ? 'add' : 'forget'} ${author}'s permission: ${errorText(failure)}`
      return false
    } finally {
      changing = null
    }
  }

  async function addAuthor() {
    if (await setAuthor(newAuthor.trim(), true)) newAuthor = ''
  }
</script>

{#if choices && (choices.shipped.length || choices.keptOff.length || choices.byAuthor.length || targeted)}
  <section aria-labelledby={id('choices-title')}>
    <h3 id={id('choices-title')} tabindex="-1">{name} permissions</h3>
    <p class="hint">
      What we'll do with {platform === 'curseforge' ? 'jars' : 'files'} not found on {name} for {name} packs.
      If you remove a setting here, it will reappear in Publish, and you'll be blocked from publishing until you choose again.
    </p>
    {#snippet fileRow(file: OverrideFileView, actions: Action[])}
      {@const label = file.name ?? file.path}
      {@const everyVersion = file.path.includes('*')}
      {@const a11yLabel = everyVersion && file.name ? `${label}, every version` : label}
      <li>
        <span class="what">
          <span>
            {#if everyVersion && !file.name}<span class="file">{file.path}</span>{:else}{label}{/if}{#if everyVersion && file.name}, every version{/if}{#if file.license}<span class="muted">{' · '}{file.license}</span>{/if}
            {#if platform === 'modrinth' && file.openLicense}{' '}<span class="chip ok">open license</span>{/if}
            {#if platform === 'curseforge' && file.allowlisted}{' '}<span class="chip ok">on CurseForge's list</span>{/if}
          </span>
          {#if file.name && !everyVersion}<span class="file path muted">{file.path}</span>{/if}
          {#if everyVersion}
            <span class="hint path">
              {#if file.name}<span class="file">{file.path}</span> · {/if}{#if file.matches.length}
                Matches <span class="file">{file.matches.map((path) => path.slice(path.lastIndexOf('/') + 1)).join(', ')}</span> now.
              {:else}
                No version of it is in the pack now.
              {/if}
            </span>
          {/if}
        </span>
        <span class="choice-actions">
          {#each actions as action (action.label)}
            {@const targetPath = action.path ?? file.path}
            <button type="button" class:quiet={action.quiet} class:busy={changing === targetPath && !action.quiet} disabled={!!changing} onclick={() => changeChoice(targetPath, action.choice)}>
              {action.label}<span class="visually-hidden">: {a11yLabel}</span>
            </button>
          {/each}
        </span>
      </li>
    {/snippet}
    {#snippet choiceGroup(key: string, titleId: string, title: string, files: OverrideFileView[], actionsFor: (file: OverrideFileView) => Action[])}
      {#if files.length === 1}
        <div class="field">
          <h4 id={titleId}>{title}</h4>
          <ul class="choices" aria-labelledby={titleId}>
            {@render fileRow(files[0], actionsFor(files[0]))}
          </ul>
        </div>
      {:else if files.length > 1}
        <details class="field choice-group" bind:open={groupsOpen[key]}>
          <summary><Twisty open={!!groupsOpen[key]} /><span class="group-title" id={titleId}>{title}</span> <span class="muted count">({files.length} files)</span></summary>
          <ul class="choices" aria-labelledby={titleId}>
            {#each files as file (file.path)}{@render fileRow(file, actionsFor(file))}{/each}
          </ul>
        </details>
      {/if}
    {/snippet}
    {#each choices.byAuthor as group, index (group.author)}
      {@const titleId = id(`author-${index}`)}
      {@const key = `author-${group.author}`}
      {#snippet authorBody()}
        <p class="hint">
          The {name} pack includes everything by {group.author}, including mods of theirs you add later.
          {#if !group.files.length}None of their files are in the pack yet.{/if}
        </p>
        {#if group.files.length}
          <ul class="choices" aria-labelledby={titleId}>
            {#each group.files as file (file.path)}{@render fileRow(file, [{ label: 'Leave out instead', choice: 'leaveOut' }])}{/each}
          </ul>
        {/if}
        <div>
          <button type="button" class:busy={changing === group.author} disabled={!!changing} onclick={() => setAuthor(group.author, false)}>
            Forget {group.author}'s permission
          </button>
        </div>
      {/snippet}
      {#if group.files.length > 1}
        <details class="field choice-group" bind:open={groupsOpen[key]}>
          <summary><Twisty open={!!groupsOpen[key]} /><span class="group-title" id={titleId}>On {group.author}'s permission</span> <span class="muted count">({group.files.length} files)</span></summary>
          {@render authorBody()}
        </details>
      {:else}
        <div class="field">
          <h4 id={titleId}>On {group.author}'s permission</h4>
          {@render authorBody()}
        </div>
      {/if}
    {/each}
    {@render choiceGroup('open-license', id('open-license-title'), 'Included under an open license', openLicense, shippedActions)}
    <!-- CurseForge checks these jars against its list itself, so only Modrinth's rest on your word. -->
    {@render choiceGroup('shipped', id('shipped-title'), platform === 'modrinth' ? 'Included on your word' : 'Included in the CurseForge pack', onYourWord, shippedActions)}
    {@render choiceGroup('kept-off', id('kept-off-title'), `Left out of the ${name} pack`, choices.keptOff, keptOffActions)}
    {#if platform === 'modrinth'}
      <form class="field" onsubmit={(e) => { e.preventDefault(); addAuthor() }}>
        <label for={id('new-author')}>Add an author who let you share their work</label>
        <div class="inline">
          <input id={id('new-author')} bind:value={newAuthor} placeholder="Their CurseForge or Modrinth name" aria-describedby={id('new-author-hint')} />
          <button type="submit" disabled={!!changing || !newAuthor.trim()}>Add</button>
        </div>
        <p class="hint" id={id('new-author-hint')}>Modrinth's moderators may ask you for proof, so keep the message where they said yes.</p>
      </form>
    {:else if !choices.shipped.length && !choices.keptOff.length}
      <p class="hint">You haven't decided about any jars CurseForge doesn't host yet.</p>
    {/if}
    {#if error}<p class="failed" role="alert">{error}</p>{/if}
  </section>
{/if}

<style>
  section {
    display: grid;
    gap: 14px;
  }

  h4,
  .group-title {
    font-size: var(--step-0);
    font-weight: 700;
  }

  .choice-group summary {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    width: fit-content;
    list-style: none;
    cursor: pointer;
  }

  .choice-group summary::-webkit-details-marker {
    display: none;
  }

  .choice-group .count {
    margin-left: 2px;
    font-size: var(--step--1);
  }

  .field {
    display: grid;
    gap: 6px;
  }

  .field > label {
    color: var(--text);
    font-size: var(--step-0);
  }

  .inline {
    display: flex;
    gap: 8px;
  }

  .inline input {
    flex: 1;
  }

  .hint {
    max-width: 52ch;
    color: var(--muted);
    font-size: var(--step--1);
  }

  .path {
    overflow-wrap: anywhere;
  }

  .choices {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .choices li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 6px 12px;
  }

  .choice-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .what {
    display: grid;
    gap: 1px;
    min-width: 0;
  }

  .what .file {
    font-size: var(--step--1);
  }
</style>
