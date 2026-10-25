# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack
Website (`website/`): Astro, Tailwind CSS v4, GSAP, pnpm; Space Grotesk (headings) + Asap Sharp (body) + JetBrains Mono (hashes), Fluent icons (Iconify, build time). Product dashboard (`dashboard/`): Astro + Tailwind v4 + Preact islands. Hosting for turtle-flow.com is undecided.

## Users
Developers and small teams who already use AI coding agents (Claude Code, Codex) on their own repositories and want to hand them whole jobs without losing control of what ships.

## Product Purpose
Turtle Flow is a local-first, CLI-first orchestrator that runs locally installed AI coding agents through a supervised, auditable workflow: specification, scope approval, plan, implementation in an isolated Git worktree, build and tests, review, verification, acceptance approval, delivery. Rust owns state, policy, budgets and approvals; the agents are workers. Success: a developer trusts it with real jobs because every decision that is theirs stops at their approval.

## Positioning
You approve the exact spec and the exact diff: approvals are bound to content hashes and go stale when the work changes. Everything runs on the developer's machine with the agent CLIs they already have, with no account and no cloud.

## Capabilities and Constraints
Built today (private, in development): workflow engine, hash-bound approvals, Git worktrees with factory-only commits, crash-safe effect log, token ledger and budgets, Claude Code and Codex adapters with independent review, `turtle` CLI. Not built: HTTP API serving the dashboard, releases, installers. There will be a free tier shipped as a free binary of proprietary, private-source software; paid tiers later (Team, Business, Enterprise). The website must say the product is in development and must not claim availability, pricing, customers or benchmarks.

## Brand Commitments
Name: Turtle Flow; binary `turtle`; domain turtle-flow.com. Logo: blue sea turtle with speed streaks (`docs/assets/turtle-flow-logo.png`). Typeface Asap Sharp (the website uses Space Grotesk for headings by the owner's choice, Asap Sharp for body text); icon set Fluent UI System Icons (Iconify); dashboard colors per `docs/DASHBOARD-UX.md` §3; the site uses the logo's blues.

## Evidence on Hand
Real workflow stages and CLI behavior (see `README.md` Status). No testimonials, customers, benchmarks, pricing or public repository: none may be invented. Example hashes shown on the site are synthetic and must be labeled so.

## Product Principles
- Supervised, not autonomous: the human decides, the agents work.
- Say what is true and unknown: unknown stays unknown.
- Local and private by default.
- Auditable: every step leaves a verifiable record.

## Accessibility & Inclusion
WCAG AA contrast, keyboard operable, respects `prefers-reduced-motion`.
