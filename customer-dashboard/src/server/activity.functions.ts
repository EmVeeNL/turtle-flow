import { createServerFn } from '@tanstack/react-start'
import { getRequestHeaders } from '@tanstack/react-start/server'
import { desc, eq, or } from 'drizzle-orm'
import { createAuthContext, hasAccountProtection } from './auth'
import { license, licenseAuditEvent } from './schema'

export const getMyLicenseActivity = createServerFn({ method: 'GET' }).handler(async () => {
  const context = await createAuthContext()
  try {
    const session = await context.auth.api.getSession({ headers: getRequestHeaders() })
    if (!session || !(await hasAccountProtection(context, session.user))) throw new Error('Unauthorized')

    return await context.db
      .select({ id: licenseAuditEvent.id, eventType: licenseAuditEvent.eventType, createdAt: licenseAuditEvent.createdAt })
      .from(licenseAuditEvent)
      .leftJoin(license, eq(licenseAuditEvent.licenseId, license.id))
      .where(or(eq(licenseAuditEvent.actorUserId, session.user.id), eq(license.userId, session.user.id)))
      .orderBy(desc(licenseAuditEvent.createdAt))
      .limit(10)
  } finally {
    await context.close()
  }
})
