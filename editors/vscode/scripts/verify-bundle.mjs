import { fileURLToPath } from 'node:url';
import { verifyBundle } from './bundle-utils.mjs';
const metadata = await verifyBundle(fileURLToPath(new URL('../bundled', import.meta.url)));
console.log(`Verified ${metadata.version} (${metadata.target}).`);
