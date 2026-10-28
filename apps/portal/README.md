# Turtle Flow Account

TanStack Start account portal for `dash.turtle-flow.com`, deployed as a Cloudflare Worker. Verified-email sign-up/sign-in, password reset, TOTP enrollment with recovery codes, passkey management, and MFA-gated account routes are implemented. Users can issue one signed, non-expiring free local license per account. Purchase checkout and product plans remain private and disabled.

## Local development

```sh
pnpm install
# Copy .dev.vars.example to .dev.vars and configure a local secret plus database.
pnpm run dev
```

## Stack

- TanStack Start + Vite and Cloudflare Workers
- Tailwind CSS v4, shadcn/ui with Base UI, and `@shadcn/lint`
- Space Grotesk, Asap Sharp, JetBrains Mono, and Fluent icons to match the Turtle Flow website
- Neon PostgreSQL through Cloudflare Hyperdrive
- Workers KV reserved for cacheable, non-authoritative config; never use it for license validity or session/revocation decisions
- Cloudflare Email Service binding named `EMAIL`

## Production setup status

- The Worker is deployed at `https://dash.turtle-flow.com`. Wrangler has the `LICENSE_CACHE` KV namespace, `HYPERDRIVE`, and Cloudflare Email Service binding configured.
- Hyperdrive uses Neon’s direct endpoint. The existing Neon owner credential is retained for now as requested; create separate migration and least-privilege runtime roles before public launch.
- A generated high-entropy `BETTER_AUTH_SECRET` is stored as a Worker secret and is not in this repository.
- `EMAIL_FROM` remains `accounts@turtle-flow.com` as requested. Complete SPF, DKIM, and DMARC setup before relying on email delivery for public sign-up.
- Database migrations are applied. The `license_user_id_unique` index enforces one license per account, including when requests race.
- The Ed25519 private signing key is stored as the Worker secret `LICENSE_SIGNING_KEY_PKCS8`; the matching public key and key ID are recorded in [LICENSE-KEYS.md](../../docs/LICENSE-KEYS.md). Never commit or expose the private key.
- Keep purchase plans, pricing, and checkout private until a payment provider, approved price IDs, refund policy, and license terms are chosen. There is no payment provider or live purchase flow configured.
- Preserve ADR-014: licenses are Ed25519-signed JSON envelopes intended for offline CLI verification. Neon stores only the signed payload, signature, and key ID; the private signing key stays in a managed signing secret and never enters the database. Hardware fingerprinting and device binding are excluded. The CLI still needs the public key added to its trust store before it can verify these downloads.

## Deployment

For a new deployment, after the database is migrated:

```sh
pnpm run cf-typegen
ppnpm run build
pnpm run deploy
```

`wrangler.jsonc` declares the `dash.turtle-flow.com` custom domain, account origin/sender variables, Cloudflare Email Service binding, `LICENSE_CACHE` namespace, and Hyperdrive config ID. Secret values are not checked in.
