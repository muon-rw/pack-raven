import type { IgnoreOption } from './api'
import type { Feedback } from './gitActions'

// In untrack mode you came to stop tracking files .gitignore doesn't cover yet, so untracking the
// matches isn't optional.
export type IgnoreRequest = { paths: string[]; options: IgnoreOption[]; chosen: number; feedback: Feedback; untrack?: boolean }

export const ignoreDialog = $state<{ request: IgnoreRequest | null }>({ request: null })

export function openIgnoreDialog(request: IgnoreRequest) {
  ignoreDialog.request = request
}
