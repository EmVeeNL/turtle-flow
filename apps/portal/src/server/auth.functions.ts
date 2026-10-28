import { createServerFn } from '@tanstack/react-start'
import { getRequestHeaders } from '@tanstack/react-start/server'
import { createAuthContext, hasAccountProtection } from './auth'

export const getSession = createServerFn({ method: 'GET' }).handler(async () => {
  const context = await createAuthContext()
  try {
    return await context.auth.api.getSession({ headers: getRequestHeaders() })
  } finally {
    await context.close()
  }
})

export const getAccountProtectionStatus = createServerFn({ method: 'GET' }).handler(async () => {
  const context = await createAuthContext()
  try {
    const session = await context.auth.api.getSession({ headers: getRequestHeaders() })
    if (!session) return { session: null, isProtected: false }
    return {
      session,
      isProtected: await hasAccountProtection(context, session.user),
    }
  } finally {
    await context.close()
  }
})
