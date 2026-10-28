import { EmailMessage } from 'cloudflare:email'
import { env } from 'cloudflare:workers'
import { eq } from 'drizzle-orm'
import { createAuth } from './auth.options'
import { passkey } from './auth-schema'
import { createDatabaseClient, createDb } from './db'

async function deliverAuthEmail(to: string, subject: string, message: string) {
  const sender = env.EMAIL_FROM
  if (!sender) throw new Error('EMAIL_FROM is not configured')
  const raw = [
    `From: Turtle Flow <${sender}>`,
    `To: ${to}`,
    `Subject: ${subject}`,
    'MIME-Version: 1.0',
    'Content-Type: text/plain; charset=UTF-8',
    'Content-Transfer-Encoding: 8bit',
    '',
    message,
  ].join('\r\n')
  await env.EMAIL.send(new EmailMessage(sender, to, raw))
}

export async function createAuthContext() {
  const client = createDatabaseClient()
  await client.connect()
  try {
    const db = createDb(client)
    const auth = createAuth({
      database: db,
      secret: env.BETTER_AUTH_SECRET,
      origin: env.APP_ORIGIN,
      deliverEmail: deliverAuthEmail,
    })
    return {
      auth,
      db,
      close: () => client.end(),
    }
  } catch (error) {
    await client.end()
    throw error
  }
}

export async function hasAccountProtection(
  context: Awaited<ReturnType<typeof createAuthContext>>,
  user: { id: string; twoFactorEnabled?: boolean | null },
) {
  if (user.twoFactorEnabled) return true
  const [registeredPasskey] = await context.db
    .select({ id: passkey.id })
    .from(passkey)
    .where(eq(passkey.userId, user.id))
    .limit(1)
  return Boolean(registeredPasskey)
}
