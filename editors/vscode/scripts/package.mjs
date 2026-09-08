import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { hostTarget, verifyBundle } from './bundle-utils.mjs';

const extension = fileURLToPath(new URL('..', import.meta.url));
const target = hostTarget();
const flags = process.argv.slice(2);
if (flags.some(flag => flag !== '--pre-release')) throw new Error('Only --pre-release is supported.');
await import('./bundle.mjs');
await verifyBundle(path.join(extension, 'bundled'), target);
const manifest = JSON.parse(await readFile(path.join(extension, 'package.json'), 'utf8'));
execFileSync(process.execPath, [path.join(extension, 'node_modules/@vscode/vsce/vsce'), 'package', '--no-dependencies', '--target', target, '--out', `rumk-${manifest.version}-${target}.vsix`, ...flags], { cwd: extension, stdio: 'inherit' });
