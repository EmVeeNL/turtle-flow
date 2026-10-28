# Monorepo layout

One repository, two toolchains: **Cargo** for Rust, **pnpm + Turborepo** for TypeScript.

```
apps/
  cli/         Rust. The `turtle` binary (package `turtle-flow`).
  dashboard/   Astro + Preact. Local dashboard, embedded into the binary by `turtle-api`.
  portal/      TanStack Start on Cloudflare Workers. Accounts, licences, CLI device linking.
  website/     Astro on Cloudflare Workers. Public site (turtle-flow.com).
crates/        Rust libraries (`turtle-domain`, `-store`, `-api`, ...). See ARCHITECTURE.md §4.1.
packages/      Shared TypeScript packages (none yet).
docs/          All documentation, decisions (adr/), RFCs, spikes and assets.
fixtures/      Test fixtures and benchmark sources.
scripts/       Release, e2e and docs tooling.
```

## Setup

```bash
corepack enable          # pnpm is pinned by `packageManager` in package.json
corepack pnpm install
```

## Common commands

| Task | Command |
|---|---|
| Build all TS apps | `corepack pnpm build` |
| Lint / typecheck / test | `corepack pnpm lint`, `corepack pnpm check`, `corepack pnpm test` |
| One app | `corepack pnpm --filter @turtle-flow/portal dev` |
| Rust tests | `cargo test --workspace` |
| Release binary with dashboard | `PUBLIC_API_ADAPTER=real corepack pnpm --filter @turtle-flow/dashboard build && cargo build --release -p turtle-flow --features embedded-dashboard` |

`apps/cli/package.json` wraps the Cargo commands so `turbo run build` orders the dashboard build before the embedding binary build.

## CI

CI runs Rust jobs for Rust paths and the web job for web paths. The docs link check runs on every push and pull request. The `all-green` job combines the results, treating skipped jobs as successful; use `all-green` as the required branch protection check.

To enable Turborepo remote caching in CI, configure these GitHub repository settings:

| Setting | Value |
|---|---|
| Actions secret `TURBO_TOKEN` | A Turborepo remote cache access token. |
| Actions variable `TURBO_TEAM` | The team slug associated with the token. |

The web job continues with local caching when either setting is unset, including on fork pull requests where the secret is unavailable.

## Rules

- Cargo owns the root `Cargo.toml`/`Cargo.lock`; workspace members are `crates/*` and `apps/cli`.
- A single `pnpm-lock.yaml` at the root. No per-app lockfiles, no npm.
- Only the root `package.json` sets `packageManager`.
- Docs live in `docs/`. App READMEs stay next to the app and only cover running that app.
- `turtle-api` embeds `apps/dashboard/dist` (`rust-embed`). Build the dashboard first when using `embedded-dashboard`.
