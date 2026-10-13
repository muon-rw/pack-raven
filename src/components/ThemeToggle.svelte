<script lang="ts">
  import { theme, toggleTheme } from '../lib/theme.svelte'

  let dark = $derived(theme.current === 'dark')
</script>

<button class="toggle" onclick={toggleTheme} aria-pressed={dark} aria-label="Dark mode" title={dark ? 'Switch to light mode' : 'Switch to dark mode'}>
  <svg viewBox="0 0 24 24" aria-hidden="true">
    {#if dark}
      <circle cx="12" cy="12" r="4.5" />
      <path d="M12 2.5v2.5M12 19v2.5M2.5 12H5M19 12h2.5M5.3 5.3l1.8 1.8M16.9 16.9l1.8 1.8M5.3 18.7l1.8-1.8M16.9 7.1l1.8-1.8" />
    {:else}
      <path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5Z" />
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
</style>
