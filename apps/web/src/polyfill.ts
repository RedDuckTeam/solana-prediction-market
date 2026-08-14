import { Buffer } from 'buffer';

/**
 * `@solana/web3.js` still reaches for Node's `Buffer`. Its own module, imported
 * first, because a module body runs after everything it imports — assigning the
 * global inside `main.tsx` was already too late.
 */
globalThis.Buffer ??= Buffer;
