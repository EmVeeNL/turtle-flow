import { passkeyClient } from '@better-auth/passkey/client'
import { createAuthClient } from 'better-auth/react'
import { twoFactorClient } from 'better-auth/client/plugins'

export const authClient = createAuthClient({
  plugins: [
    twoFactorClient({
      onTwoFactorRedirect() {
        window.location.assign('/two-factor')
      },
    }),
    passkeyClient(),
  ],
})
