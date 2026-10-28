import { passkey } from '@better-auth/passkey'
import { drizzleAdapter } from 'better-auth/adapters/drizzle'
import { betterAuth } from 'better-auth'
import { twoFactor } from 'better-auth/plugins'
import { tanstackStartCookies } from 'better-auth/tanstack-start'
import type { createDb } from './db'
import * as authSchema from './auth-schema'

export function createAuth(options: {
  database?: ReturnType<typeof createDb>
  secret: string
  origin: string
  deliverEmail: (to: string, subject: string, message: string) => Promise<void>
}) {
  return betterAuth({
    appName: 'Turtle Flow',
    baseURL: options.origin,
    secret: options.secret,
    trustedOrigins: [options.origin],
    database: options.database
      ? drizzleAdapter(options.database, {
          provider: 'pg',
          schema: {
            user: authSchema.user,
            session: authSchema.session,
            account: authSchema.account,
            verification: authSchema.verification,
            twoFactor: authSchema.twoFactor,
            passkey: authSchema.passkey,
          },
        })
      : undefined,
    emailAndPassword: {
      enabled: true,
      minPasswordLength: 12,
      requireEmailVerification: true,
      sendResetPassword: async ({ user, url }) =>
        options.deliverEmail(user.email, 'Reset your Turtle Flow password', `Reset your password using this secure link:\n\n${url}\n\nIf you did not request this, you can ignore this email.`),
    },
    emailVerification: {
      sendOnSignUp: true,
      autoSignInAfterVerification: false,
      sendVerificationEmail: async ({ user, url }) =>
        options.deliverEmail(user.email, 'Verify your Turtle Flow account', `Verify your email address using this secure link:\n\n${url}\n\nIf you did not create this account, you can ignore this email.`),
    },
    plugins: [
      twoFactor({ issuer: 'Turtle Flow', skipVerificationOnEnable: false }),
      passkey({ rpID: new URL(options.origin).hostname, rpName: 'Turtle Flow Account' }),
      tanstackStartCookies(),
    ],
  })
}
