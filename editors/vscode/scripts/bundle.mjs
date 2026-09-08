import { execFileSync } from 'node:child_process';
import { readFile, readdir, writeFile, mkdir, copyFile, chmod, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { hostTarget, targets, binaryName, sha256 } from './bundle-utils.mjs';

const extension = fileURLToPath(new URL('..', import.meta.url));
const root = path.resolve(extension, '../..');
const target = hostTarget();
const rustTarget = targets[target];
const runCargo = args => execFileSync('cargo', args, { cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
// Use a dedicated target directory so packaging cannot pick up an unrelated build.
const buildDirectory = path.join(extension, '.build');
execFileSync('cargo', ['build', '--locked', '--release', '--bin', 'rumk', '--target', rustTarget, '--target-dir', buildDirectory], { cwd: root, stdio: 'inherit' });
const binary = path.join(buildDirectory, rustTarget, 'release', binaryName(target));
const version = execFileSync(binary, ['version'], { encoding: 'utf8', timeout: 10000 }).trim();
execFileSync(binary, ['server', '--help'], { stdio: 'pipe', timeout: 10000 });
const metadata = JSON.parse(runCargo(['metadata', '--locked', '--format-version', '1', '--filter-platform', rustTarget]));
const notices = [];
for (const pkg of metadata.packages.sort((a, b) => a.name.localeCompare(b.name))) {
  const directory = path.dirname(pkg.manifest_path);
  const candidates = (await readdir(directory)).filter(name => /^(licen[sc]e|copying|unlicense)/i.test(name));
  if (pkg.license_file && !candidates.includes(pkg.license_file)) candidates.push(pkg.license_file);
  if (!candidates.length) throw new Error(`No license file found for ${pkg.name}`);
  async function collect(relative) {
    const full = path.join(directory, relative);
    try { return `${relative}\n${await readFile(full, 'utf8')}`; }
    catch (error) {
      if (error.code !== 'EISDIR') throw error;
      return (await Promise.all((await readdir(full)).sort().map(name => collect(path.join(relative, name))))).join('\n');
    }
  }
  notices.push(`${pkg.name} ${pkg.version} (${pkg.license || 'see license'})\n\n${(await Promise.all(candidates.sort().map(collect))).join('\n\n')}`);
}
const destination = path.join(extension, 'bundled');
// This directory contains generated bundle material only.
await rm(destination, { recursive: true, force: true });
await mkdir(destination, { recursive: true });
await copyFile(binary, path.join(destination, binaryName(target)));
if (process.platform !== 'win32') await chmod(path.join(destination, binaryName(target)), 0o755);
await writeFile(path.join(destination, 'THIRD_PARTY_NOTICES.txt'), notices.join('\n\n--------------------\n\n'));
const revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
const sourceDirty = execFileSync('git', ['status', '--porcelain', '--', 'src', 'Cargo.toml', 'Cargo.lock'], { cwd: root, encoding: 'utf8' }).trim() !== '';
await writeFile(path.join(destination, 'metadata.json'), JSON.stringify({ target, rustTarget, version, revision, sourceDirty, sha256: sha256(await readFile(binary)) }, null, 2) + '\n');
console.log(`Bundled ${version} for ${target}${sourceDirty ? ' (locally modified source)' : ''}.`);
