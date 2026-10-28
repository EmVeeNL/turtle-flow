# turtle-flow.com

Coming-soon page for Turtle Flow. Astro (static), Tailwind CSS v4, GSAP, pnpm. Typefaces: Space Grotesk for headings (product owner's choice for the site), Asap Sharp for body text (as in the dashboard), JetBrains Mono for hashes; same icon set (Fluent via `astro-icon`, resolved at build time) as the dashboard; colors come from the logo.

```sh
cd website
pnpm install
pnpm dev          # http://localhost:4321
pnpm build        # static output in dist/
pnpm check        # astro check
pnpm deploy       # build + wrangler deploy
```

## Deployment

Cloudflare Workers Static Assets (`wrangler.jsonc`): the Worker `turtle-flow-website` serves `dist/` and owns the custom domains `turtle-flow.com` and `www.turtle-flow.com` (the zone is in the same Cloudflare account, so `wrangler deploy` creates the DNS records and certificates). Deploy with a logged-in `wrangler` (`wrangler login`). `worker.js` runs first and 301-redirects `www.turtle-flow.com` and plain `http://` to `https://turtle-flow.com` (path and query kept); everything else is served from `dist/`.

## Notes

- There is no signup form: a disabled form read as broken, so the page shows a "Coming soon" label instead. Add a real form only together with a signup service and a privacy notice.
- The two hashes in the stage line are examples from a test run and are labeled as such on the page. Do not replace them with anything that looks like live data.
- Motion: the hero entrance (copy rising, the turtle gliding in) is pure CSS so first paint never waits for script; one GSAP timeline then draws the stage line, lights the stages and resolves the two approval hashes, and gives the turtle its slow idle drift.
- Performance: the logo is served by `astro:assets` as responsive AVIF/WebP (`src/assets/`, never a raw PNG from `public/`), the stylesheet is inlined, only the latin font subsets in use are shipped (Space Grotesk is preloaded), and the page makes no third-party requests. Lighthouse mobile on the production build: 99 performance, 100 accessibility, 100 best practices, 100 SEO.
- Motion is skipped under `prefers-reduced-motion`, and all content is visible without JavaScript.
- Licenses for the fonts are in `licenses/`.
