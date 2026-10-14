<script lang="ts">
  import { errorText } from '../lib/api'
  import { mediaUrl } from '../lib/viewers'

  let { path, archived, kind }: { path: string; archived: boolean; kind: 'video' | 'audio' } = $props()

  let url = $state<string | null>(null)
  let error = $state<string | null>(null)

  $effect(() => {
    const current = path
    let cancelled = false
    let release = () => {}
    url = null
    error = null
    mediaUrl(current, archived).then(
      (loaded) => {
        if (cancelled) return loaded.release()
        release = loaded.release
        url = loaded.url
      },
      (e) => !cancelled && (error = errorText(e)),
    )
    return () => {
      cancelled = true
      release()
    }
  })

  const unplayable = "The webview can't play this file. It may use a codec it doesn't support, so try opening it in your default app."
</script>

<div class="stage">
  {#if error}
    <p class="failed">{error}</p>
  {:else if !url}
    <p class="muted">Loading…</p>
  {:else if kind === 'video'}
    <!-- svelte-ignore a11y_media_has_caption (pack videos are menu backgrounds and trailers without captions) -->
    <video src={url} controls preload="metadata" onerror={() => (error = unplayable)}></video>
  {:else}
    <audio src={url} controls preload="metadata" onerror={() => (error = unplayable)}></audio>
  {/if}
</div>

<style>
  .stage {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    padding: 16px;
    overflow: auto;
  }

  video {
    max-width: 100%;
    max-height: 100%;
    background: #000;
    border-radius: var(--radius-control);
  }

  audio {
    width: min(480px, 100%);
  }
</style>
