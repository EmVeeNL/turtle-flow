import { createFileRoute } from '@tanstack/react-router'
import { env } from 'cloudflare:workers'
import { and, desc, eq, gt, isNull, lt, sql } from 'drizzle-orm'
import { createAuthContext, hasAccountProtection } from '../../../server/auth'
import { cliDeviceAuthorization, cliInstallation, cliUserCodeAttempt, license, licenseAuditEvent, user } from '../../../server/schema'
import { signFreeLicense } from '../../../server/license-issuing'

const alphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'
const json = (data: unknown, status = 200) => Response.json(data, { status, headers: { 'cache-control': 'no-store', 'x-content-type-options': 'nosniff' } })
const b64url = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes)).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '')
const fromB64url = (value: string) => Uint8Array.from(atob(value.replaceAll('-', '+').replaceAll('_', '/') + '='.repeat((4 - value.length % 4) % 4)), (c) => c.charCodeAt(0))
const sha256 = async (value: string) => b64url(new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(value))))

async function rateLimited(request: Request, bucket: string, maximum: number, seconds: number) {
  const identity = request.headers.get('cf-connecting-ip') ?? 'unknown-origin'
  const key = `cli-rate:${bucket}:${await sha256(identity)}`
  const current = Number(await env.LICENSE_CACHE.get(key) ?? '0')
  if (current >= maximum) return true
  await env.LICENSE_CACHE.put(key, String(current + 1), { expirationTtl: seconds })
  return false
}

async function accountCodeRateLimited(context: Awaited<ReturnType<typeof createAuthContext>>, userId: string, now: Date) {
  const windowStartedAt = new Date(Math.floor(now.getTime() / 600_000) * 600_000)
  const [counter] = await context.db.insert(cliUserCodeAttempt).values({ userId, windowStartedAt, attempts: 1 })
    .onConflictDoUpdate({
      target: cliUserCodeAttempt.userId,
      set: {
        windowStartedAt,
        attempts: sql`CASE WHEN ${cliUserCodeAttempt.windowStartedAt} = ${windowStartedAt} THEN ${cliUserCodeAttempt.attempts} + 1 ELSE 1 END`,
      },
    }).returning({ attempts: cliUserCodeAttempt.attempts })
  return (counter?.attempts ?? 0) > 20
}

async function keyedUserCode(value: string) {
  const pepper = env.CLI_DEVICE_CODE_PEPPER
  if (!pepper) throw new Error('CLI device authorization is not configured.')
  const key = await crypto.subtle.importKey('raw', new TextEncoder().encode(pepper), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign'])
  return b64url(new Uint8Array(await crypto.subtle.sign('HMAC', key, new TextEncoder().encode(value.replace(/[-\s]/g, '').toUpperCase()))))
}

function normalizeUserCode(value: string) {
  const normalized = value.replace(/[-\s]/g, '').toUpperCase()
  return /^[0-9A-HJ-KM-NP-TV-Z]{10}$/.test(normalized) ? normalized : null
}

async function body(request: Request) {
  try { return await request.json() as Record<string, unknown> } catch { return null }
}

function originOk(request: Request) {
  const origin = request.headers.get('origin')
  return origin === 'https://dash.turtle-flow.com'
}

async function handler(request: Request) {
  const path = new URL(request.url).pathname.replace(/\/$/, '')
  const now = new Date()

  if (path === '/api/cli/device/authorizations' && request.method === 'POST') {
    if (await rateLimited(request, 'create', 8, 3600)) return json({ error: 'slow_down' }, 429)
    const input = await body(request)
    if (!input || typeof input.public_key !== 'string' || typeof input.device_name !== 'string' || typeof input.platform !== 'string' || typeof input.cli_version !== 'string') return json({ error: 'invalid_request' }, 400)
    let publicBytes: Uint8Array
    try { publicBytes = fromB64url(input.public_key) } catch { return json({ error: 'invalid_public_key' }, 400) }
    if (publicBytes.length !== 32 || input.device_name.length > 100 || input.platform.length > 50 || input.cli_version.length > 50) return json({ error: 'invalid_request' }, 400)
    const deviceCode = b64url(crypto.getRandomValues(new Uint8Array(32)))
    const userCode = Array.from(crypto.getRandomValues(new Uint8Array(10)), (byte) => alphabet[byte & 31]).join('').match(/.{1,4}/g)!.join('-')
    const id = `dva_${crypto.randomUUID().replaceAll('-', '')}`
    const context = await createAuthContext()
    try {
      // Expire stale one-time secrets so only live authorization records accumulate.
      await context.db.delete(cliDeviceAuthorization).where(lt(cliDeviceAuthorization.expiresAt, now))
      await context.db.insert(cliDeviceAuthorization).values({
        id, deviceCodeHash: await sha256(deviceCode), userCodeHash: await keyedUserCode(userCode),
        publicKey: input.public_key, thumbprint: await sha256(b64url(publicBytes)),
        deviceName: input.device_name, platform: input.platform, cliVersion: input.cli_version,
        status: 'pending', expiresAt: new Date(now.getTime() + 10 * 60_000),
      })
    } finally { await context.close() }
    return json({ device_code: deviceCode, user_code: userCode, verification_uri: 'https://dash.turtle-flow.com/device', expires_in: 600, interval: 5 })
  }

  if (path === '/api/cli/device/tokens' && request.method === 'POST') {
    if (await rateLimited(request, 'poll', 130, 600)) return json({ error: 'slow_down' }, 429)
    const input = await body(request)
    if (!input || typeof input.device_code !== 'string') return json({ error: 'invalid_request' }, 400)
    const context = await createAuthContext()
    try {
      const [auth] = await context.db.select().from(cliDeviceAuthorization).where(eq(cliDeviceAuthorization.deviceCodeHash, await sha256(input.device_code))).limit(1)
      if (!auth) return json({ error: 'invalid_grant' }, 400)
      if (auth.expiresAt <= now) return json({ error: 'expired_token' }, 400)
      if (auth.status === 'denied') return json({ error: 'access_denied' }, 400)
      if (auth.status === 'consumed' || auth.consumedAt) return json({ error: 'invalid_grant' }, 400)
      if (auth.lastPolledAt && now.getTime() - auth.lastPolledAt.getTime() < auth.pollInterval * 1000) {
        await context.db.update(cliDeviceAuthorization).set({ pollInterval: auth.pollInterval + 5, lastPolledAt: now }).where(eq(cliDeviceAuthorization.id, auth.id))
        return json({ error: 'slow_down' }, 400)
      }
      await context.db.update(cliDeviceAuthorization).set({ lastPolledAt: now }).where(eq(cliDeviceAuthorization.id, auth.id))
      if (auth.status !== 'approved' || !auth.lease) return json({ error: 'authorization_pending' }, 400)
      const [consumed] = await context.db.update(cliDeviceAuthorization).set({ status: 'consumed', consumedAt: now }).where(and(eq(cliDeviceAuthorization.id, auth.id), isNull(cliDeviceAuthorization.consumedAt))).returning({ id: cliDeviceAuthorization.id })
      if (!consumed) return json({ error: 'invalid_grant' }, 400)
      return json({ lease: auth.lease })
    } finally { await context.close() }
  }

  if ((path === '/api/cli/device/authorizations/approve' || path === '/api/cli/device/authorizations/deny') && request.method === 'POST') {
    if (!originOk(request)) return json({ error: 'invalid_origin' }, 403)
    if (await rateLimited(request, 'decision', 12, 600)) return json({ error: 'slow_down' }, 429)
    const context = await createAuthContext()
    try {
      const session = await context.auth.api.getSession({ headers: request.headers })
      if (!session || !(await hasAccountProtection(context, session.user))) return json({ error: 'protected_account_required' }, 403)
      if (await accountCodeRateLimited(context, session.user.id, now)) return json({ error: 'slow_down' }, 429)
      const input = await body(request)
      const userCode = input && typeof input.user_code === 'string' ? normalizeUserCode(input.user_code) : null
      if (!userCode) return json({ error: 'invalid_request' }, 400)
      const [authorization] = await context.db.select().from(cliDeviceAuthorization)
        .where(and(eq(cliDeviceAuthorization.userCodeHash, await keyedUserCode(userCode)), eq(cliDeviceAuthorization.status, 'pending'))).limit(1)
      if (!authorization || authorization.expiresAt <= now || authorization.attempts >= 8) return json({ error: 'invalid_or_expired_code' }, 404)
      if (path.endsWith('/deny')) {
        await context.db.update(cliDeviceAuthorization).set({ status: 'denied', userId: session.user.id }).where(eq(cliDeviceAuthorization.id, authorization.id))
        return json({ status: 'denied' })
      }

      const outcome = await context.db.transaction(async (tx) => {
        // Lock the account row so simultaneous approvals cannot exceed the two-installation cap.
        await tx.select({ id: user.id }).from(user).where(eq(user.id, session.user.id)).for('update')
        const [current] = await tx.select().from(cliDeviceAuthorization).where(and(eq(cliDeviceAuthorization.id, authorization.id), eq(cliDeviceAuthorization.status, 'pending'))).limit(1)
        if (!current || current.expiresAt <= now) return { error: 'invalid_or_expired_code' as const }
        const [existing] = await tx.select().from(cliInstallation).where(and(eq(cliInstallation.userId, session.user.id), eq(cliInstallation.thumbprint, current.thumbprint))).limit(1)
        const [entitlement] = await tx.select().from(license).where(eq(license.userId, session.user.id)).limit(1)
        let currentLicense = entitlement
        if (!currentLicense) {
          const licenseId = `lic_${crypto.randomUUID().replaceAll('-', '')}`
          const timestamp = now.toISOString()
          const payload = { schema: 'turtle.license.v1', license_id: licenseId, licensee: { org: session.user.name || 'Turtle Flow User', contact: session.user.email }, tier: 'free', seats: 1, worker_limit: null, features: ['local'], deployment: ['local'], issued_at: timestamp, not_before: timestamp, expires_at: null, grace_days: 0, server_binding: null }
          const signed = await signFreeLicense(payload)
          const [created] = await tx.insert(license).values({ id: licenseId, userId: session.user.id, status: 'active', keyId: signed.keyId, signedPayload: payload, signature: signed.signature, issuedAt: now }).onConflictDoNothing({ target: license.userId }).returning()
          if (created) currentLicense = created
          else [currentLicense] = await tx.select().from(license).where(eq(license.userId, session.user.id)).limit(1)
        }
        if (!currentLicense || currentLicense.status !== 'active') return { error: 'no_active_license' as const }
        if (!existing || existing.revokedAt) {
          const [{ count }] = await tx.select({ count: sql<number>`count(*)::int` }).from(cliInstallation).where(and(eq(cliInstallation.userId, session.user.id), isNull(cliInstallation.revokedAt)))
          if (count >= 2) return { error: 'installation_limit' as const }
        }
        const installationId = existing?.id ?? `cli_${crypto.randomUUID().replaceAll('-', '')}`
        if (existing) {
          await tx.update(cliInstallation).set({ licenseId: currentLicense.id, publicKey: current.publicKey, deviceName: current.deviceName, platform: current.platform, cliVersion: current.cliVersion, lastAuthorizedAt: now, revokedAt: null }).where(eq(cliInstallation.id, existing.id))
        } else {
          await tx.insert(cliInstallation).values({ id: installationId, userId: session.user.id, licenseId: currentLicense.id, publicKey: current.publicKey, thumbprint: current.thumbprint, deviceName: current.deviceName, platform: current.platform, cliVersion: current.cliVersion, createdAt: now, lastAuthorizedAt: now })
        }
        const payload = { schema: 'turtle.license.v1', license_id: currentLicense.id, licensee: { org: session.user.name || 'Turtle Flow User', contact: session.user.email }, tier: 'free', seats: 1, worker_limit: null, features: ['local'], deployment: ['local'], issued_at: now.toISOString(), not_before: now.toISOString(), expires_at: new Date(now.getTime() + 30 * 24 * 60 * 60_000).toISOString(), grace_days: 0, server_binding: null, installation_id: installationId, installation_key_thumbprint: current.thumbprint }
        const signed = await signFreeLicense(payload)
        const lease = { payload, key_id: signed.keyId, signature: signed.signature }
        await tx.update(cliDeviceAuthorization).set({ status: 'approved', userId: session.user.id, licenseId: currentLicense.id, installationId, lease }).where(eq(cliDeviceAuthorization.id, current.id))
        await tx.insert(licenseAuditEvent).values({ id: crypto.randomUUID(), licenseId: currentLicense.id, actorUserId: session.user.id, eventType: existing ? 'cli_installation_renewed' : 'cli_installation_linked', details: { installation_id: installationId } })
        return { status: 'approved' as const, device: { name: current.deviceName, platform: current.platform }, lease_expires_at: payload.expires_at }
      })
      if ('error' in outcome) return json({ error: outcome.error }, outcome.error === 'installation_limit' ? 409 : 400)
      return json(outcome)
    } finally { await context.close() }
  }

  if (path === '/api/cli/device/authorizations/pending' && request.method === 'POST') {
    if (!originOk(request)) return json({ error: 'invalid_origin' }, 403)
    if (await rateLimited(request, 'code', 20, 600)) return json({ error: 'slow_down' }, 429)
    const context = await createAuthContext()
    try {
      const session = await context.auth.api.getSession({ headers: request.headers })
      if (!session || !(await hasAccountProtection(context, session.user))) return json({ error: 'protected_account_required' }, 403)
      if (await accountCodeRateLimited(context, session.user.id, now)) return json({ error: 'slow_down' }, 429)
      const input = await body(request)
      const userCode = input && typeof input.user_code === 'string' ? normalizeUserCode(input.user_code) : null
      if (!userCode) return json({ error: 'invalid_request' }, 400)
      const [authorization] = await context.db.update(cliDeviceAuthorization).set({ attempts: sql`${cliDeviceAuthorization.attempts} + 1` }).where(and(
        eq(cliDeviceAuthorization.userCodeHash, await keyedUserCode(userCode)),
        eq(cliDeviceAuthorization.status, 'pending'),
        lt(cliDeviceAuthorization.attempts, 8),
        gt(cliDeviceAuthorization.expiresAt, now),
      )).returning({ id: cliDeviceAuthorization.id, deviceName: cliDeviceAuthorization.deviceName, platform: cliDeviceAuthorization.platform, cliVersion: cliDeviceAuthorization.cliVersion, expiresAt: cliDeviceAuthorization.expiresAt })
      if (!authorization || authorization.expiresAt <= now) return json({ error: 'invalid_or_expired_code' }, 404)
      return json({ device: { name: authorization.deviceName, platform: authorization.platform, version: authorization.cliVersion }, expires_at: authorization.expiresAt })
    } finally { await context.close() }
  }

  if (path === '/api/cli/installations' && request.method === 'GET') {
    const context = await createAuthContext()
    try {
      const session = await context.auth.api.getSession({ headers: request.headers })
      if (!session || !(await hasAccountProtection(context, session.user))) return json({ error: 'protected_account_required' }, 403)
      const items = await context.db.select({ id: cliInstallation.id, name: cliInstallation.deviceName, platform: cliInstallation.platform, version: cliInstallation.cliVersion, createdAt: cliInstallation.createdAt, lastAuthorizedAt: cliInstallation.lastAuthorizedAt, revokedAt: cliInstallation.revokedAt }).from(cliInstallation).where(eq(cliInstallation.userId, session.user.id)).orderBy(desc(cliInstallation.lastAuthorizedAt))
      return json(items)
    } finally { await context.close() }
  }

  const revoke = path.match(/^\/api\/cli\/installations\/([^/]+)$/)
  if (revoke && request.method === 'DELETE') {
    if (!originOk(request)) return json({ error: 'invalid_origin' }, 403)
    const context = await createAuthContext()
    try {
      const session = await context.auth.api.getSession({ headers: request.headers })
      if (!session || !(await hasAccountProtection(context, session.user))) return json({ error: 'protected_account_required' }, 403)
      const [updated] = await context.db.update(cliInstallation).set({ revokedAt: now }).where(and(eq(cliInstallation.id, decodeURIComponent(revoke[1]),), eq(cliInstallation.userId, session.user.id), isNull(cliInstallation.revokedAt))).returning({ id: cliInstallation.id, licenseId: cliInstallation.licenseId })
      if (!updated) return json({ error: 'not_found' }, 404)
      await context.db.insert(licenseAuditEvent).values({ id: crypto.randomUUID(), licenseId: updated.licenseId, actorUserId: session.user.id, eventType: 'cli_installation_revoked', details: { installation_id: updated.id } })
      return json({ status: 'revoked' })
    } finally { await context.close() }
  }
  return json({ error: 'not_found' }, 404)
}

export const Route = createFileRoute('/api/cli/$')({ server: { handlers: { GET: ({ request }) => handler(request), POST: ({ request }) => handler(request), DELETE: ({ request }) => handler(request) } } })
