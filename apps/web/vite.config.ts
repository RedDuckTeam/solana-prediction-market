import { fileURLToPath, URL } from 'node:url';

import { cloudflare } from '@cloudflare/vite-plugin';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  // The worker in wrangler.jsonc runs inside the dev server, in the same
  // runtime Cloudflare will use, so `/rpc` behaves in development exactly as it
  // does deployed -- secrets from `.dev.vars` included -- with hot reload.
  plugins: [react(), tailwindcss(), cloudflare()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  build: {
    // A static bundle whose hash a user can check. That is the point of having
    // no server: everyone is served the same bytes, verifiably.
    target: 'es2022',
    sourcemap: true,
    rollupOptions: {
      output: {
        // The wallet and RPC libraries are most of the weight and never change
        // between releases; splitting them keeps them cached across deploys.
        manualChunks: {
          solana: ['@solana/web3.js', '@solana/spl-token', '@anchor-lang/core'],
          wallet: [
            '@solana/wallet-adapter-react',
            '@solana/wallet-adapter-react-ui',
            '@solana/wallet-adapter-base',
          ],
        },
      },
    },
  },
});
