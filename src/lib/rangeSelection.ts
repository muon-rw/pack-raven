import type { SvelteSet } from 'svelte/reactivity'
import { commandKey } from './platform'

export function isPlainClick(event: MouseEvent | KeyboardEvent) {
  return !event.shiftKey && !commandKey(event)
}

// Like in Explorer, Shift+click selects everything from the anchor to the row you click. We move
// the anchor on each plain or Ctrl/Cmd click, and keep it in place on Shift clicks.
export class RangeSelection {
  anchor: string | null = null

  constructor(readonly keys: SvelteSet<string>) {}

  only(key: string) {
    this.keys.clear()
    this.keys.add(key)
    this.anchor = key
  }

  toggle(key: string) {
    if (this.keys.has(key)) this.keys.delete(key)
    else this.keys.add(key)
    this.anchor = key
  }

  replace(keys: string[]) {
    this.keys.clear()
    for (const key of keys) this.keys.add(key)
    this.anchor = keys[0] ?? null
  }

  range(order: string[], to: string) {
    const end = order.indexOf(to)
    const start = this.anchor && order.includes(this.anchor) ? order.indexOf(this.anchor) : end
    this.keys.clear()
    for (const key of order.slice(Math.min(start, end), Math.max(start, end) + 1)) this.keys.add(key)
  }

  click(event: MouseEvent | KeyboardEvent, key: string, order: string[]) {
    if (event.shiftKey) this.range(order, key)
    else if (commandKey(event)) this.toggle(key)
    else this.only(key)
  }
}
