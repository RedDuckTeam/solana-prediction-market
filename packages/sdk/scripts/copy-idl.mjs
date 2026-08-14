// Copies the IDL that `anchor build` produced into the SDK.
//
// Kept as an explicit step rather than a build-time import so that the SDK is
// publishable on its own, and so that a stale IDL is a visible diff rather than
// a silent mismatch between a client and the program it talks to.
import { copyFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const source = resolve(here, '../../../target/idl/prediction_market.json');
const target = resolve(here, '../src/idl/prediction_market.json');

if (!existsSync(source)) {
  console.error('No IDL found. Run `anchor build` first.');
  process.exit(1);
}
mkdirSync(dirname(target), { recursive: true });
copyFileSync(source, target);
console.log(`IDL copied to ${target}`);
