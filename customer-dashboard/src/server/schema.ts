import { relations } from 'drizzle-orm'
import { index, integer, jsonb, pgEnum, pgTable, text, timestamp, uniqueIndex } from 'drizzle-orm/pg-core'
import { user } from './auth-schema'

export * from './auth-schema'

export const licenseStatus = pgEnum('license_status', ['active', 'expired', 'revoked'])

/** Stores the non-secret offline-verifiable envelope; the signing private key never belongs in this database. */
export const license = pgTable('license', {
  id: text('id').primaryKey(),
  userId: text('user_id').notNull().references(() => user.id, { onDelete: 'cascade' }),
  status: licenseStatus('status').notNull().default('active'),
  keyId: text('key_id').notNull(),
  signedPayload: jsonb('signed_payload').$type<Record<string, unknown>>().notNull(),
  signature: text('signature').notNull(),
  issuedAt: timestamp('issued_at', { withTimezone: true }).notNull().defaultNow(),
  expiresAt: timestamp('expires_at', { withTimezone: true }),
  revokedAt: timestamp('revoked_at', { withTimezone: true }),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull().defaultNow(),
}, (table) => [
  index('license_user_issued_idx').on(table.userId, table.issuedAt),
  uniqueIndex('license_user_id_unique').on(table.userId),
  index('license_status_idx').on(table.status),
])

/** Append-only application events; never place credentials or license secrets in details. */
export const licenseAuditEvent = pgTable('license_audit_event', {
  id: text('id').primaryKey(),
  licenseId: text('license_id').references(() => license.id, { onDelete: 'set null' }),
  actorUserId: text('actor_user_id').references(() => user.id, { onDelete: 'set null' }),
  eventType: text('event_type').notNull(),
  details: jsonb('details').$type<Record<string, string | number | boolean | null>>().notNull().default({}),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull().defaultNow(),
}, (table) => [
  index('license_audit_license_date_idx').on(table.licenseId, table.createdAt),
  index('license_audit_actor_date_idx').on(table.actorUserId, table.createdAt),
])

export const cliDeviceAuthorization = pgTable('cli_device_authorization', {
  id: text('id').primaryKey(),
  deviceCodeHash: text('device_code_hash').notNull().unique(),
  userCodeHash: text('user_code_hash').notNull(),
  publicKey: text('public_key').notNull(),
  thumbprint: text('thumbprint').notNull(),
  deviceName: text('device_name').notNull(),
  platform: text('platform').notNull(),
  cliVersion: text('cli_version').notNull(),
  status: text('status').notNull().default('pending'),
  attempts: integer('attempts').notNull().default(0),
  pollInterval: integer('poll_interval').notNull().default(5),
  lastPolledAt: timestamp('last_polled_at', { withTimezone: true }),
  userId: text('user_id').references(() => user.id, { onDelete: 'cascade' }),
  licenseId: text('license_id').references(() => license.id, { onDelete: 'set null' }),
  installationId: text('installation_id'),
  lease: jsonb('lease').$type<Record<string, unknown>>(),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull().defaultNow(),
  expiresAt: timestamp('expires_at', { withTimezone: true }).notNull(),
  consumedAt: timestamp('consumed_at', { withTimezone: true }),
})

export const cliInstallation = pgTable('cli_installation', {
  id: text('id').primaryKey(),
  userId: text('user_id').notNull().references(() => user.id, { onDelete: 'cascade' }),
  licenseId: text('license_id').notNull().references(() => license.id, { onDelete: 'cascade' }),
  publicKey: text('public_key').notNull(),
  thumbprint: text('thumbprint').notNull(),
  deviceName: text('device_name').notNull(),
  platform: text('platform').notNull(),
  cliVersion: text('cli_version').notNull(),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull().defaultNow(),
  lastAuthorizedAt: timestamp('last_authorized_at', { withTimezone: true }).notNull().defaultNow(),
  revokedAt: timestamp('revoked_at', { withTimezone: true }),
}, (table) => [
  index('cli_installation_user_active_idx').on(table.userId, table.revokedAt),
  uniqueIndex('cli_installation_user_thumbprint_unique').on(table.userId, table.thumbprint),
])

/** Atomic per-account throttle for protected device-code entry attempts. */
export const cliUserCodeAttempt = pgTable('cli_user_code_attempt', {
  userId: text('user_id').primaryKey().references(() => user.id, { onDelete: 'cascade' }),
  windowStartedAt: timestamp('window_started_at', { withTimezone: true }).notNull(),
  attempts: integer('attempts').notNull().default(0),
})

export const licenseRelations = relations(license, ({ many }) => ({ events: many(licenseAuditEvent) }))
export const licenseAuditRelations = relations(licenseAuditEvent, ({ one }) => ({ license: one(license, { fields: [licenseAuditEvent.licenseId], references: [license.id] }) }))
