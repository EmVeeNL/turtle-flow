import { drizzle } from 'drizzle-orm/node-postgres'
import { env } from 'cloudflare:workers'
import { Client } from 'pg'
import * as schema from './schema'

export function createDatabaseClient() {
  return new Client({ connectionString: env.HYPERDRIVE.connectionString })
}

export function createDb(client: Client) {
  return drizzle(client, { schema })
}
