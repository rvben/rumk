import { build } from 'esbuild';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

const result = await build({
  entryPoints: ['src/extension.ts'], outfile: 'dist/extension.js', bundle: true,
  platform: 'node', format: 'cjs', target: 'node20', external: ['vscode'],
  sourcemap: false, minify: true, metafile: true,
});

// Preserve the license notices of every dependency actually bundled into the VSIX.
const packages = new Set(Object.keys(result.metafile.inputs)
  .map(input => input.match(/^(.*node_modules\/(?:@[^/]+\/)?[^/]+)\//)?.[1])
  .filter(Boolean));
const notices = [];
for (const directory of [...packages].sort()) {
  const metadata = JSON.parse(await readFile(path.join(directory, 'package.json'), 'utf8'));
  const license = (await readdir(directory)).find(name => /^licen[sc]e(?:\..*)?$/i.test(name));
  if (!license) throw new Error(`Missing license for bundled dependency ${metadata.name}`);
  notices.push(`${metadata.name} ${metadata.version}\n\n${await readFile(path.join(directory, license), 'utf8')}`);
}
await writeFile('dist/THIRD_PARTY_NOTICES.txt', notices.join('\n\n--------------------\n\n'));
