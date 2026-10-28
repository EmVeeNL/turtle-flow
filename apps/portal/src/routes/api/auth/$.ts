import { createFileRoute } from '@tanstack/react-router'
import { createAuthContext } from '../../../server/auth'

export const Route = createFileRoute('/api/auth/$')({
  server: {
    handlers: {
      GET: async ({ request }) => {
        const context = await createAuthContext()
        try {
          return await context.auth.handler(request)
        } finally {
          await context.close()
        }
      },
      POST: async ({ request }) => {
        const context = await createAuthContext()
        try {
          return await context.auth.handler(request)
        } finally {
          await context.close()
        }
      },
    },
  },
})
