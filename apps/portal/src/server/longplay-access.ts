import { createLongplayServer } from '@tinkerlandai/longplay-sdk/server'

/** Online account observation only; this cannot issue or renew a signed license. */
export async function readLongplayAccess(
  userId: string,
  env: Record<string, string | undefined> = process.env,
  createClient = createLongplayServer,
) {
  if (!env.LONGPLAY_TEAM_FEATURE_ID) return { status: 'disabled' as const }
  try {
    if (!userId || !env.LONGPLAY_BASE_URL || !env.LONGPLAY_SECRET_KEY || !env.LONGPLAY_APP_ID ||
      !['sandbox', 'live'].includes(env.LONGPLAY_ENVIRONMENT ?? '')) throw new Error('Missing configuration')
    const client = createClient({ baseUrl: env.LONGPLAY_BASE_URL, apiKey: env.LONGPLAY_SECRET_KEY,
      scope: { projectId: env.LONGPLAY_APP_ID, environment: env.LONGPLAY_ENVIRONMENT as 'sandbox' | 'live' }, timeoutMs: 3000 })
    const { customer } = await client.getOrCreateCustomer({ externalUserId: `turtle-flow:user:${userId}` })
    const { check } = await client.checkEntitlement({ customerId: customer.id, requirement: {
      internal_feature_id: env.LONGPLAY_TEAM_FEATURE_ID, quantity: '1', properties: {},
    } })
    if (check.kind !== 'access') throw new Error('Expected access Feature')
    return { status: check.allowed ? 'allowed' as const : 'denied' as const }
  } catch {
    return { status: 'unavailable' as const }
  }
}
