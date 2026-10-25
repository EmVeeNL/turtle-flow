import eslint from '@eslint/js';
import tseslint from 'typescript-eslint';
import astro from 'eslint-plugin-astro';
export default [
  { ignores: ['dist/**', '.astro/**', 'node_modules/**', 'screenshots/**'] },
  eslint.configs.recommended,
  { files: ['public/**/*.js'], languageOptions: { globals: { document:'readonly', navigator:'readonly', localStorage:'readonly', setTimeout:'readonly', location:'readonly', Blob:'readonly', URL:'readonly' } }, rules: { 'no-empty': 'off' } },
  ...tseslint.configs.recommended,
  ...astro.configs.recommended,
  { rules: { '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }] } }
];
