<script lang="ts">
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

  const NAME = { curseforge: 'CurseForge', modrinth: 'Modrinth' } as const
  let name = $derived(NAME[platform])
  const id = (part: string) => `${platform}-${part}`

  // Changes saves each choice as you make it, so changing one here saves right away too.
  let choices = $state<OverrideChoices | null>(null)
  let changing = $state<string | null>(null)
  let error = $state<string | null>(null)
  let newAuthor = $state('')

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
      What the {name} pack does with {platform === 'curseforge' ? 'jars' : 'files'} {name} doesn't host. Unlike the settings above, packraven saves these as soon
      as you change them. Decide again to put a file back in Publish, where it blocks publishing to {name} until you choose.
    </p>
    {#snippet fileRow(file: OverrideFileView, actions: { label: string; choice: OverrideChoice | null; quiet?: boolean }[])}
      {@const label = file.name ?? file.path}
      <li>
        <span class="what">
          <span>
            {label}{#if file.license}<span class="muted"> · {file.license}</span>{/if}
            {#if platform === 'modrinth' && file.openLicense}{' '}<span class="chip ok">open license</span>{/if}
            {#if platform === 'curseforge' && file.allowlisted}{' '}<span class="chip ok">on CurseForge's list</span>{/if}
          </span>
          {#if file.name}<span class="file path muted">{file.path}</span>{/if}
        </span>
        <span class="choice-actions">
          {#each actions as action (action.label)}
            <button type="button" class:quiet={action.quiet} class:busy={changing === file.path && !action.quiet} disabled={!!changing} onclick={() => changeChoice(file.path, action.choice)}>
              {action.label}<span class="visually-hidden">: {label}</span>
            </button>
          {/each}
        </span>
      </li>
    {/snippet}
    {#each choices.byAuthor as group, index (group.author)}
      <div class="field">
        <h4 id={id(`author-${index}`)}>On {group.author}'s permission</h4>
        <p class="hint">
          The {name} pack includes everything by {group.author}, including mods of theirs you add later.
          {#if !group.files.length}None of their files are in the pack yet.{/if}
        </p>
        {#if group.files.length}
          <ul class="choices" aria-labelledby={id(`author-${index}`)}>
            {#each group.files as file (file.path)}{@render fileRow(file, [{ label: 'Omit instead', choice: 'leaveOut' }])}{/each}
          </ul>
        {/if}
        <div>
          <button type="button" class:busy={changing === group.author} disabled={!!changing} onclick={() => setAuthor(group.author, false)}>
            Forget {group.author}'s permission
          </button>
        </div>
      </div>
    {/each}
    {#if choices.shipped.length}
      <div class="field">
        <!-- CurseForge checks these jars against its list itself, so only Modrinth's rest on your word. -->
        <h4 id={id('shipped-title')}>{platform === 'modrinth' ? 'Included on your word' : 'Included in the CurseForge pack'}</h4>
        <ul class="choices" aria-labelledby={id('shipped-title')}>
          {#each choices.shipped as file (file.path)}{@render fileRow(file, [{ label: 'Omit instead', choice: 'leaveOut' }, { label: 'Decide again', choice: null, quiet: true }])}{/each}
        </ul>
      </div>
    {/if}
    {#if choices.keptOff.length}
      <div class="field">
        <h4 id={id('kept-off-title')}>Omitted from the {name} pack</h4>
        <ul class="choices" aria-labelledby={id('kept-off-title')}>
          {#each choices.keptOff as file (file.path)}{@render fileRow(file, [{ label: 'Include instead', choice: 'ship' }, { label: 'Decide again', choice: null, quiet: true }])}{/each}
        </ul>
      </div>
    {/if}
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

  h4 {
    font-size: var(--step-0);
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
    color: var(--muted);
    font-size: var(--step--1);
  }

  .path {
    overflow-wrap: anywhere;
  }

  .choices {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .choices li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .choice-actions {
    display: flex;
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
