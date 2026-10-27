import { createServerFn } from '@tanstack/react-start'
import { getRequestHeaders } from '@tanstack/react-start/server'
import { desc, eq } from 'drizzle-orm'
import { createAuthContext, hasAccountProtection } from './auth'
import { license, licenseAuditEvent } from './schema'
import { signFreeLicense } from './license-issuing'

export const getMyLicenses = createServerFn({ method: 'GET' }).handler(async () => {
  const context = await createAuthContext()
  try {
    const session = await context.auth.api.getSession({ headers: getRequestHeaders() })
    if (!session || !(await hasAccountProtection(context, session.user))) throw new Error('Unauthorized')
    return await context.db
      .select({
        id: license.id,
        status: license.status,
        keyId: license.keyId,
        issuedAt: license.issuedAt,
        expiresAt: license.expiresAt,
        revokedAt: license.revokedAt,
      })
      .from(license)
      .where(eq(license.userId, session.user.id))
      .orderBy(desc(license.issuedAt))
  } finally {
    await context.close()
  }
})

export const createMyFreeLicense = createServerFn({ method: 'POST' }).handler(async () => {
  const context = await createAuthContext()
  try {
    const session = await context.auth.api.getSession({ headers: getRequestHeaders() })
    if (!session || !(await hasAccountProtection(context, session.user))) throw new Error('Unauthorized')

    const id = `lic_${crypto.randomUUID().replaceAll('-', '')}`
    const issuedAt = new Date()
    const timestamp = issuedAt.toISOString()
    const payload = {
      schema: 'turtle.license.v1',
      license_id: id,
      licensee: {
        org: session.user.name || 'Turtle Flow User',
        contact: session.user.email,
      },
      tier: 'free',
      seats: 1,
      worker_limit: null,
      features: ['local'],
      deployment: ['local'],
      issued_at: timestamp,
      not_before: timestamp,
      expires_at: null,
      grace_days: 0,
      server_binding: null,
    }
    const signed = await signFreeLicense(payload)

    return await context.db.transaction(async (tx) => {
      const [created] = await tx
        .insert(license)
        .values({
          id,
          userId: session.user.id,
          status: 'active',
          keyId: signed.keyId,
          signedPayload: payload,
          signature: signed.signature,
          issuedAt,
        })
        .onConflictDoNothing({ target: license.userId })
        .returning({ id: license.id })

      if (!created) {
        const [existing] = await tx.select({ id: license.id }).from(license).where(eq(license.userId, session.user.id)).limit(1)
        return { created: false, licenseId: existing?.id ?? null }
      }

      await tx.insert(licenseAuditEvent).values({
        id: crypto.randomUUID(),
        licenseId: created.id,
        actorUserId: session.user.id,
        eventType: 'free_license_issued',
        details: { tier: 'free' },
      })
      return { created: true, licenseId: created.id }
    })
  } finally {
    await context.close()
  }
})
