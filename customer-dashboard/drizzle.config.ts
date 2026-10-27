import { defineConfig } from 'drizzle-kit'
import { existsSync } from 'node:fs'

if (existsSync('.env')) process.loadEnvFile('.env')

export default defineConfig({
  dialect: 'postgresql',
  schema: './src/server/schema.ts',
  out: './drizzle',
  dbCredentials: {
    url: process.env.DATABASE_URL ?? 'postgres://schema:placeholder@127.0.0.1:5432/schema',
  },
  strict: true,
  verbose: true,
})
