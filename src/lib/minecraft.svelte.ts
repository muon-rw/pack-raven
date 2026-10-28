import { api, type MinecraftAccount } from './api'

// account stays undefined until we've read it, so no view claims you're signed out before we know.
export const minecraft = $state({ account: undefined as MinecraftAccount | null | undefined, signingIn: false })

export const SIGN_IN_FIRST = 'Sign in to Minecraft to launch the client'

export async function refreshAccount() {
  minecraft.account = await api.minecraftAccount().catch(() => minecraft.account)
}

// Resolves to null when you cancel. A failure throws, so each caller shows it where you pressed the button.
export async function signIn(): Promise<MinecraftAccount | null> {
  minecraft.signingIn = true
  try {
    const signedIn = await api.minecraftSignIn()
    if (signedIn) minecraft.account = signedIn
    return signedIn
  } finally {
    minecraft.signingIn = false
  }
}

export function cancelSignIn() {
  return api.minecraftSignInCancel()
}

export async function signOut() {
  await api.minecraftSignOut()
  minecraft.account = null
}
