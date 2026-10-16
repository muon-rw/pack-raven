import type { IgnoreOption } from './api'
import type { Feedback } from './gitActions'

export type IgnoreRequest = { paths: string[]; options: IgnoreOption[]; chosen: number; feedback: Feedback }

export const ignoreDialog = $state<{ request: IgnoreRequest | null }>({ request: null })

export function openIgnoreDialog(request: IgnoreRequest) {
  ignoreDialog.request = request
}
