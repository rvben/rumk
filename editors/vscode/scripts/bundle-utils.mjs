import { createHash } from 'node:crypto';
import { readFile, access } from 'node:fs/promises';
import { constants } from 'node:fs';
import path from 'node:path';

export const targets = {
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'linux-arm64': 'aarch64-unknown-linux-musl',
  'linux-x64': 'x86_64-unknown-linux-musl',
  'win32-x64': 'x86_64-pc-windows-msvc',
};
export function hostTarget(platform = process.platform, arch = process.arch) {
  const target = `${platform}-${arch}`;
  if (!targets[target]) throw new Error(`No bundled Rumk package is configured for ${target}.`);
  return target;
}
export function binaryName(target) {
  if (!targets[target]) throw new Error(`Unsupported extension target: ${target}`);
  return target.startsWith('win32-') ? 'rumk.exe' : 'rumk';
}
export function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}
export async function verifyBundle(directory, expectedTarget = hostTarget()) {
  const metadata = JSON.parse(await readFile(path.join(directory, 'metadata.json'), 'utf8'));
  if (metadata.target !== expectedTarget || metadata.rustTarget !== targets[expectedTarget]) {
    throw new Error(`Bundled Rumk target does not match ${expectedTarget}; run npm run bundle.`);
  }
  const executable = path.join(directory, binaryName(expectedTarget));
  if (sha256(await readFile(executable)) !== metadata.sha256) {
    throw new Error('Bundled Rumk checksum mismatch; run npm run bundle.');
  }
  await access(executable, expectedTarget.startsWith('win32-') ? constants.F_OK : constants.X_OK);
  await access(path.join(directory, 'THIRD_PARTY_NOTICES.txt'));
  return metadata;
}
