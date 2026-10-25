import { defineConfig } from 'astro/config';
import preact from '@astrojs/preact';
import tailwindcss from '@tailwindcss/vite';
import icon from 'astro-icon';

export default defineConfig({
  output: 'static',
  devToolbar: { enabled: false },
  markdown: { syntaxHighlight: 'prism' },
  trailingSlash: 'never',
  integrations: [preact(), icon()],
  vite: { plugins: [tailwindcss()] },
  security: { csp: { directives: ["default-src 'self'", "img-src 'self' data:", "font-src 'self'", "connect-src 'self'", "object-src 'none'", "base-uri 'self'", "form-action 'self'"] } }
});
