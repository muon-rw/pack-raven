<script lang="ts">
  import { untrack } from 'svelte'
  import type { LogLine } from '../lib/jobs.svelte'
  import { LogModel, type Stream } from '../lib/logs'
  import LogPanel from './LogPanel.svelte'

  let { lines, label, empty }: { lines: LogLine[]; label: string; empty?: string } = $props()

  // The job store keeps only its latest few thousand lines, but this log keeps everything since
  // the job started, up to this many, so you can scroll back past the store's limit.
  const MAX_KEPT = 100_000

  const model = new LogModel()
  let version = $state(0)
  let generation = $state(0)
  let newestId = -1
  let previousLength = 0

  // The store hands us a new array as lines arrive. We add what's newer than we've seen, and
  // start over when the array shrinks, which happens when a job restarts.
  $effect(() => {
    const current = lines
    untrack(() => {
      const restarted = current.length < previousLength
      previousLength = current.length
      if (restarted || model.lines.length > MAX_KEPT) {
        model.clear()
        newestId = -1
        generation++
      }
      // The server pane joins two jobs' lines, so we sort by id to keep them in the order they
      // were written.
      const fresh = current.filter((line) => line.id > newestId).sort((a, b) => a.id - b.id)
      for (const line of fresh) model.append(line.text, line.stream as Stream)
      if (fresh.length) {
        newestId = fresh[fresh.length - 1].id
        version++
      }
    })
  })
</script>

<LogPanel {model} {version} {generation} live framed {label} {empty} />
