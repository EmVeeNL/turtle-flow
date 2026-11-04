import { createServerFn } from '@tanstack/react-start'
import { getRequestHeaders, setResponseHeader } from '@tanstack/react-start/server'
import { createAuthContext, hasAccountProtection } from './auth'
import { readLongplayAccess } from './longplay-access'

// The caller cannot select a Customer or bypass the portal's passkey/MFA policy.
export const getMyBillingAccess = createServerFn({ method: 'GET' }).handler(async () => {
  setResponseHeader('Cache-Control', 'private, no-store')
  setResponseHeader('Vary', 'Cookie, Authorization')
  const context = await createAuthContext()
  try {
    const session = await context.auth.api.getSession({ headers: getRequestHeaders() })
    if (!session || !(await hasAccountProtection(context, session.user))) throw new Error('Unauthorized')
    return await readLongplayAccess(session.user.id)
  } finally {
    await context.close()
  }
})
