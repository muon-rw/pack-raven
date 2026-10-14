<script lang="ts">
  import { theme, cycleThemeMode, type ThemeMode } from '../lib/theme.svelte'

  const NAMES: Record<ThemeMode, string> = { light: 'Light', dark: 'Dark', auto: 'Auto' }
  const NEXT: Record<ThemeMode, ThemeMode> = { light: 'dark', dark: 'auto', auto: 'light' }

  let label = $derived(theme.mode === 'auto' ? `Auto, following the system (${theme.current})` : NAMES[theme.mode])
</script>

<button class="toggle" onclick={cycleThemeMode} aria-label="Theme: {label}" title="Theme: {label}. Click for {NAMES[NEXT[theme.mode]]}.">
  <svg viewBox="0 0 24 24" aria-hidden="true">
    {#if theme.mode === 'light'}
      <circle cx="12" cy="12" r="4.5" />
      <path d="M12 2.5v2.5M12 19v2.5M2.5 12H5M19 12h2.5M5.3 5.3l1.8 1.8M16.9 16.9l1.8 1.8M5.3 18.7l1.8-1.8M16.9 7.1l1.8-1.8" />
    {:else if theme.mode === 'dark'}
      <path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5Z" />
    {:else}
      <circle cx="12" cy="12" r="8.5" />
      <path class="filled" d="M12 3.5a8.5 8.5 0 0 1 0 17Z" />
    {/if}
  </svg>
</button>

<style>
  .toggle {
    display: grid;
    place-items: center;
    margin-left: auto;
    padding: 5px;
    color: var(--rail-muted);
    background: transparent;
    border-color: transparent;
  }

  .toggle:hover:not(:disabled) {
    color: var(--rail-text);
    background: var(--rail-hi);
  }

  svg {
    width: 18px;
    height: 18px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .filled {
    fill: currentColor;
  }
</style>
