import { createFileRoute } from '@tanstack/react-router'
import { eq } from 'drizzle-orm'
import { createAuthContext, hasAccountProtection } from '../../../../server/auth'
import { license, licenseAuditEvent } from '../../../../server/schema'

export const Route = createFileRoute('/api/licenses/$licenseId/download')({
  server: {
    handlers: {
      GET: async ({ request, params }) => {
        const context = await createAuthContext()
        try {
          const session = await context.auth.api.getSession({ headers: request.headers })
          if (!session || !(await hasAccountProtection(context, session.user))) return new Response('Unauthorized', { status: 401 })

          const [ownedLicense] = await context.db
            .select({ id: license.id, userId: license.userId, keyId: license.keyId, signedPayload: license.signedPayload, signature: license.signature })
            .from(license)
            .where(eq(license.id, params.licenseId))
            .limit(1)
          if (!ownedLicense) return new Response('Not found', { status: 404 })
          if (ownedLicense.userId !== session.user.id) return new Response('Not found', { status: 404 })

          await context.db.insert(licenseAuditEvent).values({
            id: crypto.randomUUID(),
            licenseId: ownedLicense.id,
            actorUserId: session.user.id,
            eventType: 'license_file_downloaded',
            details: {},
          })
          const envelope = JSON.stringify({ payload: ownedLicense.signedPayload, key_id: ownedLicense.keyId, signature: ownedLicense.signature }, null, 2)
          const safeId = ownedLicense.id.replace(/[^A-Za-z0-9_-]/g, '') || 'license'
          return new Response(envelope, {
            headers: {
              'content-type': 'application/json; charset=utf-8',
              'content-disposition': `attachment; filename="${safeId}.turtle-license"`,
              'cache-control': 'private, no-store',
              'x-content-type-options': 'nosniff',
            },
          })
        } finally {
          await context.close()
        }
      },
    },
  },
})
