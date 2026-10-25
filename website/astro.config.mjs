import { defineConfig } from 'astro/config';
import tailwindcss from '@tailwindcss/vite';
import icon from 'astro-icon';

export default defineConfig({
  site: 'https://turtle-flow.com',
  output: 'static',
  devToolbar: { enabled: false },
  trailingSlash: 'never',
  build: { inlineStylesheets: 'always' },
  integrations: [icon()],
  vite: { plugins: [tailwindcss()] }
});
