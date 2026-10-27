import { createAuth } from './auth.options'

// Schema generation loads this config in Node, outside a Workers runtime.
export const auth = createAuth({
  secret: process.env.BETTER_AUTH_SECRET ?? 'schema-generation-secret-not-for-runtime',
  origin: 'https://dash.turtle-flow.com',
  deliverEmail: async () => {},
})
