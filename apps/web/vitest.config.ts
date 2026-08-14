import { fileURLToPath, URL } from 'node:url';

import { defineConfig } from 'vitest/config';

/**
 * Separate from `vite.config.ts` because the Cloudflare plugin refuses the
 * `resolve.external` vitest sets. Nothing under test needs a worker: these are
 * the compiler and the graph, which are pure.
 */
export default defineConfig({
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
});
