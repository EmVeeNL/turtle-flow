import test from 'node:test'
import assert from 'node:assert/strict'
import { readLongplayAccess } from './longplay-access.ts'

const env = { LONGPLAY_TEAM_FEATURE_ID: '11111111-1111-4111-8111-111111111111', LONGPLAY_APP_ID: 'app',
  LONGPLAY_BASE_URL: 'https://billing.invalid', LONGPLAY_SECRET_KEY: 'server-key', LONGPLAY_ENVIRONMENT: 'sandbox' }

test('disabled deployments do not initialize the billing client', async () => {
  assert.deepEqual(await readLongplayAccess('u', {}, () => { throw new Error('must not initialize') }), { status: 'disabled' })
})
test('uses the protected account identity and observes grant then revocation', async () => {
  let allowed = true
  const fake = (() => ({ getOrCreateCustomer: async (input: unknown) => {
    assert.deepEqual(input, { externalUserId: 'turtle-flow:user:verified-user' }); return { customer: { id: 'c' } }
  }, checkEntitlement: async () => ({ check: { kind: 'access', allowed } }) })) as unknown as Parameters<typeof readLongplayAccess>[2]
  const granted = await readLongplayAccess('verified-user', env, fake)
  assert.deepEqual(granted, { status: 'allowed' })
  assert.equal('expiresAt' in granted, false) // An access check cannot fabricate a signed license lease.
  allowed = false
  assert.deepEqual(await readLongplayAccess('verified-user', env, fake), { status: 'denied' })
})
test('misconfiguration and upstream diagnostics are not returned to the account page', async () => {
  assert.deepEqual(await readLongplayAccess('u', { LONGPLAY_TEAM_FEATURE_ID: 'set' }), { status: 'unavailable' })
  assert.deepEqual(await readLongplayAccess('u', env, () => { throw new Error('PRIVATE') }), { status: 'unavailable' })
})
test('a metered balance is not a Team access grant', async () => {
  const fake = (() => ({ getOrCreateCustomer: async () => ({ customer: { id: 'c' } }),
    checkEntitlement: async () => ({ check: { kind: 'metered', allowed: true } }) })) as unknown as Parameters<typeof readLongplayAccess>[2]
  assert.deepEqual(await readLongplayAccess('u', env, fake), { status: 'unavailable' })
})
